use brotli_decompressor::{BrotliResult, brotli_decode};
use bytes::BufMut;

use crate::{
    error::{WuffErr, bail, bail_if, bail_with_msg_if},
    types::{CollectionDirectory, Woff2, Woff2TableDirectory, WoffHeader, WoffVersion},
};

// Over 14k test fonts the max compression ratio seen to date was ~20.
// >100 suggests you wrote a bad uncompressed size.
const K_MAX_PLAUSIBLE_COMPRESSION_RATIO: f32 = 100.0;

pub fn decompress_woff2(raw_woff_data: &[u8], out: &mut impl BufMut) -> Result<(), WuffErr> {
    // Here we create a new view over the `raw_woff_data`. Because we pass `&mut input` to parsing functons,
    // they will actually mutate the slice (not the data it points to) such that it only includes unparsed data.
    //
    // However `raw_woff_data` will still contain the full data for the WOFF.
    let mut input = raw_woff_data;

    // Parse header, table directory and collection directory
    let header = WoffHeader::parse(&mut input)?;
    bail_if!(header.woff_version != WoffVersion::Woff2);

    let table_directory = Woff2TableDirectory::parse(&mut input, header.num_tables as usize)?;
    let mut collection_directory = if header.is_collection() {
        CollectionDirectory::parse(&mut input, &table_directory)?
    } else {
        CollectionDirectory::generate_for_single_font(header.flavor, &table_directory)
    };

    // Re-order tables in output (OTSpec) order
    collection_directory.sort_tables_within_each_font(&table_directory);

    // Compute compression ratio
    let compression_ratio: f32 = (header.total_sfnt_size as f32) / (raw_woff_data.len() as f32);

    // Validate header (and compression ratio)
    bail_if!(header.total_sfnt_size < 1);
    bail_with_msg_if!(
        compression_ratio > K_MAX_PLAUSIBLE_COMPRESSION_RATIO,
        "Implausible compression ratio %.01f",
        compression_ratio
    );

    let compressed_data = &input[0..(header.total_compressed_size as usize)];
    let mut uncompressed_data: Vec<u8> = Vec::with_capacity(header.total_sfnt_size as usize);
    let info = brotli_decode(compressed_data, &mut uncompressed_data); // CHECK: why is output buffer fixed size? Is there a better way to decode?
    bail_if!(!matches!(info.result, BrotliResult::ResultSuccess));
    bail_if!(info.decoded_size != uncompressed_data.len());

    // let mut metadata = RebuildMetadata::default();
    // if !WriteHeaders(data, &mut metadata, &mut hdr, out) {
    //     return FONT_COMPRESSION_FAILURE();
    // }

    // for i in 0..metadata.font_infos.len() {
    //     if PREDICT_FALSE(!ReconstructFont(
    //         &mut uncompressed_buf,
    //         &mut metadata,
    //         &hdr,
    //         i,
    //         out,
    //     )) {
    //         return FONT_COMPRESSION_FAILURE();
    //     }
    // }

    Ok(())
}

// fn generate_header(header: &WoffHeader) -> (Vec<u8>, Vec<FontInfo>) {

// }
