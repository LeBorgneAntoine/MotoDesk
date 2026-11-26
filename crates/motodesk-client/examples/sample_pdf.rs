//! Write the starter templates as PDF files, filled with the sample documents, to look at
//! what the app prints without opening it.
//!
//! ```text
//! cargo run -p motodesk-client --example sample_pdf -- C:\Temp\modeles
//! ```

use std::path::PathBuf;

use motodesk_client::templates::render::{document, Pictures};
use motodesk_client::templates::sample::{demo_shop, sample_data};
use motodesk_client::templates::starters::CATALOG;
use motodesk_client::templates::{pdf, TemplateKind};
use serde_json::{json, Value};

fn main() -> std::io::Result<()> {
    let folder = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".to_string()));
    std::fs::create_dir_all(&folder)?;
    for starter in &CATALOG {
        let data = sample_data(starter.kind, &demo_shop(), 30);
        let pages = document(&(starter.build)(), &data, &Pictures::new(), None);
        let path = folder.join(format!("{}.pdf", starter.name));
        std::fs::write(&path, pdf::write(&pages, &Pictures::new(), starter.name))?;
        println!("{}", path.display());
    }

    // A copy of a long invoice: three pages, each numbered and marked.
    let mut long = sample_data(TemplateKind::Facture, &demo_shop(), 30);
    long["items"] = Value::Array(
        (1..=30).map(|n| json!({ "label": format!("Pièce n° {n}"), "qty": n, "unit": "pce", "price_ht": 1250, "vat_rate": 2000, "total_ttc": 1500 * n })).collect(),
    );
    let pages = document(&(CATALOG[0].build)(), &long, &Pictures::new(), Some("Duplicata"));
    let path = folder.join("Facture longue (duplicata).pdf");
    std::fs::write(&path, pdf::write(&pages, &Pictures::new(), "Facture longue"))?;
    println!("{}", path.display());
    Ok(())
}
