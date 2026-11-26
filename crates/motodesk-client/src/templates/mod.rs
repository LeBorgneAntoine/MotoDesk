//! Document templates (port of `desktop/src/lib/api/templates.ts` and of `lib/templates/`): the
//! layouts invoices, quotes and delivery notes are printed with.
//!
//! A template is a page and a list of elements placed on it in millimetres ([`schema`]). Texts
//! may hold variables (`{{client.full_name}}`) that are replaced when a document is printed
//! ([`interpolate`], [`variables`]). [`render`] turns a template and its data into what is drawn
//! on each page, once, for the screen and for paper alike; [`pdf`] writes those pages to a file.
//! [`document`] is the data of a real sale, and [`print`] takes one all the way to its PDF,
//! with the trace the fiscal journal keeps of it.

pub mod document;
pub mod interpolate;
pub mod metrics;
pub mod pdf;
pub mod print;
pub mod render;
pub mod sample;
pub mod schema;
pub mod starters;
pub mod variables;

use axum::http::Method;
use serde::{Deserialize, Serialize};

use crate::error::ApiResult;
use crate::session::Client;
use crate::settings::encode;
use schema::Layout;

/// What a template is for. The app prints a document with the default template of its kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TemplateKind {
    #[default]
    Facture,
    Devis,
    BonLivraison,
    /// Anything else: a customer sheet, a product sheet. Also what an unknown kind reads as.
    #[serde(other)]
    Autre,
}

impl TemplateKind {
    pub const ALL: [TemplateKind; 4] = [TemplateKind::Facture, TemplateKind::Devis, TemplateKind::BonLivraison, TemplateKind::Autre];

    /// The identifier the service stores.
    pub fn id(self) -> &'static str {
        match self {
            TemplateKind::Facture => "facture",
            TemplateKind::Devis => "devis",
            TemplateKind::BonLivraison => "bon_livraison",
            TemplateKind::Autre => "autre",
        }
    }

    pub fn from_id(id: &str) -> Self {
        Self::ALL.into_iter().find(|kind| kind.id() == id).unwrap_or(TemplateKind::Autre)
    }

    pub fn label(self) -> &'static str {
        match self {
            TemplateKind::Facture => "Facture",
            TemplateKind::Devis => "Devis",
            TemplateKind::BonLivraison => "Bon de livraison",
            TemplateKind::Autre => "Autre",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Orientation {
    #[default]
    Portrait,
    Landscape,
}

impl Orientation {
    pub const ALL: [Orientation; 2] = [Orientation::Portrait, Orientation::Landscape];

    pub fn id(self) -> &'static str {
        match self {
            Orientation::Portrait => "portrait",
            Orientation::Landscape => "landscape",
        }
    }

    pub fn from_id(id: &str) -> Self {
        if id == "landscape" { Orientation::Landscape } else { Orientation::Portrait }
    }

    pub fn label(self) -> &'static str {
        match self {
            Orientation::Portrait => "Portrait",
            Orientation::Landscape => "Paysage",
        }
    }
}

/// A template as the library lists it: everything but its layout.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TemplateSummary {
    pub id: String,
    pub name: String,
    pub kind: TemplateKind,
    /// "A4" for every template made so far.
    #[serde(default)]
    pub page_size: String,
    #[serde(default)]
    pub orientation: Orientation,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default)]
    pub is_archived: bool,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Template {
    pub id: String,
    pub name: String,
    pub kind: TemplateKind,
    #[serde(default)]
    pub page_size: String,
    #[serde(default)]
    pub orientation: Orientation,
    /// A layout that cannot be read (written by a later version) opens as an empty page.
    #[serde(default, deserialize_with = "schema::lenient")]
    pub schema: Layout,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default)]
    pub is_archived: bool,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

impl Template {
    pub fn summary(&self) -> TemplateSummary {
        TemplateSummary {
            id: self.id.clone(),
            name: self.name.clone(),
            kind: self.kind,
            page_size: self.page_size.clone(),
            orientation: self.orientation,
            is_default: self.is_default,
            is_archived: self.is_archived,
            updated_at: self.updated_at.clone(),
        }
    }

    /// What to send to keep this template as it is, for a change of one of its fields.
    pub fn as_input(&self) -> TemplateInput {
        TemplateInput {
            name: self.name.clone(),
            kind: self.kind,
            page_size: Some(self.page_size.clone()).filter(|size| !size.is_empty()),
            orientation: Some(self.orientation),
            schema: self.schema.clone(),
            is_default: Some(self.is_default),
            is_archived: Some(self.is_archived),
        }
    }
}

/// A template to create, or what replaces an existing one. A field left `None` keeps its value
/// (or takes the service's default on creation).
///
/// The picture of the page the Svelte interface stored with each template is not sent: the
/// service then keeps the one it has, and this interface draws the page itself.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TemplateInput {
    pub name: String,
    pub kind: TemplateKind,
    pub page_size: Option<String>,
    pub orientation: Option<Orientation>,
    pub schema: Layout,
    pub is_default: Option<bool>,
    pub is_archived: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ListQuery {
    pub kind: Option<TemplateKind>,
    pub include_archived: bool,
}

/// The templates, the default ones first.
pub async fn list(client: &Client, query: &ListQuery) -> ApiResult<Vec<TemplateSummary>> {
    let mut params = Vec::new();
    if let Some(kind) = query.kind {
        params.push(format!("kind={}", kind.id()));
    }
    if query.include_archived {
        params.push("include_archived=true".to_string());
    }
    let path = if params.is_empty() { "/api/templates".to_string() } else { format!("/api/templates?{}", params.join("&")) };
    client.get(path).await
}

pub async fn get(client: &Client, id: &str) -> ApiResult<Template> {
    client.get(format!("/api/templates/{}", encode(id))).await
}

pub async fn create(client: &Client, input: &TemplateInput) -> ApiResult<Template> {
    client.post("/api/templates", input).await
}

pub async fn update(client: &Client, id: &str, input: &TemplateInput) -> ApiResult<Template> {
    client.patch(format!("/api/templates/{}", encode(id)), input).await
}

pub async fn delete(client: &Client, id: &str) -> ApiResult<()> {
    client.delete(format!("/api/templates/{}", encode(id))).await
}

pub async fn duplicate(client: &Client, id: &str) -> ApiResult<Template> {
    client.call(Method::POST, format!("/api/templates/{}/duplicate", encode(id))).send().await
}

/// Make this template the one its kind is printed with. It stops being archived.
pub async fn set_default(client: &Client, id: &str) -> ApiResult<Template> {
    client.call(Method::POST, format!("/api/templates/{}/set-default", encode(id))).send().await
}

/// The template a document of this kind is printed with: the default one, or failing that the
/// first of its kind. `None` when the library has none of this kind.
pub async fn for_printing(client: &Client, kind: TemplateKind) -> ApiResult<Option<Template>> {
    let available = list(client, &ListQuery { kind: Some(kind), include_archived: false }).await?;
    let Some(chosen) = available.iter().find(|template| template.is_default).or(available.first()) else { return Ok(None) };
    get(client, &chosen.id).await.map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_template_reads_as_the_service_writes_it() {
        let template: Template = serde_json::from_value(serde_json::json!({
            "id": "t1", "name": "Facture atelier", "kind": "facture", "page_size": "A4", "orientation": "landscape",
            "schema": { "version": 1, "page": { "width": 297, "height": 210, "margin": 12 }, "elements": [
                { "id": "a", "type": "text", "x": 12, "y": 5, "w": 118, "h": 12, "content": "{{shop.name}}", "fontSize": 16, "align": "left", "fontWeight": "bold", "color": "#ffffff" }
            ] },
            "thumbnail_data_url": "data:image/png;base64,AAAA", "is_default": true, "is_archived": false,
            "created_at": "2026-11-01T08:00:00Z", "updated_at": "2026-11-02T08:00:00Z"
        }))
        .unwrap();
        assert_eq!((template.kind, template.orientation, template.is_default), (TemplateKind::Facture, Orientation::Landscape, true));
        assert_eq!(template.schema.elements.len(), 1);
        assert_eq!(template.summary().name, "Facture atelier");

        // A kind this version does not know is filed under « Autre »; a layout it cannot read is an empty page.
        let odd: Template = serde_json::from_value(serde_json::json!({ "id": "t2", "name": "Étiquette", "kind": "etiquette", "schema": "pas un objet" })).unwrap();
        assert_eq!(odd.kind, TemplateKind::Autre);
        assert!(odd.schema.elements.is_empty());
        assert_eq!(odd.schema.page.width, 210.0);
    }

    #[test]
    fn what_is_sent_keeps_the_stored_picture_out() {
        let template: Template = serde_json::from_value(serde_json::json!({ "id": "t1", "name": "Devis", "kind": "devis", "page_size": "A4", "schema": {} })).unwrap();
        let sent = serde_json::to_value(template.as_input()).unwrap();
        assert_eq!(sent["kind"], "devis");
        assert_eq!(sent["orientation"], "portrait");
        assert_eq!(sent["schema"]["version"], 1);
        assert!(sent.get("thumbnail_data_url").is_none());
    }

    #[test]
    fn kinds_go_to_the_service_under_their_identifier() {
        for kind in TemplateKind::ALL {
            assert_eq!(serde_json::to_value(kind).unwrap(), kind.id());
            assert_eq!(TemplateKind::from_id(kind.id()), kind);
        }
        assert_eq!(TemplateKind::from_id("inconnu"), TemplateKind::Autre);
    }
}
