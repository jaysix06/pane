//! Packs what Pane installs into one compressed payload the setup program
//! carries: the release `pane.exe` (its interface compiled in) and helper
//! service, Microsoft's shader compiler, and the licenses. Build those
//! first (`scripts/build-installer.ps1` does it all).
//!
//! Payload (before deflate): "PANE1", u32 count, then per file: u16 name
//! length, UTF-8 name (relative, `/`-separated), u64 size, bytes.

use std::path::PathBuf;

fn main() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let release = root.join("target/release");
    let files: Vec<(String, PathBuf)> = vec![
        ("pane.exe".into(), release.join("pane.exe")),
        ("pane-scan-service.exe".into(), release.join("pane-scan-service.exe")),
        // DXC, beside pane.exe: zgx compiles its shaders with it (quicker to
        // a cold window than FXC). Copied from the Windows SDK by
        // scripts/build-installer.ps1.
        ("dxcompiler.dll".into(), release.join("dxcompiler.dll")),
        ("LICENSE.txt".into(), root.join("LICENSE")),
        ("licenses/GeistMono-OFL.txt".into(), root.join("assets/fonts/GeistMono-OFL.txt")),
    ];

    let mut raw = Vec::new();
    raw.extend_from_slice(b"PANE1");
    raw.extend_from_slice(&(files.len() as u32).to_le_bytes());
    for (name, path) in &files {
        println!("cargo:rerun-if-changed={}", path.display());
        let bytes = std::fs::read(path).unwrap_or_else(|e| {
            panic!("{}: {e}. Build Pane in release first: scripts/build-installer.ps1", path.display())
        });
        raw.extend_from_slice(&(name.len() as u16).to_le_bytes());
        raw.extend_from_slice(name.as_bytes());
        raw.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        raw.extend_from_slice(&bytes);
    }
    let packed = miniz_oxide::deflate::compress_to_vec(&raw, 9);
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("payload.bin");
    std::fs::write(&out, &packed).unwrap();
    println!("cargo:rustc-env=PANE_PAYLOAD_RAW_SIZE={}", raw.len());

    // The setup program's own icon and version.
    println!("cargo:rerun-if-changed=setup.rc");
    println!("cargo:rerun-if-changed=../../assets/pane.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let version = std::env::var("CARGO_PKG_VERSION").unwrap();
        let mut parts = version.split('.');
        let macros = [
            format!("PANE_V1={}", parts.next().unwrap_or("0")),
            format!("PANE_V2={}", parts.next().unwrap_or("0")),
            format!("PANE_V3={}", parts.next().unwrap_or("0").split('-').next().unwrap_or("0")),
            format!("PANE_VERSION=\"{version}\""),
        ];
        embed_resource::compile("setup.rc", macros.iter()).manifest_optional().unwrap();
    }
}
