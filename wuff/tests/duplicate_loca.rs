//! Regression test for a duplicate transformed `loca` table.
//!
//! `fixtures/duplicate-loca.woff2` is the wpt `directory-knowntags-001.woff2` font (which has
//! a transformed `glyf`/`loca` pair, and is decoded here as a control) with a byte-identical
//! copy of its `loca` table directory entry inserted: numTables incremented, header length and
//! trailing padding adjusted. A transformed `loca` declares transformLength == 0, so the copy
//! adds no bytes to the compressed block: only the table directory grows, and the brotli stream
//! is untouched.
//!
//! Only the font's `loca_idx` (the last `loca` in the directory) has its metadata computed
//! while reconstructing `glyf`, so the other `loca` used to fall through to an
//! `unreachable!()` and panic. The reference C++ woff2 decoder rejects this input, so wuff
//! must reject it too.

#![cfg(feature = "brotli")]

const VALID: &[u8] = include_bytes!("../../conformance/wpt/woff2/directory-knowntags-001.woff2");
const DUPLICATE_LOCA: &[u8] = include_bytes!("fixtures/duplicate-loca.woff2");

#[test]
fn duplicate_transformed_loca_is_rejected_without_panicking() {
    assert!(wuff::decompress_woff2(DUPLICATE_LOCA).is_err());
}

#[test]
fn base_font_with_transformed_loca_still_decodes() {
    assert!(wuff::decompress_woff2(VALID).is_ok());
}
