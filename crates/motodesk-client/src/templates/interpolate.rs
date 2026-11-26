//! Variables in a template's text (port of `desktop/src/lib/templates/interpolate.ts`).
//!
//! `{{client.full_name}}` is replaced by the value found at that path in the document's data;
//! `{{doc.total_ttc | price}}` also says how to write it. A path that leads nowhere is replaced
//! by nothing, so a template half filled in still prints cleanly.

use chrono::{DateTime, Local, NaiveDate, NaiveDateTime};
use serde_json::Value;

/// A space that does not break, as French typography puts before « € » and « % ».
const NBSP: char = '\u{a0}';

/// How a value is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Hint {
    #[default]
    Text,
    /// Cents, written as euros: `12345` reads « 123,45 € ».
    Price,
    Qty,
    /// Basis points: `2000` reads « 20 % ».
    Percent,
    Date,
}

impl Hint {
    fn from_word(word: &str) -> Self {
        match word {
            "price" => Hint::Price,
            "qty" => Hint::Qty,
            "percent" => Hint::Percent,
            "date" => Hint::Date,
            _ => Hint::Text,
        }
    }
}

/// The value at a dotted path (`client.first_name`), or `None` when the path leads nowhere.
pub fn resolve<'a>(path: &str, data: &'a Value) -> Option<&'a Value> {
    if path.is_empty() {
        return None;
    }
    path.split('.').try_fold(data, |at, part| at.as_object()?.get(part)).filter(|value| !value.is_null())
}

/// A whole number with a space between thousands: `24680` reads « 24 680 ».
pub(super) fn grouped(digits: &str) -> String {
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(NBSP);
        }
        out.push(digit);
    }
    out
}

/// A number as French writes it, with at most `max_decimals` decimals and no trailing zeros
/// unless `fixed`.
fn french_number(value: f64, max_decimals: usize, fixed: bool) -> String {
    let written = format!("{:.*}", max_decimals, value.abs());
    let (whole, decimals) = written.split_once('.').unwrap_or((&written, ""));
    let decimals = if fixed { decimals } else { decimals.trim_end_matches('0') };
    let sign = if value < 0.0 && (whole.chars().any(|c| c != '0') || decimals.chars().any(|c| c != '0')) { "-" } else { "" };
    if decimals.is_empty() { format!("{sign}{}", grouped(whole)) } else { format!("{sign}{},{decimals}", grouped(whole)) }
}

fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
    .filter(|n| n.is_finite())
}

fn plain(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

/// The day of an instant, as this PC's clock reads it: `2026-05-29T09:30:00Z` reads « 29/05/2026 ».
fn day(text: &str) -> Option<String> {
    let text = text.trim();
    let date = if let Ok(instant) = DateTime::parse_from_rfc3339(text) {
        instant.with_timezone(&Local).date_naive()
    } else if let Ok(local) = NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f") {
        local.date()
    } else {
        NaiveDate::parse_from_str(text, "%Y-%m-%d").ok()?
    };
    Some(date.format("%d/%m/%Y").to_string())
}

/// Write a value the way `hint` asks. Nothing is written for a missing or empty value.
pub fn format_value(value: Option<&Value>, hint: Hint) -> String {
    let Some(value) = value.filter(|value| !matches!(value, Value::Null) && value.as_str() != Some("")) else { return String::new() };
    match hint {
        // Every amount handed to a template is whole cents.
        Hint::Price => number(value).map_or_else(|| plain(value), |cents| format!("{}{NBSP}€", french_number(cents / 100.0, 2, true))),
        Hint::Qty => number(value).map_or_else(|| plain(value), |n| french_number(n, 2, false)),
        // Rates and discounts are basis points everywhere: 2000 is 20 %, 50 is 0,5 %.
        Hint::Percent => number(value).map_or_else(|| plain(value), |bp| format!("{}{NBSP}%", french_number(bp / 100.0, 2, false))),
        Hint::Date => value.as_str().and_then(day).unwrap_or_else(|| plain(value)),
        Hint::Text => plain(value),
    }
}

/// A piece of a template's text: plain text, or a variable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    Text(String),
    Var { path: String, hint: Hint },
}

/// A variable starting at the beginning of `text` (`{{ path | hint }}`): the variable, and how
/// many bytes it takes.
fn variable_at(text: &str) -> Option<(Segment, usize)> {
    let inner_end = text.strip_prefix("{{")?.find("}}")?;
    let inner = &text[2..2 + inner_end];
    if inner.contains('}') {
        return None;
    }
    let (path, hint) = match inner.split_once('|') {
        None => (inner.trim(), Hint::Text),
        Some((path, word)) => {
            let word = word.trim();
            // What follows the bar is one lowercase word; anything else is not a variable.
            if word.is_empty() || !word.chars().all(|c| c.is_ascii_lowercase()) {
                return None;
            }
            (path.trim(), Hint::from_word(word))
        }
    };
    if path.is_empty() || path.contains('|') {
        return None;
    }
    Some((Segment::Var { path: path.to_string(), hint }, inner_end + 4))
}

/// Split a text into its plain parts and its variables, in order.
pub fn tokenize(text: &str) -> Vec<Segment> {
    let mut out = Vec::new();
    let mut plain_from = 0;
    let mut at = 0;
    while let Some(found) = text[at..].find("{{") {
        let start = at + found;
        match variable_at(&text[start..]) {
            Some((variable, length)) => {
                if start > plain_from {
                    out.push(Segment::Text(text[plain_from..start].to_string()));
                }
                out.push(variable);
                at = start + length;
                plain_from = at;
            }
            None => at = start + 2,
        }
    }
    if plain_from < text.len() {
        out.push(Segment::Text(text[plain_from..].to_string()));
    }
    out
}

/// Replace every variable of `text` by its value in `data`.
pub fn interpolate(text: &str, data: &Value) -> String {
    tokenize(text)
        .into_iter()
        .map(|segment| match segment {
            Segment::Text(text) => text,
            Segment::Var { path, hint } => format_value(resolve(&path, data), hint),
        })
        .collect()
}

/// The path of a text that is one variable and nothing else (the source of a picture).
pub fn bare_variable(text: &str) -> Option<String> {
    match tokenize(text.trim()).as_slice() {
        [Segment::Var { path, .. }] => Some(path.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_variable_is_replaced_by_its_value() {
        let data = json!({ "client": { "first_name": "Léa" }, "doc": { "total_ttc": 12345, "empty": "" } });
        assert_eq!(interpolate("Bonjour {{client.first_name}} !", &data), "Bonjour Léa !");
        assert_eq!(interpolate("{{ client.first_name }}{{client.unknown}}{{doc.empty}}", &data), "Léa");
        assert_eq!(interpolate("Total {{doc.total_ttc | price}}", &data), "Total 123,45\u{a0}€");
        assert_eq!(interpolate("{{doc.total_ttc|price}}", &data), "123,45\u{a0}€");
        // A path through something that is not an object leads nowhere.
        assert_eq!(interpolate("[{{client.first_name.x}}]", &data), "[]");
        assert_eq!(interpolate("", &data), "");
    }

    #[test]
    fn what_is_not_a_variable_is_left_as_written() {
        let data = json!({ "a": "x" });
        assert_eq!(interpolate("{{ }} {{a|Price}} {{a | }} {a} {{a", &data), "{{ }} {{a|Price}} {{a | }} {a} {{a");
        // An unknown way of writing a value writes it plainly.
        assert_eq!(interpolate("{{a | upper}}", &data), "x");
    }

    #[test]
    fn amounts_are_cents_and_rates_basis_points() {
        let price = |cents: i64| format_value(Some(&json!(cents)), Hint::Price);
        assert_eq!(price(45), "0,45\u{a0}€");
        assert_eq!(price(8900), "89,00\u{a0}€");
        assert_eq!(price(1_234_567), "12\u{a0}345,67\u{a0}€");
        assert_eq!(price(-8339), "-83,39\u{a0}€");
        assert_eq!(price(0), "0,00\u{a0}€");
        assert_eq!(format_value(Some(&json!("abc")), Hint::Price), "abc");

        let percent = |bp: i64| format_value(Some(&json!(bp)), Hint::Percent);
        assert_eq!(percent(2000), "20\u{a0}%");
        assert_eq!(percent(550), "5,5\u{a0}%");
        assert_eq!(percent(50), "0,5\u{a0}%");

        assert_eq!(format_value(Some(&json!(3.2)), Hint::Qty), "3,2");
        assert_eq!(format_value(Some(&json!(1)), Hint::Qty), "1");
        assert_eq!(format_value(Some(&json!(1500.456)), Hint::Qty), "1\u{a0}500,46");
        assert_eq!(format_value(None, Hint::Price), "");
        assert_eq!(format_value(Some(&json!(null)), Hint::Qty), "");
    }

    #[test]
    fn a_date_is_written_day_first() {
        assert_eq!(format_value(Some(&json!("2026-05-29")), Hint::Date), "29/05/2026");
        assert_eq!(format_value(Some(&json!("2026-05-29T09:30:00")), Hint::Date), "29/05/2026");
        // Noon keeps the day in every usual time zone.
        assert_eq!(format_value(Some(&json!("2026-05-29T12:00:00Z")), Hint::Date), "29/05/2026");
        assert_eq!(format_value(Some(&json!("bientôt")), Hint::Date), "bientôt");
    }

    #[test]
    fn a_text_splits_into_its_plain_parts_and_its_variables() {
        assert_eq!(
            tokenize("Bonjour {{client.first_name}} — {{doc.date | date}}."),
            [
                Segment::Text("Bonjour ".into()),
                Segment::Var { path: "client.first_name".into(), hint: Hint::Text },
                Segment::Text(" — ".into()),
                Segment::Var { path: "doc.date".into(), hint: Hint::Date },
                Segment::Text(".".into()),
            ]
        );
        assert!(tokenize("").is_empty());
        // A third brace belongs to the path, as the Svelte editor read it.
        assert_eq!(tokenize("{{{a}}"), [Segment::Var { path: "{a".into(), hint: Hint::Text }]);
    }

    #[test]
    fn a_picture_source_is_one_variable_alone() {
        assert_eq!(bare_variable(" {{shop.logo_url}} ").as_deref(), Some("shop.logo_url"));
        assert_eq!(bare_variable("{{shop.logo_url | text}}").as_deref(), Some("shop.logo_url"));
        assert_eq!(bare_variable("https://exemple.fr/{{a}}.png"), None);
        assert_eq!(bare_variable("logo.png"), None);
    }
}
