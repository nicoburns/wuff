/* Copyright 2014 Google Inc. All Rights Reserved.

   Distributed under MIT license.
   See file LICENSE for detail or copy at https://opensource.org/licenses/MIT
*/

//! Font table tags

// Tags of popular tables.
pub const kGlyfTableTag: u32 = 0x676c7966;
pub const kHeadTableTag: u32 = 0x68656164;
pub const kLocaTableTag: u32 = 0x6c6f6361;
pub const kDsigTableTag: u32 = 0x44534947;
pub const kCffTableTag: u32 = 0x43464620;
pub const kHmtxTableTag: u32 = 0x686d7478;
pub const kHheaTableTag: u32 = 0x68686561;
pub const kMaxpTableTag: u32 = 0x6d617870;

// Note that the byte order is big-endian, not the same as ots.cc
const fn TAG(a: u8, b: u8, c: u8, d: u8) -> u32 {
    ((a as u32) << 24) | ((b as u32) << 16) | ((c as u32) << 8) | (d as u32)
}

pub static kKnownTags: [u32; 63] = [
    TAG(b'c', b'm', b'a', b'p'), // 0
    TAG(b'h', b'e', b'a', b'd'), // 1
    TAG(b'h', b'h', b'e', b'a'), // 2
    TAG(b'h', b'm', b't', b'x'), // 3
    TAG(b'm', b'a', b'x', b'p'), // 4
    TAG(b'n', b'a', b'm', b'e'), // 5
    TAG(b'O', b'S', b'/', b'2'), // 6
    TAG(b'p', b'o', b's', b't'), // 7
    TAG(b'c', b'v', b't', b' '), // 8
    TAG(b'f', b'p', b'g', b'm'), // 9
    TAG(b'g', b'l', b'y', b'f'), // 10
    TAG(b'l', b'o', b'c', b'a'), // 11
    TAG(b'p', b'r', b'e', b'p'), // 12
    TAG(b'C', b'F', b'F', b' '), // 13
    TAG(b'V', b'O', b'R', b'G'), // 14
    TAG(b'E', b'B', b'D', b'T'), // 15
    TAG(b'E', b'B', b'L', b'C'), // 16
    TAG(b'g', b'a', b's', b'p'), // 17
    TAG(b'h', b'd', b'm', b'x'), // 18
    TAG(b'k', b'e', b'r', b'n'), // 19
    TAG(b'L', b'T', b'S', b'H'), // 20
    TAG(b'P', b'C', b'L', b'T'), // 21
    TAG(b'V', b'D', b'M', b'X'), // 22
    TAG(b'v', b'h', b'e', b'a'), // 23
    TAG(b'v', b'm', b't', b'x'), // 24
    TAG(b'B', b'A', b'S', b'E'), // 25
    TAG(b'G', b'D', b'E', b'F'), // 26
    TAG(b'G', b'P', b'O', b'S'), // 27
    TAG(b'G', b'S', b'U', b'B'), // 28
    TAG(b'E', b'B', b'S', b'C'), // 29
    TAG(b'J', b'S', b'T', b'F'), // 30
    TAG(b'M', b'A', b'T', b'H'), // 31
    TAG(b'C', b'B', b'D', b'T'), // 32
    TAG(b'C', b'B', b'L', b'C'), // 33
    TAG(b'C', b'O', b'L', b'R'), // 34
    TAG(b'C', b'P', b'A', b'L'), // 35
    TAG(b'S', b'V', b'G', b' '), // 36
    TAG(b's', b'b', b'i', b'x'), // 37
    TAG(b'a', b'c', b'n', b't'), // 38
    TAG(b'a', b'v', b'a', b'r'), // 39
    TAG(b'b', b'd', b'a', b't'), // 40
    TAG(b'b', b'l', b'o', b'c'), // 41
    TAG(b'b', b's', b'l', b'n'), // 42
    TAG(b'c', b'v', b'a', b'r'), // 43
    TAG(b'f', b'd', b's', b'c'), // 44
    TAG(b'f', b'e', b'a', b't'), // 45
    TAG(b'f', b'm', b't', b'x'), // 46
    TAG(b'f', b'v', b'a', b'r'), // 47
    TAG(b'g', b'v', b'a', b'r'), // 48
    TAG(b'h', b's', b't', b'y'), // 49
    TAG(b'j', b'u', b's', b't'), // 50
    TAG(b'l', b'c', b'a', b'r'), // 51
    TAG(b'm', b'o', b'r', b't'), // 52
    TAG(b'm', b'o', b'r', b'x'), // 53
    TAG(b'o', b'p', b'b', b'd'), // 54
    TAG(b'p', b'r', b'o', b'p'), // 55
    TAG(b't', b'r', b'a', b'k'), // 56
    TAG(b'Z', b'a', b'p', b'f'), // 57
    TAG(b'S', b'i', b'l', b'f'), // 58
    TAG(b'G', b'l', b'a', b't'), // 59
    TAG(b'G', b'l', b'o', b'c'), // 60
    TAG(b'F', b'e', b'a', b't'), // 61
    TAG(b'S', b'i', b'l', b'l'), // 62
];
