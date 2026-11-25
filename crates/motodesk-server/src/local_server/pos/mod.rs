//! Till peripherals: the 80 mm ticket and a pluggable card terminal.
//!
//! Print failures never roll back a sealed sale. The TPE is called *before*
//! checkout; a refused card must not leave a numbered ticket.

pub mod escpos;
pub mod terminal;
