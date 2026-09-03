//! Built-in WOFF2 Brotli decompression, backed by Google's reference C decoder.
//!
//! This module is only compiled when the `brotli-c` feature is enabled. The decoder sources
//! (<https://github.com/google/brotli>, vendored under `vendor/brotli`) are compiled and linked by
//! `build.rs`; this module holds the hand-written FFI declarations for the one entry point we use.
//!
//! On bare WebAssembly targets (`wasm32-unknown-unknown`, `wasm32v1-none`) there is no libc, so
//! `build.rs` compiles the C code against minimal shim headers that route `malloc`/`free` to the
//! allocator functions defined at the bottom of this file. See `csrc/wasm-libc-shim`.

use alloc::{boxed::Box, vec, vec::Vec};
use core::error::Error;
use core::ffi::c_int;

use crate::WuffErr;

/// `BROTLI_DECODER_RESULT_SUCCESS` from `brotli/decode.h`.
const BROTLI_DECODER_RESULT_SUCCESS: c_int = 1;

unsafe extern "C" {
    /// One-shot decoder: `BrotliDecoderResult BrotliDecoderDecompress(size_t encoded_size,
    /// const uint8_t* encoded_buffer, size_t* decoded_size, uint8_t* decoded_buffer)`.
    ///
    /// On entry `*decoded_size` is the capacity of `decoded_buffer`; on return it is the number of
    /// bytes written. Returns `BROTLI_DECODER_RESULT_SUCCESS` only if the whole stream was decoded
    /// into the buffer (a truncated, corrupt, or too-large stream yields
    /// `BROTLI_DECODER_RESULT_ERROR`).
    fn BrotliDecoderDecompress(
        encoded_size: usize,
        encoded_buffer: *const u8,
        decoded_size: *mut usize,
        decoded_buffer: *mut u8,
    ) -> c_int;
}

pub(super) fn decompress_brotli(
    compressed_data: &[u8],
    expected_size: usize,
) -> Result<Vec<u8>, Box<dyn Error>> {
    // Allocate the output buffer once, up front, at exactly the (trusted) expected size and
    // decompress directly into it. The decoder never writes past `decoded_size` bytes, so
    // `expected_size` is a HARD upper bound on the output. This is exactly what the reference
    // woff2 C++ decoder does (`Woff2Uncompress` in `woff2_dec.cc`).
    let mut output = vec![0u8; expected_size];
    let mut decoded_size = output.len();

    // SAFETY: `compressed_data` and `output` are live, correctly-sized buffers for the duration
    // of the call, `decoded_size` tells the decoder the exact capacity of `output`, and the
    // decoder does not retain any of the pointers after returning.
    let result = unsafe {
        BrotliDecoderDecompress(
            compressed_data.len(),
            compressed_data.as_ptr(),
            &mut decoded_size,
            output.as_mut_ptr(),
        )
    };

    // Require a clean end-of-stream producing exactly `expected_size` bytes. Any trailing WOFF2
    // padding bytes (up to 3, counted in `totalCompressedSize`) are harmless: the decoder reports
    // success at end-of-stream and simply leaves them unconsumed in the input.
    if result != BROTLI_DECODER_RESULT_SUCCESS || decoded_size != expected_size {
        return Err(Box::new(WuffErr::GenericError));
    }

    Ok(output)
}

/// `malloc`/`free` replacements for bare WebAssembly targets, which have no libc.
///
/// `build.rs` compiles the vendored decoder against shim headers that `#define malloc
/// wuff_brotli_malloc` and `#define free wuff_brotli_free` on these targets, so every allocation
/// Brotli makes (they all go through `BrotliDefaultAllocFunc`/`BrotliDefaultFreeFunc` or
/// `BrotliDecoderCreateInstance`) lands here and is served by Rust's global allocator.
///
/// `free` receives no size, but Rust's `dealloc` needs the original `Layout`, so each block is
/// prefixed with a small header recording its size. The header is 16 bytes to keep the returned
/// pointer aligned to `max_align_t` on wasm32.
///
/// The `cfg` must match `is_bare_wasm_target()` in `build.rs`.
#[cfg(all(target_family = "wasm", any(target_os = "unknown", target_os = "none")))]
mod wasm_libc_shim {
    use alloc::alloc::{Layout, alloc, dealloc};
    use core::ffi::c_void;
    use core::ptr;

    const HEADER: usize = 16;

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn wuff_brotli_malloc(size: usize) -> *mut c_void {
        let Some(total) = size.checked_add(HEADER) else {
            return ptr::null_mut();
        };
        let Ok(layout) = Layout::from_size_align(total, HEADER) else {
            return ptr::null_mut();
        };
        // SAFETY: `layout` has non-zero size (at least `HEADER` bytes).
        let base = unsafe { alloc(layout) };
        if base.is_null() {
            // Brotli treats a null return as out-of-memory and fails the decode cleanly.
            return ptr::null_mut();
        }
        // SAFETY: `base` is a fresh, `HEADER`-aligned allocation of `total` bytes, so the first
        // `size_of::<usize>()` bytes can hold the header and `base + HEADER` is in bounds.
        unsafe {
            base.cast::<usize>().write(size);
            base.add(HEADER).cast()
        }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn wuff_brotli_free(ptr: *mut c_void) {
        if ptr.is_null() {
            return;
        }
        // SAFETY: `ptr` was returned by `wuff_brotli_malloc`, so `ptr - HEADER` is the start of
        // an allocation made with the layout reconstructed here, and the header holds its size.
        unsafe {
            let base = ptr.cast::<u8>().sub(HEADER);
            let size = base.cast::<usize>().read();
            dealloc(
                base,
                Layout::from_size_align_unchecked(size + HEADER, HEADER),
            );
        }
    }
}
