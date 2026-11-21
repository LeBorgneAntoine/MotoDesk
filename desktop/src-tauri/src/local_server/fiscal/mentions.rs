//! Mandatory invoice mentions.
//!
//! An invoice missing a required mention is not a valid invoice: article 242
//! nonies A of annexe II to the CGI lists what must appear, article L441-9 of
//! the code de commerce adds the late-payment terms and the 40 EUR fixed
//! recovery indemnity, and article L441-9 III makes an omission punishable by a
//! fine of up to 75 000 EUR for a natural person and 375 000 EUR for a company.
//!
//! The cheapest place to catch that is before the document is sealed, because
//! afterwards the only remedy is a credit note. So validation refuses to run on
//! an incomplete company profile and says exactly which field is missing.

use serde::Serialize;
use sqlx::SqlitePool;

use crate::local_server::error::LocalResult;

/// The company details that end up printed on every document.
#[derive(Debug, Clone, Default, Serialize)]
pub struct CompanyProfile {
    pub name: String,
    pub address: String,
    pub postal_code: String,
    pub city: String,
    pub siret: String,
    pub siren: String,
    pub vat_number: String,
    pub legal_form: String,
    pub capital_eur: i64,
    pub rcs_city: String,
    pub ape_code: String,
    pub vat_subject: bool,
    pub vat_exemption_text: String,
    pub payment_terms_days: i64,
    pub late_fee_rate_bp: i64,
    pub recovery_indemnity_cents: i64,
}

/// A field the law requires that the profile does not have.
#[derive(Debug, Clone, Serialize)]
pub struct MissingMention {
    pub key: String,
    /// French label, shown to the user as-is.
    pub label: String,
    /// The text that would go on the invoice, so the requirement is concrete.
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct ProfileCheck {
    pub complete: bool,
    pub profile: CompanyProfile,
    pub missing: Vec<MissingMention>,
}

pub async fn load(pool: &SqlitePool) -> LocalResult<CompanyProfile> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT key, value FROM app_settings WHERE deleted_at IS NULL")
            .fetch_all(pool)
            .await?;

    let mut profile = CompanyProfile {
        vat_subject: true,
        payment_terms_days: 30,
        late_fee_rate_bp: 1_000,
        recovery_indemnity_cents: 4_000,
        ..Default::default()
    };

    // The trading name is cloud-authoritative and cached rather than kept in
    // the settings table, so it is read from where the Entreprise tab writes it.
    let company: Option<(Option<String>, Option<String>)> =
        sqlx::query_as("SELECT legal_name, name FROM company_cache WHERE id = 1")
            .fetch_optional(pool)
            .await?;
    if let Some((legal_name, name)) = company {
        profile.name = legal_name
            .filter(|s| !s.trim().is_empty())
            .or(name)
            .unwrap_or_default();
    }

    let mut address_line2 = String::new();

    for (key, raw) in rows {
        // Settings are stored as JSON scalars, so "12" and 12 both occur.
        let value: serde_json::Value =
            serde_json::from_str(&raw).unwrap_or(serde_json::Value::String(raw.clone()));
        let text = match &value {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        let number = value
            .as_i64()
            .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
            .unwrap_or(0);

        match key.as_str() {
            // The legal name overrides the trading name loaded from the company
            // cache, but only when it has actually been filled in.
            "shop.legal_name" if !text.trim().is_empty() => profile.name = text,
            "contact.address_line1" => profile.address = text,
            "contact.address_line2" => address_line2 = text,
            "contact.postal_code" => profile.postal_code = text,
            "contact.city" => profile.city = text,
            "legal.siret" => profile.siret = digits(&text),
            "legal.siren" => profile.siren = digits(&text),
            "legal.vat_number" => profile.vat_number = text,
            "legal.legal_form" => profile.legal_form = text,
            "legal.capital_eur" => profile.capital_eur = number,
            "legal.rcs_city" => profile.rcs_city = text,
            "legal.ape_code" => profile.ape_code = text,
            "vat.is_subject" => profile.vat_subject = value.as_bool().unwrap_or(true),
            "vat.exemption_text" => profile.vat_exemption_text = text,
            "invoicing.payment_terms_days" => profile.payment_terms_days = number,
            "invoicing.late_fee_rate_bp" => profile.late_fee_rate_bp = number,
            "invoicing.recovery_indemnity_eur" => profile.recovery_indemnity_cents = number * 100,
            _ => {}
        }
    }

    if !address_line2.trim().is_empty() {
        profile.address = format!("{}, {}", profile.address.trim(), address_line2.trim());
    }

    // The SIREN is the first nine digits of the SIRET, so asking for it twice
    // would be a way to get two different answers.
    if profile.siren.is_empty() && profile.siret.len() >= 9 {
        profile.siren = profile.siret[..9].to_string();
    }

    Ok(profile)
}

/// What a document issued right now would be missing.
pub fn check(profile: &CompanyProfile) -> ProfileCheck {
    let mut missing = Vec::new();

    let require =
        |missing: &mut Vec<MissingMention>, ok: bool, key: &str, label: &str, reason: &str| {
            if !ok {
                missing.push(MissingMention {
                    key: key.into(),
                    label: label.into(),
                    reason: reason.into(),
                });
            }
        };

    require(
        &mut missing,
        !profile.name.trim().is_empty(),
        "company.name",
        "Dénomination sociale",
        "Le nom du vendeur doit figurer sur la facture (CGI ann. II art. 242 nonies A, 1°).",
    );
    require(
        &mut missing,
        !profile.address.trim().is_empty() && !profile.city.trim().is_empty(),
        "company.address",
        "Adresse du siège",
        "L'adresse du vendeur doit figurer sur la facture (CGI ann. II art. 242 nonies A, 1°).",
    );
    require(
        &mut missing,
        profile.siret.len() == 14,
        "legal.siret",
        "SIRET",
        "Le numéro SIRET (14 chiffres) identifie l'établissement émetteur.",
    );
    require(
        &mut missing,
        !profile.legal_form.trim().is_empty(),
        "legal.legal_form",
        "Forme juridique",
        "La forme juridique accompagne la dénomination sur tout document commercial.",
    );
    require(
        &mut missing,
        !profile.ape_code.trim().is_empty(),
        "legal.ape_code",
        "Code APE / NAF",
        "Le code d'activité complète l'identification de l'entreprise.",
    );

    // A company registered at the RCS must print its city of registration; a
    // sole trader has none, so this only applies to the corporate forms.
    if is_company(&profile.legal_form) {
        require(
            &mut missing,
            !profile.rcs_city.trim().is_empty(),
            "legal.rcs_city",
            "Ville du RCS",
            "Une société immatriculée doit indiquer son RCS et sa ville d'immatriculation (C. com. art. R123-237).",
        );
        require(
            &mut missing,
            profile.capital_eur > 0,
            "legal.capital_eur",
            "Capital social",
            "Le montant du capital social est obligatoire sur les documents d'une société (C. com. art. R123-238).",
        );
    }

    if profile.vat_subject {
        require(
            &mut missing,
            !profile.vat_number.trim().is_empty(),
            "legal.vat_number",
            "N° TVA intracommunautaire",
            "Un assujetti doit porter son numéro de TVA sur ses factures (CGI ann. II art. 242 nonies A, 2°).",
        );
    } else {
        require(
            &mut missing,
            !profile.vat_exemption_text.trim().is_empty(),
            "vat.exemption_text",
            "Mention d'exonération",
            "En franchise en base, la facture doit porter la mention « TVA non applicable, art. 293 B du CGI ».",
        );
    }

    require(
        &mut missing,
        profile.late_fee_rate_bp > 0,
        "invoicing.late_fee_rate_bp",
        "Taux des pénalités de retard",
        "Le taux des pénalités de retard est une mention obligatoire (C. com. art. L441-9).",
    );
    require(
        &mut missing,
        profile.recovery_indemnity_cents > 0,
        "invoicing.recovery_indemnity_eur",
        "Indemnité forfaitaire de recouvrement",
        "L'indemnité forfaitaire pour frais de recouvrement, fixée à 40 €, doit être mentionnée (C. com. art. D441-5).",
    );

    ProfileCheck {
        complete: missing.is_empty(),
        profile: profile.clone(),
        missing,
    }
}

/// Legal forms that are companies rather than sole traders.
fn is_company(legal_form: &str) -> bool {
    matches!(
        legal_form.trim().to_uppercase().as_str(),
        "EURL" | "SARL" | "SAS" | "SASU" | "SA" | "SNC" | "SCI" | "SELARL"
    )
}

fn digits(value: &str) -> String {
    value.chars().filter(|c| c.is_ascii_digit()).collect()
}

/// The footer block printed on every invoice, built from the profile.
pub fn payment_terms_text(profile: &CompanyProfile) -> String {
    let rate = format!(
        "{},{:02} %",
        profile.late_fee_rate_bp / 100,
        profile.late_fee_rate_bp % 100
    );
    let indemnity = format!(
        "{},{:02} €",
        profile.recovery_indemnity_cents / 100,
        profile.recovery_indemnity_cents % 100
    );
    format!(
        "Paiement à {} jours. En cas de retard de paiement, pénalités au taux de {} \
         et indemnité forfaitaire pour frais de recouvrement de {} (C. com. art. L441-10 et D441-5). \
         Pas d'escompte pour paiement anticipé.",
        profile.payment_terms_days, rate, indemnity
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn complete_sole_trader() -> CompanyProfile {
        CompanyProfile {
            name: "Garage Dupont".into(),
            address: "12 rue des Motos".into(),
            postal_code: "35000".into(),
            city: "Rennes".into(),
            siret: "12345678900012".into(),
            siren: "123456789".into(),
            vat_number: "FR12123456789".into(),
            legal_form: "EI".into(),
            capital_eur: 0,
            rcs_city: String::new(),
            ape_code: "4540Z".into(),
            vat_subject: true,
            vat_exemption_text: String::new(),
            payment_terms_days: 30,
            late_fee_rate_bp: 1_000,
            recovery_indemnity_cents: 4_000,
        }
    }

    #[test]
    fn a_complete_sole_trader_passes() {
        assert!(check(&complete_sole_trader()).complete);
    }

    #[test]
    fn a_company_must_also_declare_rcs_and_capital() {
        let mut profile = complete_sole_trader();
        profile.legal_form = "SARL".into();
        let result = check(&profile);
        assert!(!result.complete);

        let keys: Vec<&str> = result.missing.iter().map(|m| m.key.as_str()).collect();
        assert!(keys.contains(&"legal.rcs_city"));
        assert!(keys.contains(&"legal.capital_eur"));
    }

    #[test]
    fn a_short_siret_is_rejected() {
        let mut profile = complete_sole_trader();
        profile.siret = "1234".into();
        assert!(check(&profile)
            .missing
            .iter()
            .any(|m| m.key == "legal.siret"));
    }

    #[test]
    fn a_vat_free_trader_needs_the_exemption_wording_not_a_vat_number() {
        let mut profile = complete_sole_trader();
        profile.vat_subject = false;
        profile.vat_number = String::new();
        assert!(!check(&profile).complete);

        profile.vat_exemption_text = "TVA non applicable, art. 293 B du CGI.".into();
        assert!(check(&profile).complete);
    }

    #[test]
    fn missing_mentions_explain_themselves() {
        let profile = CompanyProfile::default();
        let result = check(&profile);
        assert!(!result.missing.is_empty());
        for item in &result.missing {
            assert!(!item.label.is_empty());
            assert!(item.reason.len() > 20, "reason should cite the rule");
        }
    }

    #[test]
    fn payment_terms_carry_the_rate_and_the_indemnity() {
        let text = payment_terms_text(&complete_sole_trader());
        assert!(text.contains("30 jours"));
        assert!(text.contains("10,00 %"));
        assert!(text.contains("40,00 €"));
    }
}
