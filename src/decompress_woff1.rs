use crate::{
    HEAD, Round4, compute_checksum,
    error::{WuffErr, bail_if},
    woff::headers::{TableDirectory, TableDirectoryEntry, WoffHeader, WoffVersion},
    write_table_directory_header,
};
use bytes::BufMut as _;
use std::error::Error;

#[cfg(feature = "z")]
fn decompress_z(compressed_data: &[u8], size_hint: usize) -> Result<Vec<u8>, Box<dyn Error>> {
    use flate2::{Decompress, FlushDecompress};
    let mut output: Vec<u8> = Vec::with_capacity(size_hint);
    let mut decompressor = Decompress::new(true);
    decompressor.decompress_vec(compressed_data, &mut output, FlushDecompress::Finish)?;
    Ok(output)
}

#[cfg(feature = "z")]
/// Decompress a WOFF1 file using the built-in gzip decompressor
pub fn decompress_woff1(raw_woff_data: &[u8]) -> Result<Vec<u8>, WuffErr> {
    decompress_woff1_with_custom_z(raw_woff_data, &mut decompress_z)
}

#[allow(clippy::type_complexity)]
/// Decompress a WOFF1 file using a custom gzip decompressor passed as a closure
pub fn decompress_woff1_with_custom_z(
    raw_woff_data: &[u8],
    decompress_z: &mut dyn FnMut(&[u8], usize) -> Result<Vec<u8>, Box<dyn Error>>,
) -> Result<Vec<u8>, WuffErr> {
    // Here we create a new view over the `raw_woff_data`. Because we pass `&mut input` to parsing functons,
    // they will actually mutate the slice (not the data it points to) such that it only includes unparsed data.
    //
    // However `raw_woff_data` will still contain the full data for the WOFF.
    let mut input = raw_woff_data;

    // Parse header and table directory
    let header = WoffHeader::parse(&mut input)?;
    bail_if!(header.woff_version != WoffVersion::Woff1);
    let mut table_directory = TableDirectory::parse_woff1(&mut input, header.num_tables as usize)?;

    table_directory.tables.sort_by_key(|t| t.tag);

    // Create output buffer
    let mut out: Vec<u8> = Vec::with_capacity(0 /* TODO */);
    let mut checksum: u32 = 0;

    // Write table directory header
    write_table_directory_header(&mut out, header.flavor, table_directory.len() as u16);
    checksum = checksum.wrapping_add(compute_checksum(&out));

    // Reserve space for the rest of the table directory
    let table_directory_size = table_directory.len() * 16;
    let table_directory_start = out.len();
    out.resize(out.len() + table_directory_size, 0);

    // Sort tables by offset, while keeping track of their order by tag
    // Table directory entries are stored in tag order
    // Tables themselves are stored in woff_offset order
    struct TableWithTagIdx<'a> {
        table: &'a TableDirectoryEntry,
        tag_index: usize,
    }
    let mut tables_by_offset: Vec<TableWithTagIdx> = table_directory
        .iter()
        .enumerate()
        .map(|(tag_index, table)| TableWithTagIdx { table, tag_index })
        .collect();
    tables_by_offset.sort_by_key(|t| t.table.woff_offset);

    // let mut head_table_offset = None;
    for TableWithTagIdx { table, tag_index } in tables_by_offset.into_iter() {
        let table_offset = out.len();
        let table_end = table_offset + (table.orig_length as usize);

        // Store HEAD offset for later in order to write checksum
        // if table.tag == HEAD {
        //     head_table_offset = Some(table_offset);
        // }

        // Write table directory entry for table
        let dir_entry_start = table_directory_start + (tag_index * 16);
        let dir_entry_end = dir_entry_start + 16;
        let mut dir_entry_writer = &mut out[dir_entry_start..dir_entry_end];
        dir_entry_writer.put_u32(u32::from_be_bytes(table.tag.to_be_bytes()));
        dir_entry_writer.put_u32(table.orig_checksum);
        dir_entry_writer.put_u32(table_offset as u32);
        dir_entry_writer.put_u32(table.orig_length);

        // Write table data
        let is_compressed = table.woff_length < table.orig_length;
        if is_compressed {
            let compressed_data = table.data_as_slice(raw_woff_data)?;
            let decompressed_data = decompress_z(compressed_data, table.orig_length as usize)
                .map_err(|_| WuffErr::GenericError)?;
            bail_if!(decompressed_data.len() != table.orig_length as usize);
            out.extend_from_slice(&decompressed_data);
        } else {
            out.extend_from_slice(table.data_as_slice(raw_woff_data)?);
        };

        // Pad output to 4 bytes
        out.resize(Round4!(out.len()), 0);

        // Update checksum
        checksum = checksum.wrapping_add(compute_checksum(&out[dir_entry_start..dir_entry_end]));
        checksum = checksum.wrapping_add(compute_checksum(&out[table_offset..table_end]));
    }

    // TODO: Checksum adjustment

    Ok(out)
}
