//! Input rules shared by the business routes.
//!
//! The screens validate nothing on their own, and the local API is the only door
//! to the database, so these checks live here. Messages are French and name the
//! field, because they are shown to shop staff as they are.

use crate::local_server::error::{LocalError, LocalResult};

fn bad(msg: impl Into<String>) -> LocalError {
    LocalError::BadRequest(msg.into())
}

/// Trim, and turn "nothing but spaces" into `None`.
pub fn trimmed(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// A mandatory text field: present, not blank, within `max` characters.
pub fn required(field: &str, value: &str, max: usize) -> LocalResult<String> {
    let v = value.trim();
    if v.is_empty() {
        return Err(bad(format!("{field} : champ obligatoire")));
    }
    if v.chars().count() > max {
        return Err(bad(format!("{field} : {max} caractères au maximum")));
    }
    Ok(v.to_string())
}

/// An optional text field within `max` characters. Blank becomes `None`.
pub fn optional(field: &str, value: &Option<String>, max: usize) -> LocalResult<Option<String>> {
    match trimmed(value) {
        Some(v) if v.chars().count() > max => {
            Err(bad(format!("{field} : {max} caractères au maximum")))
        }
        other => Ok(other),
    }
}

/// Digits with the separators people actually type; at least 6 digits.
pub fn phone(value: &Option<String>) -> LocalResult<Option<String>> {
    let Some(v) = optional("Téléphone", value, 30)? else {
        return Ok(None);
    };
    let allowed = v
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, ' ' | '+' | '.' | '-' | '(' | ')' | '/'));
    let digits = v.chars().filter(char::is_ascii_digit).count();
    if !allowed || digits < 6 {
        return Err(bad(
            "Téléphone : saisissez un numéro (chiffres, espaces, +, points ou tirets)",
        ));
    }
    Ok(Some(v))
}

/// Not a full RFC 5322 parser: one `@`, something on each side, a dot in the
/// domain, no spaces. Enough to catch what typos look like.
pub fn email(value: &Option<String>) -> LocalResult<Option<String>> {
    let Some(v) = optional("Courriel", value, 120)? else {
        return Ok(None);
    };
    let ok = !v.contains(char::is_whitespace)
        && v.matches('@').count() == 1
        && v.split_once('@').is_some_and(|(local, domain)| {
            !local.is_empty()
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
        });
    if !ok {
        return Err(bad("Courriel : adresse invalide (exemple : nom@domaine.fr)"));
    }
    Ok(Some(v))
}

/// Plates are stored upper-case: AB-123-CD, 123 ABC 45, 1-ABC-123 ...
pub fn plate(value: &Option<String>) -> LocalResult<Option<String>> {
    let Some(v) = optional("Plaque", value, 15)? else {
        return Ok(None);
    };
    let v = v.to_uppercase();
    let ok = v.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | ' '))
        && v.chars().filter(char::is_ascii_alphanumeric).count() >= 4;
    if !ok {
        return Err(bad("Plaque : lettres, chiffres et tirets uniquement (4 caractères au moins)"));
    }
    Ok(Some(v))
}

/// Frame / serial numbers vary a lot on bicycles, so only the character set and
/// a sane length are checked.
pub fn vin(value: &Option<String>) -> LocalResult<Option<String>> {
    let Some(v) = optional("N° de série", value, 30)? else {
        return Ok(None);
    };
    let v = v.to_uppercase();
    if v.len() < 4 || !v.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | ' ')) {
        return Err(bad("N° de série : lettres et chiffres uniquement (4 caractères au moins)"));
    }
    Ok(Some(v))
}

pub fn year(value: Option<i64>, current_year: i64) -> LocalResult<Option<i64>> {
    match value {
        Some(y) if !(1885..=current_year + 1).contains(&y) => Err(bad(format!(
            "Année : entre 1885 et {}",
            current_year + 1
        ))),
        other => Ok(other),
    }
}

/// `0..=max`, for counts, prices in cents, mileage and the like.
pub fn non_negative(field: &str, value: Option<i64>, max: i64) -> LocalResult<Option<i64>> {
    match value {
        Some(v) if v < 0 => Err(bad(format!("{field} : ne peut pas être négatif"))),
        Some(v) if v > max => Err(bad(format!("{field} : valeur trop grande"))),
        other => Ok(other),
    }
}

/// A VAT rate or discount in basis points: 0 to 100 %.
pub fn rate_bp(field: &str, value: Option<i64>) -> LocalResult<Option<i64>> {
    match value {
        Some(v) if !(0..=10_000).contains(&v) => {
            Err(bad(format!("{field} : entre 0 et 100 %")))
        }
        other => Ok(other),
    }
}

pub fn one_of(field: &str, value: &Option<String>, allowed: &[&str]) -> LocalResult<()> {
    match value.as_deref() {
        Some(v) if !allowed.contains(&v) => Err(bad(format!(
            "{field} : valeur inconnue « {v} » (attendu : {})",
            allowed.join(", ")
        ))),
        _ => Ok(()),
    }
}

pub const MAX_CENTS: i64 = 100_000_000_00; // 100 million euros
pub const VEHICLE_TYPES: &[&str] = &["moto", "bicycle", "electric"];
pub const ODOMETER_UNITS: &[&str] = &["km", "miles", "hours"];

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> Option<String> {
        Some(v.to_string())
    }

    #[test]
    fn phones_need_digits_not_letters() {
        assert_eq!(phone(&s("06 12 34 56 78")).unwrap(), s("06 12 34 56 78"));
        assert_eq!(phone(&s("+33 6.12.34.56.78")).unwrap(), s("+33 6.12.34.56.78"));
        assert_eq!(phone(&s("   ")).unwrap(), None);
        assert_eq!(phone(&None).unwrap(), None);
        assert!(phone(&s("abc")).is_err());
        assert!(phone(&s("12345")).is_err());
        assert!(phone(&s("06 12 34 56 7x")).is_err());
    }

    #[test]
    fn emails_are_checked_loosely_but_not_ignored() {
        assert_eq!(email(&s("jean.dupont@example.com")).unwrap(), s("jean.dupont@example.com"));
        assert!(email(&s("not-an-email")).is_err());
        assert!(email(&s("a@b")).is_err());
        assert!(email(&s("a b@c.fr")).is_err());
        assert!(email(&s("a@@c.fr")).is_err());
        assert_eq!(email(&s("")).unwrap(), None);
    }

    #[test]
    fn plates_and_serials_are_normalised() {
        assert_eq!(plate(&s("ab-123-cd")).unwrap(), s("AB-123-CD"));
        assert!(plate(&s("zz")).is_err());
        assert!(plate(&s("AB_123")).is_err());
        assert_eq!(vin(&s("jh2pc4001")).unwrap(), s("JH2PC4001"));
        assert!(vin(&s("ab")).is_err());
    }

    #[test]
    fn numbers_have_sensible_bounds() {
        assert!(year(Some(3050), 2026).is_err());
        assert!(year(Some(1700), 2026).is_err());
        assert_eq!(year(Some(2027), 2026).unwrap(), Some(2027));
        assert_eq!(year(None, 2026).unwrap(), None);
        assert!(non_negative("Kilométrage", Some(-500), 9_999_999).is_err());
        assert!(non_negative("Prix", Some(MAX_CENTS + 1), MAX_CENTS).is_err());
        assert_eq!(non_negative("Prix", Some(0), MAX_CENTS).unwrap(), Some(0));
        assert!(rate_bp("TVA", Some(20)).is_ok());
        assert!(rate_bp("TVA", Some(10_001)).is_err());
        assert!(rate_bp("TVA", Some(-1)).is_err());
    }

    #[test]
    fn required_text_is_trimmed_and_bounded() {
        assert_eq!(required("Nom", "  Martin ", 100).unwrap(), "Martin");
        assert!(required("Nom", "   ", 100).is_err());
        assert!(required("Nom", &"x".repeat(101), 100).is_err());
        assert!(one_of("Type", &s("moto"), VEHICLE_TYPES).is_ok());
        assert!(one_of("Type", &s("fusée"), VEHICLE_TYPES).is_err());
    }
}
