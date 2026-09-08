//! Brotli decompression via Google's C implementation (the `brotlic` crate).
//! Requires a C toolchain at build time; not no_std-compatible.

use alloc::{boxed::Box, vec, vec::Vec};
use core::error::Error;

use crate::WuffErr;

pub(super) fn decompress_brotli(
    compressed_data: &[u8],
    expected_size: usize,
) -> Result<Vec<u8>, Box<dyn Error>> {
    use brotlic::decode::{BrotliDecoder, DecoderInfo};

    // Allocate the output buffer once, up front, at exactly the (trusted) expected size.
    // The decoder never writes past the end of the slice, so `expected_size` is a hard
    // upper bound: a stream that would expand further reports `NeedsMoreOutput` rather
    // than driving an unbounded allocation.
    let mut output = vec![0u8; expected_size];
    let result = BrotliDecoder::new()
        .decompress(compressed_data, &mut output)
        .map_err(|_| WuffErr::GenericError)?;

    // Require a clean end-of-stream producing exactly `expected_size` bytes. Any trailing
    // WOFF2 padding bytes (up to 3, counted in `totalCompressedSize`) are harmless: the
    // decoder reports `Finished` at end-of-stream and simply leaves them unconsumed.
    if !matches!(result.info, DecoderInfo::Finished) || result.bytes_written != expected_size {
        return Err(Box::new(WuffErr::GenericError));
    }

    Ok(output)
}
