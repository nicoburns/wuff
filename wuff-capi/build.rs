fn main() {
    // Expose the C/C++ headers to dependent crates as DEP_WUFF_INCLUDE_DIR
    // (the `links = "wuff"` key in Cargo.toml determines the WUFF part).
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    println!("cargo::metadata=include_dir={manifest_dir}/include");
}
