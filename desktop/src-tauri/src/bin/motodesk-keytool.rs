//! Command-line tool to diagnose and repair a register whose signing key was lost.
//! See `motodesk_desktop_lib::keytool_main` and docs/KEY-REPAIR.md.

fn main() {
    std::process::exit(motodesk_desktop_lib::keytool_main(std::env::args().skip(1).collect()));
}
