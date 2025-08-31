//! Pure Rust WOFF2 decoder

#![cfg_attr(docsrs, feature(doc_cfg))]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(unused_imports)]
#![allow(dead_code)]
#![allow(clippy::needless_range_loop)]
#![allow(clippy::collapsible_if)]

mod decompress_woff1;
mod decompress_woff2;
mod error;
mod table_tags;
mod variable_length;
mod woff;

use bytes::BufMut;
pub use decompress_woff1::decompress_woff1_with_custom_z;
pub use decompress_woff2::decompress_woff2_with_custom_brotli;

#[cfg(feature = "z")]
#[cfg_attr(docsrs, doc(cfg(feature = "z")))]
pub use decompress_woff1::decompress_woff1;

#[cfg(feature = "brotli")]
#[cfg_attr(docsrs, doc(cfg(feature = "brotli")))]
pub use decompress_woff2::decompress_woff2;

const HEAD: Tag = Tag::new(b"head");
const HHEA: Tag = Tag::new(b"hhea");
const HMTX: Tag = Tag::new(b"hmtx");
const GLYF: Tag = Tag::new(b"glyf");
const LOCA: Tag = Tag::new(b"loca");

#[derive(Copy, Clone)]
pub(crate) struct Point {
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
use font_types::Tag;

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

/// Writes an OpenType table directory
///
/// <https://learn.microsoft.com/en-us/typography/opentype/spec/otff#table-directory>
fn write_table_directory_header(output: &mut impl BufMut, flavor: Tag, num_tables: u16) {
    let mut max_pow2: u16 = 0;
    while 1u32 << (max_pow2 + 1) <= (num_tables as u32) {
        max_pow2 += 1;
    }
    let entry_selector = max_pow2;
    let search_range: u16 = (1u16 << max_pow2) << 4;
    let range_shift = (((num_tables as u32) << 4) - search_range as u32) as u16;

    output.put_u32(u32::from_be_bytes(flavor.to_be_bytes())); // sfnt version
    output.put_u16(num_tables); // num_tables
    output.put_u16(search_range); // searchRange
    output.put_u16(entry_selector); // entrySelector
    output.put_u16(range_shift); // rangeShift
}
