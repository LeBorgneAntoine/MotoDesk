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

/// A moment as the service stores it (`2026-03-07T09:15:00Z`, or SQLite's
/// `2026-03-07 09:15:00`, both in UTC) → `07/03/2026 10:15` on this PC's clock. A day
/// without an hour (`2026-03-07`) stays a day: `07/03/2026`.
pub fn date_time(stored: &str) -> String {
    date_time_in(stored, &chrono::Local)
}

fn date_time_in<Tz: chrono::TimeZone>(stored: &str, zone: &Tz) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let stored = stored.trim();
    let moment = chrono::DateTime::parse_from_rfc3339(stored).map(|m| m.with_timezone(&chrono::Utc)).or_else(|_| {
        chrono::NaiveDateTime::parse_from_str(stored, "%Y-%m-%d %H:%M:%S").map(|naive| naive.and_utc())
    });
    match moment {
        Ok(moment) => moment.with_timezone(zone).format("%d/%m/%Y %H:%M").to_string(),
        Err(_) => date(stored),
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

/// Cents → `1 235 €`: whole euros, for a headline figure.
pub fn euros_whole(cents: i64) -> String {
    let rounded = (cents.unsigned_abs() + 50) / 100;
    let sign = if cents < 0 && rounded > 0 { "-" } else { "" };
    format!("{sign}{}\u{a0}€", integer(rounded as i64))
}

/// "1 234,50" or "1234.5" → cents. Empty or unreadable → `None`.
pub fn euros_to_cents(text: &str) -> Option<i64> {
    let cleaned: String = text.chars().filter(|c| !c.is_whitespace()).map(|c| if c == ',' { '.' } else { c }).collect();
    if cleaned.is_empty() {
        return None;
    }
    let value: f64 = cleaned.parse().ok()?;
    value.is_finite().then(|| (value * 100.).round() as i64)
}

/// Cents → what goes back into an amount field: `1234.50`, or nothing.
pub fn cents_to_text(cents: Option<i64>) -> String {
    cents.map(|c| format!("{:.2}", c as f64 / 100.)).unwrap_or_default()
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
    fn a_moment_is_shown_on_the_clock_of_the_pc() {
        let paris_in_winter = chrono::FixedOffset::east_opt(3600).unwrap();
        assert_eq!(date_time_in("2026-03-07T09:15:00Z", &paris_in_winter), "07/03/2026 10:15");
        assert_eq!(date_time_in("2026-03-07T23:30:12.345+00:00", &paris_in_winter), "08/03/2026 00:30");
        assert_eq!(date_time_in("2026-03-07 09:15:00", &paris_in_winter), "07/03/2026 10:15");
        assert_eq!(date_time_in("2026-03-07", &paris_in_winter), "07/03/2026", "a day has no hour to shift");
        assert_eq!(date_time_in("hier", &paris_in_winter), "hier");
        assert_eq!(date_time_in("", &paris_in_winter), "");
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
        assert_eq!(euros_whole(1_284_550), "12\u{202f}846\u{a0}€");
        assert_eq!(euros_whole(149), "1\u{a0}€");
        assert_eq!(euros_whole(150), "2\u{a0}€");
        assert_eq!(euros_whole(-150), "-2\u{a0}€");
        assert_eq!(euros_whole(-49), "0\u{a0}€");
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
    fn amounts_are_read_the_way_they_are_typed_in_france() {
        assert_eq!(euros_to_cents("1 234,50"), Some(123_450));
        assert_eq!(euros_to_cents("1234.5"), Some(123_450));
        assert_eq!(euros_to_cents("0,019"), Some(2));
        assert_eq!(euros_to_cents(""), None);
        assert_eq!(euros_to_cents("  "), None);
        assert_eq!(euros_to_cents("douze"), None);
        assert_eq!(cents_to_text(Some(123_450)), "1234.50");
        assert_eq!(cents_to_text(None), "");
    }

    #[test]
    fn initials_of_a_person_or_a_company() {
        assert_eq!(initials(Some("Léa"), "Martin"), "LM");
        assert_eq!(initials(None, "Garage Dupont"), "GA");
        assert_eq!(initials(Some(" "), ""), "?");
    }
}
