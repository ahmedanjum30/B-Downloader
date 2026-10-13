// Embeds the icon and version details into BDownloader.exe.
fn main() {
    println!("cargo:rerun-if-changed=assets/bdownloader.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon("assets/bdownloader.ico")
        .set("ProductName", "BDownloader")
        .set("FileDescription", "BDownloader")
        .set("LegalCopyright", "Copyright 2026 Ahmad Anjum. PolyForm Noncommercial 1.0.0.");
    if let Err(e) = resource.compile() {
        // Without a resource compiler the app still builds, just without an icon.
        println!("cargo:warning=could not embed the icon: {e}");
    }
}
