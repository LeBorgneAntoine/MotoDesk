//! A document on its way to paper: an invoice, a credit note, a ticket or a quote, as a PDF
//! (port of `doPrint` in `desktop/src/routes/Sales.svelte`).
//!
//! Printing a sealed document is an operation the fiscal rules want traced, and every copy
//! after the first has to be told from the original (BOI-TVA-DECLA-30-10-30 § 50). So the
//! journal entry is written before the page is drawn: it is what says whether this is the
//! original or a copy, and a copy reaches paper carrying DUPLICATA. That is why printing is in
//! two steps. [`prepare`] does everything that can fail without leaving a trace (the template,
//! the customer, who the shop is); [`PrintJob::finish`] writes the trace and the PDF. Whatever
//! else may fail (a folder that cannot be written) is to be checked between the two, so that a
//! failure does not use the original up.

use chrono::Local;
use serde_json::Value;

use super::document::{document_data, template_kind_for, DocumentContext};
use super::render::{self, Picture, Pictures};
use super::sample::{shop_block, LOGO_SOURCE};
use super::schema::{Kind, Layout};
use super::{for_printing, pdf, TemplateKind};
use crate::error::ApiError;
use crate::sales::{self, Reprint, Sale};
use crate::session::Client;
use crate::{clients, company, fiscal, settings};

/// Why a document could not be printed.
#[derive(Debug, Clone, PartialEq)]
pub enum PrintError {
    /// A draft: it has no number yet, and is not something to hand to a customer.
    Draft,
    /// The library holds no template for this kind of document.
    NoTemplate(TemplateKind),
    Service(ApiError),
}

impl From<ApiError> for PrintError {
    fn from(error: ApiError) -> Self {
        PrintError::Service(error)
    }
}

/// A document ready to be printed: everything is read, nothing is traced yet.
#[derive(Debug, Clone)]
pub struct PrintJob {
    /// The document as it will be printed. A quote has its number by now.
    pub sale: Sale,
    layout: Layout,
    data: Value,
    pictures: Pictures,
}

/// A printed document.
#[derive(Debug, Clone)]
pub struct Printed {
    pub pdf: Vec<u8>,
    /// The journal entry of this print. `None` for a quote, which is not a fiscal document:
    /// printing it again is simply printing it again.
    pub trace: Option<Reprint>,
}

/// The pictures a document shows: the shop's logo, and those a template carries in its own
/// text (`data:image/png;base64,…`). Pictures at a web address are not fetched.
async fn pictures(client: &Client, has_logo: bool, layout: &Layout, data: &Value) -> Pictures {
    let mut pictures = Pictures::new();
    for element in &layout.elements {
        let Kind::Image(image) = &element.kind else { continue };
        let source = render::image_source(image, data);
        if pictures.contains_key(&source) {
            continue;
        }
        let picture = if source == LOGO_SOURCE && has_logo {
            // A logo that cannot be read is left out: the document is still the document.
            company::logo(client).await.ok().flatten().and_then(|bytes| Picture::decode(&bytes))
        } else if source.starts_with("data:") {
            Picture::from_data_url(&source)
        } else {
            None
        };
        if let Some(picture) = picture {
            pictures.insert(source, picture);
        }
    }
    pictures
}

/// Read everything the document `sale_id` is printed from. A quote that has no number yet
/// takes it here: it is about to be handed to a customer.
pub async fn prepare(client: &Client, sale_id: &str) -> Result<PrintJob, PrintError> {
    let mut sale = sales::get(client, sale_id).await?;
    if sale.is_quote() && !sale.is_sealed() {
        sale = sales::issue_quote(client, sale_id).await?;
    } else if !sale.is_sealed() {
        return Err(PrintError::Draft);
    }

    let kind = template_kind_for(&sale);
    let template = for_printing(client, kind).await?.ok_or(PrintError::NoTemplate(kind))?;

    // The full customer record carries the address and the vehicles. A customer who is no
    // longer there prints as a passing one; any other failure stops here, before the trace.
    let customer = match sale.client_id.as_deref() {
        Some(id) => match clients::get(client, id).await {
            Ok(customer) => Some(customer),
            Err(error) if error.is_not_found() => None,
            Err(error) => return Err(error.into()),
        },
        None => None,
    };
    // Who issues the document and the legal wording are not optional on paper: failing to
    // read them fails the print.
    let company = company::get(client).await?;
    let settings = settings::list(client).await?;
    let profile = fiscal::check_profile(client).await?;

    let shop = shop_block(Some(&company), &settings);
    let data = document_data(&sale, &DocumentContext { shop: &shop, client: customer.as_ref(), profile: Some(&profile) }, Local::now());
    let pictures = pictures(client, company.has_logo, &template.schema, &data).await;
    Ok(PrintJob { sale, layout: template.schema, data, pictures })
}

impl PrintJob {
    /// Whether printing leaves a trace in the fiscal journal.
    pub fn is_traced(&self) -> bool {
        self.sale.is_sealed()
    }

    /// Trace the print when the document is a sealed one, then write it.
    pub async fn finish(&self, client: &Client) -> Result<Printed, PrintError> {
        let trace = if self.is_traced() { Some(sales::log_reprint(client, &self.sale.id).await?) } else { None };
        let mention = trace.as_ref().and_then(|trace| trace.mention.as_deref());
        let pages = render::document(&self.layout, &self.data, &self.pictures, mention);
        Ok(Printed { pdf: pdf::write(&pages, &self.pictures, &self.sale.title()), trace })
    }

    /// What to call the file: the document's number, and which copy it is. Each copy has its
    /// own name, so that writing one never runs into an earlier one still open in a reader.
    pub fn file_stem(&self, trace: Option<&Reprint>) -> String {
        match trace.filter(|trace| trace.duplicata) {
            Some(trace) => format!("{} - duplicata {}", self.sale.title(), trace.copy_number),
            None => self.sale.title(),
        }
    }
}
