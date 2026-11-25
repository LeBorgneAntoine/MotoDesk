//! OpenRouter credentials and streamed chat. The key stays in the OS
//! keychain — never in `app_settings`, which mesh-replicate.

use std::convert::Infallible;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::local_server::assistant::{
    self, complete_chat_stream, Conversation, ConversationSummary,
};
use crate::local_server::auth_guard::AdminUser;
use crate::local_server::error::{LocalError, LocalResult};
use crate::local_server::secrets;
use crate::local_server::state::SharedState;

const OPENROUTER_MODELS: &str = "https://openrouter.ai/api/v1/models";

pub fn router() -> Router<SharedState> {
    Router::new()
        .route(
            "/api/assistant/credentials",
            get(get_credentials)
                .put(put_credentials)
                .delete(delete_credentials),
        )
        .route("/api/assistant/test", post(test_connection))
        .route("/api/assistant/conversations", get(list_conversations))
        .route(
            "/api/assistant/conversations/:id",
            get(get_conversation).delete(delete_conversation),
        )
        .route("/api/assistant/chat", post(chat))
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct CredentialsStatus {
    pub configured: bool,
    pub hint: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PutBody {
    api_key: String,
}

#[derive(Debug, Serialize)]
struct TestResult {
    ok: bool,
    model_count: usize,
}

#[derive(Debug, Deserialize)]
struct ModelsResponse {
    #[serde(default)]
    data: Vec<serde_json::Value>,
}

/// Last-four hint so the UI can confirm a key is stored without echoing it.
pub fn credential_hint(key: &str) -> String {
    let trimmed = key.trim();
    let count = trimmed.chars().count();
    if count < 8 {
        return "••••".into();
    }
    let prefix: String = trimmed.chars().take(4).collect();
    let suffix: String = trimmed
        .chars()
        .rev()
        .take(4)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("{prefix}…{suffix}")
}

pub fn parse_api_key(raw: &str) -> LocalResult<String> {
    let key = raw.trim();
    if key.is_empty() {
        return Err(LocalError::BadRequest("La clé OpenRouter est vide.".into()));
    }
    if key.len() < 8 {
        return Err(LocalError::BadRequest(
            "La clé OpenRouter est trop courte.".into(),
        ));
    }
    Ok(key.to_string())
}

pub fn status_from_key(key: Option<&str>) -> CredentialsStatus {
    match key {
        Some(k) if !k.is_empty() => CredentialsStatus {
            configured: true,
            hint: Some(credential_hint(k)),
        },
        _ => CredentialsStatus {
            configured: false,
            hint: None,
        },
    }
}

pub fn require_stored_key(key: Option<String>) -> LocalResult<String> {
    key.filter(|k| !k.is_empty()).ok_or_else(|| {
        LocalError::BadRequest("Aucune clé OpenRouter n'est enregistrée.".into())
    })
}

fn status_now(data_dir: &std::path::Path) -> LocalResult<CredentialsStatus> {
    let key = secrets::load_openrouter_key(data_dir)?;
    Ok(status_from_key(key.as_deref()))
}

async fn get_credentials(
    State(state): State<SharedState>,
) -> LocalResult<Json<CredentialsStatus>> {
    Ok(Json(status_now(&state.data_dir)?))
}

async fn put_credentials(
    State(state): State<SharedState>,
    _admin: AdminUser,
    Json(body): Json<PutBody>,
) -> LocalResult<Json<CredentialsStatus>> {
    let key = parse_api_key(&body.api_key)?;
    secrets::store_openrouter_key(&state.data_dir, &key)?;
    Ok(Json(status_now(&state.data_dir)?))
}

async fn delete_credentials(
    State(state): State<SharedState>,
    _admin: AdminUser,
) -> LocalResult<StatusCode> {
    secrets::clear_openrouter_key(&state.data_dir)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn test_connection(
    State(state): State<SharedState>,
    _admin: AdminUser,
) -> LocalResult<Json<TestResult>> {
    let key = require_stored_key(secrets::load_openrouter_key(&state.data_dir)?)?;

    let response = assistant::llm_client()
        .get(OPENROUTER_MODELS)
        .header("Authorization", format!("Bearer {key}"))
        .header("HTTP-Referer", "https://motodesk.local")
        .header("X-Title", "Motodesk")
        .send()
        .await
        .map_err(|e| {
            LocalError::BadRequest(format!("OpenRouter injoignable ({e})."))
        })?;

    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(LocalError::BadRequest(
            "Clé OpenRouter refusée.".into(),
        ));
    }
    if !status.is_success() {
        return Err(LocalError::BadRequest(format!(
            "OpenRouter a répondu HTTP {status}."
        )));
    }

    let body = response
        .json::<ModelsResponse>()
        .await
        .unwrap_or(ModelsResponse { data: Vec::new() });

    Ok(Json(TestResult {
        ok: true,
        model_count: body.data.len(),
    }))
}

#[derive(Debug, Deserialize)]
struct ChatBody {
    conversation_id: Option<String>,
    message: String,
    model: String,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum ClientEvent {
    #[serde(rename = "start")]
    Start {
        conversation_id: String,
        user_message_id: String,
        assistant_message_id: String,
    },
    #[serde(rename = "delta")]
    Delta { text: String },
    #[serde(rename = "done")]
    Done { title: String },
    #[serde(rename = "error")]
    Error { message: String },
}

fn sse_event(ev: &ClientEvent) -> Event {
    Event::default()
        .json_data(ev)
        .expect("assistant sse json")
}

async fn list_conversations(
    State(state): State<SharedState>,
) -> LocalResult<Json<Vec<ConversationSummary>>> {
    Ok(Json(assistant::list_conversations(&state.db).await?))
}

async fn get_conversation(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> LocalResult<Json<Conversation>> {
    Ok(Json(assistant::get_conversation(&state.db, &id).await?))
}

async fn delete_conversation(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> LocalResult<StatusCode> {
    assistant::delete_conversation(&state.db, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn chat(
    State(state): State<SharedState>,
    Json(body): Json<ChatBody>,
) -> LocalResult<Sse<ReceiverStream<Result<Event, Infallible>>>> {
    let message = body.message.trim().to_string();
    if message.is_empty() {
        return Err(LocalError::BadRequest("Le message est vide.".into()));
    }
    let model = assistant::parse_model_slug(&body.model)?;
    let key = require_stored_key(secrets::load_openrouter_key(&state.data_dir)?)?;

    let title = assistant::title_from_prompt(&message);
    let mut conv_title = title.clone();
    let conversation_id = match body
        .conversation_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(id) => {
            let existing = assistant::get_conversation(&state.db, id).await?;
            if existing.messages.is_empty() {
                assistant::set_title(&state.db, id, &title).await?;
            } else {
                conv_title = existing.title;
            }
            id.to_string()
        }
        None => assistant::create_conversation(&state.db, &title).await?,
    };

    let user = assistant::append_message(&state.db, &conversation_id, "user", &message).await?;
    let history = assistant::history_for_model(&state.db, &conversation_id).await?;
    let assistant_msg =
        assistant::append_message(&state.db, &conversation_id, "assistant", "").await?;

    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(64);
    let start = ClientEvent::Start {
        conversation_id: conversation_id.clone(),
        user_message_id: user.id.clone(),
        assistant_message_id: assistant_msg.id.clone(),
    };
    let _ = tx.send(Ok(sse_event(&start))).await;

    let assistant_id = assistant_msg.id;
    let db = state.db.clone();
    tokio::spawn(async move {
        let (delta_tx, mut delta_rx) = mpsc::channel::<String>(64);
        let complete = tokio::spawn({
            let key = key.clone();
            let model = model.clone();
            async move { complete_chat_stream(&key, &model, &history, &delta_tx).await }
        });

        while let Some(text) = delta_rx.recv().await {
            if tx
                .send(Ok(sse_event(&ClientEvent::Delta { text })))
                .await
                .is_err()
            {
                while delta_rx.recv().await.is_some() {}
                break;
            }
        }

        match complete.await {
            Ok(Ok(full)) => {
                let body = if full.trim().is_empty() {
                    "Le modèle n'a rien renvoyé.".to_string()
                } else {
                    full
                };
                let _ = assistant::update_message_content(&db, &assistant_id, &body).await;
                let _ = assistant::upsert_markdown_artifact(&db, &conversation_id, "Réponse", &body)
                    .await;
                let _ = tx
                    .send(Ok(sse_event(&ClientEvent::Done {
                        title: conv_title,
                    })))
                    .await;
            }
            Ok(Err(err)) => {
                let message = match err {
                    LocalError::BadRequest(m) => m,
                    other => other.to_string(),
                };
                let _ = assistant::update_message_content(&db, &assistant_id, &message).await;
                let _ = tx
                    .send(Ok(sse_event(&ClientEvent::Error { message })))
                    .await;
            }
            Err(join) => {
                let message = format!("Le flux s'est interrompu ({join}).");
                let _ = assistant::update_message_content(&db, &assistant_id, &message).await;
                let _ = tx
                    .send(Ok(sse_event(&ClientEvent::Error { message })))
                    .await;
            }
        }
    });

    Ok(Sse::new(ReceiverStream::new(rx)).keep_alive(KeepAlive::default()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hint_masks_the_middle() {
        assert_eq!(credential_hint("sk-or-v1-abcdefghijkl"), "sk-o…ijkl");
        assert_eq!(credential_hint("short"), "••••");
    }

    #[test]
    fn parse_rejects_blank_and_tiny_keys() {
        assert!(parse_api_key("  ").is_err());
        assert!(parse_api_key("abc").is_err());
        assert_eq!(parse_api_key("  sk-or-v1-secret  ").unwrap(), "sk-or-v1-secret");
    }

    #[test]
    fn status_reports_configured_without_the_secret() {
        let s = status_from_key(Some("sk-or-v1-abcdefghijkl"));
        assert!(s.configured);
        assert_eq!(s.hint.as_deref(), Some("sk-o…ijkl"));
        assert!(!status_from_key(None).configured);
        assert!(!status_from_key(Some("")).configured);
    }

    #[test]
    fn test_refuses_a_missing_key() {
        let err = require_stored_key(None).unwrap_err();
        assert!(matches!(err, LocalError::BadRequest(_)));
    }
}
