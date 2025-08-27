//! Pure Rust WOFF2 decoder

#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(unused_imports)]
#![allow(dead_code)]
#![allow(clippy::needless_range_loop)]
#![allow(clippy::collapsible_if)]

mod decompress;
mod error;
mod table_tags;
mod types;
mod variable_length;
mod woff;

pub use decompress::{decompress_woff2, decompress_woff2_with_brotli};

#[derive(Copy, Clone)]
pub struct Point {
    pub x: i32,
    pub y: i32,
    pub on_curve: bool,
}

// Round a value up to the nearest multiple of 4. Don't round the value in the
// case that rounding up overflows.
//
// Implemented as a macro to make it generic over the type without horrible type bounds
macro_rules! Round4 {
    ($value:expr) => {
        match $value.checked_add(3) {
            Some(value_plus_3) => value_plus_3 & !3,
            None => $value,
        }
    };
}
use Round4;

/// Compute checksum over size bytes of buf
pub(crate) fn compute_checksum(buf: &[u8]) -> u32 {
    let mut checksum: u32 = 0;
    let mut iter = buf.chunks_exact(4);
    for chunk in &mut iter {
        let bytes: [u8; 4] = chunk.try_into().unwrap();
        checksum = checksum.wrapping_add(u32::from_be_bytes(bytes));
    }

    // Treat sizes not aligned on 4 as if it were padded to 4 with 0's.
    checksum = checksum.wrapping_add(match iter.remainder() {
        &[a, b, c] => u32::from_be_bytes([a, b, c, 0]),
        &[a, b] => u32::from_be_bytes([a, b, 0, 0]),
        &[a] => u32::from_be_bytes([a, 0, 0, 0]),
        [] => 0,
        _ => unreachable!("chunk size was 4 so remainder will be a slice of length 3 or smaller"),
    });

    checksum
}
