//! Embeds the app icon (assets/icon.ico) into GrandPiano.exe when building for
//! Windows, so Explorer, the taskbar and the Start menu shortcut show it.
//! Cross-compiling from Linux uses llvm-rc (see embed-resource's docs).

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=assets/app.rc");
    let windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    let gui = std::env::var_os("CARGO_FEATURE_GUI").is_some();
    if windows && gui && std::path::Path::new("assets/icon.ico").exists() {
        embed_resource::compile("assets/app.rc", embed_resource::NONE).manifest_optional().unwrap();
    }
}
