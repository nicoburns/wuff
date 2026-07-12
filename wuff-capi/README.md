# wuff-capi

C++ API for the [wuff](https://docs.rs/wuff) pure-Rust WOFF2 decoder, compatible
with the decoding API of the [woff2](https://github.com/google/woff2) C++
library.

Note: only the *decoding* API (`woff2/decode.h` and `woff2/output.h`) is
provided. The encoding API (`woff2/encode.h`) is not yet implemented.

## What this crate provides

- `extern "C"` symbols exported from Rust:
  - `wuff_woff2_compute_final_size` — reads the `totalSfntSize` field of a
    WOFF2 header (equivalent to `woff2::ComputeWOFF2FinalSize`)
  - `wuff_woff2_decode` — decompresses a WOFF2 font into a newly-allocated
    buffer
  - `wuff_woff2_free` — frees a buffer returned by `wuff_woff2_decode`
- C++ headers (in `include/woff2`) that provide a drop-in replacement for the
  C++ woff2 library's decoding API as header-only wrappers around the C symbols above.

## Usage (from Rust)

Add `wuff-capi` as a dependency of the crate whose build script compiles the
C/C++ code that consumes the woff2 API. This crate sets `links = "woff2"`, and
its build script exports the location of its headers, which is available in
dependent build scripts as the `DEP_WOFF2_INCLUDE_DIR` environment variable:

```rust
// build.rs of a dependent crate
let woff2_include_dir = std::env::var("DEP_WOFF2_INCLUDE_DIR").unwrap();
cc::Build::new()
    .cpp(true)
    .include(woff2_include_dir)
    // ...
```

The Rust symbols are linked automatically as part of the normal Rust build.


