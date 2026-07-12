use alloc::{boxed::Box, vec, vec::Vec};
use core::error::Error;

use crate::Tag;
use bytes::{Buf as _, BufMut};

use crate::{
    GLYF, HEAD, HMTX, LOCA, Round4, compute_checksum,
    error::{WuffErr, bail, bail_if, bail_with_msg_if},
    woff::{
        glyf_decoder::tranform_glyf_table,
        headers::{
            CollectionDirectory, CollectionDirectoryEntry, TableDirectory, TableDirectoryEntry,
            WOFF2FontInfo, WoffHeader, WoffVersion,
        },
        hmtx_decoder::{decode_hmtx_table, generate_hmtx_table},
    },
    write_table_directory_header,
};

// Over 14k test fonts the max compression ratio seen to date was ~20.
// >100 suggests you wrote a bad uncompressed size.
const K_MAX_PLAUSIBLE_COMPRESSION_RATIO: f32 = 100.0;

#[allow(clippy::type_complexity)]
/// Decompress a WOFF2 file using a custom brotli decompressor passed as a closure
pub fn decompress_woff2_with_custom_brotli(
    raw_woff_data: &[u8],
    decompress_brotli: &mut dyn FnMut(&[u8], usize) -> Result<Vec<u8>, Box<dyn Error>>,
) -> Result<Vec<u8>, WuffErr> {
    // Here we create a new view over the `raw_woff_data`. Because we pass `&mut input` to parsing functons,
    // they will actually mutate the slice (not the data it points to) such that it only includes unparsed data.
    //
    // However `raw_woff_data` will still contain the full data for the WOFF.
    let mut input = raw_woff_data;
    let full_input_len = input.len();

    // Parse header, table directory and collection directory
    let header = WoffHeader::parse(&mut input)?;
    bail_if!(header.woff_version != WoffVersion::Woff2);

    let table_directory = TableDirectory::parse_woff2(&mut input, header.num_tables as usize)?;
    let mut collection_directory = if header.is_collection() {
        CollectionDirectory::parse(&mut input, &table_directory)?
    } else {
        CollectionDirectory::generate_for_single_font(header.flavor, &table_directory)
    };

    // Validate header (blocks do not overlap, and have at most 3 bytes padding between them)

    let compressed_offset = full_input_len - input.len();
    bail_if!(compressed_offset > u32::MAX as usize);

    let mut src_offset = Round4!(compressed_offset + header.total_compressed_size as usize);
    bail_if!(src_offset > full_input_len);

    if header.meta_offset != 0 {
        bail_if!(src_offset != header.meta_offset as usize);
        src_offset = Round4!(header.meta_offset as usize + header.meta_length as usize);
        bail_if!(src_offset > u32::MAX as usize);
    }

    if header.priv_offset != 0 {
        bail_if!(src_offset != header.priv_offset as usize);
        src_offset = Round4!(header.priv_offset as usize + header.priv_length as usize);
        bail_if!(src_offset > u32::MAX as usize);
    }

    bail_if!(src_offset != Round4!(full_input_len));

    // Re-order tables in output (OTSpec) order
    collection_directory.sort_tables_within_each_font(&table_directory);
    let num_fonts = collection_directory.fonts.len();

    // Compute compression ratio using the trusted, table-directory-derived uncompressed size
    // (not the untrusted `totalSfntSize` from the file header). Perform the plausibility check
    // BEFORE decompressing so an implausible size never drives allocation/decompression.
    let compression_ratio: f32 =
        (table_directory.uncompressed_size as f32) / (raw_woff_data.len() as f32);

    // Validate header (and compression ratio)
    bail_if!(header.total_sfnt_size < 1);
    bail_with_msg_if!(
        compression_ratio > K_MAX_PLAUSIBLE_COMPRESSION_RATIO,
        "Implausible compression ratio {:.1}",
        compression_ratio
    );

    // Decompress data with brotli decoder. We pass the trusted `uncompressed_size` as the hard
    // upper bound on the size of the decompressed data.
    let compressed_data = &input[0..(header.total_compressed_size as usize)];
    let decompressed_data = decompress_brotli(compressed_data, table_directory.uncompressed_size)
        .map_err(|_| WuffErr::GenericError)?;

    // The decompressed data block must be exactly the size of the tables it contains
    // (tables are stored consecutively with no padding or extraneous data).
    // <https://www.w3.org/TR/WOFF2/#conform-mustRejectExtraData>
    bail_if!(decompressed_data.len() != table_directory.uncompressed_size);

    let mut out: Vec<u8> = Vec::with_capacity(table_directory.uncompressed_size);

    let mut out_header = generate_header(&header, &table_directory, &collection_directory);
    out.extend_from_slice(&out_header.data);

    // Metadata for tables that have been written. Index corresponds to the table's index within the tables Vec
    let mut table_metadata: Vec<Option<TableMetadata>> = vec![None; header.num_tables as usize];
    for i in 0..num_fonts {
        reconstruct_font(
            &decompressed_data,
            &header,
            &table_directory,
            &collection_directory.fonts[i],
            &mut out_header,
            &mut table_metadata,
            &mut out,
            i,
        )?;
    }

    // Update header
    out[0..out_header.data.len()].copy_from_slice(&out_header.data);

    Ok(out)
}

fn iter_tables_for_font<'a>(
    font_entry: &'a CollectionDirectoryEntry,
    tables: &'a TableDirectory,
) -> impl Iterator<Item = (usize, &'a TableDirectoryEntry)> {
    font_entry
        .table_indices
        .iter()
        .map(|table_idx| (*table_idx as usize, &tables[*table_idx as usize]))
}

// Offset tables assumed to have been written in with 0's initially.
// WOFF2Header isn't const so we can use [] instead of at() (which upsets FF)
#[allow(clippy::too_many_arguments)]
fn reconstruct_font(
    woff_data: &[u8],
    header: &WoffHeader,
    tables: &TableDirectory,
    font_entry: &CollectionDirectoryEntry,
    out_header: &mut HeaderData,
    table_metadata: &mut [Option<TableMetadata>],
    out: &mut Vec<u8>,
    font_idx: usize,
) -> Result<(), WuffErr> {
    let glyf_idx = font_entry.glyf_idx.map(|idx| idx as usize);
    let loca_idx = font_entry.loca_idx.map(|idx| idx as usize);
    let hhea_idx = font_entry.hhea_idx.map(|idx| idx as usize);

    // Check the glyf and loca tables are compatible with each other
    // 'glyf' without 'loca' doesn't make sense
    match (glyf_idx, loca_idx) {
        (Some(glyf_idx), Some(loca_idx)) => {
            bail_with_msg_if!(
                tables[glyf_idx].is_transformed() != tables[loca_idx].is_transformed(),
                "Cannot transform just one of glyf/loca"
            );
        }
        (Some(_), None) | (None, Some(_)) => {
            bail_with_msg_if!(true, "Cannot have just one of glyf/loca")
        }
        (None, None) => {}
    }

    let mut font_checksum: u32 = if header.is_collection() {
        out_header.font_infos[font_idx].header_checksum
    } else {
        out_header.checksum
    };

    // Read and store "num_hmetrics" from "hhea" table and then used to reconstruct "hmtx"
    let num_hmetrics = match hhea_idx {
        Some(hhea_idx) => {
            let hhea_table = &tables[hhea_idx];
            Some(read_num_hmetrics(hhea_table.data_as_slice(woff_data)?)?)
        }
        None => None,
    };

    // These are read from "glyf" and then used to reconstruct "hmtx"
    let mut num_glyphs = None;
    let mut x_mins = None;

    // Iterate over the tables for this font.
    // Note: tables within each font (what we are iterating over here) have already been sorted in alphabetical table tag order.
    for (table_idx, table) in iter_tables_for_font(font_entry, tables) {
        // TODO(user) a collection with optimized hmtx that reused glyf/loca
        // would fail. We don't optimize hmtx for collections yet.
        bail_if!(table.woff_offset as usize + table.woff_length as usize > woff_data.len());

        // Check to see if we have already processed and saved metadata for this table.
        // If we have then
        // There are two cases when this occurs:
        //   - When a table is reused between fonts in a collection (and this table has already been processed for an earlier font)
        //   - For the "loca" table. This table gets processed as part of processing "glyf"
        let metadata = if let Some(metadata) = table_metadata[table_idx] {
            // Tables shouldn't be reused within a single font (they should be reused between different
            // fonts in a collection). So if we encounter a table we have already computed metadata for in the first
            // font unless the table is a "loca" table because we compute metadata for this table when processing the "glyf"
            // table (so for "loca" encountering already-computed metadata doesn't necessarily indicate reuse).
            bail_if!(font_idx == 0 && table.tag != LOCA);

            metadata
        }
        // Any table which does not need to be transformed
        else if !table.is_transformed() {
            let check_sum_adjustment = if table.tag == HEAD {
                bail_if!(table.woff_length < 12);
                let checksum_slice =
                    &woff_data[(table.woff_offset as usize + 8)..(table.woff_offset as usize + 12)];
                let checksum_bytes: [u8; 4] = checksum_slice.try_into().unwrap();
                u32::from_be_bytes(checksum_bytes)
            } else {
                0
            };

            let table_data = table.data_as_slice(woff_data)?;
            let checksum = compute_checksum(table_data).wrapping_sub(check_sum_adjustment);

            let metadata = TableMetadata {
                dst_offset: out.len() as u32,
                dst_length: table.woff_length,
                checksum,
            };
            table_metadata[table_idx] = Some(metadata);

            out.extend_from_slice(table_data);
            out.resize(Round4!(out.len()), 0);

            metadata
        }
        // glyf table (also process loca table)
        else if table.tag == GLYF {
            let loca_idx =
                loca_idx.expect("We already returned an error if glyf is present but loca isn't");

            // Generate transformed glyf and loca tables
            let raw_glyf_table_data = table.data_as_slice(woff_data)?;
            let glyf_and_loca_data = tranform_glyf_table(raw_glyf_table_data)?;

            // The origLength of the loca table declared in the table directory must exactly
            // match the size of the reconstructed loca table.
            // <https://www.w3.org/TR/WOFF2/#conform-mustRejectLoca>
            bail_with_msg_if!(
                tables[loca_idx].orig_length as usize != glyf_and_loca_data.loca_table.len(),
                "loca table origLength does not match reconstructed loca size"
            );

            // Store num_glyphs and x_mins
            num_glyphs = Some(glyf_and_loca_data.num_glyphs);
            x_mins = Some(glyf_and_loca_data.x_mins);

            // Write glyf table
            let glyf_dest_offset = out.len();
            out.extend_from_slice(&glyf_and_loca_data.glyf_table);
            out.resize(Round4!(out.len()), 0);
            let glyf_metadata = TableMetadata {
                checksum: glyf_and_loca_data.glyf_checksum,
                dst_offset: glyf_dest_offset as u32,
                dst_length: glyf_and_loca_data.glyf_table.len() as u32,
            };
            table_metadata[table_idx] = Some(glyf_metadata);

            // Write loca table
            let loca_dest_offset = out.len();
            out.extend_from_slice(&glyf_and_loca_data.loca_table);
            out.resize(Round4!(out.len()), 0);
            let loca_metdata = TableMetadata {
                checksum: glyf_and_loca_data.loca_checksum,
                dst_offset: loca_dest_offset as u32,
                dst_length: glyf_and_loca_data.loca_table.len() as u32,
            };
            table_metadata[loca_idx] = Some(loca_metdata);

            // Return glyf metadata
            glyf_metadata
        } else if table.tag == LOCA {
            unreachable!("loca table is computed when glyf table is processed");
        }
        // hmtx table
        else if table.tag == HMTX {
            // Tables are sorted so all the info we need has been gathered.
            // TODO: better error_handling
            let num_glyphs = num_glyphs.ok_or(WuffErr::GenericError)?;
            let num_hmetrics = num_hmetrics.ok_or(WuffErr::GenericError)?;
            let x_mins = x_mins.as_ref().ok_or(WuffErr::GenericError)?;

            // Generate reconstructed hmtx table
            let mut raw_hmtx_table_data = table.data_as_slice(woff_data)?;
            let hmtx_data =
                decode_hmtx_table(&mut raw_hmtx_table_data, num_glyphs, num_hmetrics, x_mins)?;
            let hmtx_table = generate_hmtx_table(&hmtx_data)?;
            let checksum = compute_checksum(&hmtx_table);

            // Write table to output buffer
            let dest_offset = out.len();
            out.extend_from_slice(&hmtx_table);
            out.resize(Round4!(out.len()), 0);
            // Note: like the reference implementation, we record the origLength declared in
            // the WOFF2 table directory (rather than the size of the reconstructed table)
            // in the output table directory entry. The two may legitimately differ.
            let hmtx_metadata = TableMetadata {
                checksum,
                dst_offset: dest_offset as u32,
                dst_length: table.orig_length,
            };
            table_metadata[table_idx] = Some(hmtx_metadata);

            hmtx_metadata
        } else {
            bail!()
        };

        // Update font checksum with the checksum for the table
        font_checksum = font_checksum.wrapping_add(metadata.checksum);

        // update the table entry with real values. We replaced 0's, so update  checksum.
        out_header.update_table_entry(font_idx, table.tag, metadata);
        font_checksum = font_checksum.wrapping_add(metadata.header_checksum_contribution());

        // The table (as recorded in the output table directory) must not extend past the end
        // of the data written (including padding) so far.
        bail_if!(metadata.dst_offset as u64 + metadata.dst_length as u64 > out.len() as u64);
    }

    // Update 'head' checkSumAdjustment. We already set it to 0 and summed font.
    //
    // The 'head' table is a special case in checksum calculations, as it includes a checksumAdjustment field
    // that is calculated and written after the table’s checksum is calculated and written into the table directory entry,
    // necessarily invalidating that checksum value.
    //
    // When generating font data, to calculate and write the 'head' table checksum and checksumAdjustment field, do the following:
    //
    //   1. Set the checksumAdjustment field to 0.
    //   2. Calculate the checksum for all tables including the 'head' table and enter the value
    //      for each table into the corresponding record in the table directory.
    //   3. Calculate the checksum for the entire font.
    //   4. Subtract that value from 0xB1B0AFBA.
    //   5. Store the result in the 'head' table checksumAdjustment field.
    //
    // <https://learn.microsoft.com/en-us/typography/opentype/spec/otff#calculating-checksums>
    let checksum_adjustment = 0xB1B0AFBA_u32.wrapping_sub(font_checksum);
    if let Some(head_table_idx) = font_entry.head_idx {
        let head_table_metadata = &table_metadata[head_table_idx as usize]
            .expect("Every table in the font should have metadata at this point");
        let mut writer = &mut out[head_table_metadata.dst_offset as usize + 8..];
        writer.put_u32(checksum_adjustment);
    }

    Ok(())
}

// Get numberOfHMetrics, https://www.microsoft.com/typography/otspec/hhea.htm
fn read_num_hmetrics(mut hhea_data: &[u8]) -> Result<u16, WuffErr> {
    bail_if!(hhea_data.remaining() < 34);
    hhea_data.advance(34); // Skip 34 to reach 'hhea' numberOfHMetrics
    Ok(hhea_data.try_get_u16()?)
}

struct HeaderData {
    data: Vec<u8>,
    checksum: u32,
    font_infos: Vec<WOFF2FontInfo>,
}

#[derive(Clone, Copy, Default)]
struct TableMetadata {
    checksum: u32,
    dst_offset: u32,
    dst_length: u32,
}

impl TableMetadata {
    pub fn is_already_computed(&self) -> bool {
        self.dst_offset != 0
    }

    pub fn header_checksum_contribution(&self) -> u32 {
        self.checksum
            .wrapping_add(self.dst_offset)
            .wrapping_add(self.dst_length)
    }
}

impl HeaderData {
    /// Update the table entry with real values.
    fn update_table_entry(&mut self, font_idx: usize, tag: Tag, metadata: TableMetadata) {
        // Write data
        let table_entry_offset = self.font_infos[font_idx].table_entry_by_tag[&tag];

        let mut out = &mut self.data[(table_entry_offset + 4)..(table_entry_offset + 16)];
        out.put_u32(metadata.checksum);
        out.put_u32(metadata.dst_offset);
        out.put_u32(metadata.dst_length);

        // Update checksum
        let mut checksum = self.font_infos[font_idx].header_checksum;
        checksum = checksum.wrapping_add(metadata.checksum);
        checksum = checksum.wrapping_add(metadata.dst_offset);
        checksum = checksum.wrapping_add(metadata.dst_length);
        self.font_infos[font_idx].header_checksum = checksum;
    }
}

// First table goes after all the headers, table directory, etc
fn compute_header_size(collection_directory: &CollectionDirectory, is_collection: bool) -> usize {
    if is_collection {
        collection_directory.table_directories_required_size()
            + collection_directory.collection_header_required_size()
    } else {
        collection_directory.table_directories_required_size()
    }
}

fn generate_header(
    header: &WoffHeader,
    tables: &TableDirectory,
    collection_directory: &CollectionDirectory,
) -> HeaderData {
    let num_fonts = collection_directory.fonts.len();
    let size_of_header = compute_header_size(collection_directory, header.is_collection());
    let mut output: Vec<u8> = Vec::with_capacity(size_of_header);
    let mut font_infos: Vec<WOFF2FontInfo> = vec![WOFF2FontInfo::default(); num_fonts];

    let mut checksum: u32 = 0;

    // If TTC: write TTC header
    if header.is_collection() {
        // TTC header
        output.put_u32(u32::from_be_bytes(header.flavor.to_be_bytes())); // TAG TTCTag
        output.put_u32(collection_directory.version); // FIXED Version
        output.put_u32(num_fonts as u32); // ULONG numFonts

        // let mut offset_table_idx: usize = output.len(); // keep start of offset table for later

        // Write tableDirectoryOffsets
        let first_table_directory_offset = match collection_directory.version {
            0x00010000 => 12 + (4 * num_fonts as u32),
            0x00020000 => 12 + 12 + (4 * num_fonts as u32),
            _ => unreachable!("Only 1.0 and 2.0 are supported versions"),
        };
        let mut table_directory_offset = first_table_directory_offset;
        for font in collection_directory.fonts.iter() {
            output.put_u32(table_directory_offset);
            table_directory_offset += font.table_directory_size() as u32;
        }

        // space for DSIG fields for header v2
        if collection_directory.version == 0x00020000 {
            output.put_u32(0); // ULONG ulDsigTag
            output.put_u32(0); // ULONG ulDsigLength
            output.put_u32(0); // ULONG ulDsigOffset
        }

        checksum = checksum.wrapping_add(compute_checksum(&output));
    }

    // Write table directory(s)
    // If file is a TTC: one per font. Else for a single font: one in total.
    for (font, info) in collection_directory.fonts.iter().zip(font_infos.iter_mut()) {
        // write the actual offset table so our header doesn't lie
        // font.dst_offset = offset as u32;
        let start_offset = output.len();
        write_table_directory_header(&mut output, font.flavor, font.table_indices.len() as u16);

        for &table_index in &font.table_indices {
            let tag = tables[table_index as usize].tag;
            info.table_entry_by_tag.insert(tag, output.len());
            write_empty_offset_table_entry(&mut output, tag);
        }

        info.header_checksum = compute_checksum(&output[start_offset..]);
        checksum = checksum.wrapping_add(info.header_checksum);
    }

    HeaderData {
        data: output,
        font_infos,
        checksum,
    }
}

// Writes a single Offset Table entry
fn write_empty_offset_table_entry(output: &mut impl BufMut, tag: Tag) {
    output.put_u32(u32::from_be_bytes(tag.to_be_bytes()));
    output.put_u32(0);
    output.put_u32(0);
    output.put_u32(0);
}
