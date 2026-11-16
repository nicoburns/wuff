use alloc::vec::Vec;

use bytes::{Buf, BufMut};

use crate::error::{WuffErr, bail_if, bail_with_msg_if};

/// Decode a WOFF2 transformed hmtx table, appending the reconstructed OpenType
/// hmtx table to `out`.
///
/// <http://dev.w3.org/webfonts/WOFF2/spec/Overview.html#hmtx_table_format>
pub(crate) fn decode_hmtx_table(
    mut input: &[u8],
    num_glyphs: u16,
    num_hmetrics: u16,
    x_mins: &[i16],
    out: &mut Vec<u8>,
) -> Result<(), WuffErr> {
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
    bail_if!(x_mins.len() != num_glyphs as usize);

    // num_glyphs 0 is OK if there is no 'glyf' but cannot then xform 'hmtx'.
    bail_if!(num_hmetrics > num_glyphs);

    // "...only one entry need be in the array, but that entry is required."
    // <https://www.microsoft.com/typography/otspec/hmtx.htm>
    bail_if!(num_hmetrics < 1);

    // Validate the input length up front so the loops below can read without
    // per-element error handling.
    let proportional_lsb_bytes = if has_proportional_lsbs {
        2 * num_hmetrics as usize
    } else {
        0
    };
    let monospace_lsb_bytes = if has_monospace_lsbs {
        2 * (num_glyphs - num_hmetrics) as usize
    } else {
        0
    };
    let required = 2 * num_hmetrics as usize + proportional_lsb_bytes + monospace_lsb_bytes;
    bail_if!(input.remaining() < required);

    // The advance widths come first in the encoded table
    let advance_widths = &input[..2 * num_hmetrics as usize];
    input.advance(2 * num_hmetrics as usize);

    // Then the proportional lsbs (if present), then the monospace lsbs (if present)
    let proportional_lsbs = &input[..proportional_lsb_bytes];
    input.advance(proportional_lsb_bytes);
    let monospace_lsbs = &input[..monospace_lsb_bytes];

    // Reserve output capacity: 2 * num_glyphs (lsbs) + 2 * num_hmetrics (advance widths)
    out.reserve(2 * num_glyphs as usize + 2 * num_hmetrics as usize);

    // Proportional glyphs: advance width + lsb
    for i in 0..num_hmetrics as usize {
        out.extend_from_slice(&advance_widths[2 * i..2 * i + 2]);
        if has_proportional_lsbs {
            out.extend_from_slice(&proportional_lsbs[2 * i..2 * i + 2]);
        } else {
            out.put_i16(x_mins[i]);
        }
    }

    // Monospace glyphs: lsb only
    for (j, &x_min) in x_mins[num_hmetrics as usize..num_glyphs as usize]
        .iter()
        .enumerate()
    {
        if has_monospace_lsbs {
            out.extend_from_slice(&monospace_lsbs[2 * j..2 * j + 2]);
        } else {
            out.put_i16(x_min);
        }
    }

    Ok(())
}
