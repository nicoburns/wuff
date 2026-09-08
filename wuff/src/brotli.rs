//! Built-in WOFF2 Brotli decompression.
//!
//! This module is compiled when the `brotli` or `brotli-c` feature is enabled and
//! plugs a decompressor into [`decompress_woff2_with_custom_brotli`](crate::decompress_woff2_with_custom_brotli):
//!
//! - `brotli`: the pure-Rust `brotli-decompressor` crate, driven with an
//!   `alloc`-backed allocator so it works on `no_std` targets (with a global
//!   allocator). `brotli-unsafe` switches this backend to `brotli-decompressor`'s
//!   faster unchecked implementation (std-only).
//! - `brotli-c`: Google's C brotli library via the `brotlic` crate. Faster, but
//!   requires a C toolchain and is not no_std-compatible. Takes precedence over
//!   `brotli` when both features are enabled.

use alloc::{boxed::Box, vec, vec::Vec};
use core::error::Error;

use crate::WuffErr;
use crate::decompress_woff2_with_custom_brotli;

/// A `Box<[T]>` wrapper implementing the allocation traits that `brotli-decompressor`
/// requires, so the decoder can allocate through the global allocator (`alloc`) rather
/// than depending on `std`. This is the no_std equivalent of the crate's built-in
/// `StandardAlloc` (which is only available behind its `std` feature).
#[cfg(all(feature = "brotli", not(feature = "brotli-c")))]
struct Rebox<T>(Box<[T]>);

#[cfg(all(feature = "brotli", not(feature = "brotli-c")))]
impl<T> Default for Rebox<T> {
    fn default() -> Self {
        Rebox(Vec::new().into_boxed_slice())
    }
}

#[cfg(all(feature = "brotli", not(feature = "brotli-c")))]
impl<T> brotli_decompressor::SliceWrapper<T> for Rebox<T> {
    fn slice(&self) -> &[T] {
        &self.0
    }
}

#[cfg(all(feature = "brotli", not(feature = "brotli-c")))]
impl<T> brotli_decompressor::SliceWrapperMut<T> for Rebox<T> {
    fn slice_mut(&mut self) -> &mut [T] {
        &mut self.0
    }
}

/// Zero-sized allocator handing out `Rebox` cells backed by the global allocator.
#[cfg(all(feature = "brotli", not(feature = "brotli-c")))]
struct HeapAlloc;

#[cfg(all(feature = "brotli", not(feature = "brotli-c")))]
impl<T: Clone + Default> brotli_decompressor::Allocator<T> for HeapAlloc {
    type AllocatedMemory = Rebox<T>;
    fn alloc_cell(&mut self, len: usize) -> Rebox<T> {
        Rebox(vec![T::default(); len].into_boxed_slice())
    }
    fn free_cell(&mut self, _data: Rebox<T>) {}
}

#[cfg(all(feature = "brotli", not(feature = "brotli-c")))]
fn decompress_brotli(
    compressed_data: &[u8],
    expected_size: usize,
) -> Result<Vec<u8>, Box<dyn Error>> {
    use brotli_decompressor::{BrotliDecompressStream, BrotliResult, BrotliState};

    // Allocate the output buffer once, up front, at exactly the (trusted) expected size and
    // decompress directly into it. `BrotliDecompressStream` never writes past the end of the
    // slice, so `expected_size` is a HARD upper bound on the output: a stream that would expand
    // further stops with `NeedsMoreOutput` rather than driving an unbounded allocation. This
    // mirrors the reference C++ decoder, which decompresses into a fixed-size buffer and rejects
    // the result unless the decoded size matches exactly.
    let mut output = vec![0u8; expected_size];
    let mut available_in = compressed_data.len();
    let mut input_offset = 0usize;
    let mut available_out = output.len();
    let mut output_offset = 0usize;
    let mut total_out = 0usize;
    let mut state = BrotliState::new(HeapAlloc, HeapAlloc, HeapAlloc);

    let result = BrotliDecompressStream(
        &mut available_in,
        &mut input_offset,
        compressed_data,
        &mut available_out,
        &mut output_offset,
        &mut output,
        &mut total_out,
        &mut state,
    );

    // Require a clean end-of-stream producing exactly `expected_size` bytes. Any trailing WOFF2
    // padding bytes (up to 3, counted in `totalCompressedSize`) are harmless: the decoder reports
    // success at end-of-stream and simply leaves them unconsumed in the input.
    if !matches!(result, BrotliResult::ResultSuccess) || output_offset != expected_size {
        return Err(Box::new(WuffErr::GenericError));
    }

    Ok(output)
}

/// Brotli decompression via Google's C implementation (the `brotlic` crate).
#[cfg(feature = "brotli-c")]
fn decompress_brotli(
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

/// Decompress a WOFF2 file using the built-in brotli decompressor
pub fn decompress_woff2(raw_woff_data: &[u8]) -> Result<Vec<u8>, WuffErr> {
    decompress_woff2_with_custom_brotli(raw_woff_data, &mut decompress_brotli)
}
