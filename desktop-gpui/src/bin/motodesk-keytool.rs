//! Command-line tool to diagnose and repair a register whose signing key was lost.
//! See `motodesk_server::local_server::fiscal::keytool` and docs/KEY-REPAIR.md.

fn main() {
    std::process::exit(motodesk_server::local_server::fiscal::keytool::main(std::env::args().skip(1).collect()));
}
