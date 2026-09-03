fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
    if matches!(target_os.as_str(), "linux" | "macos") {
        println!("cargo:rustc-link-arg-benches=-rdynamic");
    }
    println!("cargo:rerun-if-changed=build.rs");
}
