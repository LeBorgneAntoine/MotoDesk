//! Application settings, a key → JSON value store.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

use crate::error::ApiResult;
use crate::products::VAT_PRESETS;
use crate::session::Client;

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct AppSetting {
    pub key: String,
    pub value: Value,
    #[serde(default)]
    pub updated_at: String,
}

/// A flat key → value map, the shape the bulk write expects.
pub type SettingsBulk = BTreeMap<String, Value>;

/// Whether the shop charges VAT: it does until it says it is not subject to it (franchise en
/// base).
pub fn charges_vat(stored: &[AppSetting]) -> bool {
    !stored.iter().any(|setting| setting.key == "vat.is_subject" && setting.value.as_bool() == Some(false))
}

/// The VAT rate a new line or product starts with: none for a shop that charges none, else the
/// shop's usual rate.
pub fn default_vat_bp(stored: &[AppSetting]) -> i64 {
    if !charges_vat(stored) {
        return 0;
    }
    let usual = stored.iter().find(|setting| setting.key == "vat.default_rate_bp").and_then(|setting| setting.value.as_i64());
    // A rate the lists cannot show would be saved without being seen.
    usual.filter(|rate| VAT_PRESETS.iter().any(|(_, bp)| bp == rate)).unwrap_or(2000)
}

pub async fn list(client: &Client) -> ApiResult<Vec<AppSetting>> {
    client.get("/api/settings").await
}

pub async fn set(client: &Client, key: &str, value: &Value) -> ApiResult<AppSetting> {
    client.put(format!("/api/settings/{}", encode(key)), &serde_json::json!({ "value": value })).await
}

pub async fn set_bulk(client: &Client, entries: &SettingsBulk) -> ApiResult<Vec<AppSetting>> {
    client.put("/api/settings", entries).await
}

pub async fn reset(client: &Client, key: &str) -> ApiResult<()> {
    client.delete(format!("/api/settings/{}", encode(key))).await
}

/// Percent-encode a value placed in a path or a query (what `encodeURIComponent` leaves alone
/// is left alone here).
pub(crate) fn encode(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')' => {
                out.push(byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::encode;

    #[test]
    fn a_search_with_spaces_and_accents_is_safe_in_a_query() {
        assert_eq!(encode("system.theme"), "system.theme");
        assert_eq!(encode("Léa & co"), "L%C3%A9a%20%26%20co");
    }
}
