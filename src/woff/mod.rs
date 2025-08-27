pub(crate) mod glyf_decoder;
pub(crate) mod hmtx_decoder;

pub struct TableMetadata {
    length: usize,
    checksum: u32,
}
