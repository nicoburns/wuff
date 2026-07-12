fn main() {
    // Expose the C/C++ headers to dependent crates as DEP_WOFF2_INCLUDE_DIR
    // (the `links = "woff2"` key in Cargo.toml determines the WOFF2 part).
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    println!("cargo::metadata=include_dir={manifest_dir}/include");
}
