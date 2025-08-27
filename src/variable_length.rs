/* Copyright 2015 Google Inc. All Rights Reserved.

   Distributed under MIT license.
   See file LICENSE for detail or copy at https://opensource.org/licenses/MIT
*/

//! Helper functions for woff2 variable length types: 255UInt16 and UIntBase128

use arrayvec::ArrayVec;
use bytes::Buf;

use crate::error::{WuffErr, bail, bail_if};

pub trait BufVariableExt {
    fn try_get_variable_255_u16(&mut self) -> Result<u16, WuffErr>;
    fn try_get_variable_128_u32(&mut self) -> Result<u32, WuffErr>;
    fn try_read_bytes_into(&mut self, n: usize, buf: &mut Vec<u8>) -> Result<(), WuffErr>;
}

impl<T: bytes::Buf> BufVariableExt for T {
    fn try_get_variable_255_u16(&mut self) -> Result<u16, WuffErr> {
        Read255UShort(self).map(|val| val as u16)
    }

    fn try_get_variable_128_u32(&mut self) -> Result<u32, WuffErr> {
        ReadBase128(self)
    }

    fn try_read_bytes_into(&mut self, n: usize, buf: &mut Vec<u8>) -> Result<(), WuffErr> {
        let orig_len = buf.len();
        buf.resize(orig_len + n, 0);
        self.try_copy_to_slice(&mut buf[orig_len..])?;
        Ok(())
    }
}

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
pub(crate) fn Read255UShort(buf: &mut impl Buf) -> Result<u32, WuffErr> {
    const kWordCode: u8 = 253;
    const kOneMoreByteCode2: u8 = 254;
    const kOneMoreByteCode1: u8 = 255;
    const kLowestUCode: u32 = 253;

    let code = buf.try_get_u8()?;
    match code {
        kWordCode => Ok(buf.try_get_u16()? as u32),
        kOneMoreByteCode1 => Ok(buf.try_get_u8()? as u32 + kLowestUCode),
        kOneMoreByteCode2 => Ok(buf.try_get_u8()? as u32 + kLowestUCode * 2),
        _ => Ok(code as u32),
    }
}

pub(crate) fn ReadBase128(buf: &mut impl Buf) -> Result<u32, WuffErr> {
    let mut result: u32 = 0;
    for i in 0..5 {
        let code = buf.try_get_u8()?;

        // Leading zeros are invalid.
        bail_if!(i == 0 && code == 0x80);
        // If any of the top seven bits are set then we're about to overflow.
        bail_if!((result & 0xfe000000) != 0);

        result = (result << 7) | ((code & 0x7f) as u32);
        if (code & 0x80) == 0 {
            return Ok(result);
        }
    }
    // Make sure not to exceed the size bound
    bail!();
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
