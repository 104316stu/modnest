// Puts the ModNest icon into the exe as icon resource 1, which GPUI uses for the window and taskbar.
fn main() {
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=../src-tauri/icons/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("app.rc", embed_resource::NONE).manifest_optional().unwrap();
    }
}
