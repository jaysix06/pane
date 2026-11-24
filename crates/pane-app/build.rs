//! Embeds Pane's icon and version information into pane.exe.

fn main() {
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
