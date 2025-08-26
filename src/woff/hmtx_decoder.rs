use bytes::{Buf, BufMut};

use crate::{
    error::{WuffErr, bail_if, bail_with_msg_if},
    woff2_common::compute_checksum,
};

pub struct HmtxData {
    num_glyphs: u16,
    num_hmetrics: u16,
    advance_widths: Vec<u16>,
    lsbs: Vec<i16>,
}

pub struct TableMetadata {
    length: usize,
    checksum: u32,
}

/// Decode a WOFF2 transformed hmtx table
///
/// <http://dev.w3.org/webfonts/WOFF2/spec/Overview.html#hmtx_table_format>
pub(crate) fn decode_hmtx_table(
    input: &mut impl Buf,
    num_glyphs: u16,
    num_hmetrics: u16,
    x_mins: &[i16],
) -> Result<HmtxData, WuffErr> {
    // Decode flags
    let hmtx_flags: u8 = input.try_get_u8()?;
    let has_proportional_lsbs: bool = (hmtx_flags & 1) == 0;
    let has_monospace_lsbs: bool = (hmtx_flags & 2) == 0;

    // Bits 2-7 are reserved and MUST be zero.
    bail_with_msg_if!(
        (hmtx_flags & 0xFC) != 0,
        "Illegal hmtx flags; bits 2-7 must be 0"
    );

    // you say you transformed but there is little evidence of it
    bail_if!(has_proportional_lsbs && has_monospace_lsbs);

    // Should always be true (*regardless* of input data) unless we've made a programming error.
    // so we assert rather than bail.
    assert!(x_mins.len() == num_glyphs as usize);

    // num_glyphs 0 is OK if there is no 'glyf' but cannot then xform 'hmtx'.
    bail_if!(num_hmetrics > num_glyphs);

    // "...only one entry need be in the array, but that entry is required."
    // <https://www.microsoft.com/typography/otspec/hmtx.htm>
    bail_if!(num_hmetrics < 1);

    // Read advance widths
    let mut advance_widths: Vec<u16> = Vec::with_capacity(num_hmetrics as usize);
    for _ in 0..num_hmetrics {
        advance_widths.push(input.try_get_u16()?);
    }

    // Read lsb (proportional) and leftSideBearing (monospace) values into the same Vec
    let mut lsbs: Vec<i16> = Vec::with_capacity(num_glyphs as usize);
    for i in 0..num_hmetrics {
        lsbs.push(match has_proportional_lsbs {
            true => input.try_get_i16()?,
            false => x_mins[i as usize],
        });
    }
    for i in num_hmetrics..num_glyphs {
        lsbs.push(match has_monospace_lsbs {
            true => input.try_get_i16()?,
            false => x_mins[i as usize],
        });
    }

    Ok(HmtxData {
        num_glyphs,
        num_hmetrics,
        advance_widths,
        lsbs,
    })
}

/// bake me a shiny new hmtx table
pub(crate) fn generate_hmtx_table(hmtx_data: &HmtxData) -> Result<Vec<u8>, WuffErr> {
    let num_glyphs = hmtx_data.num_glyphs as usize;
    let num_hmetrics = hmtx_data.num_hmetrics as usize;

    let hmtx_output_size: usize = 2 * num_glyphs + 2 * num_hmetrics;
    let mut hmtx_table: Vec<u8> = Vec::with_capacity(hmtx_output_size);
    for i in 0..num_glyphs {
        if i < num_hmetrics {
            hmtx_table.put_u16(hmtx_data.advance_widths[i]);
        }
        hmtx_table.put_i16(hmtx_data.lsbs[i]);
    }

    Ok(hmtx_table)

    // let checksum = compute_checksum(&hmtx_table);
    // out.put_slice(&hmtx_table);

    // // metadata.dst_offset set in ReconstructFont
    // Ok(TableMetadata {
    //     length: hmtx_output_size,
    //     checksum,
    // })
}
