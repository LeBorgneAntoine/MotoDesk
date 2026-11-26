//! Puts the application's icon and its description into the program (Windows): the window, the
//! task bar and the shortcuts an installer creates take the icon from there; the task manager,
//! the firewall and the file's properties take the name and the version.

fn main() {
    println!("cargo:rerun-if-changed=assets/app.rc");
    println!("cargo:rerun-if-changed=assets/icon.ico");
    #[cfg(windows)]
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        // The version the program reports is the package's: app.rc reads it from this file,
        // which the resource compiler finds in the build's own folder.
        let part = |name: &str| std::env::var(name).ok().and_then(|value| value.parse::<u16>().ok()).unwrap_or(0);
        let numbers = format!("{},{},{},0", part("CARGO_PKG_VERSION_MAJOR"), part("CARGO_PKG_VERSION_MINOR"), part("CARGO_PKG_VERSION_PATCH"));
        let text = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
        let header = format!("#define VERSION_NUMBERS {numbers}\n#define VERSION_TEXT \"{text}\"\n");
        let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo gives a build script its folder"));
        std::fs::write(out.join("version.h"), header).expect("the build's own folder can be written");

        // A program without its icon still runs: a missing resource compiler is not an error.
        let _ = embed_resource::compile("assets/app.rc", embed_resource::NONE).manifest_optional();
    }
}
