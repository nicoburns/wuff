mod glyf_decoder;
mod hmtx_decoder;

pub struct TableMetadata {
    length: usize,
    checksum: u32,
}
