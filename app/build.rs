fn main() {
    // Embed icon + manifest (DPI awareness, UTF-8 code page) into the Windows exe.
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../assets/icon.ico");
        res.set_manifest_file("../packaging/windows/rusty-poe-trade.manifest");
        res.set("ProductName", "Rusty POE Trade");
        res.set("FileDescription", "Rusty POE Trade");
        if let Err(e) = res.compile() {
            println!("cargo:warning=failed to embed Windows resources: {e}");
        }
    }
    println!("cargo:rerun-if-changed=../packaging/windows/rusty-poe-trade.manifest");
    println!("cargo:rerun-if-changed=../assets/icon.ico");
}
