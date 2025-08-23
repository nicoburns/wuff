/* Copyright 2014 Google Inc. All Rights Reserved.

   Distributed under MIT license.
   See file LICENSE for detail or copy at https://opensource.org/licenses/MIT
*/

//! Common definition for WOFF2 encoding/decoding
//! Helpers common across multiple parts of woff2

pub const kWoff2Signature: u32 = 0x774f4632; // "wOF2"

// Leave the first byte open to store flag_byte
pub const kWoff2FlagsTransform: u32 = 1 << 8; // was "unsigned int" in C

// TrueType Collection ID string: 'ttcf'
pub const kTtcFontFlavor: u32 = 0x74746366;

pub const kSfntHeaderSize: usize = 12;
pub const kSfntEntrySize: usize = 16;

#[derive(Copy, Clone)]
pub struct Point {
    pub x: i32,
    pub y: i32,
    pub on_curve: bool,
}

#[derive(Clone)]
pub struct Table {
    pub tag: u32,
    pub flags: u32,
    pub src_offset: u32,
    pub src_length: u32,

    pub transform_length: u32,

    pub dst_offset: u32,
    pub dst_length: u32,
    // pub dst_data: &'a [u8],
}

impl PartialEq for Table {
    fn eq(&self, other: &Self) -> bool {
        self.tag.eq(&other.tag)
    }
}
impl PartialOrd for Table {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.tag.partial_cmp(&other.tag)
    }
}

/// Size of the collection header. 0 if version indicates this isn't a
/// collection. Ref http://www.microsoft.com/typography/otspec/otff.htm,
/// True Type Collections
pub(crate) fn CollectionHeaderSize(header_version: u32, num_fonts: u32) -> usize {
    let mut size: usize = 0;
    if header_version == 0x00020000 {
        size += 12; // ulDsig{Tag,Length,Offset}
    }
    if header_version == 0x00010000 || header_version == 0x00020000 {
        size += 12   // TTCTag, Version, numFonts
      + 4 * (num_fonts as usize); // OffsetTable[numFonts]
    }
    size
}

// This function is the direct port of the C ComputeULongSum function.
// A more optimised version replaces it below.
//
// pub(crate) fn ComputeULongSum(buf: &[u8], size: usize) -> u32 {
//     let mut checksum: u32 = 0;
//     let aligned_size: usize = size & !3;
//     for i in (0..aligned_size).step_by(4) {
//         checksum += ((buf[i] as u32) << 24)
//             | ((buf[i + 1] as u32) << 16)
//             | ((buf[i + 2] as u32) << 8)
//             | (buf[i + 3] as u32);
//     }

//     // treat size not aligned on 4 as if it were padded to 4 with 0's
//     if size != aligned_size {
//         let mut v: u32 = 0;
//         for i in aligned_size..size {
//             v |= (buf[i] as u32) << (24 - 8 * (i & 3));
//         }
//         checksum += v;
//     }

//     checksum
// }

/// Compute checksum over size bytes of buf
pub(crate) fn ComputeULongSum(buf: &[u8], size: usize) -> u32 {
    ComputeULongSumSlice(&buf[0..size])
}

/// Compute checksum over size bytes of buf
pub(crate) fn ComputeULongSumSlice(buf: &[u8]) -> u32 {
    let mut checksum: u32 = 0;
    let mut iter = buf.chunks_exact(4);
    for chunk in &mut iter {
        // TODO: use
        checksum += ((chunk[0] as u32) << 24)
            | ((chunk[1] as u32) << 16)
            | ((chunk[2] as u32) << 8)
            | (chunk[3] as u32);
    }

    // Treat size not aligned on 4 as if it were padded to 4 with 0's.
    //
    // Note(nico): as far as I can tell the zeroes don't actually effect the checksum except in that
    // they allow the trailing non-aligned bytes to take effect.
    let remainder = iter.remainder();
    match remainder {
        &[a, b, c] => {
            checksum += ((a as u32) << 24) | ((b as u32) << 16) | ((c as u32) << 8);
        }
        &[a, b] => {
            checksum += ((a as u32) << 24) | ((b as u32) << 16);
        }
        &[a] => {
            checksum += (a as u32) << 24;
        }
        [] => {
            // Do nothing
        }
        _ => unreachable!("chunk size was 4 so remainder will be a slice of length 3 or smaller"),
    };

    checksum
}
