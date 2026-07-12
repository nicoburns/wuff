//! Preparing the WOFF2 test corpus: building the C++ woff2 reference tools,
//! downloading and extracting the google/fonts sources, and encoding every
//! ttf/otf/ttc font to WOFF2 with the reference `woff2_compress` encoder.
//!
//! Encoding is a distinct phase: the whole corpus is encoded into the cache
//! (`<data dir>/encoded`) and the extracted sources are then deleted, so the
//! subsequent test run is driven from the cache alone.

use rayon::prelude::*;
use std::fs;
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::{Config, discover_files, fatal};

const GOOGLE_FONTS_URL: &str = "https://github.com/google/fonts/archive/refs/heads/main.tar.gz";
const FONT_EXTENSIONS: &[&str] = &["ttf", "otf", "ttc"];

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
pub fn ensure_woff2_tools(woff2_dir: &Path) -> (PathBuf, PathBuf) {
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

/// Download `url` to `dest`, streaming the response body to disk with a
/// progress indicator.
fn download(url: &str, dest: &Path) -> Result<(), String> {
    const MB: f64 = (1024 * 1024) as f64;
    let mut response = ureq::get(url).call().map_err(|e| e.to_string())?;
    // GitHub serves tarballs with chunked transfer encoding, so the total
    // size (and hence a percentage) is usually unavailable.
    let total: Option<u64> = response
        .headers()
        .get("Content-Length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok());
    let mut reader = response.body_mut().as_reader();
    let mut file = io::BufWriter::new(fs::File::create(dest).map_err(|e| e.to_string())?);
    let mut buf = [0u8; 64 * 1024];
    let mut downloaded: u64 = 0;
    let mut next_report = 0;
    loop {
        let read_bytes = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if read_bytes == 0 {
            break;
        }
        file.write_all(&buf[..read_bytes])
            .map_err(|e| e.to_string())?;
        downloaded += read_bytes as u64;
        // Print a progress update roughly once per MB.
        if downloaded >= next_report {
            match total {
                Some(total) => eprint!(
                    "\rDownloaded {:.1} / {:.1} MB ({:.0}%)",
                    downloaded as f64 / MB,
                    total as f64 / MB,
                    100.0 * downloaded as f64 / total as f64,
                ),
                None => eprint!("\rDownloaded {:.1} MB", downloaded as f64 / MB),
            }
            let _ = io::stderr().flush();
            next_report = downloaded + 1024 * 1024;
        }
    }
    file.flush().map_err(|e| e.to_string())?;
    eprintln!("\rDownloaded {:.1} MB   ", downloaded as f64 / MB);
    Ok(())
}

/// Extract a `.tar.gz` file into `dest`, then return the path of the
/// archive's single top-level directory.
fn extract_tarball(tarball: &Path, dest: &Path) -> Result<PathBuf, String> {
    let file = fs::File::open(tarball).map_err(|e| e.to_string())?;
    let gz = flate2::read::GzDecoder::new(io::BufReader::new(file));
    tar::Archive::new(gz)
        .unpack(dest)
        .map_err(|e| e.to_string())?;
    let mut entries = fs::read_dir(dest).map_err(|e| e.to_string())?;
    match (entries.next(), entries.next()) {
        (Some(entry), None) => Ok(entry.map_err(|e| e.to_string())?.path()),
        _ => Err("expected exactly one top-level directory in the tarball".to_string()),
    }
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
        if let Err(e) = download(GOOGLE_FONTS_URL, &partial) {
            fatal(&format!("failed to download the google/fonts tarball: {e}"));
        }
        fs::rename(&partial, &tarball).expect("failed to move downloaded tarball into place");
    }

    eprintln!(
        "Extracting {} (this may take a minute)...",
        tarball.display()
    );
    let extracting = data_dir.join("google-fonts.extracting");
    let _ = fs::remove_dir_all(&extracting);
    let _ = fs::remove_dir_all(&fonts_dir);
    fs::create_dir_all(&extracting).expect("failed to create extraction dir");
    // The tarball contains a single top-level directory (fonts-<branch>);
    // move it into place and discard the wrapper.
    match extract_tarball(&tarball, &extracting) {
        Ok(root) => fs::rename(&root, &fonts_dir).expect("failed to move extracted fonts"),
        Err(e) => fatal(&format!("failed to extract the google/fonts tarball: {e}")),
    }
    let _ = fs::remove_dir_all(&extracting);
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

/// Encode every source font under `fonts_dir` (given as `fonts`, paths
/// relative to `fonts_dir`) to the WOFF2 cache at `encoded_dir`, in parallel.
/// Encoder failures are recorded as `.fail` markers by `encode_font` rather
/// than aborting the run, so the whole corpus is always processed.
fn encode_all(
    compress: &Path,
    fonts_dir: &Path,
    encoded_dir: &Path,
    fonts: &[PathBuf],
    scratch_root: &Path,
) {
    let total = fonts.len();
    let done = AtomicUsize::new(0);
    let failed = AtomicUsize::new(0);
    let scratch_id = AtomicUsize::new(0);
    fonts.par_iter().for_each(|rel| {
        let src = fonts_dir.join(rel);
        let dst = encoded_dir.join(format!("{}.woff2", rel.display()));
        let scratch = scratch_root.join(scratch_id.fetch_add(1, Ordering::Relaxed).to_string());
        if encode_font(compress, &src, &dst, &scratch).is_err() {
            failed.fetch_add(1, Ordering::Relaxed);
        }
        let _ = fs::remove_dir_all(&scratch);
        let done = done.fetch_add(1, Ordering::Relaxed) + 1;
        eprint!(
            "\r[{done}/{total}] encoding ({} failed)",
            failed.load(Ordering::Relaxed)
        );
        let _ = io::stderr().flush();
    });
    eprintln!();
}

/// Ensure the encoded WOFF2 cache at `encoded_dir` exists.
///
/// The source fonts are only needed to (re-)build the cache; once it exists,
/// runs are driven from the cache alone and the sources (and tarball) are not
/// kept on disk. When (re-)building is required, the whole corpus is
/// downloaded, encoded, and then the extracted sources are deleted before the
/// caller runs the tests.
pub fn build_encoded_cache(cfg: &Config, compress: &Path, encoded_dir: &Path, scratch_root: &Path) {
    let fonts_dir = cfg.data_dir.join("google-fonts");
    let source_mode = cfg.refresh_fonts || fonts_dir.is_dir() || !has_encoded_cache(encoded_dir);
    if !source_mode {
        eprintln!(
            "Using encoded font cache at {} (pass --refresh-fonts to re-download sources)",
            encoded_dir.display()
        );
        return;
    }

    if cfg.refresh_fonts {
        // The encoded cache is derived from the sources; refreshing the
        // sources invalidates it.
        let _ = fs::remove_dir_all(encoded_dir);
    }
    let fonts_dir = ensure_fonts(&cfg.data_dir, cfg.refresh_fonts);

    // Encoding step: encode the entire source corpus to the WOFF2 cache.
    // This ignores any FILTER (which restricts only the test run) so the
    // cache is always complete and the sources can be discarded.
    let mut fonts = Vec::new();
    discover_files(&fonts_dir, &fonts_dir, FONT_EXTENSIONS, &mut fonts);
    eprintln!("Encoding {} fonts to WOFF2...", fonts.len());
    encode_all(compress, &fonts_dir, encoded_dir, &fonts, scratch_root);
    let _ = fs::remove_dir_all(scratch_root);

    // The cache is now complete, so the extracted source tree (and the
    // downloaded tarball) are no longer needed and are removed before the
    // tests run; only the encoded WOFF2 cache is retained.
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
