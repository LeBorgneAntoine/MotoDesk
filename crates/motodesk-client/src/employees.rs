//! The team of an online company.
//!
//! Employee management is admin-only and authoritative on the cloud, so these calls go straight
//! to the cloud API rather than to the local mirror. A local network manages its accounts with
//! [`crate::network_accounts`] instead.

use serde::{Deserialize, Serialize};

use crate::error::{ApiError, ApiResult};
use crate::session::Client;
use crate::types::PublicUser;

/// A member of the company: a [`PublicUser`] whose `is_active` is always set.
pub type Employee = PublicUser;

/// `admin`, or anything else for an employee.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    #[default]
    Employee,
}

impl Role {
    pub fn of(user: &PublicUser) -> Self {
        if user.role == "admin" { Role::Admin } else { Role::Employee }
    }

    pub fn value(self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::Employee => "employee",
        }
    }

    pub fn from_value(value: &str) -> Self {
        if value == "admin" { Role::Admin } else { Role::Employee }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CreateEmployee {
    pub email: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    pub password: String,
    pub full_name: String,
    pub role: Role,
}

/// Only the fields that are set are changed.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct UpdateEmployee {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_active: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}

/// A code a new member pastes when signing up, to join this company.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Invite {
    pub token: String,
    #[serde(default)]
    pub expires_at: String,
    #[serde(default)]
    pub role: String,
}

/// The cloud session's access token; without one there is nobody to ask as.
fn token(client: &Client) -> ApiResult<String> {
    client
        .tokens()
        .map(|tokens| tokens.access_token)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| ApiError::new(401, "unauthorized", "Session expirée : reconnectez-vous."))
}

pub async fn list(client: &Client) -> ApiResult<Vec<Employee>> {
    client.cloud().get("/api/employees", Some(&token(client)?)).await
}

pub async fn create(client: &Client, payload: &CreateEmployee) -> ApiResult<Employee> {
    client.cloud().post("/api/employees", payload, Some(&token(client)?)).await
}

pub async fn update(client: &Client, id: &str, payload: &UpdateEmployee) -> ApiResult<Employee> {
    client.cloud().patch(&format!("/api/employees/{id}"), payload, Some(&token(client)?)).await
}

/// Block the account's sign-in without deleting it.
pub async fn deactivate(client: &Client, id: &str) -> ApiResult<()> {
    client.cloud().delete(&format!("/api/employees/{id}"), Some(&token(client)?)).await
}

/// Delete the account for good. Cannot be undone.
pub async fn delete(client: &Client, id: &str) -> ApiResult<()> {
    client.cloud().delete(&format!("/api/employees/{id}/permanent"), Some(&token(client)?)).await
}

/// An invitation for a new employee, valid seven days.
pub async fn create_invite(client: &Client) -> ApiResult<Invite> {
    client.cloud().post("/api/auth/invites", &serde_json::json!({ "role": "employee" }), Some(&token(client)?)).await
}

/// "Jean Dupont" → "JD": first letters of the first and last words.
pub fn initials(full_name: &str) -> String {
    let mut words = full_name.split_whitespace();
    let first = words.next().and_then(|w| w.chars().next());
    let last = words.next_back().and_then(|w| w.chars().next());
    let letters: String = first.into_iter().chain(last).collect();
    if letters.is_empty() { "?".into() } else { letters.to_uppercase() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials_take_the_first_and_last_words() {
        assert_eq!(initials("Jean Dupont"), "JD");
        assert_eq!(initials("  Léa  de la Tour "), "LT");
        assert_eq!(initials("Camille"), "C");
        assert_eq!(initials("   "), "?");
    }

    #[test]
    fn an_update_sends_only_what_changes() {
        let patch = UpdateEmployee { is_active: Some(false), ..Default::default() };
        assert_eq!(serde_json::to_value(&patch).unwrap(), serde_json::json!({ "is_active": false }));
        let role = UpdateEmployee { role: Some(Role::Admin), ..Default::default() };
        assert_eq!(serde_json::to_value(&role).unwrap(), serde_json::json!({ "role": "admin" }));
    }
}
