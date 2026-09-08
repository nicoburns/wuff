//! Built-in WOFF2 Brotli decompression.
//!
//! This module is compiled when the `brotli` or `brotli-c` feature is enabled and
//! plugs a decompressor into [`decompress_woff2_with_custom_brotli`](crate::decompress_woff2_with_custom_brotli):
//!
//! - `brotli` (the [`rust`] submodule): the pure-Rust `brotli-decompressor` crate,
//!   driven with an `alloc`-backed allocator so it works on `no_std` targets (with
//!   a global allocator). `brotli-unsafe` switches this backend to
//!   `brotli-decompressor`'s faster unchecked implementation (std-only).
//! - `brotli-c` (the [`c`] submodule): Google's C brotli library via the `brotlic`
//!   crate. Faster, but requires a C toolchain and is not no_std-compatible. Takes
//!   precedence over `brotli` when both features are enabled.

use alloc::vec::Vec;

use crate::WuffErr;
use crate::decompress_woff2_with_custom_brotli;

#[cfg(feature = "brotli-c")]
mod c;
#[cfg(all(feature = "brotli", not(feature = "brotli-c")))]
mod rust;

#[cfg(feature = "brotli-c")]
use c::decompress_brotli;
#[cfg(all(feature = "brotli", not(feature = "brotli-c")))]
use rust::decompress_brotli;

/// Decompress a WOFF2 file using the built-in brotli decompressor
pub fn decompress_woff2(raw_woff_data: &[u8]) -> Result<Vec<u8>, WuffErr> {
    decompress_woff2_with_custom_brotli(raw_woff_data, &mut decompress_brotli)
}
