//! Shapes shared by several areas of the API.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Who is signed in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PublicUser {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub company_id: Option<String>,
    #[serde(default)]
    pub email: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default)]
    pub full_name: String,
    #[serde(default)]
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_active: Option<bool>,
    #[serde(default)]
    pub created_at: String,
    /// `local` for an account of a standalone installation (no cloud behind it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_mode: Option<String>,
    /// Anything else the server sent: kept, so a bundle handed back to it is unchanged.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl PublicUser {
    /// A standalone installation's account: there is no cloud to ask about it.
    pub fn is_local(&self) -> bool {
        self.auth_mode.as_deref() == Some("local")
    }

    /// Managers: `admin`, and the cloud's own `staff`.
    pub fn is_admin(&self) -> bool {
        self.role == "admin" || self.role == "staff"
    }
}

/// A session: the two tokens and the user they belong to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TokenBundle {
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub access_expires_in: i64,
    #[serde(default)]
    pub refresh_token: String,
    #[serde(default)]
    pub refresh_expires_in: i64,
    pub user: PublicUser,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bundle_survives_a_round_trip_with_fields_this_build_does_not_know() {
        let raw = serde_json::json!({
            "access_token": "a", "access_expires_in": 900,
            "refresh_token": "r", "refresh_expires_in": 604800,
            "later_field": 3,
            "user": { "id": "u1", "email": "a@b.fr", "full_name": "Léa", "role": "admin",
                      "created_at": "2026-01-01T00:00:00Z", "auth_mode": "local", "new_flag": true }
        });
        let bundle: TokenBundle = serde_json::from_value(raw.clone()).unwrap();
        assert!(bundle.user.is_local() && bundle.user.is_admin());
        assert_eq!(serde_json::to_value(&bundle).unwrap(), raw);
    }
}
