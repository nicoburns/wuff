//! Running the conformance tests: decode each WOFF2 file (from the encoded
//! cache or the committed wpt suite) with all three decoders, compare their
//! output, and summarise the results into a report.

use rayon::prelude::*;
use std::fmt::Write as _;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

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

/// What a `TestCase` decodes (or why it can't).
enum CaseInput {
    /// A ready-made WOFF2 file to decode: an encoded-cache entry for the
    /// google/fonts corpus, or a committed file for the wpt suite.
    /// `reject_ok` is true when consistent rejection by all three decoders
    /// is an acceptable outcome (the wpt suite contains deliberately invalid
    /// WOFF2 files; encoder-produced input should always decode).
    Decode { woff2: PathBuf, reject_ok: bool },
    /// The C++ reference encoder could not encode the source font (recorded
    /// as a `.fail` marker in the cache); there is nothing to decode.
    EncodeFailed(String),
}

/// A single WOFF2 decoding test.
pub struct TestCase {
    /// Display name, also used for sorting, filtering and reporting.
    pub name: PathBuf,
    input: CaseInput,
}

fn test_font(decompress: &Path, case: &TestCase, scratch: &Path) -> Outcome {
    let (woff2, reject_ok) = match &case.input {
        CaseInput::Decode { woff2, reject_ok } => (woff2, *reject_ok),
        CaseInput::EncodeFailed(msg) => return Outcome::EncodeFail(msg.clone()),
    };
    let woff2_bytes = match fs::read(woff2) {
        Ok(bytes) => bytes,
        Err(e) => return Outcome::EncodeFail(format!("failed to read encoded font: {e}")),
    };

    let cpp = cpp_decode(decompress, woff2, scratch);
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
        (Err(_), Err(_), None) if reject_ok => Outcome::PassConsistentReject,
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

/// Recursively find all entries in the encoded cache under `dir`, turning
/// them into test cases: `.woff2` files become decode tests, while `.fail`
/// markers (fonts the reference encoder rejected) become `EncodeFailed`
/// cases. The case name is the original font path (i.e. with the `.woff2`
/// or `.fail` suffix stripped) relative to `root`.
fn discover_cached_cases(root: &Path, dir: &Path, out: &mut Vec<TestCase>) {
    for entry in fs::read_dir(dir).expect("failed to read encoded cache dir") {
        let path = entry.expect("failed to read dir entry").path();
        if path.is_dir() {
            discover_cached_cases(root, &path, out);
            continue;
        }
        let Some(rel) = path.strip_prefix(root).unwrap().to_str() else {
            continue;
        };
        if let Some(name) = rel.strip_suffix(".woff2") {
            out.push(TestCase {
                name: PathBuf::from(name),
                input: CaseInput::Decode {
                    woff2: path.clone(),
                    reject_ok: false,
                },
            });
        } else if let Some(name) = rel.strip_suffix(".fail") {
            let msg = fs::read_to_string(&path).unwrap_or_default();
            out.push(TestCase {
                name: PathBuf::from(name),
                input: CaseInput::EncodeFailed(msg),
            });
        }
    }
}

/// Build the full list of test cases: every entry in the encoded cache plus
/// the committed wpt WOFF2 conformance suite (decoded as-is).
pub fn discover_cases(encoded_dir: &Path, wpt_dir: &Path) -> Vec<TestCase> {
    let mut cases = Vec::new();
    discover_cached_cases(encoded_dir, encoded_dir, &mut cases);

    // Ready-made WOFF2 files from the wpt WOFF2 conformance suite, committed
    // to the repository. These are decoded as-is, with no encoding step.
    if wpt_dir.is_dir() {
        let mut wpt_files = Vec::new();
        crate::discover_files(wpt_dir, wpt_dir, &["woff2"], &mut wpt_files);
        cases.extend(wpt_files.into_iter().map(|rel| TestCase {
            name: PathBuf::from("wpt").join(&rel),
            input: CaseInput::Decode {
                woff2: wpt_dir.join(&rel),
                reject_ok: true,
            },
        }));
    }
    cases
}

/// Run every test case in parallel, print a live progress line, write the
/// report file to `data_dir`, and print a summary. Returns true if there were
/// conformance failures (mismatches, disagreements, or consistent rejects of
/// encoder-produced input), i.e. the process should exit non-zero.
pub fn run_and_report(
    decompress: &Path,
    cases: &[TestCase],
    scratch_root: &Path,
    data_dir: &Path,
) -> bool {
    let total = cases.len();
    eprintln!("Testing {total} fonts...");

    let done = AtomicUsize::new(0);
    let failed = AtomicUsize::new(0);
    let scratch_id = AtomicUsize::new(0);
    let failures: Mutex<Vec<(PathBuf, Outcome)>> = Mutex::new(Vec::new());

    cases.par_iter().for_each(|case| {
        let scratch = scratch_root.join(scratch_id.fetch_add(1, Ordering::Relaxed).to_string());
        let outcome = test_font(decompress, case, &scratch);
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
    let _ = fs::remove_dir_all(scratch_root);
    eprintln!();

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
    let report_path = data_dir.join("report.txt").canonicalize().unwrap();
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
    mismatch + disagreement + consistent_reject > 0
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
