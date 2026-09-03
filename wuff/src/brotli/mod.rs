//! Built-in Brotli backends for WOFF2 decoding.
//!
//! Two backends are available, selected by Cargo features:
//!
//! * `brotli` — the pure-Rust `brotli-decompressor` crate (`rust_backend.rs`).
//! * `brotli-c` — Google's reference C implementation, vendored under `vendor/brotli` and compiled
//!   by `build.rs` (`c_backend.rs`).
//!
//! Cargo features are additive, so both may end up enabled at once (e.g. `brotli-c` on top of the
//! default features). In that case the C backend is used; disable default features to avoid also
//! compiling the Rust one.

#[cfg(feature = "brotli-c")]
mod c_backend;
#[cfg(all(feature = "brotli", not(feature = "brotli-c")))]
mod rust_backend;

#[cfg(feature = "brotli-c")]
use c_backend::decompress_brotli;
#[cfg(all(feature = "brotli", not(feature = "brotli-c")))]
use rust_backend::decompress_brotli;

use alloc::vec::Vec;

use crate::WuffErr;
use crate::decompress_woff2_with_custom_brotli;

/// Decompress a WOFF2 file using the built-in brotli decompressor
///
/// The decompressor is Google's reference C implementation when the `brotli-c` feature is
/// enabled, and the pure-Rust `brotli-decompressor` crate otherwise.
pub fn decompress_woff2(raw_woff_data: &[u8]) -> Result<Vec<u8>, WuffErr> {
    decompress_woff2_with_custom_brotli(raw_woff_data, &mut decompress_brotli)
}
