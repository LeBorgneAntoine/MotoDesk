//! Motodesk's embedded data service.
//!
//! Everything a PC needs to work on its own: the local API, the SQLite database, the fiscal
//! journal, and the sync with the other PCs and the cloud. It knows nothing about the window
//! that hosts it, so the same service runs under the Tauri shell and under the GPUI one.
//!
//! A host starts it with [`local_server::start`], and tells it how to follow the "keep running
//! in the background" preference with [`background::set_host`].

pub mod background;
pub mod local_server;
pub mod logs;
pub mod previous_install;
pub mod startup;
