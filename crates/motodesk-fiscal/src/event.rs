//! What the permanent fiscal journal records.
//!
//! § 50 of BOI-TVA-DECLA-30-10-30 lists the data that must be kept: the
//! document number, the line detail, the settlement details and the trace of
//! every correction. § 240 adds the trace of archive generation. Everything
//! below is one of those, plus the events an inspector uses to prove nothing is
//! missing between two documents.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FiscalEventType {
    /// A register was opened for the first time; records the software version
    /// and public key that the following entries are signed with.
    RegisterOpened,
    /// The register's signing key changed (the old one was lost). Signed with the new
    /// key, it says from which entry on that key signs.
    RegisterKeyChanged,
    /// An invoice was validated. Irreversible.
    Invoice,
    /// A counter ticket (ticket de caisse) was validated. Irreversible, and it
    /// moves turnover the same way an invoice does.
    Receipt,
    /// A credit note: the "minus" operation § 90 requires instead of an edit.
    CreditNote,
    /// Money received against a document.
    Payment,
    /// A payment recorded in error, reversed by an opposite entry.
    PaymentReversal,
    ClosureDay,
    ClosureMonth,
    ClosureYear,
    /// A fiscal archive was produced (§ 240 requires this to be traced).
    ArchiveCreated,
    /// Data was purged after archiving (§ 250).
    DataPurged,
    /// The original of a document went to paper or PDF.
    DocumentPrinted,
    /// A document was printed again; the copy carries the DUPLICATA mention.
    DocumentReprinted,
    /// A setting that affects invoicing or VAT was changed.
    FiscalSettingChanged,
    /// The integrity checker was run, with its verdict.
    IntegrityChecked,
    /// A cashier opened a shift and declared the cash float.
    SessionOpened,
    /// A cashier closed a shift and declared a counted drawer total.
    SessionClosed,
    /// The cash drawer was opened without a sale (no-sale).
    DrawerOpened,
}

impl FiscalEventType {
    pub fn as_str(self) -> &'static str {
        match self {
            FiscalEventType::RegisterOpened => "register_opened",
            FiscalEventType::RegisterKeyChanged => "register_key_changed",
            FiscalEventType::Invoice => "invoice",
            FiscalEventType::Receipt => "receipt",
            FiscalEventType::CreditNote => "credit_note",
            FiscalEventType::Payment => "payment",
            FiscalEventType::PaymentReversal => "payment_reversal",
            FiscalEventType::ClosureDay => "closure_day",
            FiscalEventType::ClosureMonth => "closure_month",
            FiscalEventType::ClosureYear => "closure_year",
            FiscalEventType::ArchiveCreated => "archive_created",
            FiscalEventType::DataPurged => "data_purged",
            FiscalEventType::DocumentPrinted => "document_printed",
            FiscalEventType::DocumentReprinted => "document_reprinted",
            FiscalEventType::FiscalSettingChanged => "fiscal_setting_changed",
            FiscalEventType::IntegrityChecked => "integrity_checked",
            FiscalEventType::SessionOpened => "session_opened",
            FiscalEventType::SessionClosed => "session_closed",
            FiscalEventType::DrawerOpened => "drawer_opened",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "register_opened" => FiscalEventType::RegisterOpened,
            "register_key_changed" => FiscalEventType::RegisterKeyChanged,
            "invoice" => FiscalEventType::Invoice,
            "receipt" => FiscalEventType::Receipt,
            "credit_note" => FiscalEventType::CreditNote,
            "payment" => FiscalEventType::Payment,
            "payment_reversal" => FiscalEventType::PaymentReversal,
            "closure_day" => FiscalEventType::ClosureDay,
            "closure_month" => FiscalEventType::ClosureMonth,
            "closure_year" => FiscalEventType::ClosureYear,
            "archive_created" => FiscalEventType::ArchiveCreated,
            "data_purged" => FiscalEventType::DataPurged,
            "document_printed" => FiscalEventType::DocumentPrinted,
            "document_reprinted" => FiscalEventType::DocumentReprinted,
            "fiscal_setting_changed" => FiscalEventType::FiscalSettingChanged,
            "integrity_checked" => FiscalEventType::IntegrityChecked,
            "session_opened" => FiscalEventType::SessionOpened,
            "session_closed" => FiscalEventType::SessionClosed,
            "drawer_opened" => FiscalEventType::DrawerOpened,
            _ => return None,
        })
    }

    /// Whether the entry moves revenue, and so feeds the period and perpetual
    /// totals. Only invoices and credit notes do; payments settle an amount
    /// already counted, and counting both would double the turnover.
    pub fn affects_turnover(self) -> bool {
        matches!(
            self,
            FiscalEventType::Invoice | FiscalEventType::Receipt | FiscalEventType::CreditNote
        )
    }

    /// Whether the entry is a closure, which must never be purged (§ 170).
    pub fn is_closure(self) -> bool {
        matches!(
            self,
            FiscalEventType::ClosureDay
                | FiscalEventType::ClosureMonth
                | FiscalEventType::ClosureYear
        )
    }
}

/// Closure granularity. All three are mandatory and cumulative (§ 170).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClosureKind {
    Day,
    Month,
    Year,
}

impl ClosureKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ClosureKind::Day => "day",
            ClosureKind::Month => "month",
            ClosureKind::Year => "year",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "day" => ClosureKind::Day,
            "month" => ClosureKind::Month,
            "year" => ClosureKind::Year,
            _ => return None,
        })
    }

    pub fn event(self) -> FiscalEventType {
        match self {
            ClosureKind::Day => FiscalEventType::ClosureDay,
            ClosureKind::Month => FiscalEventType::ClosureMonth,
            ClosureKind::Year => FiscalEventType::ClosureYear,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_names_round_trip() {
        for e in [
            FiscalEventType::RegisterOpened,
            FiscalEventType::Invoice,
            FiscalEventType::Receipt,
            FiscalEventType::CreditNote,
            FiscalEventType::Payment,
            FiscalEventType::PaymentReversal,
            FiscalEventType::ClosureDay,
            FiscalEventType::ClosureMonth,
            FiscalEventType::ClosureYear,
            FiscalEventType::ArchiveCreated,
            FiscalEventType::DataPurged,
            FiscalEventType::DocumentPrinted,
            FiscalEventType::DocumentReprinted,
            FiscalEventType::FiscalSettingChanged,
            FiscalEventType::IntegrityChecked,
            FiscalEventType::SessionOpened,
            FiscalEventType::SessionClosed,
            FiscalEventType::DrawerOpened,
        ] {
            assert_eq!(FiscalEventType::parse(e.as_str()), Some(e));
        }
        assert_eq!(FiscalEventType::parse("nope"), None);
    }

    #[test]
    fn only_documents_move_turnover() {
        assert!(FiscalEventType::Invoice.affects_turnover());
        assert!(FiscalEventType::Receipt.affects_turnover());
        assert!(FiscalEventType::CreditNote.affects_turnover());
        assert!(!FiscalEventType::Payment.affects_turnover());
        assert!(!FiscalEventType::SessionOpened.affects_turnover());
        assert!(!FiscalEventType::DrawerOpened.affects_turnover());
        assert!(!FiscalEventType::ClosureDay.affects_turnover());
    }

    #[test]
    fn closure_kinds_round_trip() {
        for k in [ClosureKind::Day, ClosureKind::Month, ClosureKind::Year] {
            assert_eq!(ClosureKind::parse(k.as_str()), Some(k));
            assert!(k.event().is_closure());
        }
    }
}
