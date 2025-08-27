/* Copyright 2013 Google Inc. All Rights Reserved.

   Distributed under MIT license.
   See file LICENSE for detail or copy at https://opensource.org/licenses/MIT
*/

use crate::FONT_COMPRESSION_FAILURE;
const ONE_GIGABYTE: usize = 1024 * 1024 * 1024;

// -----------------------------------------------------------------------------
// Buffer helper class
//
// This class perform some trival buffer operations while checking for
// out-of-bounds errors. As a family they return false if anything is amiss,
// updating the current offset otherwise.
// -----------------------------------------------------------------------------
pub struct Buffer<'a> {
    buffer: &'a [u8],
    offset: usize,
}

impl bytes::Buf for Buffer<'_> {
    fn remaining(&self) -> usize {
        self.buffer.len() - self.offset
    }

    fn chunk(&self) -> &[u8] {
        self.remaining_as_slice()
    }

    fn advance(&mut self, cnt: usize) {
        if !self.Skip(cnt) {
            panic!("Tried to advance past the end of the buffer");
        }
    }
}

impl Buffer<'_> {
    pub fn new<'b>(data: &'b [u8]) -> Buffer<'b> {
        Buffer {
            buffer: data,
            offset: 0,
        }
    }

    pub fn Skip(&mut self, n_bytes: usize) -> bool {
        if n_bytes > ONE_GIGABYTE {
            return FONT_COMPRESSION_FAILURE();
        }
        if self.offset + n_bytes > self.buffer.len() || (self.offset > self.buffer.len() - n_bytes)
        {
            return FONT_COMPRESSION_FAILURE();
        }

        self.offset += n_bytes;
        true
    }

    pub fn Read(&mut self, data: &mut [u8], n_bytes: usize) -> bool {
        if n_bytes > ONE_GIGABYTE {
            return FONT_COMPRESSION_FAILURE();
        }
        if self.offset + n_bytes > self.buffer.len() || (self.offset > self.buffer.len() - n_bytes)
        {
            return FONT_COMPRESSION_FAILURE();
        }

        // TODO: consider optimising copy
        let src = &self.buffer[self.offset..self.offset + n_bytes];
        let dest = &mut data[0..n_bytes];
        dest.copy_from_slice(src);

        self.offset += n_bytes;
        true
    }

    #[inline(always)]
    fn read_n_bytes<const N: usize>(&mut self) -> Result<[u8; N], ()> {
        if self.offset + N > self.buffer.len() {
            return Err(());
        }
        // TODO: consider optimising
        let bytes: [u8; N] = self.buffer[self.offset..self.offset + N]
            .try_into()
            .unwrap();
        self.offset += N;
        Ok(bytes)
    }

    #[inline]
    pub fn ReadU8(&mut self, value: &mut u8) -> bool {
        let Ok(bytes) = self.read_n_bytes::<1>() else {
            return FONT_COMPRESSION_FAILURE();
        };
        *value = bytes[0];
        true
    }

    #[inline]
    pub fn ReadU16(&mut self, value: &mut u16) -> bool {
        let Ok(bytes) = self.read_n_bytes::<2>() else {
            return FONT_COMPRESSION_FAILURE();
        };
        *value = u16::from_be_bytes(bytes);
        true
    }

    #[inline]
    pub fn ReadS16(&mut self, value: &mut i16) -> bool {
        let Ok(bytes) = self.read_n_bytes::<2>() else {
            return FONT_COMPRESSION_FAILURE();
        };
        *value = i16::from_be_bytes(bytes);
        true
    }

    #[inline]
    pub fn ReadU24(&mut self, value: &mut u32) -> bool {
        let Ok(bytes) = self.read_n_bytes::<4>() else {
            return FONT_COMPRESSION_FAILURE();
        };
        *value = (bytes[0] as u32) << 16 | (bytes[1] as u32) << 8 | (bytes[2] as u32);
        true
    }

    #[inline]
    pub fn ReadU32(&mut self, value: &mut u32) -> bool {
        let Ok(bytes) = self.read_n_bytes::<4>() else {
            return FONT_COMPRESSION_FAILURE();
        };
        *value = u32::from_be_bytes(bytes);
        true
    }

    #[inline]
    pub fn ReadS32(&mut self, value: &mut i32) -> bool {
        let Ok(bytes) = self.read_n_bytes::<4>() else {
            return FONT_COMPRESSION_FAILURE();
        };
        *value = i32::from_be_bytes(bytes);
        true
    }

    #[inline]
    pub fn ReadTag(&mut self, value: &mut u32) -> bool {
        let Ok(bytes) = self.read_n_bytes::<4>() else {
            return FONT_COMPRESSION_FAILURE();
        };
        *value = u32::from_ne_bytes(bytes);
        true
    }

    #[inline]
    pub fn ReadR64(&mut self, value: &mut u64) -> bool {
        let Ok(bytes) = self.read_n_bytes::<8>() else {
            return FONT_COMPRESSION_FAILURE();
        };
        *value = u64::from_ne_bytes(bytes);
        true
    }

    pub fn remaining_as_slice(&self) -> &[u8] {
        &self.buffer[self.offset..]
    }

    pub fn buffer(&self) -> &[u8] {
        self.buffer
    }

    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn set_offset(&mut self, new_offset: usize) {
        self.offset = new_offset;
    }
}
