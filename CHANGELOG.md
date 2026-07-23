# Changelog

## Unreleased
- Fix a memory-usage regression introduced in 0.2.7: the WOFF2 output buffer could retain up to
  ~2x its final size in unused capacity. It is now shrunk to fit before being returned.

## 0.2.8
- Remove `arrayvec` dependency

## 0.2.7

- The `wuff` crate is now `no_std` compatible
- A `wuff-capi` crate has been added exposing a C API.
  Additionally header files are provided that provide an API that is drop-in compatible with the C++ woff2 library's API.

A number of small bugs have been fixed:

- Return an error rather than panicking on malformed WOFF2 inputs (decompression failures, unexpected decompressed data sizes, and other malformed data)
- Bound WOFF2 decompression by the table-directory size rather than the untrusted `totalSfntSize`
- Reject fonts where the reconstructed `loca` or `hmtx` tables don't match the original size
- Reject WOFF files with overlapping blocks, or with more than 3 bytes of padding between blocks
- Fix inverted TTC and TTF header checksum
- Fix forwards compatibility with future transform formats
- Only reject a non-zero reserved field for WOFF1 files


## 0.2.6
- Fix issue where `wuff` would panic on WOFF2 files where the section containing the encoded
  brotli stream had padding bytes (use `write` rather than `write_all`) (#9)

## 0.2.5
- Remove `font-types` dependency

## 0.2.4
- Upgrade `font-types` to v0.11

## 0.2.3
- Implement the `Error` trait for the `WuffErr` type

## 0.2.2
- Fix validation of WOFF files that do not have transformed loca/glyf tables. Previously these files were being incorrectly rejected.

## 0.2.1
- Make `WuffErr` type public

## 0.2.0
- Added support for WOFF1

## 0.1.1
- Minor documentation updates

## 0.1.0
- Initial release with WOFF2 supportg