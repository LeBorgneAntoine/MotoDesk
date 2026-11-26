//! The fiscal register: its identity, its journal, the closures, the archives and the
//! integrity check (port of `desktop/src/lib/api/fiscal.ts`).
//!
//! These are the controls an inspector asks to see under CGI art. 286 I-3° bis. Everything is
//! read-only except closing a period and producing an archive, both of which append to the
//! journal and can never be undone.

use serde::{Deserialize, Serialize};

use crate::error::ApiResult;
use crate::session::Client;
use crate::settings::encode;

// ---- the company profile -----------------------------------------------------------------------

/// A mention an invoice must carry and the company profile does not supply.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct MissingMention {
    pub key: String,
    pub label: String,
    #[serde(default)]
    pub reason: String,
}

/// What the shop's profile says of how its documents are worded. (The identity itself is read
/// from the settings; see `templates::sample::shop_block`.)
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct CompanyProfile {
    /// False for a shop that charges no VAT (franchise en base): its documents say why.
    pub vat_subject: bool,
    pub vat_exemption_text: String,
    /// How long a customer has to pay.
    pub payment_terms_days: i64,
}

impl Default for CompanyProfile {
    fn default() -> Self {
        CompanyProfile { vat_subject: true, vat_exemption_text: String::new(), payment_terms_days: 30 }
    }
}

/// What an invoice issued right now would be missing.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct ProfileCheck {
    #[serde(default)]
    pub complete: bool,
    #[serde(default)]
    pub profile: CompanyProfile,
    #[serde(default)]
    pub missing: Vec<MissingMention>,
    #[serde(default)]
    pub payment_terms_text: String,
}

impl ProfileCheck {
    /// The missing mentions, in a sentence; `None` when the profile is complete.
    pub fn blocker(&self) -> Option<String> {
        (!self.complete).then(|| self.missing.iter().map(|m| m.label.as_str()).collect::<Vec<_>>().join(", "))
    }
}

pub async fn check_profile(client: &Client) -> ApiResult<ProfileCheck> {
    client.get("/api/fiscal/profile").await
}

// ---- the register ------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RegisterInfo {
    pub id: String,
    /// The short code that appears in every number this device issues.
    pub code: String,
    #[serde(default)]
    pub public_key: String,
    /// `keychain` or `database`.
    #[serde(default)]
    pub key_store: String,
    #[serde(default)]
    pub software_version: String,
    #[serde(default)]
    pub opened_at: String,
    #[serde(default)]
    pub last_seq: i64,
    #[serde(default)]
    pub last_chain_hash: String,
    /// Reset by each annual closure.
    #[serde(default)]
    pub period_grand_total_cents: i64,
    /// Never reset, from the first use of the software.
    #[serde(default)]
    pub perpetual_total_cents: i64,
    /// How far the newest journal entry is ahead of this computer's clock, in seconds.
    #[serde(default)]
    pub journal_ahead_secs: i64,
}

pub async fn register(client: &Client) -> ApiResult<RegisterInfo> {
    client.get("/api/fiscal/register").await
}

// ---- the journal -------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct JournalEntry {
    pub seq: i64,
    #[serde(default)]
    pub register_code: String,
    pub event_type: String,
    #[serde(default)]
    pub occurred_at: String,
    #[serde(default)]
    pub document_ref: Option<String>,
    #[serde(default)]
    pub total_ttc_cents: i64,
    #[serde(default)]
    pub chain_hash: String,
}

impl JournalEntry {
    /// Whether what was typed finds this entry: a piece of its document's number, the start of
    /// its hash, or its sequence number.
    pub fn matches(&self, needle: &str) -> bool {
        let needle = needle.trim().to_lowercase();
        needle.is_empty()
            || self.document_ref.as_deref().is_some_and(|reference| reference.to_lowercase().contains(&needle))
            || self.chain_hash.to_lowercase().starts_with(&needle)
            || self.seq.to_string() == needle
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct JournalQuery {
    pub from: String,
    pub to: String,
    /// One of [`JOURNAL_EVENTS`], or empty for all.
    pub event_type: String,
    pub limit: Option<u32>,
}

pub async fn journal(client: &Client, query: &JournalQuery) -> ApiResult<Vec<JournalEntry>> {
    let mut params = Vec::new();
    for (name, value) in [("from", &query.from), ("to", &query.to), ("event_type", &query.event_type)] {
        if !value.trim().is_empty() {
            params.push(format!("{name}={}", encode(value.trim())));
        }
    }
    if let Some(limit) = query.limit {
        params.push(format!("limit={limit}"));
    }
    let qs = if params.is_empty() { String::new() } else { format!("?{}", params.join("&")) };
    client.get(format!("/api/fiscal/journal{qs}")).await
}

/// (event, what it is called), in the order the filter lists them.
pub const JOURNAL_EVENTS: [(&str, &str); 11] = [
    ("register_opened", "Ouverture de caisse"),
    ("invoice", "Facture"),
    ("credit_note", "Avoir"),
    ("payment", "Règlement"),
    ("payment_reversal", "Annulation de règlement"),
    ("document_printed", "Impression"),
    ("document_reprinted", "Duplicata"),
    ("closure_day", "Clôture journalière"),
    ("closure_month", "Clôture mensuelle"),
    ("closure_year", "Clôture annuelle"),
    ("archive_created", "Archivage"),
];

/// What a journal event is called. An unknown event is shown as it came.
pub fn journal_event_label(event: &str) -> &str {
    JOURNAL_EVENTS.iter().find(|(value, _)| *value == event).map(|(_, label)| *label).unwrap_or(event)
}

// ---- integrity ---------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RegisterIntegrity {
    pub register_id: String,
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub entries: i64,
    #[serde(default)]
    pub last_seq: i64,
    #[serde(default)]
    pub signatures_verified: i64,
    /// Entries whose signature the register itself declares can no longer be verified (the
    /// key that made it was lost). Their hashes still chain.
    #[serde(default)]
    pub declared_unverifiable: i64,
    pub ok: bool,
    /// The first break found, in French.
    #[serde(default)]
    pub problem: Option<String>,
    #[serde(default)]
    pub problem_seq: Option<i64>,
    /// What the cross-checks against counters, documents and closures found.
    #[serde(default)]
    pub findings: Vec<String>,
}

impl RegisterIntegrity {
    /// The findings that say more than the problem already shown.
    pub fn further_findings(&self) -> &[String] {
        match (&self.problem, self.findings.first()) {
            (Some(problem), Some(first)) if problem == first => &self.findings[1..],
            _ => &self.findings,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct IntegrityReport {
    pub ok: bool,
    #[serde(default)]
    pub checked_at: String,
    #[serde(default)]
    pub registers: Vec<RegisterIntegrity>,
}

/// Recompute every hash and check every signature, from the first entry to the last. Changes
/// no data; the result is written to the journal.
pub async fn verify_chains(client: &Client) -> ApiResult<IntegrityReport> {
    client.post("/api/fiscal/verify", &serde_json::json!({})).await
}

// ---- closures ----------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClosureKind {
    Day,
    Month,
    Year,
}

impl ClosureKind {
    /// "Journalière", "Mensuelle", "Annuelle": as in "clôture journalière".
    pub fn label(self) -> &'static str {
        match self {
            ClosureKind::Day => "Journalière",
            ClosureKind::Month => "Mensuelle",
            ClosureKind::Year => "Annuelle",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Closure {
    pub id: String,
    pub kind: ClosureKind,
    #[serde(default)]
    pub period_start: String,
    #[serde(default)]
    pub period_end: String,
    #[serde(default)]
    pub document_count: i64,
    #[serde(default)]
    pub total_ht_cents: i64,
    #[serde(default)]
    pub total_vat_cents: i64,
    #[serde(default)]
    pub total_ttc_cents: i64,
    /// Never reset, from the first use of the software.
    #[serde(default)]
    pub perpetual_total_cents: i64,
    #[serde(default)]
    pub closed_at: String,
}

pub async fn list_closures(client: &Client, limit: u32) -> ApiResult<Vec<Closure>> {
    client.get(format!("/api/fiscal/closures?limit={limit}")).await
}

/// Close a period. `on` is any day inside it (`2026-11-05`); without one, the service takes
/// the latest finished period of that kind. Administrators only; cannot be undone.
pub async fn close_period(client: &Client, kind: ClosureKind, on: Option<&str>) -> ApiResult<Closure> {
    client.post("/api/fiscal/closures", &serde_json::json!({ "kind": kind, "on": on })).await
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
struct PendingDays {
    #[serde(default)]
    days: Vec<String>,
}

/// The business days with activity that have not been closed yet, oldest first.
pub async fn pending_closures(client: &Client) -> ApiResult<Vec<String>> {
    client.get::<PendingDays>("/api/fiscal/closures/pending").await.map(|pending| pending.days)
}

// ---- archives ----------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Archive {
    pub id: String,
    #[serde(default)]
    pub period_start: String,
    #[serde(default)]
    pub period_end: String,
    #[serde(default)]
    pub file_path: String,
    #[serde(default)]
    pub file_bytes: i64,
    #[serde(default)]
    pub entry_count: i64,
    #[serde(default)]
    pub first_seq: i64,
    #[serde(default)]
    pub last_seq: i64,
    #[serde(default)]
    pub created_at: String,
}

pub async fn list_archives(client: &Client) -> ApiResult<Vec<Archive>> {
    client.get("/api/fiscal/archives").await
}

/// Produce the archive of a closed year. Administrators only.
pub async fn create_archive(client: &Client, year: i32) -> ApiResult<Archive> {
    client.post("/api/fiscal/archives", &serde_json::json!({ "year": year })).await
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ArchiveVerification {
    pub ok: bool,
    #[serde(default)]
    pub problem: Option<String>,
}

/// Check that an archive's file still has the hash it was given.
pub async fn verify_archive(client: &Client, id: &str) -> ApiResult<ArchiveVerification> {
    client.post(format!("/api/fiscal/archives/{id}/verify"), &serde_json::json!({})).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_incomplete_profile_names_what_is_missing() {
        let check: ProfileCheck = serde_json::from_str(
            r#"{"complete":false,"profile":{"name":"Garage"},"payment_terms_text":"",
                "missing":[{"key":"legal.siret","label":"SIRET","reason":"obligatoire"},
                           {"key":"contact.city","label":"Ville","reason":"obligatoire"}]}"#,
        )
        .unwrap();
        assert_eq!(check.blocker().as_deref(), Some("SIRET, Ville"));
        assert_eq!(check.profile, CompanyProfile::default(), "what the profile does not say reads as the usual case");
        assert_eq!(ProfileCheck { complete: true, ..Default::default() }.blocker(), None);
    }

    fn entry(seq: i64, document: Option<&str>, hash: &str) -> JournalEntry {
        serde_json::from_value(serde_json::json!({
            "seq": seq, "event_type": "invoice", "document_ref": document, "chain_hash": hash
        }))
        .unwrap()
    }

    #[test]
    fn a_journal_entry_is_found_by_its_document_its_hash_or_its_number() {
        let line = entry(42, Some("FA-2026-CF60F-00001"), "a1b2c3d4e5");
        assert!(line.matches(""));
        assert!(line.matches(" cf60f-0000 "));
        assert!(line.matches("A1B2"));
        assert!(!line.matches("b2c3"), "a hash is found by its start, not its middle");
        assert!(line.matches("42"));
        assert!(!line.matches("4"), "a sequence number is found whole");
        assert!(!entry(7, None, "ffff").matches("fa-"));
    }

    #[test]
    fn events_and_closures_have_their_french_names() {
        assert_eq!(journal_event_label("payment_reversal"), "Annulation de règlement");
        assert_eq!(journal_event_label("migration"), "migration");
        assert_eq!(ClosureKind::Month.label(), "Mensuelle");
        assert_eq!(serde_json::to_value(ClosureKind::Year).unwrap(), "year");
    }

    #[test]
    fn a_problem_is_not_repeated_among_the_findings() {
        let broken: RegisterIntegrity = serde_json::from_value(serde_json::json!({
            "register_id": "r1", "ok": false, "problem": "empreinte rompue",
            "findings": ["empreinte rompue", "compteur en avance"]
        }))
        .unwrap();
        assert_eq!(broken.further_findings(), ["compteur en avance"]);
        let other: RegisterIntegrity =
            serde_json::from_value(serde_json::json!({ "register_id": "r1", "ok": true, "findings": ["une remarque"] })).unwrap();
        assert_eq!(other.further_findings(), ["une remarque"]);
    }
}
