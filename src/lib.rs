//! Pure Rust WOFF2 decoder

#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(unused_imports)]
#![allow(dead_code)]
#![allow(clippy::needless_range_loop)]

pub mod buffer;
pub mod table_tags;
pub mod woff2_common;

#[inline(always)]
pub fn PREDICT_FALSE(cond: bool) -> bool {
    cond
}

#[inline(always)]
pub fn PREDICT_TRUE(cond: bool) -> bool {
    cond
}


#[inline(always)]
pub fn FONT_COMPRESSION_FAILURE() -> bool {
    false
}

#[inline]
fn StoreU32(dst: &mut [u8], offset: usize, x: u32) -> usize {
    dst[offset] = (x >> 24) as u8;
    dst[offset + 1] = (x >> 16) as u8;
    dst[offset + 2] = (x >> 8) as u8;
    dst[offset + 3] = x as u8;

    offset + 4
}

#[inline]
fn Store16(dst: &mut [u8], offset: usize, x: i32) -> usize {
    dst[offset] = (x >> 8) as u8;
    dst[offset + 1] = x as u8;

    offset + 2
}

#[inline]
fn StoreU32_mut(dst: &mut [u8], offset: &mut usize, x: u32) {
    dst[*offset] = (x >> 24) as u8;
    dst[*offset + 1] = (x >> 16) as u8;
    dst[*offset + 2] = (x >> 8) as u8;
    dst[*offset + 3] = x as u8;

    *offset += 4
}

#[inline]
fn Store16_mut(dst: &mut [u8], offset: &mut usize, x: i32) {
    dst[*offset] = (x >> 8) as u8;
    dst[*offset + 1] = x as u8;

    *offset += 2
}

// #[inline]
// fn StoreBytes(data: &mut[u8], offset: usize, uint8_t* dst) {
//   memcpy(&dst[*offset], data, len);
//   *offset += len;
// }

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