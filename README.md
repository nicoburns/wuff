# Wuff

[![Crates.io](https://img.shields.io/crates/v/wuff.svg)](https://crates.io/crates/wuff)
[![Docs](https://img.shields.io/docsrs/wuff/latest)](https://docs.rs/wuff)
[![Crates.io License](https://img.shields.io/crates/l/wuff)](#license)

A lightweight, pure-Rust decoder for [WOFF](https://www.w3.org/TR/WOFF/) and [WOFF2](https://www.w3.org/TR/WOFF2/) web fonts.

Wuff takes a compressed WOFF or WOFF2 file and decodes it back into a plain
[OpenType/TrueType (sfnt)](https://learn.microsoft.com/en-us/typography/opentype/spec/otff)
font that can be parsed by standard font tooling.

It is hand-ported from Google's [woff2](https://github.com/google/woff2) C++ library and has been verified to produce
byte-identical output to Google's woff2 library for every font in the [google/fonts](https://github.com/google/fonts) repository

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

## Conformance testing

The `conformance` crate is a test harness which verifies that three decoders
produce byte-identical output for every font it tests:

1. Google's C++ woff2 reference decoder (the `woff2_decompress` binary)
2. The wuff Rust decoder (this crate)
3. The `wuff-capi` C++ wrapper around the wuff decoder

Requirements: a C++ toolchain, plus `cmake` and `brotli` to build the C++ woff2 tools.

Its inputs are:

- The WOFF2 files from the `css/WOFF2` section of the [web-platform-tests](https://github.com/web-platform-tests/wpt)
  committed to this repository under `conformance/fonts/wpt/`. This test suite contains deliberately
  invalid WOFF2 files, so consistent rejection by all three decoders is considered a test pass.
- Every `ttf`/`otf`/`ttc` font in the [google/fonts](https://github.com/google/fonts)
  repository, encoded to WOFF2 with the C++ reference encoder (`woff2_compress`).

Run it with:

```sh
cargo run -rp conformance
cargo run -rp conformance --refresh-fonts  # re-download google/fonts and rebuild the cache
```

The first run downloads the google/fonts repository (~1.5GB) and encodes every
font at maximum Brotli quality, which takes a while. The encoded WOFF2
files (~820MB) are cached in `.data/encoded`. Use `--refresh-fonts` to a force a cache refresh.

## Repository layout

This repository contains both the published crate and the reference material
used to develop it:

- `wuff/` - the published crate: an idiomatic Rust rewrite of the decoder.
- `wuff-capi/` - a C API for the wuff decoder, usable as a drop-in replacement for the woff2 C++ library's decoding API.
- `conformance/` - the conformance test harness described above.
- `woff2/` - a copy of Google's [woff2](https://github.com/google/woff2/) C++
  library, used as the reference implementation.

## License

Licensed under the [MIT License](./LICENSE).
