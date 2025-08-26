/* Copyright 2014 Google Inc. All Rights Reserved.

   Distributed under MIT license.
   See file LICENSE for detail or copy at https://opensource.org/licenses/MIT
*/

//! Library for converting WOFF2 format font files to their TTF versions.

use std::collections::BTreeMap;

use arrayvec::ArrayVec;
use brotli_decompressor::{BrotliResult, brotli_decode};

use crate::buffer::Buffer;
use crate::table_tags::{
    kGlyfTableTag, kHeadTableTag, kHheaTableTag, kHmtxTableTag, kKnownTags, kLocaTableTag,
};
use crate::variable_length::{Read255UShort, ReadBase128};
use crate::woff2_common::{
    CollectionHeaderSize, ComputeULongSum, Point, Table, kSfntEntrySize, kSfntHeaderSize,
    kTtcFontFlavor, kWoff2FlagsTransform, kWoff2Signature,
};
use crate::{
    FONT_COMPRESSION_FAILURE, PREDICT_FALSE, PREDICT_TRUE, Round4, Store16, Store16_mut, StoreU32,
    WOFF2Out,
};

// simple glyph flags
const kGlyfOnCurve: i32 = 1 << 0;
const kGlyfXShort: i32 = 1 << 1;
const kGlyfYShort: i32 = 1 << 2;
const kGlyfRepeat: i32 = 1 << 3;
const kGlyfThisXIsSame: i32 = 1 << 4;
const kGlyfThisYIsSame: i32 = 1 << 5;
const kOverlapSimple: i32 = 1 << 6;

// composite glyph flags
// See CompositeGlyph.java in sfntly for full definitions
const FLAG_ARG_1_AND_2_ARE_WORDS: u16 = 1 << 0;
const FLAG_WE_HAVE_A_SCALE: u16 = 1 << 3;
const FLAG_MORE_COMPONENTS: u16 = 1 << 5;
const FLAG_WE_HAVE_AN_X_AND_Y_SCALE: u16 = 1 << 6;
const FLAG_WE_HAVE_A_TWO_BY_TWO: u16 = 1 << 7;
const FLAG_WE_HAVE_INSTRUCTIONS: u16 = 1 << 8;

// glyf flags
const FLAG_OVERLAP_SIMPLE_BITMAP: u16 = 1 << 0;

const kCheckSumAdjustmentOffset: usize = 8;

const kEndPtsOfContoursOffset: usize = 10;
const kCompositeGlyphBegin: usize = 10;

// 98% of Google Fonts have no glyph above 5k bytes
// Largest glyph ever observed was 72k bytes
const kDefaultGlyphBuf: usize = 5120;

// Over 14k test fonts the max compression ratio seen to date was ~20.
// >100 suggests you wrote a bad uncompressed size.
const kMaxPlausibleCompressionRatio: f32 = 100.0;

// metadata for a TTC font entry
#[derive(Clone, Default)]
struct TtcFont {
    flavor: u32,
    dst_offset: u32,
    header_checksum: u32,
    table_indices: Vec<u16>,
}

#[derive(Clone, Default)]
struct WOFF2Header {
    flavor: u32,
    header_version: u32,
    num_tables: u16,
    compressed_offset: u64,
    compressed_length: u32,
    uncompressed_size: u32,
    tables: Vec<Table>,      // num_tables unique tables
    ttc_fonts: Vec<TtcFont>, // metadata to help rebuild font
}

/**
 * Accumulates data we may need to reconstruct a single font. One per font
 * created for a TTC.
 */
#[derive(Clone, Default)]
struct WOFF2FontInfo {
    num_glyphs: u16,
    index_format: u16,
    num_hmetrics: u16,
    x_mins: Vec<i16>,
    table_entry_by_tag: BTreeMap<u32, u32>,
}

// Accumulates metadata as we rebuild the font
#[derive(Clone, Default)]
struct RebuildMetadata {
    header_checksum: u32, // set by WriteHeaders
    font_infos: Vec<WOFF2FontInfo>,
    // checksums for tables that have been written.
    // (tag, src_offset) => checksum. Need both because 0-length loca.
    checksums: BTreeMap<(u32, u32), TableMetadata>,
}

#[derive(Clone, Copy, Default)]
struct TableMetadata {
    checksum: u32,
    dst_offset: u32,
    dst_length: u32,
}

fn WithSign(flag: i32, baseval: i32) -> i32 {
    // Precondition: 0 <= baseval < 65536 (to avoid integer overflow)
    if (flag & 1) != 0 { baseval } else { -baseval }
}

fn _SafeIntAddition(a: i32, b: i32, result: &mut i32) -> bool {
    if PREDICT_FALSE(((a > 0) && (b > i32::MAX - a)) || ((a < 0) && (b < i32::MIN - a))) {
        return false;
    }
    *result = a + b;
    true
}

fn TripletDecode(
    flags_in: &[u8],
    in_: &[u8],
    result: &mut Vec<Point>,
    in_bytes_consumed: &mut usize,
) -> bool {
    let mut x: i32 = 0;
    let mut y: i32 = 0;

    if PREDICT_FALSE(flags_in.len() > in_.len()) {
        return FONT_COMPRESSION_FAILURE();
    }

    let mut triplet_index: usize = 0;

    for &flag in flags_in {
        let on_curve: bool = (flag >> 7) == 0;
        let flag = (flag & 0x7f) as i32;

        let n_data_bytes: usize = if flag < 84 {
            1
        } else if flag < 120 {
            2
        } else if flag < 124 {
            3
        } else {
            4
        };

        // Second condition was "triplet_index + n_data_bytes < triplet_index" in C. Clippy detected as checking for overflow
        // in a way that doesn't work in Rust (because Rust panics rather than wraps in debug mode)
        if PREDICT_FALSE(
            (triplet_index + n_data_bytes) > in_.len()
                || triplet_index.checked_add(n_data_bytes).is_none(),
        ) {
            return FONT_COMPRESSION_FAILURE();
        }

        let dx: i32;
        let dy: i32;
        if flag < 10 {
            dx = 0;
            dy = WithSign(flag, ((flag & 14) << 7) + in_[triplet_index] as i32);
        } else if flag < 20 {
            dx = WithSign(flag, (((flag - 10) & 14) << 7) + in_[triplet_index] as i32);
            dy = 0;
        } else if flag < 84 {
            let b0: i32 = flag - 20;
            let b1: i32 = in_[triplet_index] as i32;
            dx = WithSign(flag, 1 + (b0 & 0x30) + (b1 >> 4));
            dy = WithSign(flag >> 1, 1 + ((b0 & 0x0c) << 2) + (b1 & 0x0f));
        } else if flag < 120 {
            let b0: i32 = flag - 84;
            dx = WithSign(flag, 1 + ((b0 / 12) << 8) + in_[triplet_index] as i32);
            dy = WithSign(
                flag >> 1,
                1 + (((b0 % 12) >> 2) << 8) + in_[triplet_index + 1] as i32,
            );
        } else if flag < 124 {
            let b2: i32 = in_[triplet_index + 1] as i32;
            dx = WithSign(flag, ((in_[triplet_index] as i32) << 4) + (b2 >> 4));
            dy = WithSign(
                flag >> 1,
                ((b2 & 0x0f) << 8) + in_[triplet_index + 2] as i32,
            );
        } else {
            dx = WithSign(
                flag,
                ((in_[triplet_index] as i32) << 8) + in_[triplet_index + 1] as i32,
            );
            dy = WithSign(
                flag >> 1,
                ((in_[triplet_index + 2] as i32) << 8) + in_[triplet_index + 3] as i32,
            );
        }
        triplet_index += n_data_bytes;
        if !_SafeIntAddition(x, dx, &mut x) {
            return false;
        }
        if !_SafeIntAddition(y, dy, &mut y) {
            return false;
        }

        result.push(Point { x, y, on_curve }); // CHECK: was *result++
    }

    *in_bytes_consumed = triplet_index;
    true
}

// This function stores just the point data. On entry, dst points to the
// beginning of a simple glyph. Returns true on success.
fn StorePoints(
    points: &[Point],
    n_contours: u32,
    instruction_length: u32,
    has_overlap_bit: bool,
    dst: &mut [u8],
    glyph_size: &mut usize,
) -> bool {
    // I believe that n_contours < 65536, in which case this is safe. However, a
    // comment and/or an assert would be good.
    assert!(n_contours < 65536);
    let mut flag_offset: usize =
        kEndPtsOfContoursOffset + 2 * (n_contours as usize) + 2 + (instruction_length as usize);
    let mut last_flag: i32 = -1;
    let mut repeat_count: u8 = 0;
    let mut last_x: i32 = 0;
    let mut last_y: i32 = 0;
    let mut x_bytes: u32 = 0;
    let mut y_bytes: u32 = 0;

    for (i, point) in points.iter().enumerate() {
        let mut flag: i32 = if point.on_curve { kGlyfOnCurve } else { 0 };

        if has_overlap_bit && i == 0 {
            flag |= kOverlapSimple;
        }

        let dx: i32 = point.x - last_x;
        let dy: i32 = point.y - last_y;
        if dx == 0 {
            flag |= kGlyfThisXIsSame;
        } else if dx > -256 && dx < 256 {
            flag |= kGlyfXShort | (if dx > 0 { kGlyfThisXIsSame } else { 0 });
            x_bytes += 1;
        } else {
            x_bytes += 2;
        }
        if dy == 0 {
            flag |= kGlyfThisYIsSame;
        } else if dy > -256 && dy < 256 {
            flag |= kGlyfYShort | (if dy > 0 { kGlyfThisYIsSame } else { 0 });
            y_bytes += 1;
        } else {
            y_bytes += 2;
        }

        if flag == last_flag && repeat_count != 255 {
            dst[flag_offset - 1] |= kGlyfRepeat as u8;
            repeat_count += 1;
        } else {
            if repeat_count != 0 {
                if PREDICT_FALSE(flag_offset >= dst.len()) {
                    return FONT_COMPRESSION_FAILURE();
                }
                dst[flag_offset] = repeat_count;
                flag_offset += 1;
            }
            if PREDICT_FALSE(flag_offset >= dst.len()) {
                return FONT_COMPRESSION_FAILURE();
            }
            dst[flag_offset] = flag as u8;
            flag_offset += 1;
            repeat_count = 0;
        }
        last_x = point.x;
        last_y = point.y;
        last_flag = flag;
    }

    if repeat_count != 0 {
        if PREDICT_FALSE(flag_offset >= dst.len()) {
            return FONT_COMPRESSION_FAILURE();
        }
        dst[flag_offset] = repeat_count;
        flag_offset += 1;
    }
    let xy_bytes: u32 = x_bytes + y_bytes;
    if PREDICT_FALSE(
        xy_bytes < x_bytes
            || flag_offset.checked_add(xy_bytes as usize).is_none()
            || flag_offset + (xy_bytes as usize) > dst.len(),
    ) {
        return FONT_COMPRESSION_FAILURE();
    }

    let mut x_offset: usize = flag_offset;
    let mut y_offset: usize = flag_offset + (x_bytes as usize);
    last_x = 0;
    last_y = 0;
    for point in points {
        let dx: i32 = point.x - last_x;
        if dx == 0 {
            // pass
        } else if dx > -256 && dx < 256 {
            x_offset += 1;
            dst[x_offset] = dx.unsigned_abs() as u8;
        } else {
            // will always fit for valid input, but overflow is harmless
            x_offset = Store16(dst, x_offset, dx);
        }
        last_x += dx;

        let dy: i32 = point.y - last_y;
        if dy == 0 {
            // pass
        } else if dy > -256 && dy < 256 {
            y_offset += 1;
            dst[y_offset] = dy.unsigned_abs() as u8;
        } else {
            y_offset = Store16(dst, y_offset, dy);
        }
        last_y += dy;
    }

    *glyph_size = y_offset;
    true
}

/// Compute the bounding box of the coordinates, and store into a glyf buffer.
/// A precondition is that there are at least 10 bytes available.
/// dst should point to the beginning of a 'glyf' record.
fn ComputeBbox(points: &[Point], dst: &mut [u8]) {
    let mut x_min: i32 = 0;
    let mut y_min: i32 = 0;
    let mut x_max: i32 = 0;
    let mut y_max: i32 = 0;

    if !points.is_empty() {
        x_min = points[0].x;
        x_max = points[0].x;
        y_min = points[0].y;
        y_max = points[0].y;
    }
    for &Point { x, y, .. } in points.iter().skip(1) {
        x_min = x.min(x_min);
        x_max = x.max(x_max);
        y_min = y.min(y_min);
        y_max = y.max(y_max);
    }
    let mut offset: usize = 2;
    offset = Store16(dst, offset, x_min);
    offset = Store16(dst, offset, y_min);
    offset = Store16(dst, offset, x_max);
    offset = Store16(dst, offset, y_max);

    // Last value of offset is not used
    let _ = offset;
}

fn SizeOfComposite(
    composite_stream: &mut Buffer<'_>,
    size: &mut usize,
    have_instructions: &mut bool,
) -> bool {
    let start_offset: usize = composite_stream.offset();
    let mut we_have_instructions: bool = false;

    let mut flags: u16 = FLAG_MORE_COMPONENTS;
    while flags & FLAG_MORE_COMPONENTS != 0 {
        if PREDICT_FALSE(!composite_stream.ReadU16(&mut flags)) {
            return FONT_COMPRESSION_FAILURE();
        }
        we_have_instructions |= (flags & FLAG_WE_HAVE_INSTRUCTIONS) != 0;
        let mut arg_size: usize = 2; // glyph index
        if flags & FLAG_ARG_1_AND_2_ARE_WORDS != 0 {
            arg_size += 4;
        } else {
            arg_size += 2;
        }
        if flags & FLAG_WE_HAVE_A_SCALE != 0 {
            arg_size += 2;
        } else if flags & FLAG_WE_HAVE_AN_X_AND_Y_SCALE != 0 {
            arg_size += 4;
        } else if flags & FLAG_WE_HAVE_A_TWO_BY_TWO != 0 {
            arg_size += 8;
        }
        if PREDICT_FALSE(!composite_stream.Skip(arg_size)) {
            return FONT_COMPRESSION_FAILURE();
        }
    }

    *size = composite_stream.offset() - start_offset;
    *have_instructions = we_have_instructions;

    true
}

fn Pad4(out: &mut impl WOFF2Out) -> bool {
    let zeroes: [u8; 3] = [0, 0, 0];
    if PREDICT_FALSE(out.Size() + 3 < out.Size()) {
        // CHECKME: is this an overflow check? If so, ensure it doesn't panic
        return FONT_COMPRESSION_FAILURE();
    }
    let pad_bytes: usize = Round4!(out.Size()) - out.Size();
    if pad_bytes > 0 && PREDICT_FALSE(!out.Write(&zeroes[0..pad_bytes])) {
        return FONT_COMPRESSION_FAILURE();
    }

    true
}

/// Build TrueType loca table
// TODO: investigate using write-fonts
fn StoreLoca(
    loca_values: &[u32],
    index_format: i32,
    checksum: &mut u32,
    out: &mut impl WOFF2Out,
) -> bool {
    // TODO(user) figure out what index format to use based on whether max
    // offset fits into uint16_t or not
    let loca_size = loca_values.len();
    let offset_size: usize = if index_format != 0 { 4 } else { 2 };

    if PREDICT_FALSE((loca_size << 2) >> 2 != loca_size) {
        return FONT_COMPRESSION_FAILURE();
    }

    // FIXME: logic below is not correct. In original C code StoreU32 and Store16 push to the Vec
    // and ComputeULongSum can read up to 3 bytes past the end of the slice.
    // Possible the Store* functions out to take `impl Write`
    let mut loca_content: Vec<u8> = Vec::with_capacity(loca_size * offset_size);
    let mut offset: usize = 0;
    for &value in loca_values {
        if index_format != 0 {
            offset = StoreU32(&mut loca_content, offset, value);
        } else {
            offset = Store16(&mut loca_content, offset, value as i32 >> 1); // CHECKME: u32 to i32 coercion
        }
    }

    *checksum = ComputeULongSum(loca_content.as_slice(), loca_content.len());
    if PREDICT_FALSE(!out.Write(&loca_content)) {
        return FONT_COMPRESSION_FAILURE();
    }

    true
}

// Reconstruct entire glyf table based on transformed original
fn ReconstructGlyf(
    data: &[u8],
    glyf_table: &Table,
    glyf_metadata: &mut TableMetadata,
    loca_table: &Table,
    loca_metdata: &mut TableMetadata,
    info: &mut WOFF2FontInfo,
    out: &mut impl WOFF2Out,
) -> bool {
    const kNumSubStreams: usize = 7;

    let mut file = Buffer::new(&data[0..(glyf_table.transform_length as usize)]);
    let mut version: u16 = 0;
    let mut substreams: ArrayVec<&[u8], 7> = ArrayVec::new();
    let glyf_start: usize = out.Size();

    if PREDICT_FALSE(!file.ReadU16(&mut version)) {
        return FONT_COMPRESSION_FAILURE();
    }

    let mut flags: u16 = 0;
    if PREDICT_FALSE(!file.ReadU16(&mut flags)) {
        return FONT_COMPRESSION_FAILURE();
    }
    let has_overlap_bitmap: bool = (flags & FLAG_OVERLAP_SIMPLE_BITMAP) != 0;

    if PREDICT_FALSE(!file.ReadU16(&mut info.num_glyphs) || !file.ReadU16(&mut info.index_format)) {
        return FONT_COMPRESSION_FAILURE();
    }

    // https://dev.w3.org/webfonts/WOFF2/spec/#conform-mustRejectLoca
    // dst_length here is origLength in the spec
    let expected_loca_dst_length: u32 =
        (if info.index_format != 0 { 4 } else { 2 }) * (info.num_glyphs as u32 + 1);

    if PREDICT_FALSE(loca_table.dst_length != expected_loca_dst_length) {
        return FONT_COMPRESSION_FAILURE();
    }

    let mut offset: usize = (2 + kNumSubStreams) * 4;
    if PREDICT_FALSE(offset > glyf_table.transform_length as usize) {
        return FONT_COMPRESSION_FAILURE();
    }

    // Invariant from here on: data_size >= offset
    for _ in 0..kNumSubStreams {
        let mut substream_size: u32 = 0;
        if PREDICT_FALSE(!file.ReadU32(&mut substream_size)) {
            return FONT_COMPRESSION_FAILURE();
        }
        if PREDICT_FALSE(substream_size > glyf_table.transform_length - offset as u32) {
            return FONT_COMPRESSION_FAILURE();
        }
        substreams.push(&data[offset..(offset + (substream_size as usize))]);

        offset += substream_size as usize;
    }

    let mut n_contour_stream = Buffer::new(substreams[0]);
    let mut n_points_stream = Buffer::new(substreams[1]);
    let mut flag_stream = Buffer::new(substreams[2]);
    let mut glyph_stream = Buffer::new(substreams[3]);
    let mut composite_stream = Buffer::new(substreams[4]);
    let mut bbox_stream = Buffer::new(substreams[5]);
    let mut instruction_stream = Buffer::new(substreams[6]);

    let mut overlap_bitmap: Option<&[u8]> = None;
    if has_overlap_bitmap {
        let overlap_bitmap_length = (info.num_glyphs as usize + 7) >> 3;
        overlap_bitmap = Some(&data[offset..(offset + (overlap_bitmap_length))]);
        if PREDICT_FALSE(overlap_bitmap_length > glyf_table.transform_length as usize - offset) {
            return FONT_COMPRESSION_FAILURE();
        }
    }

    let mut loca_values: Vec<u32> = Vec::with_capacity(info.num_glyphs as usize + 1);

    // Safe because num_glyphs is bounded
    let bitmap_length: usize = ((info.num_glyphs as usize + 31) >> 5) << 2;
    if !bbox_stream.Skip(bitmap_length) {
        return FONT_COMPRESSION_FAILURE();
    }

    // Temp buffer for glyph's.
    // let glyph_buf_size : usize = kDefaultGlyphBuf;
    // std::unique_ptr<uint8_t[]> glyph_buf(new uint8_t[glyph_buf_size]);
    let mut glyph_buf: Vec<u8> = Vec::with_capacity(kDefaultGlyphBuf);
    info.x_mins.resize(info.num_glyphs as usize, 0); // explicit 0 added in port. I think this is implicit in C++.

    for i in 0..(info.num_glyphs as usize) {
        let mut glyph_size: usize = 0;
        let mut n_contours: u16 = 0;
        let bbox_bitmap: &[u8] = bbox_stream.buffer();
        let have_bbox = (bbox_bitmap[i >> 3] & (0x80 >> (i & 7))) != 0;

        if PREDICT_FALSE(!n_contour_stream.ReadU16(&mut n_contours)) {
            return FONT_COMPRESSION_FAILURE();
        }

        if n_contours == 0xffff {
            // composite glyph
            let mut have_instructions = false;
            let mut instruction_size: u32 = 0;
            if PREDICT_FALSE(!have_bbox) {
                // composite glyphs must have an explicit bbox
                return FONT_COMPRESSION_FAILURE();
            }

            let mut composite_size: usize = 0;
            if PREDICT_FALSE(!SizeOfComposite(
                &mut composite_stream,
                &mut composite_size,
                &mut have_instructions,
            )) {
                return FONT_COMPRESSION_FAILURE();
            }
            if have_instructions
                && PREDICT_FALSE(!Read255UShort(&mut glyph_stream, &mut instruction_size))
            {
                return FONT_COMPRESSION_FAILURE();
            }

            let size_needed: usize = 12 + composite_size + (instruction_size as usize);
            if PREDICT_FALSE(glyph_buf.len() < size_needed) {
                glyph_buf.resize(size_needed, 0); // CHECK: with .reset(..) in C++
            }

            glyph_size = Store16(glyph_buf.as_mut_slice(), glyph_size, n_contours as i32); // CHECK: u16 to i32 conversion
            if PREDICT_FALSE(!bbox_stream.Read(&mut glyph_buf[glyph_size..], 8)) {
                return FONT_COMPRESSION_FAILURE();
            }
            glyph_size += 8;

            if PREDICT_FALSE(!composite_stream.Read(&mut glyph_buf[glyph_size..], composite_size)) {
                return FONT_COMPRESSION_FAILURE();
            }
            glyph_size += composite_size;
            if have_instructions {
                glyph_size = Store16(
                    glyph_buf.as_mut_slice(),
                    glyph_size,
                    instruction_size as i32,
                );
                if PREDICT_FALSE(
                    !instruction_stream
                        .Read(&mut glyph_buf[glyph_size..], instruction_size as usize),
                ) {
                    return FONT_COMPRESSION_FAILURE();
                }
                glyph_size += instruction_size as usize;
            }
        } else if n_contours > 0 {
            // simple glyph
            let mut n_points_vec = Vec::with_capacity(n_contours as usize);
            let mut total_n_points: u32 = 0;
            let mut n_points_contour: u32 = 0;
            for _ in 0..n_contours {
                if PREDICT_FALSE(!Read255UShort(&mut n_points_stream, &mut n_points_contour)) {
                    return FONT_COMPRESSION_FAILURE();
                }
                n_points_vec.push(n_points_contour);
                if PREDICT_FALSE(total_n_points.checked_add(n_points_contour).is_none()) {
                    return FONT_COMPRESSION_FAILURE();
                }
                total_n_points += n_points_contour;
            }
            let flag_size: usize = total_n_points as usize;
            if PREDICT_FALSE(flag_size > flag_stream.len() - flag_stream.offset()) {
                return FONT_COMPRESSION_FAILURE();
            }

            let flags_buf = flag_stream.remaining_as_slice();
            let triplet_buf = glyph_stream.remaining_as_slice();

            let mut triplet_bytes_consumed: usize = 0;

            let mut points = Vec::with_capacity(total_n_points as usize);
            if PREDICT_FALSE(!TripletDecode(
                flags_buf,
                triplet_buf,
                &mut points,
                &mut triplet_bytes_consumed,
            )) {
                return FONT_COMPRESSION_FAILURE();
            }

            if PREDICT_FALSE(!flag_stream.Skip(flag_size)) {
                return FONT_COMPRESSION_FAILURE();
            }

            if PREDICT_FALSE(!glyph_stream.Skip(triplet_bytes_consumed)) {
                return FONT_COMPRESSION_FAILURE();
            }

            let mut instruction_size: u32 = 0;
            if PREDICT_FALSE(!Read255UShort(&mut glyph_stream, &mut instruction_size)) {
                return FONT_COMPRESSION_FAILURE();
            }

            if PREDICT_FALSE(total_n_points >= (1 << 27) || instruction_size >= (1 << 30)) {
                return FONT_COMPRESSION_FAILURE();
            }

            let size_needed: usize = 12
                + 2 * (n_contours as usize)
                + 5 * (total_n_points as usize)
                + (instruction_size as usize);
            if PREDICT_FALSE(glyph_buf.len() < size_needed) {
                glyph_buf.resize(size_needed, 0);
            }

            glyph_size = Store16(glyph_buf.as_mut_slice(), glyph_size, n_contours as i32);
            if have_bbox {
                if PREDICT_FALSE(!bbox_stream.Read(&mut glyph_buf[glyph_size..], 8)) {
                    return FONT_COMPRESSION_FAILURE();
                }
            } else {
                ComputeBbox(points.as_slice(), glyph_buf.as_mut_slice());
            }
            glyph_size = kEndPtsOfContoursOffset;
            let mut end_point: i32 = -1;
            for countour in n_points_vec {
                end_point += countour as i32;
                if PREDICT_FALSE(end_point >= 65536) {
                    return FONT_COMPRESSION_FAILURE();
                }
                glyph_size = Store16(glyph_buf.as_mut_slice(), glyph_size, end_point);
            }

            glyph_size = Store16(
                glyph_buf.as_mut_slice(),
                glyph_size,
                instruction_size as i32,
            );
            if PREDICT_FALSE(
                !instruction_stream.Read(&mut glyph_buf[glyph_size..], instruction_size as usize),
            ) {
                return FONT_COMPRESSION_FAILURE();
            }
            glyph_size += instruction_size as usize;

            let has_overlap_bit: bool =
                overlap_bitmap.is_some_and(|bitmap| (bitmap[i >> 3] & (0x80 >> (i & 7))) != 0);

            if PREDICT_FALSE(!StorePoints(
                points.as_slice(),
                n_contours as u32,
                instruction_size,
                has_overlap_bit,
                glyph_buf.as_mut_slice(),
                &mut glyph_size,
            )) {
                return FONT_COMPRESSION_FAILURE();
            }
        } else {
            // n_contours == 0; empty glyph. Must NOT have a bbox.
            if PREDICT_FALSE(have_bbox) {
                #[cfg(feature = "font_compression_bin")]
                eprintln!("Empty glyph has a bbox");
                return FONT_COMPRESSION_FAILURE();
            }
        }

        loca_values[i] = (out.Size() - glyf_start) as u32;
        if PREDICT_FALSE(!out.Write(glyph_buf.as_mut_slice())) {
            return FONT_COMPRESSION_FAILURE();
        }

        // TODO(user) Old code aligned glyphs ... but do we actually need to?
        if PREDICT_FALSE(!Pad4(out)) {
            return FONT_COMPRESSION_FAILURE();
        }

        glyf_metadata.checksum += ComputeULongSum(glyph_buf.as_mut_slice(), glyph_size);

        // We may need x_min to reconstruct 'hmtx'
        if n_contours > 0 {
            let mut x_min_buf = Buffer::new(&glyph_buf[2..4]);
            if PREDICT_FALSE(!x_min_buf.ReadS16(&mut info.x_mins[i])) {
                return FONT_COMPRESSION_FAILURE();
            }
        }
    }

    // glyf_table dst_offset was set by ReconstructFont
    glyf_metadata.dst_length = out.Size() as u32 - glyf_table.dst_offset;
    // loca[n] will be equal the length of the glyph data ('glyf') table
    loca_values[info.num_glyphs as usize] = glyf_table.dst_length;
    if PREDICT_FALSE(!StoreLoca(
        &loca_values,
        info.index_format as i32,
        &mut loca_metdata.checksum,
        out,
    )) {
        return FONT_COMPRESSION_FAILURE();
    }
    loca_metdata.dst_offset = out.Size() as u32;
    loca_metdata.dst_length = out.Size() as u32 - loca_table.dst_offset;

    true
}

fn FindTable<'b>(tables: &[&'b Table], tag: u32) -> Option<&'b Table> {
    tables.iter().find(|t| t.tag == tag).copied()
}

/// Get numberOfHMetrics, https://www.microsoft.com/typography/otspec/hhea.htm
fn ReadNumHMetrics(data: &[u8], num_hmetrics: &mut u16) -> bool {
    // Skip 34 to reach 'hhea' numberOfHMetrics
    let mut buffer = Buffer::new(data);
    if PREDICT_FALSE(!buffer.Skip(34) || !buffer.ReadU16(num_hmetrics)) {
        return FONT_COMPRESSION_FAILURE();
    }
    true
}

// http://dev.w3.org/webfonts/WOFF2/spec/Overview.html#hmtx_table_format
fn ReconstructTransformedHmtx(
    transformed_buf: &[u8],
    num_glyphs: u16,
    num_hmetrics: u16,
    x_mins: &[i16],
    metadata: &mut TableMetadata,
    out: &mut impl WOFF2Out,
) -> bool {
    let mut hmtx_buff_in = Buffer::new(transformed_buf);

    let mut hmtx_flags: u8 = 0;
    if PREDICT_FALSE(!hmtx_buff_in.ReadU8(&mut hmtx_flags)) {
        return FONT_COMPRESSION_FAILURE();
    }

    let mut advance_widths: Vec<u16> = Vec::with_capacity(num_hmetrics as usize);
    let mut lsbs: Vec<i16> = Vec::with_capacity(num_hmetrics as usize);
    let has_proportional_lsbs: bool = (hmtx_flags & 1) == 0;
    let has_monospace_lsbs: bool = (hmtx_flags & 2) == 0;

    // Bits 2-7 are reserved and MUST be zero.
    if (hmtx_flags & 0xFC) != 0 {
        #[cfg(feature = "font_compression_bin")]
        eprintln!("Illegal hmtx flags; bits 2-7 must be 0");
        return FONT_COMPRESSION_FAILURE();
    }

    // you say you transformed but there is little evidence of it
    if has_proportional_lsbs && has_monospace_lsbs {
        return FONT_COMPRESSION_FAILURE();
    }

    assert!(x_mins.len() == num_glyphs as usize);

    // num_glyphs 0 is OK if there is no 'glyf' but cannot then xform 'hmtx'.
    if PREDICT_FALSE(num_hmetrics > num_glyphs) {
        return FONT_COMPRESSION_FAILURE();
    }

    // https://www.microsoft.com/typography/otspec/hmtx.htm
    // "...only one entry need be in the array, but that entry is required."
    if PREDICT_FALSE(num_hmetrics < 1) {
        return FONT_COMPRESSION_FAILURE();
    }

    for _ in 0..num_hmetrics {
        let mut advance_width: u16 = 0;
        if PREDICT_FALSE(!hmtx_buff_in.ReadU16(&mut advance_width)) {
            return FONT_COMPRESSION_FAILURE();
        }
        advance_widths.push(advance_width);
    }

    for i in 0..num_hmetrics {
        let mut lsb: i16 = 0;
        if has_proportional_lsbs {
            if PREDICT_FALSE(!hmtx_buff_in.ReadS16(&mut lsb)) {
                return FONT_COMPRESSION_FAILURE();
            }
        } else {
            lsb = x_mins[i as usize];
        }
        lsbs.push(lsb);
    }

    for i in num_hmetrics..num_glyphs {
        let mut lsb: i16 = 0;
        if has_monospace_lsbs {
            if PREDICT_FALSE(!hmtx_buff_in.ReadS16(&mut lsb)) {
                return FONT_COMPRESSION_FAILURE();
            }
        } else {
            lsb = x_mins[i as usize];
        }
        lsbs.push(lsb);
    }

    // bake me a shiny new hmtx table
    let hmtx_output_size: usize = 2 * (num_glyphs as usize) + 2 * (num_hmetrics as usize);
    let mut hmtx_table: Vec<u8> = Vec::with_capacity(hmtx_output_size);
    // uint8_t* dst = &hmtx_table[0];
    let mut dst_offset: usize = 0;
    for i in 0..num_glyphs {
        if i < num_hmetrics {
            Store16_mut(
                hmtx_table.as_mut_slice(),
                &mut dst_offset,
                advance_widths[i as usize].into(),
            );
        }
        Store16_mut(
            hmtx_table.as_mut_slice(),
            &mut dst_offset,
            lsbs[i as usize].into(),
        );
    }

    // metadata.dst_offset set in ReconstructFont
    metadata.checksum = ComputeULongSum(&hmtx_table, hmtx_output_size);
    metadata.dst_length = hmtx_output_size as u32;
    if PREDICT_FALSE(!out.Write(&hmtx_table[0..hmtx_output_size])) {
        return FONT_COMPRESSION_FAILURE();
    }

    true
}

fn Woff2Uncompress(src: &[u8], dst: &mut [u8]) -> bool {
    let info = brotli_decode(src, dst);

    // CHECK: why is output buffer fixed size? Is there a better way to decode?
    if PREDICT_FALSE(
        !matches!(info.result, BrotliResult::ResultSuccess) || info.decoded_size != dst.len(),
    ) {
        return FONT_COMPRESSION_FAILURE();
    }

    true
}

fn ReadTableDirectory(file: &mut Buffer<'_>, tables: &mut Vec<Table>, num_tables: usize) -> bool {
    let mut src_offset: usize = 0;
    for _ in 0..num_tables {
        let mut flag_byte: u8 = 0;
        if PREDICT_FALSE(!file.ReadU8(&mut flag_byte)) {
            return FONT_COMPRESSION_FAILURE();
        }
        let mut tag: u32 = 0;
        if flag_byte & 0x3f == 0x3f {
            if PREDICT_FALSE(!file.ReadU32(&mut tag)) {
                return FONT_COMPRESSION_FAILURE();
            }
        } else {
            tag = kKnownTags[(flag_byte & 0x3f) as usize];
        }
        let mut flags: u32 = 0;
        let xform_version: u8 = (flag_byte >> 6) & 0x03;

        // 0 means xform for glyph/loca, non-0 for others
        if tag == kGlyfTableTag || tag == kLocaTableTag {
            if xform_version == 0 {
                flags |= kWoff2FlagsTransform;
            }
        } else if xform_version != 0 {
            flags |= kWoff2FlagsTransform;
        }
        flags |= xform_version as u32;

        let mut dst_length: u32 = 0;
        if PREDICT_FALSE(!ReadBase128(file, &mut dst_length)) {
            return FONT_COMPRESSION_FAILURE();
        }
        let mut transform_length: u32 = dst_length;
        if (flags & kWoff2FlagsTransform) != 0 {
            if PREDICT_FALSE(!ReadBase128(file, &mut transform_length)) {
                return FONT_COMPRESSION_FAILURE();
            }
            if PREDICT_FALSE(tag == kLocaTableTag && transform_length != 0) {
                return FONT_COMPRESSION_FAILURE();
            }
        }
        if PREDICT_FALSE(src_offset.checked_add(transform_length as usize).is_none()) {
            return FONT_COMPRESSION_FAILURE();
        }

        tables.push(Table {
            tag,
            flags,
            src_offset: src_offset as u32,
            src_length: transform_length,
            transform_length,
            dst_offset: 0, // Filled in later
            dst_length,
        });
        src_offset += transform_length as usize;
    }

    true
}

// Writes a single Offset Table entry
fn StoreOffsetTable(result: &mut [u8], mut offset: usize, flavor: u32, num_tables: u16) -> usize {
    offset = StoreU32(result, offset, flavor); // sfnt version
    offset = Store16(result, offset, num_tables.into()); // num_tables
    let mut max_pow2: u16 = 0;
    while 1u16 << (max_pow2 + 1) <= num_tables {
        max_pow2 += 1;
    }
    let output_search_range: u16 = (1u16 << max_pow2) << 4;
    offset = Store16(result, offset, output_search_range.into()); // searchRange
    offset = Store16(result, offset, max_pow2.into()); // entrySelector
    // rangeShift
    offset = Store16(
        result,
        offset,
        ((num_tables << 4) - output_search_range).into(),
    );
    offset
}

fn StoreTableEntry(result: &mut [u8], mut offset: usize, tag: u32) -> usize {
    offset = StoreU32(result, offset, tag);
    offset = StoreU32(result, offset, 0);
    offset = StoreU32(result, offset, 0);
    offset = StoreU32(result, offset, 0);
    offset
}

// First table goes after all the headers, table directory, etc
fn ComputeOffsetToFirstTable(hdr: &WOFF2Header) -> usize {
    let mut offset: usize = kSfntHeaderSize + kSfntEntrySize * (hdr.num_tables as usize);
    if hdr.header_version != 0 {
        offset = CollectionHeaderSize(hdr.header_version, hdr.ttc_fonts.len() as u32)
            + kSfntHeaderSize * hdr.ttc_fonts.len();
        for ttc_font in hdr.ttc_fonts.iter() {
            offset += kSfntEntrySize * ttc_font.table_indices.len();
        }
    }
    offset
}

fn Tables(hdr: &WOFF2Header, font_index: usize) -> Vec<&Table> {
    let mut tables: Vec<&Table> = Vec::new();
    if PREDICT_FALSE(hdr.header_version != 0) {
        tables.reserve(hdr.ttc_fonts[font_index].table_indices.len());
        for index in hdr.ttc_fonts[font_index].table_indices.iter() {
            tables.push(&hdr.tables[*index as usize]);
        }
    } else {
        tables.reserve(hdr.tables.len());
        for table in hdr.tables.iter() {
            tables.push(table);
        }
    }
    tables
}

// Offset tables assumed to have been written in with 0's initially.
// WOFF2Header isn't const so we can use [] instead of at() (which upsets FF)
fn ReconstructFont(
    transformed_buf: &mut [u8],
    metadata: &mut RebuildMetadata,
    hdr: &WOFF2Header,
    font_index: usize,
    out: &mut impl WOFF2Out,
) -> bool {
    let mut dest_offset: usize = out.Size();
    let mut table_entry: [u8; 12] = [0; 12];
    let info: &mut WOFF2FontInfo = &mut metadata.font_infos[font_index];
    let checksums = &mut metadata.checksums;
    let tables: Vec<&Table> = Tables(hdr, font_index);

    // 'glyf' without 'loca' doesn't make sense
    let glyf_table = FindTable(&tables, kGlyfTableTag);
    let loca_table = FindTable(&tables, kLocaTableTag);

    // Check the glyf and loca tables are compatible with each other
    match (glyf_table, loca_table) {
        (Some(glyf_table), Some(loca_table)) => {
            if PREDICT_FALSE(
                (glyf_table.flags & kWoff2FlagsTransform)
                    != (loca_table.flags & kWoff2FlagsTransform),
            ) {
                #[cfg(feature = "font_compression_bin")]
                eprint!("Cannot transform just one of glyf/loca");
                return FONT_COMPRESSION_FAILURE();
            }
        }
        (Some(_), None) | (None, Some(_)) => {
            #[cfg(feature = "font_compression_bin")]
            eprint!("Cannot have just one of glyf/loca");
            return FONT_COMPRESSION_FAILURE();
        }
        (None, None) => {}
    }

    let mut font_checksum: u32 = if hdr.header_version == 0 {
        metadata.header_checksum
    } else {
        hdr.ttc_fonts[font_index].header_checksum
    };

    let mut loca_metadata = TableMetadata {
        checksum: 0,
        dst_offset: 0,
        dst_length: 0,
    };
    for table in tables.iter() {
        let checksum_key: (u32, u32) = (table.tag, table.src_offset);
        let reused: bool = checksums.contains_key(&checksum_key);
        if PREDICT_FALSE(font_index == 0 && reused) {
            return FONT_COMPRESSION_FAILURE();
        }

        // TODO(user) a collection with optimized hmtx that reused glyf/loca
        // would fail. We don't optimize hmtx for collections yet.
        if PREDICT_FALSE(
            (table.src_offset as u64) + (table.src_length as u64) > (transformed_buf.len() as u64),
        ) {
            return FONT_COMPRESSION_FAILURE();
        }

        if table.tag == kHheaTableTag
            && !ReadNumHMetrics(
                &transformed_buf
                    [(table.src_offset as usize)..((table.src_offset + table.src_length) as usize)],
                &mut info.num_hmetrics,
            )
        {
            return FONT_COMPRESSION_FAILURE();
        }

        // let checksum: u32;
        // let dst_offset: u32;
        // let dst_length: u32;
        // If we are processing a reused table then access the already-computed metadata
        let metadata = if reused {
            checksums[&checksum_key]
        } else {
            // Any table which not need to be transformed
            let metadata = if (table.flags & kWoff2FlagsTransform) != kWoff2FlagsTransform {
                if table.tag == kHeadTableTag {
                    if PREDICT_FALSE(table.src_length < 12) {
                        return FONT_COMPRESSION_FAILURE();
                    }

                    // NOTE: Writing to the decompressed WOFF seems weird
                    // checkSumAdjustment = 0
                    StoreU32(&mut transformed_buf[(table.src_offset as usize)..], 8, 0);
                }
                let metadata = TableMetadata {
                    dst_offset: dest_offset as u32,
                    dst_length: table.src_length, // CHECK: logic is that if table isn't transformed that length wont change
                    checksum: ComputeULongSum(
                        &transformed_buf[(table.src_offset as usize)..],
                        table.src_length as usize,
                    ),
                };
                if PREDICT_FALSE(!out.Write(
                    &transformed_buf[(table.src_offset as usize)
                        ..((table.src_offset + table.src_length) as usize)],
                )) {
                    return FONT_COMPRESSION_FAILURE();
                }
                metadata
            }
            // glyf table (also process loca table)
            else if table.tag == kGlyfTableTag {
                let mut glyf_metadata = TableMetadata {
                    checksum: 0,
                    dst_offset: dest_offset as u32,
                    dst_length: 0,
                };

                let loca_table = loca_table
                    .expect("We already returned an error if glyf is present but loca isn't");
                if PREDICT_FALSE(!ReconstructGlyf(
                    &transformed_buf[(table.src_offset as usize)..],
                    table,
                    &mut glyf_metadata,
                    loca_table,
                    &mut loca_metadata,
                    info,
                    out,
                )) {
                    return FONT_COMPRESSION_FAILURE();
                }
                glyf_metadata
            }
            // loca table (retrieve data)
            else if table.tag == kLocaTableTag {
                // All the work was done by ReconstructGlyf. We already know checksum.
                loca_metadata
            }
            // hmtx table
            else if table.tag == kHmtxTableTag {
                let mut hmtx_metadata = TableMetadata {
                    checksum: 0,
                    dst_offset: dest_offset as u32,
                    dst_length: 0,
                };

                // Tables are sorted so all the info we need has been gathered.
                if PREDICT_FALSE(!ReconstructTransformedHmtx(
                    &transformed_buf[(table.src_offset as usize)..],
                    info.num_glyphs,
                    info.num_hmetrics,
                    info.x_mins.as_slice(),
                    &mut hmtx_metadata,
                    out,
                )) {
                    return FONT_COMPRESSION_FAILURE();
                }
                hmtx_metadata
            } else {
                return FONT_COMPRESSION_FAILURE(); // transform unknown
            };

            // Insert the computed table metadata into the cache in case the table is reused
            checksums.insert(checksum_key, metadata);

            metadata
        };
        font_checksum += metadata.checksum;

        // update the table entry with real values.
        StoreU32(&mut table_entry, 0, metadata.checksum);
        StoreU32(&mut table_entry, 4, metadata.dst_offset);
        StoreU32(&mut table_entry, 8, metadata.dst_length);
        if PREDICT_FALSE(!out.WriteAtOffset(
            &table_entry,
            info.table_entry_by_tag[&table.tag] as usize + 4,
        )) {
            return FONT_COMPRESSION_FAILURE();
        }

        // We replaced 0's. Update overall checksum.
        font_checksum += ComputeULongSum(&table_entry, 12);

        if PREDICT_FALSE(!Pad4(out)) {
            return FONT_COMPRESSION_FAILURE();
        }

        if PREDICT_FALSE((table.dst_offset + table.dst_length) as usize > out.Size()) {
            return FONT_COMPRESSION_FAILURE();
        }
        dest_offset = out.Size();
    }

    // Update 'head' checkSumAdjustment. We already set it to 0 and summed font.
    let head_table = FindTable(&tables, kHeadTableTag);
    if let Some(head_table) = head_table {
        if PREDICT_FALSE(head_table.dst_length < 12) {
            return FONT_COMPRESSION_FAILURE();
        }
        let mut checksum_adjustment: [u8; 4] = [0; 4];
        StoreU32(&mut checksum_adjustment, 0, 0xB1B0AFBA - font_checksum);
        if PREDICT_FALSE(
            !out.WriteAtOffset(&checksum_adjustment, head_table.dst_offset as usize + 8),
        ) {
            return FONT_COMPRESSION_FAILURE();
        }
    }

    true
}

fn ReadWOFF2Header(data: &[u8], hdr: &mut WOFF2Header) -> bool {
    let mut file = Buffer::new(data);

    let mut signature: u32 = 0;
    if PREDICT_FALSE(
        !file.ReadU32(&mut signature)
            || signature != kWoff2Signature
            || !file.ReadU32(&mut hdr.flavor),
    ) {
        return FONT_COMPRESSION_FAILURE();
    }

    // TODO(user): Should call IsValidVersionTag() here.

    let mut reported_length: u32 = 0;
    if PREDICT_FALSE(!file.ReadU32(&mut reported_length) || data.len() != reported_length as usize)
    {
        return FONT_COMPRESSION_FAILURE();
    }
    if PREDICT_FALSE(!file.ReadU16(&mut hdr.num_tables) || hdr.num_tables == 0) {
        return FONT_COMPRESSION_FAILURE();
    }

    // We don't care about these fields of the header:
    //   uint16_t reserved
    //   uint32_t total_sfnt_size, we don't believe this, will compute later
    if PREDICT_FALSE(!file.Skip(6)) {
        return FONT_COMPRESSION_FAILURE();
    }
    if PREDICT_FALSE(!file.ReadU32(&mut hdr.compressed_length)) {
        return FONT_COMPRESSION_FAILURE();
    }
    // We don't care about these fields of the header:
    //   uint16_t major_version, minor_version
    if PREDICT_FALSE(!file.Skip(2 * 2)) {
        return FONT_COMPRESSION_FAILURE();
    }
    let mut meta_offset: u32 = 0;
    let mut meta_length: u32 = 0;
    let mut meta_length_orig: u32 = 0;
    if PREDICT_FALSE(
        !file.ReadU32(&mut meta_offset)
            || !file.ReadU32(&mut meta_length)
            || !file.ReadU32(&mut meta_length_orig),
    ) {
        return FONT_COMPRESSION_FAILURE();
    }
    if meta_offset != 0 {
        if PREDICT_FALSE(
            meta_offset as usize >= data.len()
                || data.len() - (meta_offset as usize) < meta_length as usize,
        ) {
            return FONT_COMPRESSION_FAILURE();
        }
    }
    let mut priv_offset: u32 = 0;
    let mut priv_length: u32 = 0;
    if PREDICT_FALSE(!file.ReadU32(&mut priv_offset) || !file.ReadU32(&mut priv_length)) {
        return FONT_COMPRESSION_FAILURE();
    }
    if priv_offset != 0 {
        if PREDICT_FALSE(
            priv_offset as usize >= data.len()
                || data.len() - (priv_offset as usize) < priv_length as usize,
        ) {
            return FONT_COMPRESSION_FAILURE();
        }
    }
    hdr.tables.reserve_exact(hdr.num_tables as usize);
    if PREDICT_FALSE(!ReadTableDirectory(
        &mut file,
        &mut hdr.tables,
        hdr.num_tables as usize,
    )) {
        return FONT_COMPRESSION_FAILURE();
    }

    // Before we sort for output the last table end is the uncompressed size.
    let last_table = hdr.tables.last().expect("Font must have at last one table"); // CHECK: have we already validated this?
    hdr.uncompressed_size = last_table.src_offset + last_table.src_length;
    if PREDICT_FALSE(hdr.uncompressed_size < last_table.src_offset) {
        return FONT_COMPRESSION_FAILURE();
    }

    hdr.header_version = 0;

    if hdr.flavor == kTtcFontFlavor {
        if PREDICT_FALSE(!file.ReadU32(&mut hdr.header_version)) {
            return FONT_COMPRESSION_FAILURE();
        }
        if PREDICT_FALSE(hdr.header_version != 0x00010000 && hdr.header_version != 0x00020000) {
            return FONT_COMPRESSION_FAILURE();
        }
        let mut num_fonts: u32 = 0;
        if PREDICT_FALSE(!Read255UShort(&mut file, &mut num_fonts) || num_fonts == 0) {
            return FONT_COMPRESSION_FAILURE();
        }
        hdr.ttc_fonts.reserve_exact(num_fonts as usize);

        for _ in 0..num_fonts {
            let mut num_tables: u32 = 0;
            if PREDICT_FALSE(!Read255UShort(&mut file, &mut num_tables) || num_tables == 0) {
                return FONT_COMPRESSION_FAILURE();
            }
            let mut flavor: u32 = 0;
            if PREDICT_FALSE(!file.ReadU32(&mut flavor)) {
                return FONT_COMPRESSION_FAILURE();
            }

            let mut table_indices = Vec::with_capacity(num_tables as usize);

            let mut glyf_idx: u32 = 0;
            let mut loca_idx: u32 = 0;

            for _ in 0..num_tables {
                let mut table_idx: u32 = 0;
                if PREDICT_FALSE(
                    !Read255UShort(&mut file, &mut table_idx) || table_idx >= hdr.num_tables as u32,
                ) {
                    return FONT_COMPRESSION_FAILURE();
                }
                table_indices.push(table_idx as u16);

                let table = &hdr.tables[table_idx as usize];
                if table.tag == kLocaTableTag {
                    loca_idx = table_idx;
                }
                if table.tag == kGlyfTableTag {
                    glyf_idx = table_idx;
                }
            }

            // if we have both glyf and loca make sure they are consecutive
            // if we have just one we'll reject the font elsewhere
            if glyf_idx > 0 || loca_idx > 0 {
                if PREDICT_FALSE(glyf_idx > loca_idx || loca_idx - glyf_idx != 1) {
                    #[cfg(feature = "font_compression_bin")]
                    eprint!("TTC font {i} has non-consecutive glyf/loca");
                    return FONT_COMPRESSION_FAILURE();
                }
            }

            hdr.ttc_fonts.push(TtcFont {
                flavor,
                dst_offset: 0,
                header_checksum: 0,
                table_indices,
            })
        }
    }

    hdr.compressed_offset = file.offset() as u64;
    if PREDICT_FALSE(hdr.compressed_offset > u32::MAX as u64) {
        return FONT_COMPRESSION_FAILURE();
    }
    let mut src_offset: u64 = Round4!(hdr.compressed_offset + hdr.compressed_length as u64);

    if PREDICT_FALSE(src_offset > data.len() as u64) {
        #[cfg(feature = "font_compression_bin")]
        {
            let first_table_offset: usize = ComputeOffsetToFirstTable(hdr);
            let dst_offset: u64 = first_table_offset as u64;
            eprint!("offset fail; src_offset {src_offset} length {length} dst_offset {dst_offset}");
        }
        return FONT_COMPRESSION_FAILURE();
    }

    if meta_offset != 0 {
        if PREDICT_FALSE(src_offset != meta_offset as u64) {
            return FONT_COMPRESSION_FAILURE();
        }
        src_offset = Round4!(meta_offset + meta_length) as u64;
        if PREDICT_FALSE(src_offset > u32::MAX as u64) {
            return FONT_COMPRESSION_FAILURE();
        }
    }

    if priv_offset != 0 {
        if PREDICT_FALSE(src_offset != priv_offset as u64) {
            return FONT_COMPRESSION_FAILURE();
        }
        src_offset = Round4!(priv_offset + priv_length) as u64;
        if PREDICT_FALSE(src_offset > u32::MAX as u64) {
            return FONT_COMPRESSION_FAILURE();
        }
    }

    if PREDICT_FALSE(src_offset != Round4!(data.len() as u64)) {
        return FONT_COMPRESSION_FAILURE();
    }

    true
}

/// Write everything before the actual table data
fn WriteHeaders(
    _data: &[u8], // weird, but this param isn't used in the C version
    metadata: &mut RebuildMetadata,
    hdr: &mut WOFF2Header,
    out: &mut impl WOFF2Out,
) -> bool {
    let size_of_header = ComputeOffsetToFirstTable(hdr);
    let mut output: Vec<u8> = vec![0; size_of_header];

    // Re-order tables in output (OTSpec) order
    let mut sorted_tables: Vec<Table> = hdr.tables.clone();
    if hdr.header_version != 0 {
        // collection; we have to sort the table offset vector in each font
        for ttc_font in &mut hdr.ttc_fonts {
            ttc_font
                .table_indices
                .sort_by_cached_key(|idx| hdr.tables[*idx as usize].tag);
        }
    } else {
        sorted_tables.sort_by_key(|table| table.tag);
    }

    // Start building the font
    let result = &mut output;
    let mut offset: usize = 0;
    if hdr.header_version != 0 {
        // TTC header
        offset = StoreU32(result, offset, hdr.flavor); // TAG TTCTag
        offset = StoreU32(result, offset, hdr.header_version); // FIXED Version
        offset = StoreU32(result, offset, hdr.ttc_fonts.len() as u32); // ULONG numFonts
        // Space for ULONG OffsetTable[numFonts] (zeroed initially)
        let mut offset_table: usize = offset; // keep start of offset table for later
        for _ in 0..hdr.ttc_fonts.len() {
            offset = StoreU32(result, offset, 0); // will fill real values in later
        }
        // space for DSIG fields for header v2
        if hdr.header_version == 0x00020000 {
            offset = StoreU32(result, offset, 0); // ULONG ulDsigTag
            offset = StoreU32(result, offset, 0); // ULONG ulDsigLength
            offset = StoreU32(result, offset, 0); // ULONG ulDsigOffset
        }

        // write Offset Tables and store the location of each in TTC Header
        metadata
            .font_infos
            .resize(hdr.ttc_fonts.len(), WOFF2FontInfo::default());
        for i in 0..hdr.ttc_fonts.len() {
            let ttc_font = &mut hdr.ttc_fonts[i];

            // write Offset Table location into TTC Header
            offset_table = StoreU32(result, offset_table, offset as u32);

            // write the actual offset table so our header doesn't lie
            ttc_font.dst_offset = offset as u32;
            offset = StoreOffsetTable(
                result,
                offset,
                ttc_font.flavor,
                ttc_font.table_indices.len() as u16,
            );

            for table_index in &ttc_font.table_indices {
                let tag: u32 = hdr.tables[*table_index as usize].tag;
                metadata.font_infos[i]
                    .table_entry_by_tag
                    .insert(tag, offset as u32);
                offset = StoreTableEntry(result, offset, tag);
            }

            ttc_font.header_checksum = ComputeULongSum(
                &result[(ttc_font.dst_offset as usize)..],
                offset - ttc_font.dst_offset as usize,
            );
        }
    } else {
        metadata.font_infos.resize(1, WOFF2FontInfo::default());
        offset = StoreOffsetTable(result, offset, hdr.flavor, hdr.num_tables);
        for i in 0..(hdr.num_tables as usize) {
            metadata.font_infos[0]
                .table_entry_by_tag
                .insert(sorted_tables[i].tag, offset as u32);
            offset = StoreTableEntry(result, offset, sorted_tables[i].tag);
        }
    }

    if PREDICT_FALSE(!out.Write(&output)) {
        return FONT_COMPRESSION_FAILURE();
    }
    metadata.header_checksum = ComputeULongSum(&output, output.len());
    true
}

fn ComputeWOFF2FinalSize(data: &[u8]) -> usize {
    let mut file = Buffer::new(data);
    let mut total_length: u32 = 0;

    if !file.Skip(16) || !file.ReadU32(&mut total_length) {
        return 0;
    }

    total_length as usize
}

// In-memory conversion (requires WOFF2MemoryOut)
//
// fn ConvertWOFF2ToTTF(uint8_t *result, size_t result_length,
//                        const uint8_t *data, size_t length)  -> bool{
//   WOFF2MemoryOut out(result, result_length);
//   let mut out =
//   return ConvertWOFF2ToTTF(data, length, &out);
// }

fn ConvertWOFF2ToTTF(data: &[u8], out: &mut impl WOFF2Out) -> bool {
    let mut hdr = WOFF2Header::default();
    if !ReadWOFF2Header(data, &mut hdr) {
        return FONT_COMPRESSION_FAILURE();
    }

    let mut metadata = RebuildMetadata::default();
    if !WriteHeaders(data, &mut metadata, &mut hdr, out) {
        return FONT_COMPRESSION_FAILURE();
    }

    let compression_ratio: f32 = (hdr.uncompressed_size as f32) / (data.len() as f32);
    if compression_ratio > kMaxPlausibleCompressionRatio {
        #[cfg(feature = "font_compression_bin")]
        eprint!("Implausible compression ratio %.01f", compression_ratio);
        return FONT_COMPRESSION_FAILURE();
    }

    let src_buf = &data[(hdr.compressed_offset as usize)
        ..((hdr.compressed_offset + hdr.compressed_length as u64) as usize)]; // const uint8_t* src_buf = data + hdr.compressed_offset;
    let mut uncompressed_buf: Vec<u8> = Vec::with_capacity(hdr.uncompressed_size as usize);
    if PREDICT_FALSE(hdr.uncompressed_size < 1) {
        return FONT_COMPRESSION_FAILURE();
    }
    if PREDICT_FALSE(!Woff2Uncompress(src_buf, &mut uncompressed_buf)) {
        return FONT_COMPRESSION_FAILURE();
    }

    for i in 0..metadata.font_infos.len() {
        if PREDICT_FALSE(!ReconstructFont(
            &mut uncompressed_buf,
            &mut metadata,
            &hdr,
            i,
            out,
        )) {
            return FONT_COMPRESSION_FAILURE();
        }
    }

    true
}
