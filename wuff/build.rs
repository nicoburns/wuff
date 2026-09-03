//! Build script for `wuff`.
//!
//! The only thing this does is compile Google's reference Brotli decoder (vendored under
//! `vendor/brotli`) when the `brotli-c` feature is enabled. For every other configuration it is a
//! no-op, so the pure-Rust build stays free of any C toolchain requirement.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    // Only the `brotli-c` backend has anything to build. Cargo exposes enabled features to build
    // scripts as `CARGO_FEATURE_<NAME>` environment variables.
    if std::env::var_os("CARGO_FEATURE_BROTLI_C").is_some() {
        brotli_c::build();
    }
}

#[cfg(feature = "brotli-c")]
mod brotli_c {
    use std::env;
    use std::path::PathBuf;

    /// Decoder sources from upstream `c/common` and `c/dec`. Keep in sync with the file list in
    /// `vendor/brotli/README.md` when updating the vendored copy.
    const SOURCES: &[&str] = &[
        "common/constants.c",
        "common/context.c",
        "common/dictionary.c",
        "common/platform.c",
        "common/shared_dictionary.c",
        "common/transform.c",
        "dec/bit_reader.c",
        "dec/decode.c",
        "dec/huffman.c",
        "dec/prefix.c",
        "dec/state.c",
        "dec/static_init.c",
    ];

    pub fn build() {
        let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
        let vendor_dir = manifest_dir.join("vendor").join("brotli");
        let shim_dir = manifest_dir.join("csrc").join("wasm-libc-shim");

        println!("cargo:rerun-if-changed={}", vendor_dir.display());
        println!("cargo:rerun-if-changed={}", shim_dir.display());

        let mut build = cc::Build::new();
        build.include(vendor_dir.join("include"));
        for source in SOURCES {
            build.file(vendor_dir.join(source));
        }
        // Third-party code: don't spam the build log with its warnings.
        build.warnings(false);

        if is_bare_wasm_target() {
            // Bare WebAssembly targets (`wasm32-unknown-unknown`, as used by wasm-bindgen, and
            // `wasm32v1-none`) have no libc, and clang ships no libc headers for them. Brotli only
            // needs `<string.h>`, `<stdlib.h>` and `<sys/types.h>`, so we drop the (non-existent)
            // system include path and provide our own minimal headers instead:
            //
            // * `memcpy`/`memmove`/`memset`/`memcmp` are provided at link time by Rust's
            //   `compiler_builtins`, which the Rust toolchain always links on these targets.
            // * `malloc`/`free` are redirected to `wuff_brotli_malloc`/`wuff_brotli_free`, which
            //   `src/brotli/c_backend.rs` implements on top of Rust's global allocator.
            //
            // The end result is a wasm module with no `env` imports, which is what wasm-bindgen
            // requires.
            build.flag("-nostdlibinc");
            build.include(&shim_dir);
        }

        build.compile("wuff_brotli_dec");
    }

    /// `true` for WebAssembly targets without an operating system / libc. Must match the `cfg`
    /// guarding the allocator shims in `src/brotli/c_backend.rs`.
    fn is_bare_wasm_target() -> bool {
        let family = env::var("CARGO_CFG_TARGET_FAMILY").unwrap_or_default();
        let os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
        family.split(',').any(|f| f == "wasm") && (os == "unknown" || os == "none")
    }
}

#[cfg(not(feature = "brotli-c"))]
mod brotli_c {
    pub fn build() {
        unreachable!("CARGO_FEATURE_BROTLI_C is set but the `brotli-c` cfg is not")
    }
}
