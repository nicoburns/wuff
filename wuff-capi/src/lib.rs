//! C API for the [wuff](https://docs.rs/wuff) WOFF2 decoder.
//!
//! This crate exposes `extern "C"` symbols wrapping wuff's WOFF2 decoder, plus
//! (in its `include` directory) C++ headers (`woff2/decode.h` and
//! `woff2/output.h`) which reimplement the decoding API of the
//! [woff2](https://github.com/google/woff2) C++ library on top of those
//! symbols. This allows the crate to be used as a drop-in replacement for the
//! woff2 library by C/C++ code (such as the `ots` sanitiser) that consumes
//! only its decoding API.
//!
//! Build scripts of dependent crates can locate the headers via the
//! `DEP_WOFF2_INCLUDE_DIR` environment variable.

use std::panic::catch_unwind;

/// Compute the size of the final uncompressed font, or 0 on error.
///
/// This reads the `totalSfntSize` field of the WOFF2 header. It is the
/// C equivalent of the woff2 library's `woff2::ComputeWOFF2FinalSize`.
///
/// # Safety
///
/// `data` must either be null (in which case 0 is returned) or point to
/// `length` bytes of readable memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wuff_woff2_compute_final_size(data: *const u8, length: usize) -> usize {
    if data.is_null() || length < 20 {
        return 0;
    }
    let bytes = unsafe { std::slice::from_raw_parts(data, length) };
    u32::from_be_bytes(bytes[16..20].try_into().unwrap()) as usize
}

/// Decompress a WOFF2 font into a newly-allocated buffer.
///
/// On success, returns a pointer to the decompressed font and stores its
/// length in `*result_length`. The buffer must be freed by passing the
/// returned pointer and length to [`wuff_woff2_free`].
///
/// On failure, returns null and stores 0 in `*result_length`.
///
/// # Safety
///
/// - `data` must either be null (in which case null is returned) or point to
///   `length` bytes of readable memory.
/// - `result_length` must be a valid pointer to a writable `size_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wuff_woff2_decode(
    data: *const u8,
    length: usize,
    result_length: *mut usize,
) -> *mut u8 {
    unsafe { *result_length = 0 };
    if data.is_null() {
        return std::ptr::null_mut();
    }
    let bytes = unsafe { std::slice::from_raw_parts(data, length) };

    // Catch panics: unwinding across an `extern "C"` boundary would abort.
    let result = catch_unwind(|| wuff::decompress_woff2(bytes));
    match result {
        Ok(Ok(decompressed)) => {
            let boxed: Box<[u8]> = decompressed.into_boxed_slice();
            let len = boxed.len();
            let ptr = Box::into_raw(boxed) as *mut u8;
            unsafe { *result_length = len };
            ptr
        }
        Ok(Err(_)) | Err(_) => std::ptr::null_mut(),
    }
}

/// Free a buffer previously returned by [`wuff_woff2_decode`].
///
/// # Safety
///
/// - `ptr` must either be null (in which case this is a no-op) or a pointer
///   previously returned by [`wuff_woff2_decode`], with `length` being the
///   value stored in `*result_length` by that call.
/// - The buffer must not have already been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wuff_woff2_free(ptr: *mut u8, length: usize) {
    if ptr.is_null() {
        return;
    }
    let slice = std::ptr::slice_from_raw_parts_mut(ptr, length);
    drop(unsafe { Box::from_raw(slice) });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_final_size_null_and_short() {
        unsafe {
            assert_eq!(wuff_woff2_compute_final_size(std::ptr::null(), 0), 0);
            let short = [0u8; 19];
            assert_eq!(
                wuff_woff2_compute_final_size(short.as_ptr(), short.len()),
                0
            );
        }
    }

    #[test]
    fn compute_final_size_reads_total_sfnt_size() {
        let mut header = [0u8; 48];
        header[16..20].copy_from_slice(&0x0001_2345u32.to_be_bytes());
        unsafe {
            assert_eq!(
                wuff_woff2_compute_final_size(header.as_ptr(), header.len()),
                0x0001_2345
            );
        }
    }

    #[test]
    fn decode_invalid_data_returns_null() {
        let garbage = [0u8; 64];
        let mut len = usize::MAX;
        let ptr = unsafe { wuff_woff2_decode(garbage.as_ptr(), garbage.len(), &mut len) };
        assert!(ptr.is_null());
        assert_eq!(len, 0);
    }

    #[test]
    fn free_null_is_noop() {
        unsafe { wuff_woff2_free(std::ptr::null_mut(), 0) };
    }
}
