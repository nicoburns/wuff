//! Conformance test harness comparing three WOFF2 decoders:
//!
//! 1. The C++ woff2 reference decoder (via the `woff2_decompress` binary)
//! 2. The Rust `wuff` decoder (called in-process)
//! 3. The `wuff-capi` C++ wrapper headers (via a C++ shim compiled by build.rs)
//!
//! Inputs are produced by encoding every ttf/otf/ttc font from the
//! <https://github.com/google/fonts> repository to WOFF2 using the C++
//! `woff2_compress` reference encoder. Only the encoded WOFF2 files are
//! cached long-term (in `<data dir>/encoded`, default data dir:
//! `<repo root>/data`); the downloaded tarball and extracted source tree are
//! deleted once the cache has been fully built, and subsequent runs are
//! driven from the cache alone. Pass `--refresh-fonts` to re-download the
//! sources and rebuild the cache from scratch.
//!
//! In addition, WOFF2 files from the wpt (web-platform-tests) WOFF2
//! conformance suite, committed to the repository under `conformance/wpt/`,
//! are decoded as-is. As that suite contains deliberately invalid files,
//! consistent rejection by all three decoders is an acceptable outcome for
//! these inputs (reported as "pass (wpt reject)").
//!
//! Usage:
//!
//! ```text
//! cargo run -p conformance --release -- [FILTER...] [--limit N] [--jobs N]
//!     [--data-dir DIR] [--refresh-fonts]
//! ```
//!
//! The harness asserts that all three decoders produce byte-identical output
//! and exits non-zero if any font fails.

// Force-link the wuff-capi crate: nothing references it from Rust, but the
// C++ shim (src/capi_shim.cpp) needs its exported wuff_woff2_* C symbols.
use wuff_capi as _;

use rayon::prelude::*;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::{fs, io::Write as _};

const GOOGLE_FONTS_URL: &str = "https://github.com/google/fonts/archive/refs/heads/main.tar.gz";
const FONT_EXTENSIONS: &[&str] = &["ttf", "otf", "ttc"];

/// FFI bindings to the C++ shim (src/capi_shim.cpp) which decodes via the
/// wuff-capi C++ wrapper headers (`woff2::ConvertWOFF2ToTTF` etc).
mod capi {
    unsafe extern "C" {
        fn conformance_capi_decode(
            data: *const u8,
            length: usize,
            result_length: *mut usize,
        ) -> *mut u8;
        fn conformance_capi_free(ptr: *mut u8);
    }

    pub fn decode(data: &[u8]) -> Option<Vec<u8>> {
        let mut len = 0usize;
        let ptr = unsafe { conformance_capi_decode(data.as_ptr(), data.len(), &mut len) };
        if ptr.is_null() {
            return None;
        }
        let out = unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec();
        unsafe { conformance_capi_free(ptr) };
        Some(out)
    }
}

struct Config {
    data_dir: PathBuf,
    woff2_dir: PathBuf,
    filters: Vec<String>,
    limit: Option<usize>,
    jobs: Option<usize>,
    refresh_fonts: bool,
}

fn parse_args() -> Config {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut cfg = Config {
        data_dir: repo_root.join("data"),
        woff2_dir: repo_root.join("woff2"),
        filters: Vec::new(),
        limit: None,
        jobs: None,
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
            "--limit" => {
                cfg.limit = Some(value("--limit").parse().unwrap_or_else(|_| {
                    fatal("--limit requires an integer value");
                }))
            }
            "--jobs" => {
                cfg.jobs = Some(value("--jobs").parse().unwrap_or_else(|_| {
                    fatal("--jobs requires an integer value");
                }))
            }
            "--refresh-fonts" => cfg.refresh_fonts = true,
            "--help" | "-h" => {
                println!(
                    "Usage: conformance [FILTER...] [--limit N] [--jobs N] \
                     [--data-dir DIR] [--refresh-fonts]\n\n\
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

fn run_command(cmd: &mut Command, what: &str) {
    let status = cmd
        .status()
        .unwrap_or_else(|e| fatal(&format!("failed to run {what}: {e}")));
    if !status.success() {
        fatal(&format!("{what} failed with {status}"));
    }
}

/// Locate (building if necessary) the C++ reference woff2_compress and
/// woff2_decompress binaries.
fn ensure_woff2_tools(woff2_dir: &Path) -> (PathBuf, PathBuf) {
    let build_dir = woff2_dir.join("build");
    let compress = build_dir.join("woff2_compress");
    let decompress = build_dir.join("woff2_decompress");
    if !compress.is_file() || !decompress.is_file() {
        eprintln!("Building C++ woff2 tools in {}...", build_dir.display());
        run_command(
            Command::new("cmake")
                .arg("-S")
                .arg(woff2_dir)
                .arg("-B")
                .arg(&build_dir)
                .arg("-DCMAKE_BUILD_TYPE=Release"),
            "cmake configure of the woff2 library (is brotli installed?)",
        );
        run_command(
            Command::new("cmake")
                .arg("--build")
                .arg(&build_dir)
                .arg("--parallel"),
            "cmake build of the woff2 library",
        );
        if !compress.is_file() || !decompress.is_file() {
            fatal("woff2_compress/woff2_decompress still missing after build");
        }
    }
    (compress, decompress)
}

/// Download (if needed) and extract (if needed) the google/fonts repository.
/// Returns the directory containing the extracted font tree.
fn ensure_fonts(data_dir: &Path, refresh: bool) -> PathBuf {
    let fonts_dir = data_dir.join("google-fonts");
    if fonts_dir.is_dir() && !refresh {
        return fonts_dir;
    }
    fs::create_dir_all(data_dir).expect("failed to create data dir");

    let tarball = data_dir.join("google-fonts.tar.gz");
    if !tarball.is_file() || refresh {
        eprintln!("Downloading {GOOGLE_FONTS_URL} (~1GB, this may take a while)...");
        let partial = data_dir.join("google-fonts.tar.gz.partial");
        run_command(
            Command::new("curl")
                .arg("--location")
                .arg("--fail")
                .arg("--progress-bar")
                .arg("--output")
                .arg(&partial)
                .arg(GOOGLE_FONTS_URL),
            "download of the google/fonts tarball with curl",
        );
        fs::rename(&partial, &tarball).expect("failed to move downloaded tarball into place");
    }

    eprintln!("Extracting {}...", tarball.display());
    let extracting = data_dir.join("google-fonts.extracting");
    let _ = fs::remove_dir_all(&extracting);
    let _ = fs::remove_dir_all(&fonts_dir);
    fs::create_dir_all(&extracting).expect("failed to create extraction dir");
    run_command(
        Command::new("tar")
            .arg("-xzf")
            .arg(&tarball)
            .arg("-C")
            .arg(&extracting)
            .arg("--strip-components=1"),
        "extraction of the google/fonts tarball with tar",
    );
    fs::rename(&extracting, &fonts_dir).expect("failed to move extracted fonts into place");
    // The tarball is no longer needed once extracted; only the encoded WOFF2
    // files are kept long-term.
    let _ = fs::remove_file(&tarball);
    fonts_dir
}

/// Returns true if `dir` contains at least one `.woff2` file (i.e. an
/// encoded-font cache from a previous run exists).
fn has_encoded_cache(dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        if path.is_dir() {
            has_encoded_cache(&path)
        } else {
            path.extension().is_some_and(|e| e == "woff2")
        }
    })
}

/// Recursively find all encoded fonts under the cache dir, returning the
/// original font paths (i.e. with the `.woff2` suffix stripped) relative to
/// `root`.
fn discover_encoded_fonts(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("failed to read encoded fonts dir") {
        let path = entry.expect("failed to read dir entry").path();
        if path.is_dir() {
            discover_encoded_fonts(root, &path, out);
        } else if let Some(rel) = path
            .strip_prefix(root)
            .unwrap()
            .to_str()
            .and_then(|s| s.strip_suffix(".woff2"))
        {
            out.push(PathBuf::from(rel));
        }
    }
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

/// Encode `src` to WOFF2 with the C++ reference encoder, caching the result
/// at `dst`. Failures are cached too (as a `.fail` marker file) so that fonts
/// the encoder rejects are not retried on every run.
fn encode_font(compress: &Path, src: &Path, dst: &Path, scratch: &Path) -> Result<(), String> {
    if dst.is_file() {
        return Ok(());
    }
    let fail_marker = PathBuf::from(format!("{}.fail", dst.display()));
    if fail_marker.is_file() {
        return Err(fs::read_to_string(&fail_marker).unwrap_or_default());
    }

    // woff2_compress writes its output next to its input (replacing the
    // extension), so encode in a private scratch dir to avoid collisions.
    fs::create_dir_all(scratch).map_err(|e| e.to_string())?;
    let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("ttf");
    let input = scratch.join(format!("input.{ext}"));
    fs::copy(src, &input).map_err(|e| e.to_string())?;
    let output = Command::new(compress)
        .arg(&input)
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        fs::create_dir_all(dst.parent().unwrap()).map_err(|e| e.to_string())?;
        fs::rename(scratch.join("input.woff2"), dst).map_err(|e| e.to_string())?;
        Ok(())
    } else {
        let msg = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if let Some(parent) = fail_marker.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(&fail_marker, &msg);
        Err(msg)
    }
}

/// Decode a WOFF2 file with the C++ reference decoder binary.
fn cpp_decode(decompress: &Path, woff2_path: &Path, scratch: &Path) -> Result<Vec<u8>, String> {
    // woff2_decompress writes its output next to its input (truncating at
    // the last '.' and appending ".ttf"), so decode a copy in a private
    // scratch dir rather than writing alongside the input file.
    fs::create_dir_all(scratch).map_err(|e| e.to_string())?;
    let input = scratch.join("decode_input.woff2");
    fs::copy(woff2_path, &input).map_err(|e| e.to_string())?;
    let output = Command::new(decompress)
        .arg(&input)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    fs::read(scratch.join("decode_input.ttf")).map_err(|e| e.to_string())
}

/// Parse the sfnt table directory (handling TTC collections), returning
/// (tag, offset, length) records for diff reporting. Best-effort.
fn sfnt_tables(data: &[u8]) -> Option<Vec<(String, u32, u32)>> {
    fn u16_at(data: &[u8], at: usize) -> Option<u16> {
        Some(u16::from_be_bytes(data.get(at..at + 2)?.try_into().ok()?))
    }
    fn u32_at(data: &[u8], at: usize) -> Option<u32> {
        Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?))
    }
    fn font_tables(
        data: &[u8],
        base: usize,
        prefix: &str,
        out: &mut Vec<(String, u32, u32)>,
    ) -> Option<()> {
        let num_tables = u16_at(data, base + 4)? as usize;
        for i in 0..num_tables {
            let rec = base + 12 + i * 16;
            let tag_bytes = data.get(rec..rec + 4)?;
            let tag: String = tag_bytes
                .iter()
                .map(|&b| if b.is_ascii_graphic() { b as char } else { '?' })
                .collect();
            out.push((
                format!("{prefix}{tag}"),
                u32_at(data, rec + 8)?,
                u32_at(data, rec + 12)?,
            ));
        }
        Some(())
    }

    let mut out = Vec::new();
    if data.get(0..4) == Some(b"ttcf") {
        let num_fonts = u32_at(data, 8)?;
        for i in 0..num_fonts {
            let offset = u32_at(data, 12 + i as usize * 4)? as usize;
            font_tables(data, offset, &format!("font{i}/"), &mut out)?;
        }
    } else {
        font_tables(data, 0, "", &mut out)?;
    }
    Some(out)
}

/// Describe how two decoded fonts differ: lengths, first differing byte, and
/// (where parseable) which sfnt tables differ.
fn describe_mismatch(name_a: &str, a: &[u8], name_b: &str, b: &[u8]) -> String {
    let mut msg = format!(
        "{name_a} ({} bytes) != {name_b} ({} bytes)",
        a.len(),
        b.len()
    );
    if let Some(offset) = (0..a.len().min(b.len())).find(|&i| a[i] != b[i]) {
        write!(
            msg,
            "; first diff at offset {offset:#x} ({:#04x} != {:#04x})",
            a[offset], b[offset]
        )
        .unwrap();
    } else {
        write!(msg, "; one is a prefix of the other").unwrap();
    }
    if let (Some(tables_a), Some(tables_b)) = (sfnt_tables(a), sfnt_tables(b)) {
        let differing: Vec<String> = tables_a
            .iter()
            .filter(
                |(tag, offset, len)| match tables_b.iter().find(|(tag_b, ..)| tag_b == tag) {
                    Some((_, offset_b, len_b)) => {
                        (offset, len) != (offset_b, len_b)
                            || a.get(*offset as usize..(*offset + *len) as usize)
                                != b.get(*offset_b as usize..(*offset_b + *len_b) as usize)
                    }
                    None => true,
                },
            )
            .map(|(tag, ..)| tag.clone())
            .chain(
                tables_b
                    .iter()
                    .filter(|(tag, ..)| !tables_a.iter().any(|(tag_a, ..)| tag_a == tag))
                    .map(|(tag, ..)| format!("{tag} (missing from {name_a})")),
            )
            .collect();
        if differing.is_empty() {
            write!(msg, "; table directories match (diff in headers/padding)").unwrap();
        } else {
            write!(msg, "; differing tables: {}", differing.join(", ")).unwrap();
        }
    }
    msg
}

/// `woff2::kDefaultMaxSize`: the output cap baked into the woff2_decompress
/// reference binary. Fonts that decompress to more than this are rejected by
/// the C++ CLI regardless of validity, so for them only wuff and wuff-capi
/// outputs can be compared.
const CPP_CLI_MAX_OUTPUT_SIZE: usize = 30 * 1024 * 1024;

enum Outcome {
    Pass,
    /// wuff and wuff-capi agree; the C++ CLI could not decode the font only
    /// because its output exceeds the CLI's hardcoded 30MB cap.
    PassCppSizeCapped,
    /// All three decoders rejected an input for which rejection is an
    /// acceptable outcome (the committed wpt suite contains deliberately
    /// invalid WOFF2 files).
    PassConsistentReject,
    /// All three decoders rejected the font (still suspicious for
    /// encoder-produced input, but at least consistent).
    ConsistentReject {
        cpp_err: String,
        wuff_err: String,
    },
    /// The C++ reference encoder could not encode this font; nothing to test.
    EncodeFail(String),
    /// The decoders disagreed about whether the font is valid.
    Disagreement(String),
    /// All three decoders succeeded but the outputs were not byte-identical.
    Mismatch(String),
}

/// A single WOFF2 decoding test.
struct TestCase {
    /// Display name, also used for sorting, filtering and reporting.
    name: PathBuf,
    /// Source font to encode with woff2_compress (google/fonts corpus), or
    /// None when `woff2` is a ready-made WOFF2 file.
    src: Option<PathBuf>,
    /// The WOFF2 file to decode: the encoded-cache path for google/fonts,
    /// or the committed file itself for the wpt suite.
    woff2: PathBuf,
    /// Whether consistent rejection by all three decoders is an acceptable
    /// outcome (true for the wpt suite, which contains deliberately
    /// invalid WOFF2 files; false for encoder-produced input).
    reject_ok: bool,
}

fn test_font(compress: &Path, decompress: &Path, case: &TestCase, scratch: &Path) -> Outcome {
    if let Some(src) = &case.src
        && let Err(msg) = encode_font(compress, src, &case.woff2, scratch)
    {
        return Outcome::EncodeFail(msg);
    }
    let woff2_bytes = match fs::read(&case.woff2) {
        Ok(bytes) => bytes,
        Err(e) => return Outcome::EncodeFail(format!("failed to read encoded font: {e}")),
    };

    let cpp = cpp_decode(decompress, &case.woff2, scratch);
    let wuff = wuff::decompress_woff2(&woff2_bytes);
    let capi = capi::decode(&woff2_bytes);

    match (&cpp, &wuff, &capi) {
        (Ok(cpp_out), Ok(wuff_out), Some(capi_out)) => {
            if cpp_out != wuff_out {
                Outcome::Mismatch(describe_mismatch("cpp", cpp_out, "wuff", wuff_out))
            } else if cpp_out != capi_out {
                Outcome::Mismatch(describe_mismatch("cpp", cpp_out, "capi", capi_out))
            } else {
                Outcome::Pass
            }
        }
        (Err(_), Err(_), None) if case.reject_ok => Outcome::PassConsistentReject,
        (Err(cpp_err), Err(wuff_err), None) => Outcome::ConsistentReject {
            cpp_err: cpp_err.clone(),
            wuff_err: wuff_err.to_string(),
        },
        // The reference CLI rejects any font that decompresses to more than
        // 30MB (woff2::kDefaultMaxSize); compare wuff and capi only.
        (Err(_), Ok(wuff_out), Some(capi_out)) if wuff_out.len() > CPP_CLI_MAX_OUTPUT_SIZE => {
            if wuff_out == capi_out {
                Outcome::PassCppSizeCapped
            } else {
                Outcome::Mismatch(describe_mismatch("wuff", wuff_out, "capi", capi_out))
            }
        }
        _ => {
            let status = |ok: bool| if ok { "accepted" } else { "rejected" };
            let mut msg = format!(
                "cpp {}, wuff {}, capi {}",
                status(cpp.is_ok()),
                status(wuff.is_ok()),
                status(capi.is_some()),
            );
            if let Err(e) = &cpp {
                write!(msg, "; cpp error: {e}").unwrap();
            }
            if let Err(e) = &wuff {
                write!(msg, "; wuff error: {e}").unwrap();
            }
            Outcome::Disagreement(msg)
        }
    }
}

fn main() {
    let cfg = parse_args();
    if let Some(jobs) = cfg.jobs {
        rayon::ThreadPoolBuilder::new()
            .num_threads(jobs)
            .build_global()
            .expect("failed to configure thread pool");
    }

    let (compress, decompress) = ensure_woff2_tools(&cfg.woff2_dir);
    let encoded_dir = cfg.data_dir.join("encoded");
    let fonts_dir = cfg.data_dir.join("google-fonts");
    let scratch_root = cfg.data_dir.join("tmp");
    let _ = fs::remove_dir_all(&scratch_root);

    // Source fonts are only needed to (re-)build the encoded WOFF2 cache;
    // once it exists, runs are driven from the cache alone and the sources
    // (and tarball) are not kept on disk.
    let source_mode = cfg.refresh_fonts || fonts_dir.is_dir() || !has_encoded_cache(&encoded_dir);
    let mut fonts = Vec::new();
    if source_mode {
        if cfg.refresh_fonts {
            // The encoded cache is derived from the sources; refreshing the
            // sources invalidates it.
            let _ = fs::remove_dir_all(&encoded_dir);
        }
        let fonts_dir = ensure_fonts(&cfg.data_dir, cfg.refresh_fonts);
        discover_files(&fonts_dir, &fonts_dir, FONT_EXTENSIONS, &mut fonts);
    } else {
        eprintln!(
            "Using encoded font cache at {} (pass --refresh-fonts to re-download sources)",
            encoded_dir.display()
        );
        discover_encoded_fonts(&encoded_dir, &encoded_dir, &mut fonts);
    }

    let mut cases: Vec<TestCase> = fonts
        .into_iter()
        .map(|rel| TestCase {
            src: Some(fonts_dir.join(&rel)),
            woff2: encoded_dir.join(format!("{}.woff2", rel.display())),
            name: rel,
            reject_ok: false,
        })
        .collect();

    // Ready-made WOFF2 files from the wpt WOFF2 conformance suite, committed
    // to the repository. These are decoded as-is, with no encoding step.
    let wpt_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("wpt");
    if wpt_dir.is_dir() {
        let mut wpt_files = Vec::new();
        discover_files(&wpt_dir, &wpt_dir, &["woff2"], &mut wpt_files);
        cases.extend(wpt_files.into_iter().map(|rel| TestCase {
            name: PathBuf::from("wpt").join(&rel),
            woff2: wpt_dir.join(&rel),
            src: None,
            reject_ok: true,
        }));
    }

    cases.sort_by(|a, b| a.name.cmp(&b.name));
    if !cfg.filters.is_empty() {
        cases.retain(|case| {
            let path = case.name.to_string_lossy();
            cfg.filters
                .iter()
                .any(|filter| path.contains(filter.as_str()))
        });
    }
    if let Some(limit) = cfg.limit {
        cases.truncate(limit);
    }
    let total = cases.len();
    eprintln!("Testing {total} fonts...");

    let done = AtomicUsize::new(0);
    let failed = AtomicUsize::new(0);
    let scratch_id = AtomicUsize::new(0);
    let failures: Mutex<Vec<(PathBuf, Outcome)>> = Mutex::new(Vec::new());

    cases.par_iter().for_each(|case| {
        let scratch = scratch_root.join(scratch_id.fetch_add(1, Ordering::Relaxed).to_string());
        let outcome = test_font(&compress, &decompress, case, &scratch);
        let _ = fs::remove_dir_all(&scratch);
        if !matches!(outcome, Outcome::Pass) {
            if !matches!(
                outcome,
                Outcome::PassCppSizeCapped | Outcome::PassConsistentReject
            ) {
                failed.fetch_add(1, Ordering::Relaxed);
                let (category, details) = describe_outcome(&outcome);
                eprintln!("\r{}: {}: {}", category, case.name.display(), details);
            }
            failures.lock().unwrap().push((case.name.clone(), outcome));
        }
        let done = done.fetch_add(1, Ordering::Relaxed) + 1;
        eprint!(
            "\r[{done}/{total}] {} not ok",
            failed.load(Ordering::Relaxed)
        );
        let _ = std::io::stderr().flush();
    });
    let _ = fs::remove_dir_all(&scratch_root);
    eprintln!();

    // Once every font has been encoded into the cache, the extracted source
    // tree is no longer needed. Only drop it after a complete (unfiltered)
    // run so that partial runs can still encode the remaining fonts later.
    if source_mode && cfg.filters.is_empty() && cfg.limit.is_none() {
        eprintln!(
            "Removing extracted source fonts ({}); encoded WOFF2 cache retained",
            fonts_dir.display()
        );
        // Retry once: transient errors (e.g. Spotlight indexing on macOS)
        // can interrupt large recursive deletes.
        if fs::remove_dir_all(&fonts_dir).is_err()
            && let Err(e) = fs::remove_dir_all(&fonts_dir)
        {
            eprintln!(
                "warning: failed to remove {}: {e}; it can be deleted manually",
                fonts_dir.display()
            );
        }
        let _ = fs::remove_file(cfg.data_dir.join("google-fonts.tar.gz"));
    }

    let mut failures = failures.into_inner().unwrap();
    failures.sort_by(|a, b| a.0.cmp(&b.0));

    // Summarise and write a report file.
    let mut counts = [0usize; 7];
    let mut report = String::new();
    for (rel, outcome) in &failures {
        let (category, details) = describe_outcome(outcome);
        counts[category_index(outcome)] += 1;
        writeln!(report, "{}: {}: {}", category, rel.display(), details).unwrap();
    }
    let report_path = cfg.data_dir.join("report.txt");
    fs::write(&report_path, &report).expect("failed to write report");

    let [
        encode_fail,
        consistent_reject,
        disagreement,
        mismatch,
        cpp_size_capped,
        wpt_reject,
        _,
    ] = counts;
    let pass = total - failures.len();
    println!("\nResults:");
    println!("  pass:                    {pass}");
    println!("  pass (cpp size-capped):  {cpp_size_capped}");
    println!("  pass (wpt reject):       {wpt_reject}");
    println!("  mismatch:                {mismatch}");
    println!("  disagreement:            {disagreement}");
    println!("  consistent reject:       {consistent_reject}");
    println!("  encode fail:             {encode_fail}");
    println!("Report written to {}", report_path.display());

    // Encoder failures don't reflect on the decoders under test; everything
    // else (mismatches, disagreements, consistent rejects of encoder output)
    // is a conformance failure.
    if mismatch + disagreement + consistent_reject > 0 {
        std::process::exit(1);
    }
}

fn category_index(outcome: &Outcome) -> usize {
    match outcome {
        Outcome::EncodeFail(_) => 0,
        Outcome::ConsistentReject { .. } => 1,
        Outcome::Disagreement(_) => 2,
        Outcome::Mismatch(_) => 3,
        Outcome::PassCppSizeCapped => 4,
        Outcome::PassConsistentReject => 5,
        Outcome::Pass => 6,
    }
}

fn describe_outcome(outcome: &Outcome) -> (&'static str, String) {
    match outcome {
        Outcome::Pass => ("PASS", String::new()),
        Outcome::PassCppSizeCapped => (
            "PASS (CPP SIZE-CAPPED)",
            "wuff and capi agree; cpp CLI rejects >30MB output".to_string(),
        ),
        Outcome::PassConsistentReject => (
            "PASS (WPT REJECT)",
            "all three decoders reject this (possibly deliberately invalid) input".to_string(),
        ),
        Outcome::EncodeFail(msg) => ("ENCODE FAIL", msg.clone()),
        Outcome::ConsistentReject { cpp_err, wuff_err } => (
            "CONSISTENT REJECT",
            format!("cpp error: {cpp_err}; wuff error: {wuff_err}"),
        ),
        Outcome::Disagreement(msg) => ("DISAGREEMENT", msg.clone()),
        Outcome::Mismatch(msg) => ("MISMATCH", msg.clone()),
    }
}
