use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=../../assets/reader-limits.json");
    println!("cargo:rerun-if-changed=../../scripts/generate-reader-limits.py");
    let limits_out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let generated = Command::new("python3")
        .arg("../../scripts/generate-reader-limits.py")
        .arg("--out-dir")
        .arg(&limits_out)
        .status()
        .expect("Generate shared reader limits");
    assert!(generated.success(), "Reader limit generation failed");

    println!("cargo:rerun-if-changed=../../assets/locales");
    println!("cargo:rerun-if-changed=../../scripts/check-locales.py");
    println!("cargo:rerun-if-changed=../../crates/glim-app/src");
    println!("cargo:rerun-if-changed=src");
    let checked = Command::new("python3")
        .arg("../../scripts/check-locales.py")
        .status()
        .expect("Check locale catalogs");
    assert!(checked.success(), "Locale catalog validation failed");
    let mut catalogs = std::fs::read_dir("../../assets/locales")
        .expect("locale directory")
        .map(|entry| entry.expect("locale entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect::<Vec<_>>();
    catalogs.sort();
    let entries = catalogs
        .iter()
        .map(|path| {
            let code = path.file_stem().unwrap().to_str().unwrap();
            let path = std::fs::canonicalize(path).unwrap();
            format!("({code:?}, include_str!({:?}))", path.to_str().unwrap())
        })
        .collect::<Vec<_>>()
        .join(",\n");
    std::fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("locales.rs"),
        format!("const BUNDLED_CATALOGS: &[(&str, &str)] = &[{entries}];"),
    )
    .expect("embed locale catalogs");
    println!("cargo:rerun-if-changed=../../native/Preview.swift");
    println!("cargo:rerun-if-changed=../../native/ImagePreview.swift");
    println!("cargo:rerun-if-changed=../../native/FloatingReaderToolbar.swift");
    println!("cargo:rerun-if-changed=../../native/HTMLPreview.swift");
    println!("cargo:rerun-if-changed=../../native/SQLitePreview.swift");
    println!("cargo:rerun-if-changed=../../native/WorkbookPreview.swift");
    println!("cargo:rerun-if-changed=../../native/PreviewMain.swift");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let arch = match env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("aarch64") => "arm64",
        Ok("x86_64") => "x86_64",
        _ => panic!("Unsupported macOS architecture"),
    };
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("glim-preview");
    let main = output.with_file_name("main.swift");
    std::fs::copy("../../native/PreviewMain.swift", &main)
        .expect("Cannot prepare native preview entry point");
    let mut compiler = Command::new("xcrun");
    let sdk = Command::new("xcrun")
        .arg("--show-sdk-path")
        .output()
        .expect("macOS SDK path");
    let sdk = PathBuf::from(String::from_utf8_lossy(&sdk.stdout).trim());
    compiler.arg("swiftc");
    if sdk
        .join("System/Library/Frameworks/AppKit.framework/Headers/NSGlassEffectView.h")
        .is_file()
    {
        compiler.arg("-DGLIM_NATIVE_GLASS");
    }
    let status = compiler
        .args([
            "-O",
            "-swift-version",
            "5",
            "-target",
            &format!("{arch}-apple-macos11.0"),
            "../../native/SQLitePreview.swift",
            "../../native/WorkbookPreview.swift",
            "../../native/Preview.swift",
            "../../native/ImagePreview.swift",
            "../../native/FloatingReaderToolbar.swift",
            "../../native/HTMLPreview.swift",
        ])
        .arg(output.with_file_name("ReaderLimits.swift"))
        .arg(main)
        .arg("-o")
        .arg(output)
        .status()
        .expect("Native previews require Apple Command Line Tools (swiftc)");
    assert!(status.success(), "Native preview helper compilation failed");
}
