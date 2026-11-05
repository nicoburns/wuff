//! Built-in WOFF2 Brotli decompression.
//!
//! This module is compiled when the `brotli` feature is enabled and plugs a
//! decompressor into [`decompress_woff2_with_custom_brotli`](crate::decompress_woff2_with_custom_brotli):
//! the pure-Rust `brotli-decompressor` crate (the [`rust`] submodule), driven with
//! an `alloc`-backed allocator so it works on `no_std` targets (with a global
//! allocator). `brotli-unsafe` switches it to `brotli-decompressor`'s faster
//! unchecked implementation (std-only).

use alloc::vec::Vec;

use crate::WuffErr;
use crate::decompress_woff2_with_custom_brotli;

mod rust;

use rust::decompress_brotli;

/// Decompress a WOFF2 file using the built-in brotli decompressor
pub fn decompress_woff2(raw_woff_data: &[u8]) -> Result<Vec<u8>, WuffErr> {
    decompress_woff2_with_custom_brotli(raw_woff_data, &mut decompress_brotli)
}
