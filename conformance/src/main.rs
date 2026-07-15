//! Conformance test harness comparing three WOFF2 decoders:
//!
//! 1. The C++ woff2 reference decoder (via the `woff2_decompress` binary)
//! 2. The Rust `wuff` decoder (called in-process)
//! 3. The `wuff-capi` C++ wrapper headers (via a C++ shim compiled by build.rs)
//!
//! Inputs are produced by encoding every ttf/otf/ttc font from the
//! <https://github.com/google/fonts> repository to WOFF2 using the C++
//! `woff2_compress` reference encoder. This happens in three distinct
//! phases: the sources are downloaded and extracted, then a separate
//! encoding step encodes the whole corpus into the WOFF2 cache (in
//! `<data dir>/encoded`, default data dir: `<repo root>/data`), and finally
//! the tests run against that cache. The downloaded tarball and extracted
//! source tree are deleted after encoding and before the tests run, so only
//! the encoded WOFF2 files are kept long-term and subsequent runs are driven
//! from the cache alone. Pass `--refresh-fonts` to re-download the sources
//! and rebuild the cache from scratch.
//!
//! These three phases map onto the modules of this crate: `prepare` handles
//! downloading and WOFF2 encoding, `runner` handles decoding/comparison and
//! reporting, and this top-level module ties them together (argument parsing
//! plus a few shared helpers).
//!
//! In addition, ready-made WOFF2 files committed to the repository under
//! `conformance/fonts/` (such as the wpt (web-platform-tests) WOFF2
//! conformance suite in `conformance/fonts/wpt/`) are decoded as-is. As
//! these suites contain deliberately invalid files, consistent rejection by
//! all three decoders is an acceptable outcome for these inputs (reported as
//! "pass (wpt reject)").
//!
//! Usage:
//!
//! ```text
//! cargo run -p conformance --release -- [FILTER...] [--data-dir DIR] [--refresh-fonts]
//! ```
//!
//! The harness asserts that all three decoders produce byte-identical output
//! and exits non-zero if any font fails.

// Force-link the wuff-capi crate: nothing references it from Rust, but the
// C++ shim (src/capi_shim.cpp) needs its exported wuff_woff2_* C symbols.
use wuff_capi as _;

mod prepare;
mod runner;

use std::fs;
use std::path::{Path, PathBuf};

struct Config {
    data_dir: PathBuf,
    woff2_dir: PathBuf,
    filters: Vec<String>,
    refresh_fonts: bool,
}

fn parse_args() -> Config {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut cfg = Config {
        data_dir: repo_root.join("data"),
        woff2_dir: repo_root.join("woff2"),
        filters: Vec::new(),
        refresh_fonts: false,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |name: &str| {
            args.next()
                .unwrap_or_else(|| fatal(&format!("{name} requires a value")))
        };
        match arg.as_str() {
            "--data-dir" => cfg.data_dir = PathBuf::from(value("--data-dir")),
            "--refresh-fonts" => cfg.refresh_fonts = true,
            "--help" | "-h" => {
                println!(
                    "Usage: conformance [FILTER...] [--data-dir DIR] [--refresh-fonts]\n\n\
                     FILTER: only test fonts whose path contains the substring"
                );
                std::process::exit(0);
            }
            other if other.starts_with('-') => fatal(&format!("unknown flag: {other}")),
            other => cfg.filters.push(other.to_string()),
        }
    }
    cfg
}

fn fatal(msg: &str) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(2);
}

/// Recursively find all files under `dir` with one of the given (lowercase)
/// extensions, returned as paths relative to `root`.
fn discover_files(root: &Path, dir: &Path, extensions: &[&str], out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("failed to read fonts dir") {
        let path = entry.expect("failed to read dir entry").path();
        if path.is_dir() {
            discover_files(root, &path, extensions, out);
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| extensions.contains(&e.to_ascii_lowercase().as_str()))
        {
            out.push(path.strip_prefix(root).unwrap().to_path_buf());
        }
    }
}

fn main() {
    let cfg = parse_args();

    let encoded_dir = cfg.data_dir.join("encoded");
    let scratch_root = cfg.data_dir.join("tmp");
    let _ = fs::remove_dir_all(&scratch_root);

    // Phase 1 & 2: build the reference tools, then download and encode the
    // corpus into the cache (deleting the sources afterwards).
    let (compress, decompress) = prepare::ensure_woff2_tools(&cfg.woff2_dir);
    prepare::build_encoded_cache(&cfg, &compress, &encoded_dir, &scratch_root);

    // Phase 3: collect the test cases (cache + committed font suites), apply
    // the user's FILTER, and run them.
    let fonts_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fonts");
    let mut cases = runner::discover_cases(&encoded_dir, &fonts_dir);

    cases.sort_by(|a, b| a.name.cmp(&b.name));
    if !cfg.filters.is_empty() {
        cases.retain(|case| {
            let path = case.name.to_string_lossy();
            cfg.filters
                .iter()
                .any(|filter| path.contains(filter.as_str()))
        });
    }

    if runner::run_and_report(&decompress, &cases, &scratch_root, &cfg.data_dir) {
        std::process::exit(1);
    }
}
