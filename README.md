# Wuff

Port of https://github.com/google/woff2/ to Rust, with the aim of creating a lightweight pure-rust decoder for WOFF files.
Both WOFF and WOFF2 formats are supported.

## Status

The decoder is ported and producing byte-identical files to the woff2 library for every font in https://github.com/google/fonts.

## Files

- The `woff2` directory contains a copy of https://github.com/google/woff2/
- The `old` directory contains the initial translation of the C++ code into Rust
- The `src` directory contains a rewrite into idiomatic Rust