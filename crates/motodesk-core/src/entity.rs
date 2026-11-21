use serde::{Deserialize, Serialize};

/// Replicated entity kinds tracked in the change log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityType {
    User,
    Client,
    Vehicle,
    Category,
    Product,
    AppSetting,
    DocTemplate,
    Sale,
    SaleLine,
    Payment,
    /// A row of the permanent fiscal journal. Replicated, but never merged:
    /// see [`EntityType::is_append_only`].
    FiscalEntry,
    Supplier,
    WorkshopOrder,
    StockMovement,
}

impl EntityType {
    pub fn as_str(self) -> &'static str {
        match self {
            EntityType::User => "user",
            EntityType::Client => "client",
            EntityType::Vehicle => "vehicle",
            EntityType::Category => "category",
            EntityType::Product => "product",
            EntityType::AppSetting => "app_setting",
            EntityType::DocTemplate => "doc_template",
            EntityType::Sale => "sale",
            EntityType::SaleLine => "sale_line",
            EntityType::Payment => "payment",
            EntityType::FiscalEntry => "fiscal_entry",
            EntityType::Supplier => "supplier",
            EntityType::WorkshopOrder => "workshop_order",
            EntityType::StockMovement => "stock_movement",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "user" => Some(EntityType::User),
            "client" => Some(EntityType::Client),
            "vehicle" => Some(EntityType::Vehicle),
            "category" => Some(EntityType::Category),
            "product" => Some(EntityType::Product),
            "app_setting" => Some(EntityType::AppSetting),
            "doc_template" => Some(EntityType::DocTemplate),
            "sale" => Some(EntityType::Sale),
            "sale_line" => Some(EntityType::SaleLine),
            "payment" => Some(EntityType::Payment),
            "fiscal_entry" => Some(EntityType::FiscalEntry),
            "supplier" => Some(EntityType::Supplier),
            "workshop_order" => Some(EntityType::WorkshopOrder),
            "stock_movement" => Some(EntityType::StockMovement),
            _ => None,
        }
    }

    /// Entities that may only ever be inserted.
    ///
    /// The fiscal journal is hash-chained and signed; an update or a delete
    /// arriving from a peer is either a bug or an attack, and in both cases the
    /// right answer is to ignore it rather than to merge it. Fiscal law is
    /// equally blunt about this: BOI-TVA-DECLA-30-10-30 § 90 allows corrections
    /// only as new opposite entries.
    pub fn is_append_only(self) -> bool {
        matches!(self, EntityType::FiscalEntry)
    }
}

/// Keys excluded from field-level merge (sync metadata).
pub const SYNC_META_KEYS: &[&str] = &[
    "row_version",
    "deleted_at",
    "field_meta",
    "created_at",
    "updated_at",
];

/// Payload keys that mark a business row as sealed into the fiscal journal.
///
/// `locked_at` is the one to rely on: it is text, so it survives the loose
/// column-to-JSON conversion the mesh uses when reading a local row, whereas an
/// integer column may not be present in a snapshot at all.
pub const FISCAL_SEAL_KEYS: &[&str] = &["locked_at", "fiscal_seq"];

/// Whether a payload belongs to a document that has been sealed.
pub fn is_sealed(payload: &serde_json::Value) -> bool {
    FISCAL_SEAL_KEYS.iter().any(|key| match payload.get(*key) {
        None | Some(serde_json::Value::Null) => false,
        Some(serde_json::Value::String(s)) => !s.trim().is_empty(),
        Some(_) => true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn every_variant_round_trips() {
        for e in [
            EntityType::User,
            EntityType::Client,
            EntityType::Vehicle,
            EntityType::Category,
            EntityType::Product,
            EntityType::AppSetting,
            EntityType::DocTemplate,
            EntityType::Sale,
            EntityType::SaleLine,
            EntityType::Payment,
            EntityType::FiscalEntry,
            EntityType::Supplier,
            EntityType::WorkshopOrder,
            EntityType::StockMovement,
        ] {
            assert_eq!(EntityType::parse(e.as_str()), Some(e));
        }
    }

    #[test]
    fn only_the_journal_is_append_only() {
        assert!(EntityType::FiscalEntry.is_append_only());
        assert!(!EntityType::Sale.is_append_only());
        assert!(!EntityType::Payment.is_append_only());
    }

    #[test]
    fn recognises_a_sealed_document() {
        assert!(is_sealed(&json!({ "locked_at": "2026-08-21T10:00:00Z" })));
        assert!(is_sealed(&json!({ "fiscal_seq": 12 })));
        assert!(is_sealed(&json!({ "fiscal_seq": "12" })));
    }

    #[test]
    fn a_draft_is_not_sealed() {
        assert!(!is_sealed(&json!({ "status": "draft" })));
        assert!(!is_sealed(&json!({ "locked_at": null })));
        assert!(!is_sealed(&json!({ "locked_at": "" })));
        assert!(!is_sealed(
            &json!({ "locked_at": "  ", "fiscal_seq": null })
        ));
    }
}
