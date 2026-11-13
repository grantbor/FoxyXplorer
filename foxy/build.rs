fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    if target_os == "windows" {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "FoxyXplorer");
        res.set("FileDescription", "Картографическое приложение");
        res.set("LegalCopyright", "GPL-3.0");
        res.compile().expect("failed to compile Windows resources");

        // Пересобирать .exe, если иконка изменилась
        println!("cargo:rerun-if-changed=assets/icon.ico");
    }
}
