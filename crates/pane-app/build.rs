//! Embeds Pane's icon and version information into pane.exe, and in a
//! release build its interface: the scripts in ui/, compiled to Luau
//! bytecode (a script that doesn't parse fails the build), with the images
//! they show. A debug build reads ui/ from the disk and reloads it on save.

fn main() {
    if std::env::var("PROFILE").as_deref() == Ok("release") {
        zgx_script::build::bundle("../../ui", "ui").expect("bundle ui/ into pane.exe");
    }
    println!("cargo:rerun-if-changed=pane.rc");
    println!("cargo:rerun-if-changed=../../assets/pane.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let version = std::env::var("CARGO_PKG_VERSION").unwrap();
    let mut parts = version.split('.').map(|p| p.split('-').next().unwrap_or("0").to_string());
    let (v1, v2, v3) = (
        parts.next().unwrap_or_default(),
        parts.next().unwrap_or_default(),
        parts.next().unwrap_or_default(),
    );
    let macros = [
        format!("PANE_V1={v1}"),
        format!("PANE_V2={v2}"),
        format!("PANE_V3={v3}"),
        format!("PANE_VERSION=\"{version}\""),
    ];
    embed_resource::compile("pane.rc", macros.iter()).manifest_optional().unwrap();
}
