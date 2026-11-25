//! A stand-in for the cloud's team service, to look at the online Employés screen with people
//! in it. The real cloud is not in this repository; this one keeps a small team in memory and
//! forgets it when it stops.
//!
//! ```text
//! cargo run -p motodesk-gpui --example team_cloud            # listens on 127.0.0.1:8791
//! MOTODESK_DEV_TEAM_CLOUD=http://127.0.0.1:8791 cargo run -p motodesk-gpui
//! ```
//!
//! `MOTODESK_DEV_TEAM_CLOUD` exists in debug builds only: the Employés screen then shows the
//! online team and asks this address for it, whatever the way the shop signs in.

use std::sync::{Arc, Mutex};

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde_json::{Value, json};

type Team = Arc<Mutex<Vec<Value>>>;

fn member(id: &str, name: &str, username: Option<&str>, role: &str, active: bool) -> Value {
    json!({
        "id": id, "company_id": "demo", "email": format!("{id}@atelier-demo.fr"), "username": username,
        "full_name": name, "role": role, "is_active": active, "created_at": "2026-01-05T08:00:00Z"
    })
}

fn refusal(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "error": { "code": "bad_request", "message": message } }))).into_response()
}

async fn list(State(team): State<Team>) -> Json<Value> {
    Json(Value::Array(team.lock().unwrap().clone()))
}

async fn create(State(team): State<Team>, Json(new): Json<Value>) -> Response {
    let mut team = team.lock().unwrap();
    if team.iter().any(|m| m["email"] == new["email"]) {
        return refusal(StatusCode::CONFLICT, "Cet email est déjà utilisé.");
    }
    let id = format!("m{}", team.len() + 1);
    let mut created = member(&id, new["full_name"].as_str().unwrap_or_default(), new["username"].as_str(), "employee", true);
    created["email"] = new["email"].clone();
    created["role"] = new["role"].clone();
    team.push(created.clone());
    (StatusCode::CREATED, Json(created)).into_response()
}

async fn update(State(team): State<Team>, Path(id): Path<String>, Json(patch): Json<Value>) -> Response {
    let mut team = team.lock().unwrap();
    let Some(found) = team.iter_mut().find(|m| m["id"] == id.as_str()) else {
        return refusal(StatusCode::NOT_FOUND, "Employé introuvable.");
    };
    for field in ["email", "full_name", "username", "role", "is_active"] {
        if let Some(value) = patch.get(field) {
            found[field] = value.clone();
        }
    }
    Json(found.clone()).into_response()
}

async fn deactivate(State(team): State<Team>, Path(id): Path<String>) -> StatusCode {
    for found in team.lock().unwrap().iter_mut().filter(|m| m["id"] == id.as_str()) {
        found["is_active"] = json!(false);
    }
    StatusCode::NO_CONTENT
}

async fn remove(State(team): State<Team>, Path(id): Path<String>) -> StatusCode {
    team.lock().unwrap().retain(|m| m["id"] != id.as_str());
    StatusCode::NO_CONTENT
}

async fn invite() -> Json<Value> {
    Json(json!({ "token": "INVITATION-DE-DEMONSTRATION", "expires_at": "2026-12-02T00:00:00Z", "role": "employee" }))
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let team: Team = Arc::new(Mutex::new(vec![
        member("camille", "Camille Le Gall", Some("camille"), "admin", true),
        member("lea", "Léa Martin", Some("lea"), "employee", true),
        member("noel", "Noël Garçon", None, "employee", true),
        member("maelle", "Maëlle Kervella", Some("maelle"), "admin", true),
        member("remi", "Rémi Dubois", Some("remi"), "employee", false),
    ]));
    let app = Router::new()
        .route("/api/employees", get(list).post(create))
        .route("/api/employees/:id", delete(deactivate).patch(update))
        .route("/api/employees/:id/permanent", delete(remove))
        .route("/api/auth/invites", post(invite))
        .with_state(team);
    let address = std::env::args().nth(1).unwrap_or_else(|| "127.0.0.1:8791".into());
    let listener = tokio::net::TcpListener::bind(&address).await.expect("the address is free");
    println!("stand-in team service on http://{address}");
    axum::serve(listener, app).await.expect("the service runs");
}
