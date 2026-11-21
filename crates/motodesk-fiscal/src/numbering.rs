//! Document numbering.
//!
//! CGI ann. II art. 242 nonies A requires an unbroken sequence based on a
//! continuous chronology. The existing `document_counters` table allocated
//! `FA-{year}-{n}` per company, which is fine for one machine and wrong for
//! this one: two mesh peers working offline both hand out `FA-2026-00042`.
//!
//! Each register therefore owns its own series. Sequences stay unbroken within
//! a series, which is what the rule asks for, and no allocation ever needs the
//! network. This is the same answer a shop with several tills uses.

use serde::{Deserialize, Serialize};

use crate::error::FiscalError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentKind {
    Invoice,
    CreditNote,
    Quote,
    Receipt,
}

impl DocumentKind {
    /// Prefix shown to the user and printed on the document.
    pub fn prefix(self) -> &'static str {
        match self {
            DocumentKind::Invoice => "FA",
            DocumentKind::CreditNote => "AV",
            DocumentKind::Quote => "DE",
            DocumentKind::Receipt => "RC",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            DocumentKind::Invoice => "invoice",
            DocumentKind::CreditNote => "credit_note",
            DocumentKind::Quote => "quote",
            DocumentKind::Receipt => "receipt",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "invoice" => DocumentKind::Invoice,
            "credit_note" => DocumentKind::CreditNote,
            "quote" => DocumentKind::Quote,
            "receipt" => DocumentKind::Receipt,
            _ => return None,
        })
    }

    /// Quotes commit to nothing, so they stay outside the fiscal journal.
    pub fn is_fiscal(self) -> bool {
        !matches!(self, DocumentKind::Quote)
    }
}

/// A register's short code, used in every number it issues.
///
/// Kept to letters and digits so it cannot introduce a separator into a
/// document number and make two different numbers parse the same way.
pub fn normalise_register_code(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .take(6)
        .collect();
    if cleaned.is_empty() {
        "C1".to_string()
    } else {
        cleaned
    }
}

/// Build a document number, for example `FA-2026-C1-00042`.
pub fn format_number(
    kind: DocumentKind,
    year: i32,
    register_code: &str,
    value: i64,
) -> Result<String, FiscalError> {
    if value < 1 {
        return Err(FiscalError::Overflow("document number below 1"));
    }
    Ok(format!(
        "{}-{}-{}-{:05}",
        kind.prefix(),
        year,
        normalise_register_code(register_code),
        value
    ))
}

/// Split a number back into its parts, for imports and for the audit screens.
pub fn parse_number(reference: &str) -> Option<(DocumentKind, i32, String, i64)> {
    let parts: Vec<&str> = reference.split('-').collect();
    if parts.len() != 4 {
        return None;
    }
    let kind = match parts[0] {
        "FA" => DocumentKind::Invoice,
        "AV" => DocumentKind::CreditNote,
        "DE" => DocumentKind::Quote,
        "RC" => DocumentKind::Receipt,
        _ => return None,
    };
    Some((
        kind,
        parts[1].parse().ok()?,
        parts[2].to_string(),
        parts[3].parse().ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_with_a_register_segment() {
        assert_eq!(
            format_number(DocumentKind::Invoice, 2026, "C1", 42).unwrap(),
            "FA-2026-C1-00042"
        );
        assert_eq!(
            format_number(DocumentKind::CreditNote, 2026, "atelier", 7).unwrap(),
            "AV-2026-ATELIE-00007"
        );
    }

    #[test]
    fn two_registers_never_collide_on_the_same_counter_value() {
        let a = format_number(DocumentKind::Invoice, 2026, "C1", 42).unwrap();
        let b = format_number(DocumentKind::Invoice, 2026, "C2", 42).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn round_trips() {
        let n = format_number(DocumentKind::Invoice, 2026, "C1", 12345).unwrap();
        let (kind, year, code, value) = parse_number(&n).unwrap();
        assert_eq!(kind, DocumentKind::Invoice);
        assert_eq!(year, 2026);
        assert_eq!(code, "C1");
        assert_eq!(value, 12345);
    }

    #[test]
    fn numbers_above_five_digits_still_work() {
        let n = format_number(DocumentKind::Invoice, 2026, "C1", 1_234_567).unwrap();
        assert_eq!(n, "FA-2026-C1-1234567");
        assert_eq!(parse_number(&n).unwrap().3, 1_234_567);
    }

    #[test]
    fn register_codes_are_sanitised() {
        assert_eq!(normalise_register_code("c-1"), "C1");
        assert_eq!(normalise_register_code("  "), "C1");
        assert_eq!(normalise_register_code("comptoir-principal"), "COMPTO");
    }

    #[test]
    fn rejects_a_zero_counter() {
        assert!(format_number(DocumentKind::Invoice, 2026, "C1", 0).is_err());
    }

    #[test]
    fn quotes_stay_out_of_the_journal() {
        assert!(!DocumentKind::Quote.is_fiscal());
        assert!(DocumentKind::Invoice.is_fiscal());
        assert!(DocumentKind::Receipt.is_fiscal());
        assert!(DocumentKind::CreditNote.is_fiscal());
    }

    #[test]
    fn rejects_foreign_references() {
        assert!(parse_number("FA-2026-00042").is_none());
        assert!(parse_number("XX-2026-C1-00042").is_none());
        assert!(parse_number("").is_none());
    }
}
