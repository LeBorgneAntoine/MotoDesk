//! How values are shown (port of the parts of `desktop/src/lib/format.ts` and
//! `recoveryKey.ts` the migrated screens use).

use motodesk_client::ApiError;

/// What to tell the user about a failed call. The service answers in French, so its message
/// is shown as it is; its details, when it gives some, are more precise.
pub fn humanize_error(error: &ApiError, fallback: &str) -> String {
    if !error.details.is_empty() {
        return error.details.join("\n");
    }
    if error.message.is_empty() { fallback.to_string() } else { error.message.clone() }
}

/// `2026-03-07T09:15:00Z` → `07/03/2026`. Anything that is not a date is shown as it came.
pub fn date(iso: &str) -> String {
    let day = iso.split('T').next().unwrap_or(iso);
    let mut parts = day.split('-');
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(y), Some(m), Some(d), None) if y.len() == 4 && m.len() == 2 && d.len() == 2 => format!("{d}/{m}/{y}"),
        _ => iso.to_string(),
    }
}

/// Cents → `1 234,50 €`, with the narrow no-break spaces French typography uses.
pub fn euros(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let cents = cents.unsigned_abs();
    let units = (cents / 100).to_string();
    let mut grouped = String::new();
    for (i, digit) in units.chars().enumerate() {
        if i > 0 && (units.len() - i).is_multiple_of(3) {
            grouped.push('\u{202f}');
        }
        grouped.push(digit);
    }
    format!("{sign}{grouped},{:02}\u{a0}€", cents % 100)
}

/// Whole number with thousands separated: `12 500`.
pub fn integer(value: i64) -> String {
    let text = euros(value.saturating_mul(100));
    text.split(',').next().unwrap_or_default().to_string()
}

/// Whether what was typed is the last group of the recovery key, however it was typed
/// (lower case, spaces, O for 0, I or L for 1).
pub fn last_group_matches(key: &str, typed: &str) -> bool {
    fn clean(s: &str) -> String {
        s.to_uppercase()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .map(|c| match c {
                'O' => '0',
                'I' | 'L' => '1',
                other => other,
            })
            .collect()
    }
    let last = key.rsplit('-').next().unwrap_or_default();
    !last.is_empty() && clean(typed) == clean(last)
}

/// The initials shown in a person's square: "Léa Martin" → "LM".
pub fn initials(first_name: Option<&str>, last_name: &str) -> String {
    let first = first_name.and_then(|n| n.trim().chars().next());
    let last = last_name.trim().chars().next();
    match (first, last) {
        (Some(f), Some(l)) => format!("{f}{l}"),
        (None, Some(_)) => last_name.trim().chars().take(2).collect(),
        (Some(f), None) => f.to_string(),
        (None, None) => "?".into(),
    }
    .to_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "7QK2M-9ABCD-EFGHJ-KMNPQ-RST01";

    #[test]
    fn accepts_the_last_group_however_it_is_typed() {
        assert!(last_group_matches(KEY, "RST01"));
        assert!(last_group_matches(KEY, " rst ol "));
    }

    #[test]
    fn refuses_anything_else() {
        assert!(!last_group_matches(KEY, ""));
        assert!(!last_group_matches(KEY, "RST0"));
        assert!(!last_group_matches(KEY, "KMNPQ"));
        assert!(!last_group_matches("", ""));
    }

    #[test]
    fn dates_and_amounts_read_the_french_way() {
        assert_eq!(date("2026-03-07T09:15:00Z"), "07/03/2026");
        assert_eq!(date("2026-03-07"), "07/03/2026");
        assert_eq!(date("hier"), "hier");
        assert_eq!(euros(123_450), "1\u{202f}234,50\u{a0}€");
        assert_eq!(euros(-5), "-0,05\u{a0}€");
        assert_eq!(euros(0), "0,00\u{a0}€");
        assert_eq!(integer(12_500), "12\u{202f}500");
    }

    #[test]
    fn the_servers_details_win_over_its_message() {
        let mut error = ApiError::new(400, "bad_request", "Requête invalide");
        assert_eq!(humanize_error(&error, "Erreur"), "Requête invalide");
        error.details = vec!["Le nom est obligatoire.".into(), "L'email est invalide.".into()];
        assert_eq!(humanize_error(&error, "Erreur"), "Le nom est obligatoire.\nL'email est invalide.");
        assert_eq!(humanize_error(&ApiError::new(0, "x", ""), "Erreur"), "Erreur");
    }

    #[test]
    fn initials_of_a_person_or_a_company() {
        assert_eq!(initials(Some("Léa"), "Martin"), "LM");
        assert_eq!(initials(None, "Garage Dupont"), "GA");
        assert_eq!(initials(Some(" "), ""), "?");
    }
}
