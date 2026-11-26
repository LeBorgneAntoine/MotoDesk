//! The assistant (port of `desktop/src/lib/api/assistant.ts` and of `lib/assistant/`'s `map`,
//! `models`, `sse` and `types`): the OpenRouter key of this PC, its conversations, and the
//! answer of the model as it is being written.
//!
//! The key and the chosen model belong to this PC. The key is kept by the service outside the
//! shop's settings (those travel to every PC); the model is kept in a file of its own.

use std::path::Path;

use axum::http::Method;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::device_store::DeviceFile;
use crate::error::ApiResult;
use crate::session::Client;
use crate::settings::encode;

// ---- the key -----------------------------------------------------------------------------------

/// Whether a key is stored. The key itself never comes back: only its first and last characters.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct Credentials {
    pub configured: bool,
    #[serde(default)]
    pub hint: Option<String>,
}

pub async fn credentials(client: &Client) -> ApiResult<Credentials> {
    client.get("/api/assistant/credentials").await
}

/// Administrators only.
pub async fn store_key(client: &Client, api_key: &str) -> ApiResult<Credentials> {
    client.put("/api/assistant/credentials", &serde_json::json!({ "api_key": api_key })).await
}

/// Administrators only.
pub async fn forget_key(client: &Client) -> ApiResult<()> {
    client.delete("/api/assistant/credentials").await
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Reachable {
    pub ok: bool,
    #[serde(default)]
    pub model_count: usize,
}

/// Ask OpenRouter, with the stored key, which models it offers. Administrators only.
pub async fn test_connection(client: &Client) -> ApiResult<Reachable> {
    client.post("/api/assistant/test", &serde_json::json!({})).await
}

// ---- the model ---------------------------------------------------------------------------------

/// (OpenRouter identifier, name shown).
pub const CURATED_MODELS: [(&str, &str); 4] = [
    ("anthropic/claude-sonnet-4.5", "Claude Sonnet 4.5"),
    ("openai/gpt-4.1", "GPT-4.1"),
    ("google/gemini-2.5-flash", "Gemini 2.5 Flash"),
    ("openai/gpt-4o-mini", "GPT-4o mini"),
];

pub const DEFAULT_MODEL: &str = CURATED_MODELS[0].0;

/// The choice "another model, typed by hand" in the list of models.
pub const CUSTOM_MODEL: &str = "custom";

/// An OpenRouter identifier reads `provider/model`, with an optional `:variant`.
pub fn is_valid_model_slug(raw: &str) -> bool {
    let Some((provider, model)) = raw.trim().split_once('/') else { return false };
    let plain = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-');
    !provider.is_empty() && provider.chars().all(plain) && !model.is_empty() && model.chars().all(|c| plain(c) || matches!(c, ':' | '+'))
}

/// A typed identifier wins; otherwise the model picked in the list.
pub fn resolve_model_id(selected: &str, custom_slug: &str) -> String {
    let custom = custom_slug.trim();
    if !custom.is_empty() {
        return custom.to_string();
    }
    if selected == CUSTOM_MODEL || selected.is_empty() { DEFAULT_MODEL.to_string() } else { selected.to_string() }
}

/// What this PC remembers about its assistant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Prefs {
    pub default_model: String,
    pub custom_slug: String,
    /// The list of conversations is folded away.
    pub rail_collapsed: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self { default_model: DEFAULT_MODEL.to_string(), custom_slug: String::new(), rail_collapsed: false }
    }
}

impl Prefs {
    /// The model a message is sent to.
    pub fn model(&self) -> String {
        resolve_model_id(&self.default_model, &self.custom_slug)
    }
}

const PREFS_FILE: &str = "assistant.store.json";
const PREFS_KEY: &str = "assistant.config.v1";

/// Where [`Prefs`] are kept between two runs.
#[derive(Debug, Clone)]
pub struct PrefsStore {
    file: DeviceFile,
}

impl PrefsStore {
    /// `data_dir` is the app's data folder.
    pub fn in_dir(data_dir: &Path) -> Self {
        Self { file: DeviceFile::in_dir(data_dir, PREFS_FILE) }
    }

    pub fn load(&self) -> Prefs {
        self.file.get(PREFS_KEY).unwrap_or_default()
    }

    pub fn save(&self, prefs: &Prefs) -> std::io::Result<()> {
        self.file.set(PREFS_KEY, prefs)
    }
}

// ---- conversations -----------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ConversationSummary {
    pub id: String,
    pub title: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub id: String,
    pub role: Role,
    pub content: String,
    pub created_at: String,
}

/// Where a request to act on the shop's data stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    Pending,
    Approved,
    Denied,
}

impl Permission {
    pub fn label(self) -> &'static str {
        match self {
            Permission::Pending => "En attente",
            Permission::Approved => "Accordée",
            Permission::Denied => "Refusée",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kpi {
    pub label: String,
    pub value: String,
    pub hint: Option<String>,
}

/// What the canvas can show. The service only keeps documents for now; the other kinds are
/// what the demonstration shows while no key is set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtifactBody {
    /// A document, written in markdown.
    Markdown(String),
    Table { columns: Vec<String>, rows: Vec<Vec<String>> },
    Kpis(Vec<Kpi>),
    Permission { tool: String, summary: String, status: Permission },
    Question { prompt: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    pub id: String,
    pub title: String,
    pub body: ArtifactBody,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conversation {
    pub id: String,
    pub title: String,
    pub updated_at: String,
    pub messages: Vec<Message>,
    pub artifacts: Vec<Artifact>,
    pub active_artifact_id: Option<String>,
}

impl Conversation {
    pub fn summary(&self) -> ConversationSummary {
        ConversationSummary { id: self.id.clone(), title: self.title.clone(), updated_at: self.updated_at.clone() }
    }

    /// What the canvas shows: the artifact that was picked, or the latest one.
    pub fn active_artifact(&self) -> Option<&Artifact> {
        let picked = self.active_artifact_id.as_deref();
        self.artifacts.iter().find(|artifact| Some(artifact.id.as_str()) == picked).or(self.artifacts.last())
    }
}

#[derive(Deserialize)]
struct MessageDto {
    id: String,
    role: String,
    content: String,
    #[serde(default)]
    created_at: String,
}

#[derive(Deserialize)]
struct ArtifactDto {
    id: String,
    title: String,
    kind: String,
    #[serde(default)]
    payload: Value,
}

#[derive(Deserialize)]
struct ConversationDto {
    id: String,
    title: String,
    updated_at: String,
    #[serde(default)]
    messages: Vec<MessageDto>,
    #[serde(default)]
    artifacts: Vec<ArtifactDto>,
    #[serde(default)]
    active_artifact_id: Option<String>,
}

/// A message this interface cannot show (a role it does not know) is left out.
fn message_from(dto: MessageDto) -> Option<Message> {
    let role = match dto.role.as_str() {
        "user" => Role::User,
        "assistant" => Role::Assistant,
        _ => return None,
    };
    Some(Message { id: dto.id, role, content: dto.content, created_at: dto.created_at })
}

/// An artifact of a kind this interface does not know is left out.
fn artifact_from(dto: ArtifactDto) -> Option<Artifact> {
    if dto.kind != "markdown" {
        return None;
    }
    let body = dto.payload.get("body").and_then(Value::as_str).unwrap_or_default().to_string();
    Some(Artifact { id: dto.id, title: dto.title, body: ArtifactBody::Markdown(body) })
}

fn conversation_from(dto: ConversationDto) -> Conversation {
    let artifacts: Vec<Artifact> = dto.artifacts.into_iter().filter_map(artifact_from).collect();
    // The artifact that was open may be one that was left out: the latest stands in.
    let active = dto
        .active_artifact_id
        .filter(|id| artifacts.iter().any(|artifact| &artifact.id == id))
        .or_else(|| artifacts.last().map(|artifact| artifact.id.clone()));
    Conversation {
        id: dto.id,
        title: dto.title,
        updated_at: dto.updated_at,
        messages: dto.messages.into_iter().filter_map(message_from).collect(),
        artifacts,
        active_artifact_id: active,
    }
}

/// The conversations kept on this PC, the latest first.
pub async fn conversations(client: &Client) -> ApiResult<Vec<ConversationSummary>> {
    client.get("/api/assistant/conversations").await
}

pub async fn conversation(client: &Client, id: &str) -> ApiResult<Conversation> {
    let dto: ConversationDto = client.get(format!("/api/assistant/conversations/{}", encode(id))).await?;
    Ok(conversation_from(dto))
}

pub async fn delete_conversation(client: &Client, id: &str) -> ApiResult<()> {
    client.delete(format!("/api/assistant/conversations/{}", encode(id))).await
}

const TITLE_MAX: usize = 48;

/// The title a conversation takes from its first message, as the service names it.
pub fn title_from_prompt(prompt: &str) -> String {
    let compact = prompt.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.is_empty() {
        return "Nouvelle conversation".to_string();
    }
    if compact.chars().count() <= TITLE_MAX {
        return compact;
    }
    let cut: String = compact.chars().take(TITLE_MAX - 1).collect();
    format!("{}…", cut.trim_end())
}

// ---- talking -----------------------------------------------------------------------------------

/// One message to the model. Without `conversation_id`, the service opens a conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChatRequest {
    pub conversation_id: Option<String>,
    pub message: String,
    pub model: String,
}

/// What the service says while the model answers, in this order: one `Start`, the answer in
/// pieces, then `Done` or `Error`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ChatEvent {
    /// The message is recorded: the conversation and both messages have their identifiers.
    Start { conversation_id: String, user_message_id: String, assistant_message_id: String },
    /// The next piece of the answer.
    Delta { text: String },
    /// The answer is whole and kept.
    Done { title: String },
    /// The model could not answer; `message` says why and stands as the answer.
    Error { message: String },
}

/// Reads what the service writes (`data: {...}` lines) as it arrives, in pieces that may end
/// anywhere: in the middle of a line, or of an accented letter.
#[derive(Debug, Default)]
pub struct EventLines {
    pending: Vec<u8>,
}

impl EventLines {
    /// Add a piece; the events of the lines it completes.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<ChatEvent> {
        self.pending.extend_from_slice(chunk);
        let mut events = Vec::new();
        while let Some(end) = self.pending.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=end).collect();
            let line = String::from_utf8_lossy(&line);
            // Anything else (a comment that keeps the connection open, a blank line) is not an event.
            if let Some(event) = line.trim_end().strip_prefix("data:").and_then(|data| serde_json::from_str(data.trim()).ok()) {
                events.push(event);
            }
        }
        events
    }
}

/// Send a message; `on_event` hears the answer as it is written. Returns when the answer is
/// over. A refusal before anything is written (no key, an unknown model) is the error returned.
pub async fn stream_chat(client: &Client, request: &ChatRequest, mut on_event: impl FnMut(ChatEvent)) -> ApiResult<()> {
    let mut lines = EventLines::default();
    client
        .stream(Method::POST, "/api/assistant/chat", request, |chunk| {
            lines.push(chunk).into_iter().for_each(&mut on_event);
            Ok(())
        })
        .await
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_model_identifier_reads_provider_slash_model() {
        assert!(is_valid_model_slug("anthropic/claude-sonnet-4.5"));
        assert!(is_valid_model_slug("  openai/gpt-4o-mini:free "));
        assert!(!is_valid_model_slug("claude"));
        assert!(!is_valid_model_slug("anthropic/"));
        assert!(!is_valid_model_slug("anthropic/claude sonnet"));
        assert!(!is_valid_model_slug("a:b/c"));
    }

    #[test]
    fn a_typed_identifier_wins_over_the_list() {
        assert_eq!(resolve_model_id("openai/gpt-4.1", ""), "openai/gpt-4.1");
        assert_eq!(resolve_model_id("openai/gpt-4.1", " mistralai/mistral-large "), "mistralai/mistral-large");
        assert_eq!(resolve_model_id(CUSTOM_MODEL, ""), DEFAULT_MODEL);
        assert_eq!(resolve_model_id("", ""), DEFAULT_MODEL);
    }

    #[test]
    fn preferences_are_kept_in_the_file_the_tauri_app_used() {
        let dir = std::env::temp_dir().join(format!("motodesk-assistant-prefs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = PrefsStore::in_dir(&dir);
        assert_eq!(store.load(), Prefs::default());

        // Another key of the same file is left alone.
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(PREFS_FILE), r#"{"other":1,"assistant.config.v1":{"defaultModel":"openai/gpt-4.1","railCollapsed":true}}"#).unwrap();
        let mut prefs = store.load();
        assert_eq!((prefs.default_model.as_str(), prefs.custom_slug.as_str(), prefs.rail_collapsed), ("openai/gpt-4.1", "", true));
        assert_eq!(prefs.model(), "openai/gpt-4.1");

        prefs.custom_slug = "mistralai/mistral-large".into();
        store.save(&prefs).unwrap();
        assert_eq!(store.load().model(), "mistralai/mistral-large");
        let after: Value = serde_json::from_slice(&std::fs::read(dir.join(PREFS_FILE)).unwrap()).unwrap();
        assert_eq!(after["other"], 1);
        assert_eq!(after[PREFS_KEY]["customSlug"], "mistralai/mistral-large");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_conversation_keeps_what_this_interface_can_show() {
        let dto: ConversationDto = serde_json::from_value(serde_json::json!({
            "id": "c1", "title": "Révision", "updated_at": "2026-11-26T10:00:00Z",
            "messages": [
                { "id": "m1", "role": "user", "content": "Bonjour", "created_at": "2026-11-26T10:00:00Z" },
                { "id": "m2", "role": "system", "content": "consigne", "created_at": "" },
                { "id": "m3", "role": "assistant", "content": "Bonjour !", "created_at": "" },
            ],
            "artifacts": [
                { "id": "a1", "title": "Réponse", "kind": "markdown", "payload": { "body": "# Titre" } },
                { "id": "a2", "title": "Graphique", "kind": "chart", "payload": {} },
            ],
            "active_artifact_id": "a2",
        }))
        .unwrap();
        let conversation = conversation_from(dto);
        assert_eq!(conversation.messages.iter().map(|m| (m.id.as_str(), m.role)).collect::<Vec<_>>(), [("m1", Role::User), ("m3", Role::Assistant)]);
        assert_eq!(conversation.artifacts.len(), 1);
        // The open artifact was of an unknown kind: the latest known one stands in.
        assert_eq!(conversation.active_artifact_id.as_deref(), Some("a1"));
        assert_eq!(conversation.active_artifact().map(|a| &a.body), Some(&ArtifactBody::Markdown("# Titre".into())));
    }

    #[test]
    fn a_title_is_the_first_message_cut_short() {
        assert_eq!(title_from_prompt("  Trouve   le client\nDupont "), "Trouve le client Dupont");
        assert_eq!(title_from_prompt("   "), "Nouvelle conversation");
        let long = title_from_prompt(&"mot ".repeat(30));
        assert_eq!(long.chars().count(), TITLE_MAX);
        assert!(long.ends_with("mot…"));
        // A cut that falls on a space does not leave it before the ellipsis.
        assert_eq!(title_from_prompt(&format!("{} suite et fin", "a".repeat(46))), format!("{}…", "a".repeat(46)));
    }

    #[test]
    fn events_are_read_whatever_the_size_of_the_pieces() {
        let whole = concat!(
            ": ouverture\n\n",
            "data: {\"type\":\"start\",\"conversation_id\":\"c1\",\"user_message_id\":\"u1\",\"assistant_message_id\":\"a1\"}\n\n",
            "data: {\"type\":\"delta\",\"text\":\"Révision \"}\r\n\r\n",
            "data: pas du json\n",
            "data: {\"type\":\"inconnu\"}\n",
            "data: {\"type\":\"delta\",\"text\":\"à 600 km\"}\n\n",
            "data: {\"type\":\"done\",\"title\":\"Révision\"}\n\n",
        )
        .as_bytes();
        let expected = vec![
            ChatEvent::Start { conversation_id: "c1".into(), user_message_id: "u1".into(), assistant_message_id: "a1".into() },
            ChatEvent::Delta { text: "Révision ".into() },
            ChatEvent::Delta { text: "à 600 km".into() },
            ChatEvent::Done { title: "Révision".into() },
        ];
        // In one piece, byte by byte (every accented letter is cut in two), and in uneven pieces.
        for size in [whole.len(), 1, 7] {
            let mut lines = EventLines::default();
            let events: Vec<ChatEvent> = whole.chunks(size).flat_map(|chunk| lines.push(chunk)).collect();
            assert_eq!(events, expected, "pieces of {size}");
        }
    }

    #[test]
    fn a_line_that_is_not_finished_waits_for_its_end() {
        let mut lines = EventLines::default();
        assert!(lines.push(b"data: {\"type\":\"error\",\"mess").is_empty());
        assert_eq!(lines.push(b"age\":\"Cl\xc3\xa9 refus\xc3\xa9e.\"}\n"), [ChatEvent::Error { message: "Clé refusée.".into() }]);
    }
}
