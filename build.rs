fn main() {
    // Embed the app icon into the Windows executable, so Explorer and the taskbar show it.
    // Checked at runtime because build scripts are compiled for the host, not the target.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=assets/icon.ico");
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.compile().expect("failed to embed Windows resources");
    }
}
