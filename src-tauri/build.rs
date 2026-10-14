fn main() {
    let target = std::env::var("TARGET").unwrap();
    let platform = if target.contains("windows") {
        "windows"
    } else if target.contains("apple-darwin") {
        "macos"
    } else if target.contains("linux") {
        "linux"
    } else {
        panic!("Unsupported QPDF platform: {target}");
    };
    let architecture = if target.starts_with("x86_64") {
        "x64"
    } else if target.starts_with("aarch64") {
        "arm64"
    } else {
        panic!("Unsupported QPDF architecture: {target}");
    };
    let archive = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .join(format!("resources/qpdf/qpdf-{platform}-{architecture}.zip"));
    assert!(
        archive.is_file(),
        "QPDF runtime missing. Run: node scripts/prepare-qpdf.mjs {platform}-{architecture}"
    );
    println!("cargo:rerun-if-changed={}", archive.display());
    println!("cargo:rerun-if-changed=resources/qpdf/manifest.json");
    println!(
        "cargo:rustc-env=CPAHDOCS_QPDF_ARCHIVE={}",
        archive.display()
    );
    tauri_build::build()
}
