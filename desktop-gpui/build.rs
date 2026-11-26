//! Puts the application's icon into the program (Windows): the window, the task bar and the
//! shortcuts an installer creates all take it from there.

fn main() {
    println!("cargo:rerun-if-changed=assets/app.rc");
    println!("cargo:rerun-if-changed=assets/icon.ico");
    #[cfg(windows)]
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        // A program without its icon still runs: a missing resource compiler is not an error.
        let _ = embed_resource::compile("assets/app.rc", embed_resource::NONE).manifest_optional();
    }
}
