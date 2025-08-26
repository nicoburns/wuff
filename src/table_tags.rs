/* Copyright 2014 Google Inc. All Rights Reserved.

   Distributed under MIT license.
   See file LICENSE for detail or copy at https://opensource.org/licenses/MIT
*/

//! Font table tags

use font_types::Tag;

// Tags of popular tables.
pub const kGlyfTableTag: u32 = 0x676c7966;
pub const kHeadTableTag: u32 = 0x68656164;
pub const kLocaTableTag: u32 = 0x6c6f6361;
pub const kDsigTableTag: u32 = 0x44534947;
pub const kCffTableTag: u32 = 0x43464620;
pub const kHmtxTableTag: u32 = 0x686d7478;
pub const kHheaTableTag: u32 = 0x68686561;
pub const kMaxpTableTag: u32 = 0x6d617870;

pub static KNOWN_TABLE_TAGS: [Tag; 63] = [
    Tag::new(b"cmap"), // 0
    Tag::new(b"head"), // 1
    Tag::new(b"hhea"), // 2
    Tag::new(b"hmtx"), // 3
    Tag::new(b"maxp"), // 4
    Tag::new(b"name"), // 5
    Tag::new(b"OS/2"), // 6
    Tag::new(b"post"), // 7
    Tag::new(b"cvt "), // 8
    Tag::new(b"fpgm"), // 9
    Tag::new(b"glyf"), // 10
    Tag::new(b"loca"), // 11
    Tag::new(b"prep"), // 12
    Tag::new(b"CFF "), // 13
    Tag::new(b"VORG"), // 14
    Tag::new(b"EBDT"), // 15
    Tag::new(b"EBLC"), // 16
    Tag::new(b"gasp"), // 17
    Tag::new(b"hdmx"), // 18
    Tag::new(b"kern"), // 19
    Tag::new(b"LTSH"), // 20
    Tag::new(b"PCLT"), // 21
    Tag::new(b"VDMX"), // 22
    Tag::new(b"vhea"), // 23
    Tag::new(b"vmtx"), // 24
    Tag::new(b"BASE"), // 25
    Tag::new(b"GDEF"), // 26
    Tag::new(b"GPOS"), // 27
    Tag::new(b"GSUB"), // 28
    Tag::new(b"EBSC"), // 29
    Tag::new(b"JSTF"), // 30
    Tag::new(b"MATH"), // 31
    Tag::new(b"CBDT"), // 32
    Tag::new(b"CBLC"), // 33
    Tag::new(b"COLR"), // 34
    Tag::new(b"CPAL"), // 35
    Tag::new(b"SVG "), // 36
    Tag::new(b"sbix"), // 37
    Tag::new(b"acnt"), // 38
    Tag::new(b"avar"), // 39
    Tag::new(b"bdat"), // 40
    Tag::new(b"bloc"), // 41
    Tag::new(b"bsln"), // 42
    Tag::new(b"cvar"), // 43
    Tag::new(b"fdsc"), // 44
    Tag::new(b"feat"), // 45
    Tag::new(b"fmtx"), // 46
    Tag::new(b"fvar"), // 47
    Tag::new(b"gvar"), // 48
    Tag::new(b"hsty"), // 49
    Tag::new(b"just"), // 50
    Tag::new(b"lcar"), // 51
    Tag::new(b"mort"), // 52
    Tag::new(b"morx"), // 53
    Tag::new(b"opbd"), // 54
    Tag::new(b"prop"), // 55
    Tag::new(b"trak"), // 56
    Tag::new(b"Zapf"), // 57
    Tag::new(b"Silf"), // 58
    Tag::new(b"Glat"), // 59
    Tag::new(b"Gloc"), // 60
    Tag::new(b"Feat"), // 61
    Tag::new(b"Sill"), // 62
];
