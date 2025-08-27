use std::io::{Cursor, Write};

use arrayvec::ArrayVec;
use bytes::{Buf, BufMut};

use crate::{
    Point, Round4, compute_checksum,
    error::{WuffErr, bail_if, bail_with_msg_if, u32_will_overflow, usize_will_overflow},
    variable_length::BufVariableExt as _,
};

// simple glyph flags
const GLYF_ON_CURVE: u8 = 1 << 0;
const GLYF_X_SHORT: u8 = 1 << 1;
const GLYF_Y_SHORT: u8 = 1 << 2;
const GLYF_REPEAT: u8 = 1 << 3;
const GLYF_THIS_X_IS_SAME: u8 = 1 << 4;
const GLYF_THIS_Y_IS_SAME: u8 = 1 << 5;
const OVERLAP_SIMPLE: u8 = 1 << 6;

const NUM_SUB_STREAMS: usize = 7;
const FLAG_OVERLAP_SIMPLE_BITMAP: u16 = 1 << 0;
// 98% of Google Fonts have no glyph above 5k bytes. Largest glyph ever observed was 72k bytes
const DEFAULT_GLYPH_BUF_SIZE: usize = 5120;

const FLAG_ARG_1_AND_2_ARE_WORDS: u16 = 1 << 0;
const FLAG_WE_HAVE_A_SCALE: u16 = 1 << 3;
const FLAG_MORE_COMPONENTS: u16 = 1 << 5;
const FLAG_WE_HAVE_AN_X_AND_Y_SCALE: u16 = 1 << 6;
const FLAG_WE_HAVE_A_TWO_BY_TWO: u16 = 1 << 7;
const FLAG_WE_HAVE_INSTRUCTIONS: u16 = 1 << 8;

const END_PTS_OF_CONTOURS_OFFSET: usize = 10;
const COMPOSITE_GLYPH_BEGIN: usize = 10;

pub struct GlyfAndLocaData {
    /// The number of glyphs in the glyf table
    pub num_glyphs: u16,
    /// loca index format
    pub index_format: u16,
    /// The x_min of the bounding box of each glyph. Used to reconstruct hmtx table
    pub x_mins: Vec<i16>,
    /// Encoded Open Type "glyf" table
    pub glyf_table: Vec<u8>,
    /// Checksum for "glyf" table
    pub glyf_checksum: u32,
    /// Encoded Open Type "loca" table
    pub loca_table: Vec<u8>,
    /// Checksum for "loca" table
    pub loca_checksum: u32,
}

/// Decode a WOFF2 transformed glyf table
///
/// <https://www.w3.org/TR/WOFF2/#glyf_table_format>
pub(crate) fn tranform_glyf_table(data: &[u8]) -> Result<GlyfAndLocaData, WuffErr> {
    GlyfDecoder::new(data)?.transform()
}

pub struct GlyfDecoder<'a> {
    // State
    n_contour_stream: &'a [u8],
    n_points_stream: &'a [u8],
    flag_stream: &'a [u8],
    glyph_stream: &'a [u8],
    composite_stream: &'a [u8],
    bbox_bitmap: &'a [u8],
    bbox_stream: &'a [u8],
    instruction_stream: &'a [u8],
    overlap_bitmap: Option<&'a [u8]>,
    glyph_buf: Vec<u8>,

    // Output data
    num_glyphs: u16,
    index_format: u16,
}

impl GlyfDecoder<'_> {
    pub fn new<'a>(data: &'a [u8]) -> Result<GlyfDecoder<'a>, WuffErr> {
        let mut input = data;
        let _: u16 = input.try_get_u16()?; // first 2 bytes are reserved
        let flags: u16 = input.try_get_u16()?;
        let has_overlap_bitmap: bool = (flags & FLAG_OVERLAP_SIMPLE_BITMAP) != 0;
        let num_glyphs = input.try_get_u16()?;
        let index_format = input.try_get_u16()?;

        let mut offset: usize = (2 + NUM_SUB_STREAMS) * 4;
        bail_if!(offset > data.len());

        // Invariant from here on: data_size >= offset
        let mut substreams: ArrayVec<&[u8], NUM_SUB_STREAMS> = ArrayVec::new();
        for _ in 0..NUM_SUB_STREAMS {
            let substream_size: usize = input.try_get_u32()? as usize;
            bail_if!(substream_size > data.len() - offset);
            substreams.push(&data[offset..(offset + substream_size)]);
            offset += substream_size;
        }

        // Safe because num_glyphs is bounded
        let bitmap_length: usize = ((num_glyphs as usize + 31) >> 5) << 2;
        bail_if!(bitmap_length > substreams[5].len());

        let n_contour_stream = substreams[0];
        let n_points_stream = substreams[1];
        let flag_stream = substreams[2];
        let glyph_stream = substreams[3];
        let composite_stream = substreams[4];
        let (bbox_bitmap, bbox_stream) = substreams[5].split_at(bitmap_length);
        let instruction_stream = substreams[6];

        let mut overlap_bitmap: Option<&[u8]> = None;
        if has_overlap_bitmap {
            let overlap_bitmap_length = (num_glyphs as usize + 7) >> 3;
            overlap_bitmap = Some(&data[offset..(offset + (overlap_bitmap_length))]);
            bail_if!(overlap_bitmap_length > data.len() - offset);
        }

        // Scratch buffer to decode glyphs int.
        let glyph_buf: Vec<u8> = Vec::with_capacity(DEFAULT_GLYPH_BUF_SIZE);

        Ok(GlyfDecoder {
            n_contour_stream,
            n_points_stream,
            flag_stream,
            glyph_stream,
            composite_stream,
            bbox_bitmap,
            bbox_stream,
            instruction_stream,
            overlap_bitmap,
            glyph_buf,
            num_glyphs,
            index_format,
        })
    }

    pub fn transform(mut self) -> Result<GlyfAndLocaData, WuffErr> {
        // Setup state
        let mut glyf_checksum: u32 = 0;
        let mut glyf_table: Vec<u8> = Vec::with_capacity(self.num_glyphs as usize * 12);
        let mut loca_values: Vec<u32> = Vec::with_capacity(self.num_glyphs as usize + 1);
        let mut x_mins: Vec<i16> = Vec::with_capacity(self.num_glyphs as usize);

        // Iterate over each glyph
        for i in 0..(self.num_glyphs as usize) {
            loca_values.push(glyf_table.len() as u32);

            let n_contours: i16 = self.n_contour_stream.try_get_i16()?;
            let glyph_has_bbox = (self.bbox_bitmap[i >> 3] & (0x80 >> (i & 7))) != 0;

            self.glyph_buf.clear();
            if n_contours == -1 {
                // composite glyphs must have an explicit bbox
                bail_if!(!glyph_has_bbox);
                self.parse_composite_glyph()?;
            } else if n_contours > 0 {
                // Note: while this look similar to the glyph_has_bbox code above, it's indexing into a different bitmap
                let has_overlap_bit: bool = self
                    .overlap_bitmap
                    .is_some_and(|bitmap| (bitmap[i >> 3] & (0x80 >> (i & 7))) != 0);
                self.parse_simple_glyph(n_contours, glyph_has_bbox, has_overlap_bit)?;
            } else {
                // n_contours == 0; empty glyph. Must NOT have a bbox.
                bail_with_msg_if!(glyph_has_bbox, "Empty glyph has a bbox")
            }

            glyf_checksum = glyf_checksum.wrapping_add(compute_checksum(&self.glyph_buf));

            // Write glyph to output table and pad output
            //
            // TODO(user) Old code aligned glyphs ... but do we actually need to?
            // (definitely useful for loca)
            glyf_table.extend_from_slice(&self.glyph_buf);
            glyf_table.resize(Round4!(glyf_table.len()), 0);

            // Read the x_min of the glyph in case we nede it to reconstruct 'hmtx'
            // The x_min value an i16 stored as bytes 2-4 in the glyph header.
            if n_contours > 0 {
                let x_min = i16::from_be_bytes(self.glyph_buf[2..4].try_into().unwrap());
                x_mins.push(x_min);
            }
        }

        // loca[n] will be equal the length of the glyph data ('glyf') table
        loca_values.push(glyf_table.len() as u32);

        // Generate loca table
        let (loca_table, loca_checksum) = generate_loca_table(&loca_values, self.index_format)?;

        Ok(GlyfAndLocaData {
            num_glyphs: self.num_glyphs,
            index_format: self.index_format,
            x_mins,
            loca_table,
            loca_checksum,
            glyf_table,
            glyf_checksum,
        })
    }

    /// Parse glyph data into `self.glyph_buf`
    fn parse_composite_glyph(&mut self) -> Result<(), WuffErr> {
        // Create a new iterator over the composite stream when computing the size so that we
        // we can "rewind" and copy the bytes counted here below.
        let mut ro_composite_stream = self.composite_stream;
        let (composite_size, have_instructions) =
            compute_size_of_composite(&mut ro_composite_stream)?;

        let instruction_size: u16 = if have_instructions {
            self.glyph_stream.try_get_variable_255_u16()?
        } else {
            0
        };

        let size_needed: usize = 12 + composite_size + (instruction_size as usize);
        if size_needed > self.glyph_buf.capacity() {
            self.glyph_buf
                .reserve(size_needed - self.glyph_buf.capacity());
        }

        let n_contours: i16 = -1; // All composite glyphs has n_contours = -1
        self.glyph_buf.put_i16(n_contours);

        self.bbox_stream
            .try_read_bytes_into(8, &mut self.glyph_buf)?;
        self.composite_stream
            .try_read_bytes_into(composite_size, &mut self.glyph_buf)?;

        if have_instructions {
            self.glyph_buf.put_u16(instruction_size);
            self.instruction_stream
                .try_read_bytes_into(instruction_size as usize, &mut self.glyph_buf)?;
        }

        Ok(())
    }

    fn parse_simple_glyph(
        &mut self,
        n_contours: i16,
        glyph_has_bbox: bool,
        has_overlap_bit: bool,
    ) -> Result<(), WuffErr> {
        let n_contours = n_contours as usize;

        // simple glyph
        let mut n_points_vec: Vec<u16> = Vec::with_capacity(n_contours);
        let mut total_n_points: u32 = 0;
        for _ in 0..n_contours {
            let n_points_contour: u16 = self.n_points_stream.try_get_variable_255_u16()?;
            n_points_vec.push(n_points_contour);
            bail_if!(u32_will_overflow(total_n_points, n_points_contour as u32));
            total_n_points += n_points_contour as u32;
        }
        let flag_size: usize = total_n_points as usize;
        bail_if!(flag_size > self.flag_stream.len());

        let flags_buf = self.flag_stream;
        let triplet_buf = self.glyph_stream;

        let mut triplet_bytes_consumed: usize = 0;

        let mut points = Vec::with_capacity(total_n_points as usize);
        triplet_bytes_consumed +=
            decode_triplet(&flags_buf[0..flag_size], triplet_buf, &mut points)?;

        self.flag_stream.advance(flag_size);
        self.glyph_stream.advance(triplet_bytes_consumed); // FIXME: pass glyph_stream directly to decode_triplet instead?

        let instruction_size: u16 = self.glyph_stream.try_get_variable_255_u16()?;
        bail_if!(total_n_points >= (1 << 27) || instruction_size as u32 >= (1 << 30));

        // Reserve needed size to reduce allocations
        let size_needed: usize =
            12 + 2 * n_contours + 5 * (total_n_points as usize) + (instruction_size as usize);
        if self.glyph_buf.capacity() < size_needed {
            self.glyph_buf
                .reserve(size_needed - self.glyph_buf.capacity());
        }

        self.glyph_buf.put_i16(n_contours as i16);

        if glyph_has_bbox {
            self.bbox_stream
                .try_read_bytes_into(8, &mut self.glyph_buf)?;
        } else {
            write_bbox(points.as_slice(), &mut self.glyph_buf);
        }

        // From this point, stop writing to the end of the glyph buffer and write to earlier in the buffer
        // let mut writer = &mut÷ self.glyph_buf[END_PTS_OF_CONTOURS_OFFSET..];

        let mut end_point: i32 = -1;
        for countour in n_points_vec {
            end_point += countour as i32;
            bail_if!(end_point >= 65536);
            self.glyph_buf.put_u16(end_point as u16);
        }

        self.glyph_buf.put_u16(instruction_size);
        self.instruction_stream
            .try_read_bytes_into(instruction_size as usize, &mut self.glyph_buf)?;

        write_glyph_points(points.as_slice(), has_overlap_bit, &mut self.glyph_buf)?;

        Ok(())
    }
}

// This function stores just the point data. On entry, dst points to the
// beginning of a simple glyph. Returns true on success.
fn write_glyph_points(
    points: &[Point],
    has_overlap_bit: bool,
    dst: &mut impl BufMut,
) -> Result<(), WuffErr> {
    // Write flags
    let mut last_flag: u8 = u8::MAX; // not a valid flag so next flag will never be equal to it
    let mut repeat_count: u8 = 0;
    let mut last_x: i32 = 0;
    let mut last_y: i32 = 0;
    for (i, point) in points.iter().enumerate() {
        // Compute flag value
        let flag = {
            let mut flag: u8 = 0;

            if point.on_curve {
                flag |= GLYF_ON_CURVE;
            }
            if has_overlap_bit && i == 0 {
                flag |= OVERLAP_SIMPLE;
            }

            // Handle x
            let dx: i32 = point.x - last_x;
            if dx == 0 {
                flag |= GLYF_THIS_X_IS_SAME;
            } else if dx > -256 && dx < 256 {
                flag |= GLYF_X_SHORT | (if dx > 0 { GLYF_THIS_X_IS_SAME } else { 0 });
            } else {
                // Do nothing
            }

            // Handle y
            let dy: i32 = point.y - last_y;
            if dy == 0 {
                flag |= GLYF_THIS_Y_IS_SAME;
            } else if dy > -256 && dy < 256 {
                flag |= GLYF_Y_SHORT | (if dy > 0 { GLYF_THIS_Y_IS_SAME } else { 0 });
            } else {
                // Do nothing
            }

            flag
        };

        // Compare flag value with previous value and write previous value if appropriate.
        //
        // To keep writes to the output buffer strictly append-only we don't write flags immediately.
        // Instead, we keep track of the "last_flag" along with an associated "repeat_count" until we
        // know that this flag will not be repeated.

        // If the current flag is the same as the previous one and we have not yet reached the repeat count
        // limit (limit is 255 so it fits in one byte), then we increment the repeat count.
        if flag == last_flag && repeat_count < 255 {
            repeat_count += 1;
        }
        // Else if either:
        //   - The current flag is not the same as the previous one.
        //   - We have hit the repeat count limit
        else {
            // If the previous flag has a repeat count associated with it then
            // set the GLYF_REPEAT flag on that flag and write 2 bytes:
            //
            //   1. The previous flag
            //   2. The repeat count
            //
            // Note: it is important that we delay setting the GLYF_REPEAT until this point
            // otherwise the "if flag == last_flag" will fail to detect repeats.
            if repeat_count > 0 {
                dst.put_u8(last_flag | GLYF_REPEAT);
                dst.put_u8(repeat_count);
            }
            // If the repeat count is 0 then just write the previous flag
            else {
                dst.put_u8(last_flag);
            }

            // Reset the repeat count to 0
            repeat_count = 0;
        }

        // Store values from this iteration
        last_x = point.x;
        last_y = point.y;
        last_flag = flag;
    }

    // Write final flag
    if repeat_count > 0 {
        dst.put_u8(last_flag | GLYF_REPEAT);
        dst.put_u8(repeat_count);
    } else {
        dst.put_u8(last_flag);
    };

    // Write x coordinates
    last_x = 0;
    for point in points {
        let dx: i32 = point.x - last_x;
        if dx == 0 {
            // do nothing
        } else if dx > -256 && dx < 256 {
            dst.put_u8(dx.unsigned_abs() as u8);
        } else {
            // will always fit for valid input, but overflow is harmless
            dst.put_i16(dx as i16)
        }
        last_x += dx;
    }

    // Write y coordinates
    last_y = 0;
    for point in points {
        let dy: i32 = point.y - last_y;
        if dy == 0 {
            // do nothing
        } else if dy > -256 && dy < 256 {
            dst.put_u8(dy.unsigned_abs() as u8);
        } else {
            dst.put_i16(dy as i16)
        }
        last_y += dy;
    }

    Ok(())
}

/// Compute the bounding box of the coordinates, and store into a glyf buffer.
/// A precondition is that there are at least 10 bytes available.
/// dst should point to the beginning of a 'glyf' record.
fn write_bbox(points: &[Point], dst: &mut impl BufMut) {
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

    dst.put_i16(x_min as i16);
    dst.put_i16(y_min as i16);
    dst.put_i16(x_max as i16);
    dst.put_i16(y_max as i16);
}

fn compute_size_of_composite(composite_stream: &mut impl Buf) -> Result<(usize, bool), WuffErr> {
    let mut bytes_read: usize = 0;
    let mut we_have_instructions: bool = false;
    let mut flags: u16 = FLAG_MORE_COMPONENTS;
    while flags & FLAG_MORE_COMPONENTS != 0 {
        flags = composite_stream.try_get_u16()?;
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
        composite_stream.advance(arg_size);

        // 2 bytes for the flags + arg_size
        bytes_read += 2 + arg_size
    }

    Ok((bytes_read, we_have_instructions))
}

fn decode_triplet(flags_in: &[u8], in_: &[u8], result: &mut Vec<Point>) -> Result<usize, WuffErr> {
    #[inline(always)]
    fn with_sign(flag: i32, baseval: i32) -> i32 {
        // Precondition: 0 <= baseval < 65536 (to avoid integer overflow)
        if (flag & 1) != 0 { baseval } else { -baseval }
    }

    #[inline(always)]
    fn safe_add(a: i32, b: i32) -> Result<i32, WuffErr> {
        bail_if!(((a > 0) && (b > i32::MAX - a)) || ((a < 0) && (b < i32::MIN - a)));
        Ok(a + b)
    }

    let mut x: i32 = 0;
    let mut y: i32 = 0;

    bail_if!(flags_in.len() > in_.len());

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
        bail_if!(
            usize_will_overflow(triplet_index, n_data_bytes)
                || (triplet_index + n_data_bytes) > in_.len()
        );

        let dx: i32;
        let dy: i32;
        if flag < 10 {
            dx = 0;
            dy = with_sign(flag, ((flag & 14) << 7) + in_[triplet_index] as i32);
        } else if flag < 20 {
            dx = with_sign(flag, (((flag - 10) & 14) << 7) + in_[triplet_index] as i32);
            dy = 0;
        } else if flag < 84 {
            let b0: i32 = flag - 20;
            let b1: i32 = in_[triplet_index] as i32;
            dx = with_sign(flag, 1 + (b0 & 0x30) + (b1 >> 4));
            dy = with_sign(flag >> 1, 1 + ((b0 & 0x0c) << 2) + (b1 & 0x0f));
        } else if flag < 120 {
            let b0: i32 = flag - 84;
            dx = with_sign(flag, 1 + ((b0 / 12) << 8) + in_[triplet_index] as i32);
            dy = with_sign(
                flag >> 1,
                1 + (((b0 % 12) >> 2) << 8) + in_[triplet_index + 1] as i32,
            );
        } else if flag < 124 {
            let b2: i32 = in_[triplet_index + 1] as i32;
            dx = with_sign(flag, ((in_[triplet_index] as i32) << 4) + (b2 >> 4));
            dy = with_sign(
                flag >> 1,
                ((b2 & 0x0f) << 8) + in_[triplet_index + 2] as i32,
            );
        } else {
            dx = with_sign(
                flag,
                ((in_[triplet_index] as i32) << 8) + in_[triplet_index + 1] as i32,
            );
            dy = with_sign(
                flag >> 1,
                ((in_[triplet_index + 2] as i32) << 8) + in_[triplet_index + 3] as i32,
            );
        }
        triplet_index += n_data_bytes;
        x = safe_add(x, dx)?;
        y = safe_add(y, dy)?;

        result.push(Point { x, y, on_curve }); // CHECK: was *result++
    }

    Ok(triplet_index)
}

/// Generate a loca table given a slice of loca offsets and an index format
///
/// See <https://developer.apple.com/fonts/TrueType-Reference-Manual/RM06/Chap6loca.html>
pub(crate) fn generate_loca_table(
    loca_values: &[u32],
    index_format: u16,
) -> Result<(Vec<u8>, u32), WuffErr> {
    let loca_size = loca_values.len();
    let offset_size: usize = if index_format != 0 { 4 } else { 2 };
    bail_if!((loca_size << 2) >> 2 != loca_size);

    let mut loca_content: Vec<u8> = Vec::with_capacity(loca_size * offset_size);
    if index_format != 0 {
        for &value in loca_values {
            // loca long version. The actual local offset is stored.
            loca_content.put_u32(value);
        }
    } else {
        for &value in loca_values {
            // loca short version. The actual local offset divided by 2 is stored.
            // Right shift is a cheap divide by 2
            loca_content.put_u16((value >> 1) as u16);
        }
    }

    let checksum = compute_checksum(&loca_content);

    Ok((loca_content, checksum))
}

// MOVE assert up:
//
// // https://dev.w3.org/webfonts/WOFF2/spec/#conform-mustRejectLoca
// // dst_length here is origLength in the spec
// let expected_loca_dst_length: u32 =
//     (if info.index_format != 0 { 4 } else { 2 }) * (info.num_glyphs as u32 + 1);

// if PREDICT_FALSE(loca_table.dst_length != expected_loca_dst_length) {
//     return FONT_COMPRESSION_FAILURE();
// }
