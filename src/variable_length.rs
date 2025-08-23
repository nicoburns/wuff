/* Copyright 2015 Google Inc. All Rights Reserved.

   Distributed under MIT license.
   See file LICENSE for detail or copy at https://opensource.org/licenses/MIT
*/

//! Helper functions for woff2 variable length types: 255UInt16 and UIntBase128

use arrayvec::ArrayVec;

use crate::{FONT_COMPRESSION_FAILURE, buffer::Buffer};

fn Size255UShort(value: u16) -> usize {
    if value < 253 {
        1
    } else if value < 762 {
        2
    } else {
        3
    }
}

fn Write255UShort(value: i32) -> ArrayVec<u8, 3> {
    let mut packed: ArrayVec<u8, 3> = ArrayVec::new();
    if value < 253 {
        packed.push(value as u8);
    } else if value < 506 {
        packed.push(255);
        packed.push((value - 253) as u8);
    } else if value < 762 {
        packed.push(254);
        packed.push((value - 506) as u8);
    } else {
        packed.push(253);
        packed.push((value >> 8) as u8);
        packed.push((value & 0xff) as u8);
    }
    packed
}

pub(crate) fn Store255UShort(val: i32, offset: &mut usize, dst: &mut [u8]) {
    let packed = Write255UShort(val);
    for packed_byte in packed.into_iter() {
        dst[*offset] = packed_byte;
        *offset += 1;
    }
}

// Based on section 6.1.1 of MicroType Express draft spec
pub(crate) fn Read255UShort(buf: &mut Buffer<'_>, value: &mut u32) -> bool {
    const kWordCode: u8 = 253;
    const kOneMoreByteCode2: u8 = 254;
    const kOneMoreByteCode1: u8 = 255;
    const kLowestUCode: u32 = 253;

    let mut code: u8 = 0;
    if !buf.ReadU8(&mut code) {
        return FONT_COMPRESSION_FAILURE();
    }

    if code == kWordCode {
        let mut result: u16 = 0;
        if !buf.ReadU16(&mut result) {
            return FONT_COMPRESSION_FAILURE();
        }
        *value = result as u32;
        true
    } else if code == kOneMoreByteCode1 {
        let mut result: u8 = 0;
        if !buf.ReadU8(&mut result) {
            return FONT_COMPRESSION_FAILURE();
        }
        *value = (result as u32) + kLowestUCode;
        true
    } else if code == kOneMoreByteCode2 {
        let mut result: u8 = 0;
        if !buf.ReadU8(&mut result) {
            return FONT_COMPRESSION_FAILURE();
        }
        *value = (result as u32) + kLowestUCode * 2;
        true
    } else {
        *value = code as u32;
        true
    }
}

pub(crate) fn ReadBase128(buf: &mut Buffer<'_>, value: &mut u32) -> bool {
    let mut result: u32 = 0;
    for i in 0..5 {
        let mut code: u8 = 0;
        if !buf.ReadU8(&mut code) {
            return FONT_COMPRESSION_FAILURE();
        }
        // Leading zeros are invalid.
        if i == 0 && code == 0x80 {
            return FONT_COMPRESSION_FAILURE();
        }
        // If any of the top seven bits are set then we're about to overflow.
        if (result & 0xfe000000) != 0 {
            return FONT_COMPRESSION_FAILURE();
        }
        result = (result << 7) | ((code & 0x7f) as u32);
        if (code & 0x80) == 0 {
            *value = result;
            return true;
        }
    }
    // Make sure not to exceed the size bound
    FONT_COMPRESSION_FAILURE()
}

fn Base128Size(mut n: usize) -> usize {
    let mut size: usize = 1;
    while n < 128 {
        n >>= 7;
        size += 1;
    }
    size
}

pub(crate) fn StoreBase128(len: usize, offset: &mut usize, dst: &mut [u8]) {
    let size: usize = Base128Size(len);
    for i in 0..size {
        let mut b: u8 = ((len >> (7 * (size - i - 1))) & 0x7f) as u8;
        if i < size - 1 {
            b |= 0x80;
        }
        dst[*offset] = b;
        *offset += 1;
    }
}
