use std::collections::HashMap;
use std::ops::{Deref, DerefMut};

use bytes::Buf;
use font_types::Tag;

use crate::error::{WuffErr, bail, bail_if, bail_with_msg_if, usize_will_overflow};
use crate::table_tags::KNOWN_TABLE_TAGS;
use crate::variable_length::BufVariableExt;

pub const WOFF1_SIG: Tag = Tag::new(b"woFF");
pub const WOFF2_SIG: Tag = Tag::new(b"woF2");

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum WoffVersion {
    Woff1 = 1,
    Woff2 = 2,
}

pub struct Woff2 {
    pub header: WoffHeader,
    pub table_directory: Woff2TableDirectory,
    pub collection_directory: Option<CollectionDirectory>,
}

/// Accumulates data we may need to reconstruct a single font.
///
/// For a TTC, we store one per font in the collection.
/// For a single font we store exactly one of these in total.
#[derive(Clone, Default)]
pub(crate) struct WOFF2FontInfo {
    /// The total number of glyphs in the font
    pub num_glyphs: u16,
    /// The number of hmetrics (= number of proportional glyphs)
    /// (the number of monospace glyphs is `num_glphs - num_hmetrics`)
    pub num_hmetrics: u16,
    /// Index format of the "loca" table.
    /// Read from the header of WOFF's *transformed* "glyf" table
    ///
    /// See <https://learn.microsoft.com/en-us/typography/opentype/spec/loca>
    /// And <https://www.w3.org/TR/WOFF2/#glyf_table_format>
    pub index_format: u16,
    /// The minimum x coordinate for each glyph in the font.
    /// Read from the "glyf" table. Used to reconstruct the "hmtx" table.
    pub x_mins: Vec<i16>,
    /// Map of table tag to the byte offset of that table's entry in the table directory in the output file
    /// Allows the checksum, offset and length of the table to be written into the table directory once they are known.
    pub table_entry_by_tag: HashMap<Tag, usize>,
    /// Checksum of the ouput header
    pub header_checksum: u32,
}

/// WOFF header that can represent either a WOFF1 or WOFF2 header
///
/// <https://www.w3.org/TR/WOFF2/#woff20Header>
pub struct WoffHeader {
    // This isn't in the header, but we compute it from the "tag" and store it for convenience.
    pub woff_version: WoffVersion,
    /// b"woFF" or b"wOF2"
    pub signature: Tag,
    /// The "sfnt version" of the input font.
    pub flavor: Tag,
    /// Total size of the WOFF file.
    pub length: u32,
    /// Number of entries in directory of font tables.
    pub num_tables: u16,
    /// Reserved; set to 0.
    pub reserved: u16,
    /// Total size needed for the uncompressed font data, including the sfnt header, directory, and font tables (including padding).
    pub total_sfnt_size: u32,
    /// (WOFF2 only) Total length of the compressed data block.
    pub total_compressed_size: u32,
    /// Major version of the WOFF file.
    pub major_version: u16,
    /// Minor version of the WOFF file.
    pub minor_version: u16,
    /// Offset to metadata block, from beginning of WOFF file.
    pub meta_offset: u32,
    /// Length of compressed metadata block.
    pub meta_length: u32,
    /// Uncompressed size of metadata block.
    pub meta_orig_length: u32,
    /// Offset to private data block, from beginning of WOFF file.
    pub priv_offset: u32,
    /// Length of private data block.
    pub priv_length: u32,
}

impl WoffHeader {
    pub fn parse(input: &mut impl Buf) -> Result<Self, WuffErr> {
        let input_len = input.remaining();
        let input_len_u32 = input_len as u32;

        // Read signature, validate it, and determine WOFF version
        let signature = Tag::from_u32(input.try_get_u32()?);
        let woff_version = match signature.as_ref() {
            b"woFF" => WoffVersion::Woff1,
            b"woF2" => WoffVersion::Woff2,
            _ => return Err(WuffErr::GenericError),
        };

        // Parse other fields
        let header = Self {
            woff_version,
            signature,
            flavor: Tag::from_u32(input.try_get_u32()?),
            length: input.try_get_u32()?,
            num_tables: input.try_get_u16()?,
            reserved: input.try_get_u16()?,
            total_sfnt_size: input.try_get_u32()?,
            // totalCompressedSize field only exists in WOFF2 headers. We simply set it to zero for WOFF1.
            total_compressed_size: match woff_version {
                WoffVersion::Woff1 => 0,
                WoffVersion::Woff2 => input.try_get_u32()?,
            },
            major_version: input.try_get_u16()?,
            minor_version: input.try_get_u16()?,
            meta_offset: input.try_get_u32()?,
            meta_length: input.try_get_u32()?,
            meta_orig_length: input.try_get_u32()?,
            priv_offset: input.try_get_u32()?,
            priv_length: input.try_get_u32()?,
        };

        // Validate
        bail_if!(header.length != input_len_u32);
        bail_if!(header.num_tables == 0);
        bail_if!(header.reserved != 0);
        if header.meta_offset != 0 {
            bail_if!(
                header.meta_offset >= input_len_u32
                    || input_len_u32 - header.meta_offset < header.meta_length
            );
        }
        if header.priv_offset != 0 {
            bail_if!(
                header.priv_offset >= input_len_u32
                    || input_len_u32 - header.priv_offset < header.priv_length
            );
        }

        Ok(header)
    }

    pub fn is_collection(&self) -> bool {
        const TTC_COLLECTION_FLAVOR: Tag = Tag::new(b"ttcf");
        self.flavor == TTC_COLLECTION_FLAVOR
    }
}

pub struct TableDirectory<T> {
    pub tables: Vec<T>,
    /// Size of the table directory (in the WOFF) in bytes
    pub size: usize,
}
pub type Woff2TableDirectory = TableDirectory<Woff2TableDirectoryEntry>;

impl<T> Deref for TableDirectory<T> {
    type Target = Vec<T>;
    fn deref(&self) -> &Self::Target {
        &self.tables
    }
}
impl<T> DerefMut for TableDirectory<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.tables
    }
}

impl<T> TableDirectory<T> {
    /// Size of the table directory (in the WOFF) in bytes
    pub fn size(&self) -> usize {
        self.size
    }
}

impl Woff2TableDirectory {
    pub fn parse(input: &mut impl Buf, num_tables: usize) -> Result<Self, WuffErr> {
        let initial_remaining = input.remaining();

        // Tables in the CompressedFontData field of the WOFF are stored directly after each other
        // in the order they specified in the header. So we can determine the offset for each table
        // by adding up the lengths of each table (which are stored in the directory entries).
        //
        // <https://www.w3.org/TR/WOFF2/#table_format>
        let mut offset_in_woff: usize = 0;

        let mut tables = Vec::with_capacity(num_tables);
        for _ in 0..num_tables {
            let mut table = Woff2TableDirectoryEntry::parse(input)?;
            table.woff_offset = offset_in_woff as u32;

            // Check for for overflow
            bail_if!(usize_will_overflow(
                offset_in_woff,
                table.woff_length as usize
            ));

            // Add the length of the table to offset_in_woff to determine the offset of the next table
            offset_in_woff += table.woff_length as usize;

            tables.push(table);
        }

        // Because the table directory is variable length, we compute it's size (in bytes) by tracking how
        // much data we have processed during processing. This allows us to know the offset that the next
        // section of the file begins at.
        let size_of_directory = input.remaining() - initial_remaining;

        Ok(Self {
            tables,
            size: size_of_directory,
        })
    }

    pub fn sort_tables(&mut self) {
        self.tables.sort_by_key(|table| table.tag);
    }
}

/// <https://www.w3.org/TR/WOFF2/#table_dir_format>
pub struct Woff2TableDirectoryEntry {
    /// 4-byte tag (optional)
    pub tag: Tag,
    /// 2 bits representing the format of the table
    pub format: u8,
    /// Length of original table. This may be innacurate in the case of transformed tables.
    pub orig_length: u32, // uBase128,
    /// Offset of the table within the (decompressed) CompressedFontData field of the WOFF
    pub woff_offset: u32, // Computed
    /// Length of the table within the (decompressed) CompressedFontData field of the WOFF
    pub woff_length: u32, // uBase128,
}

impl Woff2TableDirectoryEntry {
    /// Whether the table has been transformed
    ///
    /// For all tables in a font, except for 'glyf' and 'loca' tables, transformation version 0 indicates the null transform
    /// where the original table data is passed directly to the Brotli compressor for inclusion in the compressed data stream.
    /// For 'glyf' and 'loca' tables, transformation version 3 indicates the null transform where the original table data was
    /// passed directly to the Brotli compressor without applying any pre-processing defined in subclause 5.1 and subclause 5.3.
    pub fn is_transformed(&self) -> bool {
        match self.tag.as_ref() {
            // For the glyf and loca tables, format 0 indicates transformed
            b"glyf" | b"loca" => self.format == 0,
            // For all other tables, format 0 indicates untransformed
            _ => self.format == 0,
        }
    }

    pub fn data_as_slice<'a>(&self, data: &'a [u8]) -> Result<&'a [u8], WuffErr> {
        let end = self.woff_offset as usize + self.woff_length as usize;
        data.get((self.woff_offset as usize)..end)
            .ok_or(WuffErr::GenericError)
    }
}

impl Woff2TableDirectoryEntry {
    pub fn parse(input: &mut impl Buf) -> Result<Self, WuffErr> {
        let flags = input.try_get_u8()?;
        let (tag, format) = Self::parse_flags(flags);

        let entry = Self {
            // Note: we only parse the tag field from the input if it is not contained within the flags
            tag: match tag {
                Some(tag) => tag,
                None => Tag::from_u32(input.try_get_u32()?),
            },
            format,
            orig_length: input.try_get_variable_128_u32()?,
            woff_offset: 0, // Set in TableDirectory parse function
            woff_length: input.try_get_variable_128_u32()?,
        };

        // Validate
        bail_if!(entry.tag.as_ref() == b"loca" && entry.woff_length != 0);

        Ok(entry)
    }

    /// Parse flags field into "known tag" and "format"
    ///
    /// The interpretation of the flags field is as follows. Bits [0..5] contain an index to the "known tag" table,
    /// which represents tags likely to appear in fonts. If the tag is not present in this table, then the value of
    /// this bit field is 63. Bits 6 and 7 indicate the preprocessing transformation version number (0-3) that was
    /// applied to each table.
    pub fn parse_flags(flags: u8) -> (Option<Tag>, u8) {
        const TAG_MASK: u8 = 0b00111111;
        const FORMAT_MASK: u8 = 0b11000000;
        let tag_bits = flags & TAG_MASK;
        let format = (flags & FORMAT_MASK) >> 6;
        let tag = KNOWN_TABLE_TAGS.get(tag_bits as usize).copied();
        (tag, format)
    }
}

/// <https://www.w3.org/TR/WOFF2/#collection_dir_format>
pub struct CollectionDirectory {
    /// The Version of the TTC Header in the original font.
    pub version: u32,
    /// Number of fonts in the file
    pub fonts: Vec<CollectionDirectoryEntry>,
}

impl CollectionDirectory {
    pub fn parse(
        input: &mut impl Buf,
        table_directory: &Woff2TableDirectory,
    ) -> Result<Self, WuffErr> {
        let version = input.try_get_u32()?;
        let num_fonts = input.try_get_variable_255_u16()?;

        bail_if!(version != 0x00010000 && version != 0x00020000);
        bail_if!(num_fonts == 0);

        let mut fonts = Vec::with_capacity(num_fonts as usize);
        for _ in 0..num_fonts {
            fonts.push(CollectionDirectoryEntry::parse(input, table_directory)?);
        }

        Ok(Self { version, fonts })
    }

    /// Generate a fake `CollectionDirectory` for a single font so that we can share
    /// serialization logic between collection and single fonts.
    pub fn generate_for_single_font(flavor: Tag, table_directory: &Woff2TableDirectory) -> Self {
        let table_indices: Vec<u16> = (0..(table_directory.len() as u16)).collect();
        let mut head_idx: Option<u16> = None;
        let mut hhea_idx: Option<u16> = None;
        let mut glyf_idx: Option<u16> = None;
        let mut loca_idx: Option<u16> = None;
        for (table_index, table) in table_directory.tables.iter().enumerate() {
            match table.tag.as_ref() {
                b"head" => head_idx = Some(table_index as u16),
                b"hhea" => hhea_idx = Some(table_index as u16),
                b"glyf" => glyf_idx = Some(table_index as u16),
                b"loca" => loca_idx = Some(table_index as u16),
                _ => { /* do nothing */ }
            }
        }
        Self {
            version: 0x00010000, // Hardcode: will be ignored
            fonts: vec![CollectionDirectoryEntry {
                flavor,
                table_indices,
                head_idx,
                hhea_idx,
                glyf_idx,
                loca_idx,
            }],
        }
    }

    pub fn sort_tables_within_each_font(&mut self, tables: &Woff2TableDirectory) {
        for font in &mut self.fonts {
            font.table_indices
                .sort_by_cached_key(|idx| tables[*idx as usize].tag);
        }
    }

    /// Size of the collection header. 0 if version indicates this isn't a
    /// collection. Ref http://www.microsoft.com/typography/otspec/otff.htm,
    /// True Type Collections
    pub(crate) fn required_size(&self) -> usize {
        let mut size: usize = 0;
        if self.version == 0x00020000 {
            size += 12; // ulDsig{Tag,Length,Offset}
        }
        if self.version == 0x00010000 || self.version == 0x00020000 {
            size += 12   // TTCTag, Version, numFonts
          + 4 * (self.fonts.len()); // OffsetTable[numFonts]
        }
        size
    }

    /// Size of the collection header. 0 if version indicates this isn't a
    /// collection. Ref http://www.microsoft.com/typography/otspec/otff.htm,
    /// True Type Collections
    pub(crate) fn collection_header_required_size(&self) -> usize {
        let mut size: usize = 0;
        if self.version == 0x00020000 {
            size += 12; // ulDsig{Tag,Length,Offset}
        }
        if self.version == 0x00010000 || self.version == 0x00020000 {
            size += 12   // TTCTag, Version, numFonts
          + 4 * (self.fonts.len()); // OffsetTable[numFonts]
        }
        size
    }

    pub(crate) fn table_directories_required_size(&self) -> usize {
        pub const TABLE_DIRECTORY_HEADER_SIZE: usize = 12;
        pub const TABLE_DIRECTORY_ENTRY_SIZE: usize = 16;
        self.collection_header_required_size()
            + (TABLE_DIRECTORY_HEADER_SIZE * self.fonts.len())
            + self
                .fonts
                .iter()
                .map(|font| TABLE_DIRECTORY_ENTRY_SIZE * font.table_indices.len())
                .sum::<usize>()
    }
}

/// <https://www.w3.org/TR/WOFF2/#collection_dir_format>
pub struct CollectionDirectoryEntry {
    /// The "sfnt version" of the font
    pub flavor: Tag,
    /// In a TTC file, each font reference some subset of the tables in the file.
    /// This field records which tables this particular font references.
    pub table_indices: Vec<u16>, //255UInt16

    // Check the indices of specific tables that we want random access to
    pub head_idx: Option<u16>,
    pub hhea_idx: Option<u16>,
    pub glyf_idx: Option<u16>,
    pub loca_idx: Option<u16>,
}

impl CollectionDirectoryEntry {
    pub fn parse(input: &mut impl Buf, tables: &Woff2TableDirectory) -> Result<Self, WuffErr> {
        let num_tables = input.try_get_variable_255_u16()?;
        let flavor = Tag::from_u32(input.try_get_u32()?);

        bail_if!(num_tables == 0);

        let mut head_idx: Option<u16> = None;
        let mut hhea_idx: Option<u16> = None;
        let mut glyf_idx: Option<u16> = None;
        let mut loca_idx: Option<u16> = None;
        let mut table_indices = Vec::with_capacity(num_tables as usize);
        for _ in 0..num_tables {
            let table_index = input.try_get_variable_255_u16()?;
            bail_if!(table_index as usize > tables.len());

            match tables[table_index as usize].tag.as_ref() {
                b"head" => head_idx = Some(table_index),
                b"hhea" => hhea_idx = Some(table_index),
                b"glyf" => glyf_idx = Some(table_index),
                b"loca" => loca_idx = Some(table_index),
                _ => { /* do nothing */ }
            }

            table_indices.push(table_index);
        }

        // If we have both glyf and loca make sure they are consecutive
        // Reject if we only have one
        match (glyf_idx, loca_idx) {
            (Some(glyf_idx), Some(loca_idx)) => {
                bail_with_msg_if!(
                    glyf_idx > loca_idx || loca_idx - glyf_idx != 1,
                    "TTC font {i} has non-consecutive glyf/loca"
                );
            }
            (Some(_), None) | (None, Some(_)) => bail!(),
            (None, None) => {}
        };

        Ok(Self {
            flavor,
            table_indices,
            head_idx,
            hhea_idx,
            glyf_idx,
            loca_idx,
        })
    }

    pub fn num_tables(&self) -> usize {
        self.table_indices.len()
    }

    /// The size required for a table directory for this font
    pub fn table_directory_size(&self) -> usize {
        12 + (16 * self.num_tables())
    }
}
