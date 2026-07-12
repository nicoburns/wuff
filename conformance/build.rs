fn main() {
    // wuff-capi exposes its C++ header directory via DEP_WOFF2_INCLUDE_DIR
    // (thanks to `links = "woff2"` in its Cargo.toml).
    let include_dir = std::env::var("DEP_WOFF2_INCLUDE_DIR").unwrap();
    println!("cargo::rerun-if-changed=src/capi_shim.cpp");
    cc::Build::new()
        .cpp(true)
        .std("c++11")
        .include(&include_dir)
        .file("src/capi_shim.cpp")
        .compile("conformance_capi_shim");
}
