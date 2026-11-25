//! The team calls go to the cloud, which is not in this repository: a stand-in records what
//! it is sent, so the paths, the verbs, the session token and the bodies are pinned here.

use std::sync::{Arc, Mutex};

use axum::body::to_bytes;
use axum::extract::{Request, State};
use axum::http::{header, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use motodesk_client::employees::{self, CreateEmployee, Role, UpdateEmployee};
use motodesk_client::{Client, Cloud, Local, TokenBundle, TokenStore};
use serde_json::{json, Value};

/// (verb, path, bearer token, body) of every request the stand-in received.
type Seen = Arc<Mutex<Vec<(String, String, String, Value)>>>;

fn employee(id: &str, name: &str, active: bool) -> Value {
    json!({
        "id": id, "company_id": "c1", "email": format!("{id}@atelier.fr"), "username": id,
        "full_name": name, "role": "employee", "is_active": active, "created_at": "2026-01-05T08:00:00Z"
    })
}

async fn stand_in(State(seen): State<Seen>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let token = parts
        .headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or_default()
        .to_string();
    let bytes = to_bytes(body, 1 << 20).await.unwrap_or_default();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    let path = parts.uri.path().to_string();
    seen.lock().unwrap().push((parts.method.to_string(), path.clone(), token.clone(), body));

    if token != "access-1" {
        return (StatusCode::UNAUTHORIZED, Json(json!({ "error": { "code": "unauthorized", "message": "Session expirée." } })))
            .into_response();
    }
    match (parts.method, path.as_str()) {
        (Method::GET, "/api/employees") => Json(json!([employee("lea", "Léa Martin", true)])).into_response(),
        (Method::POST, "/api/employees") => (StatusCode::CREATED, Json(employee("noel", "Noël Garçon", true))).into_response(),
        (Method::PATCH, "/api/employees/lea") => Json(employee("lea", "Léa Martin", false)).into_response(),
        (Method::DELETE, "/api/employees/lea") | (Method::DELETE, "/api/employees/lea/permanent") => {
            StatusCode::NO_CONTENT.into_response()
        }
        (Method::POST, "/api/auth/invites") => {
            Json(json!({ "token": "INV-123", "expires_at": "2026-12-02T00:00:00Z", "role": "employee" })).into_response()
        }
        _ => (StatusCode::NOT_FOUND, Json(json!({ "error": { "code": "not_found", "message": "Introuvable." } }))).into_response(),
    }
}

async fn client(access_token: &str) -> (Client, Seen) {
    let seen: Seen = Arc::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = Router::new().fallback(stand_in).with_state(seen.clone());
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let dir = std::env::temp_dir().join(format!("motodesk_cloud_{}", uuid::Uuid::new_v4()));
    let client = Client::new(Local::new(Router::new(), "unused"), Cloud::new(format!("http://{address}")), TokenStore::in_dir(&dir));
    let session: TokenBundle = serde_json::from_value(json!({
        "access_token": access_token, "access_expires_in": 900, "refresh_token": "", "refresh_expires_in": 0,
        "user": { "id": "admin", "email": "admin@atelier.fr", "full_name": "Camille", "role": "admin", "created_at": "" }
    }))
    .unwrap();
    client.set_tokens(Some(session));
    (client, seen)
}

#[tokio::test]
async fn every_team_call_reaches_the_cloud_with_the_session_and_the_right_shape() {
    let (api, seen) = client("access-1").await;

    let team = employees::list(&api).await.unwrap();
    assert_eq!(team.len(), 1);
    assert_eq!((team[0].full_name.as_str(), team[0].is_active, Role::of(&team[0])), ("Léa Martin", Some(true), Role::Employee));

    let created = employees::create(
        &api,
        &CreateEmployee {
            email: "noel@atelier.fr".into(),
            username: None,
            password: "mot de passe".into(),
            full_name: "Noël Garçon".into(),
            role: Role::Employee,
        },
    )
    .await
    .unwrap();
    assert_eq!(created.id, "noel");

    let off = employees::update(&api, "lea", &UpdateEmployee { is_active: Some(false), ..Default::default() }).await.unwrap();
    assert_eq!(off.is_active, Some(false));

    employees::deactivate(&api, "lea").await.unwrap();
    employees::delete(&api, "lea").await.unwrap();
    assert_eq!(employees::create_invite(&api).await.unwrap().token, "INV-123");

    let seen = seen.lock().unwrap();
    let calls: Vec<(&str, &str)> = seen.iter().map(|(verb, path, ..)| (verb.as_str(), path.as_str())).collect();
    assert_eq!(
        calls,
        vec![
            ("GET", "/api/employees"),
            ("POST", "/api/employees"),
            ("PATCH", "/api/employees/lea"),
            ("DELETE", "/api/employees/lea"),
            ("DELETE", "/api/employees/lea/permanent"),
            ("POST", "/api/auth/invites"),
        ]
    );
    assert!(seen.iter().all(|(_, _, token, _)| token == "access-1"), "every call carries the session");
    // A new employee without a username sends none; an update sends only what changes.
    assert_eq!(
        seen[1].3,
        json!({ "email": "noel@atelier.fr", "password": "mot de passe", "full_name": "Noël Garçon", "role": "employee" })
    );
    assert_eq!(seen[2].3, json!({ "is_active": false }));
    assert_eq!(seen[5].3, json!({ "role": "employee" }));
}

#[tokio::test]
async fn a_refusal_from_the_cloud_comes_back_with_its_message() {
    let (api, _) = client("stale").await;
    let refused = employees::list(&api).await.unwrap_err();
    assert_eq!((refused.status, refused.code.as_str(), refused.message.as_str()), (401, "unauthorized", "Session expirée."));
}
