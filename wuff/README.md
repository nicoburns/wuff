# Wuff

[![Crates.io](https://img.shields.io/crates/v/wuff.svg)](https://crates.io/crates/wuff)
[![Docs](https://img.shields.io/docsrs/wuff/latest)](https://docs.rs/wuff)
[![Crates.io License](https://img.shields.io/crates/l/wuff)](#license)

A lightweight, pure-Rust decoder for [WOFF](https://www.w3.org/TR/WOFF/) and [WOFF2](https://www.w3.org/TR/WOFF2/) web fonts.

Wuff takes a compressed WOFF or WOFF2 file and decodes it back into a plain
[OpenType/TrueType (sfnt)](https://learn.microsoft.com/en-us/typography/opentype/spec/otff)
font that can be parsed by standard font tooling.

It is hand-ported from Google's [woff2](https://github.com/google/woff2) C++ library and has been verified to produce
byte-identical output to Google's woff2 library for every font in the [google/fonts](https://github.com/google/fonts) repository.

## Why Wuff?

- **Pure Rust** — no C/C++ toolchain or bindings required.
- **Support both WOFF and WOFF2 formats**
- **Lightweight** — minimal dependencies, with the compression backends behind
  optional features.
- **Bring your own decompressor** — use the bundled backends, or plug in your
  own Brotli/zlib implementation.

## Encoding

Wuff currently only includes the decoder and does not yet have a port of the encoder. For encoding, consider:
  - https://github.com/bearcove/woofwoof (Rust bindings to the C++ woff2 library)
  - https://github.com/0x6b/ttf2woff2 (AI-assisted port of the C++ woff2 encoder to Rust)

## Usage

Decode a WOFF2 file into an OpenType/TrueType font:

```rust
let woff2_bytes = std::fs::read("font.woff2")?;
let otf_bytes = wuff::decompress_woff2(&woff2_bytes)?;
```

Decode a WOFF (version 1) file:

```rust
let woff_bytes = std::fs::read("font.woff")?;
let otf_bytes = wuff::decompress_woff1(&woff_bytes)?;
```

### Custom decompressors

If you'd rather not pull in the bundled compression crates (for example to share
a Brotli implementation you already depend on), disable the default features and
supply your own decompression closure:

```rust
use wuff::decompress_woff2_with_custom_brotli;

let otf_bytes = decompress_woff2_with_custom_brotli(&woff2_bytes, &mut |input, hint| {
    // Decompress `input`, optionally using `hint` as a size hint for the output buffer.
    my_brotli_decompress(input, hint)
})?;
```

A matching `decompress_woff1_with_custom_z` is available for WOFF1.

## Feature flags

- `brotli` *(default)* — bundle a Brotli backend for WOFF2 decoding (`decompress_woff2`).
- `z` *(default)* — bundle a zlib backend for WOFF1 decoding (`decompress_woff1`).

Disable default features to bring your own decompressors via the
`decompress_woff2_with_custom_brotli` / `decompress_woff1_with_custom_z` entry points.

## About this repository

This crate lives in the [`wuff` workspace](https://github.com/nicoburns/wuff)
alongside a C-API wrapper (`wuff-capi`), a conformance test harness, and a copy
of the reference C++ woff2 library. See the
[repository README](https://github.com/nicoburns/wuff#readme) for details.

## License

Licensed under the [MIT License](https://github.com/nicoburns/wuff/blob/main/LICENSE).
