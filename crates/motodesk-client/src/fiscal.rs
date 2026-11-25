//! The fiscal side of the shop (port of the parts of `desktop/src/lib/api/fiscal.ts` the
//! migrated screens use; the journal, the closures and the archives come with Comptabilité).

use serde::Deserialize;

use crate::error::ApiResult;
use crate::session::Client;

/// A mention an invoice must carry and the company profile does not supply.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct MissingMention {
    pub key: String,
    pub label: String,
    #[serde(default)]
    pub reason: String,
}

/// What an invoice issued right now would be missing.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct ProfileCheck {
    #[serde(default)]
    pub complete: bool,
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
        assert_eq!(ProfileCheck { complete: true, ..Default::default() }.blocker(), None);
    }
}
