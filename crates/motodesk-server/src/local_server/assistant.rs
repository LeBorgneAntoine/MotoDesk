//! Device-local assistant: conversations in SQLite, OpenRouter SSE parsing.
//! The API key never lives here — callers load it from the OS keychain.

use std::sync::OnceLock;
use std::time::Duration;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::local_server::error::{LocalError, LocalResult};

/// OpenRouter's API. The service state carries it (`LocalState::openrouter_url`) so that tests
/// can stand a local server in its place; nothing else ever replaces it.
pub const OPENROUTER_API: &str = "https://openrouter.ai/api/v1";

pub fn chat_completions_url(base: &str) -> String {
    format!("{base}/chat/completions")
}

pub fn models_url(base: &str) -> String {
    format!("{base}/models")
}

pub const SYSTEM_PROMPT: &str = "\
Tu es l'assistant de Motodesk, un logiciel de gestion pour un atelier moto en France.
Tu réponds en français, de façon concise et professionnelle.
Tu n'as pas encore accès aux données de l'atelier (clients, stock, caisse, factures).
Si on te les demande, explique-le clairement et propose un plan ou un brouillon.
Tu n'inventes pas de numéros de facture ou de tickets scellés.
Tu peux structurer ta réponse en markdown (titres, listes, tableaux).";

const TITLE_MAX: usize = 48;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SsePayload {
    Text(String),
    Done,
    ApiError(String),
}

pub fn parse_sse_data(data: &str) -> Option<SsePayload> {
    let trimmed = data.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed == "[DONE]" {
        return Some(SsePayload::Done);
    }
    let v: Value = serde_json::from_str(trimmed).ok()?;
    if let Some(msg) = v
        .get("error")
        .and_then(|e| e.get("message"))
        .and_then(|m| m.as_str())
        .filter(|s| !s.is_empty())
    {
        return Some(SsePayload::ApiError(msg.to_string()));
    }
    if v.get("error").is_some() {
        return Some(SsePayload::ApiError("Erreur OpenRouter.".into()));
    }
    let content = v
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|c| c.first())
        .and_then(|c| c.get("delta"))
        .and_then(|d| d.get("content"))
        .and_then(|c| c.as_str())
        .filter(|s| !s.is_empty())?;
    Some(SsePayload::Text(content.to_string()))
}

/// Pull complete `data:` payloads out of a chunked SSE buffer.
///
/// The buffer holds bytes, not text: a chunk from the network can end between the bytes of one
/// character (the « é » of « réglée »), and only a whole line is sure to be whole characters.
pub fn drain_sse_lines(buffer: &mut Vec<u8>) -> Vec<String> {
    let mut out = Vec::new();
    while let Some(idx) = buffer.iter().position(|b| *b == b'\n') {
        let raw: Vec<u8> = buffer.drain(..=idx).collect();
        let mut line = String::from_utf8_lossy(&raw).into_owned();
        if line.ends_with('\n') {
            line.pop();
        }
        if line.ends_with('\r') {
            line.pop();
        }
        if let Some(rest) = line.strip_prefix("data:") {
            let data = rest.trim();
            if !data.is_empty() {
                out.push(data.to_string());
            }
        }
    }
    out
}

pub fn extract_error_message(body: &str) -> Option<String> {
    let v: Value = serde_json::from_str(body).ok()?;
    v.get("error")
        .and_then(|e| e.get("message"))
        .and_then(|m| m.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

pub fn title_from_prompt(prompt: &str) -> String {
    let compact: String = prompt.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.is_empty() {
        return "Nouvelle conversation".into();
    }
    if compact.chars().count() <= TITLE_MAX {
        return compact;
    }
    let mut out: String = compact.chars().take(TITLE_MAX - 1).collect();
    while out.ends_with(char::is_whitespace) {
        out.pop();
    }
    out.push('…');
    out
}

pub fn parse_model_slug(raw: &str) -> LocalResult<String> {
    let slug = raw.trim();
    if slug.is_empty() {
        return Err(LocalError::BadRequest("Le modèle est vide.".into()));
    }
    let Some((provider, model)) = slug.split_once('/') else {
        return Err(LocalError::BadRequest(
            "Le slug du modèle doit ressembler à fournisseur/modèle.".into(),
        ));
    };
    let provider_ok = !provider.is_empty()
        && provider
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    let model_ok = !model.is_empty()
        && model
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ':' | '+'));
    if !provider_ok || !model_ok {
        return Err(LocalError::BadRequest(
            "Le slug du modèle doit ressembler à fournisseur/modèle.".into(),
        ));
    }
    Ok(slug.to_string())
}

pub(crate) fn llm_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .connect_timeout(Duration::from_secs(15))
            .build()
            .expect("openrouter client")
    })
}

/// Stream a completion from the OpenRouter API at `base`; each content delta is forwarded on
/// `deltas`.
pub async fn complete_chat_stream(
    base: &str,
    key: &str,
    model: &str,
    history: &[(String, String)],
    deltas: &mpsc::Sender<String>,
) -> LocalResult<String> {
    let mut messages = vec![json!({ "role": "system", "content": SYSTEM_PROMPT })];
    for (role, content) in history {
        if content.is_empty() {
            continue;
        }
        messages.push(json!({ "role": role, "content": content }));
    }

    let response = llm_client()
        .post(chat_completions_url(base))
        .header("Authorization", format!("Bearer {key}"))
        .header("HTTP-Referer", "https://motodesk.local")
        .header("X-Title", "Motodesk")
        .json(&json!({
            "model": model,
            "stream": true,
            "messages": messages,
        }))
        .send()
        .await
        .map_err(|e| LocalError::BadRequest(format!("OpenRouter injoignable ({e}).")))?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        let msg = extract_error_message(&body)
            .unwrap_or_else(|| format!("OpenRouter a répondu HTTP {status}."));
        return Err(LocalError::BadRequest(msg));
    }

    let mut full = String::new();
    let mut buf: Vec<u8> = Vec::new();
    let mut response = response;
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                buf.extend_from_slice(&chunk);
                for data in drain_sse_lines(&mut buf) {
                    match parse_sse_data(&data) {
                        Some(SsePayload::Text(t)) => {
                            full.push_str(&t);
                            if deltas.send(t).await.is_err() {
                                return Ok(full);
                            }
                        }
                        Some(SsePayload::Done) => return Ok(full),
                        Some(SsePayload::ApiError(m)) => {
                            return Err(LocalError::BadRequest(m));
                        }
                        None => {}
                    }
                }
            }
            Ok(None) => break,
            Err(e) => {
                return Err(LocalError::BadRequest(format!(
                    "Flux OpenRouter interrompu ({e})."
                )));
            }
        }
    }
    Ok(full)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationSummary {
    pub id: String,
    pub title: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactRow {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub title: String,
    pub updated_at: String,
    pub messages: Vec<ChatMessage>,
    pub artifacts: Vec<ArtifactRow>,
    pub active_artifact_id: Option<String>,
}

pub async fn list_conversations(db: &SqlitePool) -> LocalResult<Vec<ConversationSummary>> {
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT id, title, updated_at FROM assistant_conversations ORDER BY updated_at DESC",
    )
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, title, updated_at)| ConversationSummary {
            id,
            title,
            updated_at,
        })
        .collect())
}

pub async fn get_conversation(db: &SqlitePool, id: &str) -> LocalResult<Conversation> {
    let head: Option<(String, String, String, Option<String>)> = sqlx::query_as(
        "SELECT id, title, updated_at, active_artifact_id FROM assistant_conversations WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(db)
    .await?;
    let Some((cid, title, updated_at, active_artifact_id)) = head else {
        return Err(LocalError::NotFound);
    };

    let msg_rows: Vec<(String, String, String, String)> = sqlx::query_as(
        "SELECT id, role, content, created_at FROM assistant_messages
         WHERE conversation_id = $1 ORDER BY seq ASC",
    )
    .bind(id)
    .fetch_all(db)
    .await?;

    let art_rows: Vec<(String, String, String, String)> = sqlx::query_as(
        "SELECT id, title, kind, payload_json FROM assistant_artifacts
         WHERE conversation_id = $1 ORDER BY seq ASC",
    )
    .bind(id)
    .fetch_all(db)
    .await?;

    Ok(Conversation {
        id: cid,
        title,
        updated_at,
        active_artifact_id,
        messages: msg_rows
            .into_iter()
            .map(|(id, role, content, created_at)| ChatMessage {
                id,
                role,
                content,
                created_at,
            })
            .collect(),
        artifacts: art_rows
            .into_iter()
            .map(|(id, title, kind, payload_json)| ArtifactRow {
                id,
                title,
                kind,
                payload: serde_json::from_str(&payload_json).unwrap_or(Value::Null),
            })
            .collect(),
    })
}

pub async fn delete_conversation(db: &SqlitePool, id: &str) -> LocalResult<()> {
    let res = sqlx::query("DELETE FROM assistant_conversations WHERE id = $1")
        .bind(id)
        .execute(db)
        .await?;
    if res.rows_affected() == 0 {
        return Err(LocalError::NotFound);
    }
    Ok(())
}

pub async fn create_conversation(db: &SqlitePool, title: &str) -> LocalResult<String> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO assistant_conversations (id, title, created_at, updated_at)
         VALUES ($1, $2, $3, $3)",
    )
    .bind(&id)
    .bind(title)
    .bind(&now)
    .execute(db)
    .await?;
    Ok(id)
}

pub async fn append_message(
    db: &SqlitePool,
    conversation_id: &str,
    role: &str,
    content: &str,
) -> LocalResult<ChatMessage> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let (seq,): (i64,) = sqlx::query_as(
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM assistant_messages WHERE conversation_id = $1",
    )
    .bind(conversation_id)
    .fetch_one(db)
    .await?;
    sqlx::query(
        "INSERT INTO assistant_messages (id, conversation_id, role, content, created_at, seq)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(&id)
    .bind(conversation_id)
    .bind(role)
    .bind(content)
    .bind(&now)
    .bind(seq)
    .execute(db)
    .await?;
    sqlx::query("UPDATE assistant_conversations SET updated_at = $2 WHERE id = $1")
        .bind(conversation_id)
        .bind(&now)
        .execute(db)
        .await?;
    Ok(ChatMessage {
        id,
        role: role.to_string(),
        content: content.to_string(),
        created_at: now,
    })
}

pub async fn update_message_content(
    db: &SqlitePool,
    id: &str,
    content: &str,
) -> LocalResult<()> {
    sqlx::query("UPDATE assistant_messages SET content = $2 WHERE id = $1")
        .bind(id)
        .bind(content)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn set_title(db: &SqlitePool, conversation_id: &str, title: &str) -> LocalResult<()> {
    sqlx::query("UPDATE assistant_conversations SET title = $2 WHERE id = $1")
        .bind(conversation_id)
        .bind(title)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn upsert_markdown_artifact(
    db: &SqlitePool,
    conversation_id: &str,
    title: &str,
    body: &str,
) -> LocalResult<String> {
    let now = Utc::now().to_rfc3339();
    let payload = serde_json::json!({ "body": body }).to_string();
    let existing: Option<(String,)> = sqlx::query_as(
        "SELECT id FROM assistant_artifacts
         WHERE conversation_id = $1 AND kind = 'markdown'
         ORDER BY seq DESC LIMIT 1",
    )
    .bind(conversation_id)
    .fetch_optional(db)
    .await?;

    if let Some((id,)) = existing {
        sqlx::query(
            "UPDATE assistant_artifacts SET title = $2, payload_json = $3 WHERE id = $1",
        )
        .bind(&id)
        .bind(title)
        .bind(&payload)
        .execute(db)
        .await?;
        sqlx::query(
            "UPDATE assistant_conversations SET active_artifact_id = $2, updated_at = $3 WHERE id = $1",
        )
        .bind(conversation_id)
        .bind(&id)
        .bind(&now)
        .execute(db)
        .await?;
        return Ok(id);
    }

    let id = Uuid::new_v4().to_string();
    let (seq,): (i64,) = sqlx::query_as(
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM assistant_artifacts WHERE conversation_id = $1",
    )
    .bind(conversation_id)
    .fetch_one(db)
    .await?;
    sqlx::query(
        "INSERT INTO assistant_artifacts (id, conversation_id, title, kind, payload_json, created_at, seq)
         VALUES ($1, $2, $3, 'markdown', $4, $5, $6)",
    )
    .bind(&id)
    .bind(conversation_id)
    .bind(title)
    .bind(&payload)
    .bind(&now)
    .bind(seq)
    .execute(db)
    .await?;
    sqlx::query(
        "UPDATE assistant_conversations SET active_artifact_id = $2, updated_at = $3 WHERE id = $1",
    )
    .bind(conversation_id)
    .bind(&id)
    .bind(&now)
    .execute(db)
    .await?;
    Ok(id)
}

pub async fn history_for_model(
    db: &SqlitePool,
    conversation_id: &str,
) -> LocalResult<Vec<(String, String)>> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT role, content FROM assistant_messages
         WHERE conversation_id = $1 ORDER BY seq ASC",
    )
    .bind(conversation_id)
    .fetch_all(db)
    .await?;
    const KEEP: usize = 40;
    if rows.len() > KEEP {
        Ok(rows[rows.len() - KEEP..].to_vec())
    } else {
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_server::test_support::shop;

    #[test]
    fn sse_reads_content_and_done() {
        let line = r#"{"choices":[{"delta":{"content":"Bonjour"}}]}"#;
        assert_eq!(
            parse_sse_data(line),
            Some(SsePayload::Text("Bonjour".into()))
        );
        assert_eq!(parse_sse_data("[DONE]"), Some(SsePayload::Done));
        assert_eq!(
            parse_sse_data(r#"{"error":{"message":"Rate limit"}}"#),
            Some(SsePayload::ApiError("Rate limit".into()))
        );
        assert_eq!(
            parse_sse_data(r#"{"choices":[{"delta":{"role":"assistant"}}]}"#),
            None
        );
    }

    #[test]
    fn drain_holds_a_partial_line() {
        let mut buf = b"data: {\"a\":".to_vec();
        assert!(drain_sse_lines(&mut buf).is_empty());
        buf.extend_from_slice(b"1}\n");
        assert_eq!(drain_sse_lines(&mut buf), vec![r#"{"a":1}"#]);
        assert!(buf.is_empty());
    }

    #[test]
    fn drain_waits_for_the_rest_of_a_character_cut_in_two() {
        let line = "data: {\"t\":\"réglée\"}\r\n".as_bytes();
        let cut = line.iter().position(|b| *b == 0xC3).unwrap() + 1;
        let mut buf = line[..cut].to_vec();
        assert!(drain_sse_lines(&mut buf).is_empty());
        buf.extend_from_slice(&line[cut..]);
        assert_eq!(drain_sse_lines(&mut buf), vec![r#"{"t":"réglée"}"#]);
    }

    #[test]
    fn extract_reads_openrouter_error() {
        assert_eq!(
            extract_error_message(r#"{"error":{"message":"Rate limit"}}"#).as_deref(),
            Some("Rate limit")
        );
        assert_eq!(extract_error_message("not json"), None);
    }

    #[test]
    fn title_truncates_like_the_ui() {
        assert_eq!(title_from_prompt("  Chercher Dupont  "), "Chercher Dupont");
        assert_eq!(title_from_prompt("   "), "Nouvelle conversation");
        let title = title_from_prompt(&"A".repeat(80));
        assert!(title.ends_with('…'));
        assert_eq!(title.chars().count(), TITLE_MAX);
    }

    #[test]
    fn model_slug_accepts_provider_model() {
        assert!(parse_model_slug("anthropic/claude-sonnet-4.5").is_ok());
        assert!(parse_model_slug("openai/gpt-4o:online").is_ok());
        assert!(parse_model_slug("claude").is_err());
        assert!(parse_model_slug("").is_err());
    }

    #[tokio::test]
    async fn persist_round_trip() {
        let state = shop().await;
        let id = create_conversation(&state.db, "Nouvelle conversation")
            .await
            .unwrap();
        append_message(&state.db, &id, "user", "Bonjour")
            .await
            .unwrap();
        append_message(&state.db, &id, "assistant", "Salut")
            .await
            .unwrap();
        let last = get_conversation(&state.db, &id).await.unwrap();
        let aid = last.messages[1].id.clone();
        update_message_content(&state.db, &aid, "Salut !")
            .await
            .unwrap();
        upsert_markdown_artifact(&state.db, &id, "Réponse", "# Salut")
            .await
            .unwrap();
        let conv = get_conversation(&state.db, &id).await.unwrap();
        assert_eq!(conv.messages.len(), 2);
        assert_eq!(conv.messages[1].content, "Salut !");
        assert_eq!(conv.artifacts.len(), 1);
        assert_eq!(conv.artifacts[0].kind, "markdown");
        assert_eq!(conv.artifacts[0].payload["body"], "# Salut");
        delete_conversation(&state.db, &id).await.unwrap();
        assert!(get_conversation(&state.db, &id).await.is_err());
    }
}
