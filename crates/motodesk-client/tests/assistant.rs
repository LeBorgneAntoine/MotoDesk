//! The assistant against the service's own handlers, for everything that stays on this PC: the
//! key, the conversations, what is refused before a model is asked.
//!
//! The answer of a model comes from OpenRouter, which no test calls: a stand-in writes an
//! answer the way the service does, in pieces that end anywhere.

use std::convert::Infallible;

use axum::body::{Body, Bytes};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use motodesk_client::assistant::{self, ArtifactBody, ChatEvent, ChatRequest, Role};
use motodesk_client::local_account::{self, SetupPayload};
use motodesk_client::{Client, Cloud, Local, TokenStore};
use motodesk_server::local_server::state::SharedState;
use motodesk_server::local_server::{assistant as kept, loopback_api, test_support};
use serde_json::{json, Value};

/// A shop on a local network, its administrator signed in; the service's state comes along to
/// write what only a model's answer would.
async fn shop() -> (Client, SharedState) {
    let state = test_support::shop().await;
    let token = state.local_token.clone();
    let store = TokenStore::in_dir(&state.data_dir);
    let api = Client::new(Local::new(loopback_api(state.clone()), &token), Cloud::new("http://127.0.0.1:9"), store);
    let password = "mot de passe admin".to_string();
    let session = local_account::setup(&api, &SetupPayload { username: "camille".into(), password: password.clone(), confirm: password, force: true })
        .await
        .expect("the first administrator creates the network");
    api.set_tokens(Some(session.bundle));
    (api, state)
}

fn request(message: &str, model: &str) -> ChatRequest {
    ChatRequest { conversation_id: None, message: message.into(), model: model.into() }
}

#[tokio::test]
async fn the_key_is_kept_on_this_pc_and_never_comes_back() {
    let (api, state) = shop().await;
    let none = assistant::credentials(&api).await.unwrap();
    assert_eq!((none.configured, none.hint), (false, None));

    // Too short to be a key.
    let refused = assistant::store_key(&api, "abc").await.unwrap_err();
    assert_eq!(refused.status, 400);
    assert!(!assistant::credentials(&api).await.unwrap().configured);

    let kept = assistant::store_key(&api, "  sk-or-v1-abcdefghijkl  ").await.unwrap();
    assert_eq!((kept.configured, kept.hint.as_deref()), (true, Some("sk-o…ijkl")));
    assert_eq!(assistant::credentials(&api).await.unwrap(), kept);
    // Beside the database, not in the settings that travel to the other PCs.
    assert_eq!(std::fs::read_to_string(state.data_dir.join("openrouter.key")).unwrap(), "sk-or-v1-abcdefghijkl");

    assistant::forget_key(&api).await.unwrap();
    assert!(!assistant::credentials(&api).await.unwrap().configured);
    assert!(!state.data_dir.join("openrouter.key").exists());
    let _ = std::fs::remove_dir_all(&state.data_dir);
}

#[tokio::test]
async fn nothing_is_asked_of_a_model_without_a_key_a_message_and_a_model() {
    let (api, state) = shop().await;
    let mut heard = Vec::new();

    let no_key = assistant::stream_chat(&api, &request("Bonjour", "openai/gpt-4.1"), |event| heard.push(event)).await.unwrap_err();
    assert_eq!((no_key.status, no_key.message.as_str()), (400, "Aucune clé OpenRouter n'est enregistrée."));
    let tested = assistant::test_connection(&api).await.unwrap_err();
    assert_eq!(tested.message, "Aucune clé OpenRouter n'est enregistrée.");

    let empty = assistant::stream_chat(&api, &request("   ", "openai/gpt-4.1"), |event| heard.push(event)).await.unwrap_err();
    assert_eq!((empty.status, empty.message.as_str()), (400, "Le message est vide."));
    let model = assistant::stream_chat(&api, &request("Bonjour", "gpt"), |event| heard.push(event)).await.unwrap_err();
    assert_eq!(model.message, "Le slug du modèle doit ressembler à fournisseur/modèle.");

    // A refusal writes nothing: no event, and no conversation left behind.
    assert!(heard.is_empty());
    assert!(assistant::conversations(&api).await.unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&state.data_dir);
}

#[tokio::test]
async fn conversations_are_listed_opened_and_deleted() {
    let (api, state) = shop().await;
    assert!(assistant::conversations(&api).await.unwrap().is_empty());

    // What a finished exchange leaves in the database.
    let id = kept::create_conversation(&state.db, "Révision des 600 km").await.unwrap();
    kept::append_message(&state.db, &id, "user", "Que faut-il prévoir ?").await.unwrap();
    kept::append_message(&state.db, &id, "assistant", "## Révision\n\n- vidange").await.unwrap();
    kept::upsert_markdown_artifact(&state.db, &id, "Réponse", "## Révision\n\n- vidange").await.unwrap();

    let listed = assistant::conversations(&api).await.unwrap();
    assert_eq!(listed.iter().map(|c| (c.id.as_str(), c.title.as_str())).collect::<Vec<_>>(), [(id.as_str(), "Révision des 600 km")]);

    let opened = assistant::conversation(&api, &id).await.unwrap();
    assert_eq!(opened.summary(), listed[0]);
    assert_eq!(
        opened.messages.iter().map(|m| (m.role, m.content.as_str())).collect::<Vec<_>>(),
        [(Role::User, "Que faut-il prévoir ?"), (Role::Assistant, "## Révision\n\n- vidange")]
    );
    let shown = opened.active_artifact().expect("the answer is on the canvas");
    assert_eq!((shown.title.as_str(), &shown.body), ("Réponse", &ArtifactBody::Markdown("## Révision\n\n- vidange".into())));

    assistant::delete_conversation(&api, &id).await.unwrap();
    assert!(assistant::conversations(&api).await.unwrap().is_empty());
    assert!(assistant::conversation(&api, &id).await.unwrap_err().is_not_found());
    assert!(assistant::delete_conversation(&api, &id).await.unwrap_err().is_not_found());
    let _ = std::fs::remove_dir_all(&state.data_dir);
}

/// Answers like the service's chat route: an event stream, here cut at awkward places (in the
/// middle of a line, in the middle of « é »).
async fn written_in_pieces(Json(asked): Json<Value>) -> Response {
    if asked["message"] == "refuse" {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": { "code": "bad_request", "message": "Aucune clé OpenRouter n'est enregistrée." } })))
            .into_response();
    }
    let whole = format!(
        concat!(
            ": keep-alive\n\n",
            "data: {{\"type\":\"start\",\"conversation_id\":\"c1\",\"user_message_id\":\"u1\",\"assistant_message_id\":\"a1\"}}\n\n",
            "data: {{\"type\":\"delta\",\"text\":\"Modèle {} : \"}}\n\n",
            "data: {{\"type\":\"delta\",\"text\":\"révision prévue.\"}}\n\n",
            "data: {{\"type\":\"done\",\"title\":\"{}\"}}\n\n",
        ),
        asked["model"].as_str().unwrap_or_default(),
        asked["message"].as_str().unwrap_or_default(),
    );
    let pieces: Vec<Result<Bytes, Infallible>> = whole.as_bytes().chunks(11).map(|piece| Ok(Bytes::copy_from_slice(piece))).collect();
    ([(header::CONTENT_TYPE, "text/event-stream")], Body::from_stream(futures_util::stream::iter(pieces))).into_response()
}

#[tokio::test]
async fn an_answer_is_heard_piece_by_piece_in_order() {
    let dir = std::env::temp_dir().join(format!("motodesk-assistant-stream-{}", std::process::id()));
    let service = Router::new().route("/api/assistant/chat", post(written_in_pieces));
    let api = Client::new(Local::new(service, "token"), Cloud::new("http://127.0.0.1:9"), TokenStore::in_dir(&dir));

    let mut heard = Vec::new();
    assistant::stream_chat(&api, &request("Révision", "openai/gpt-4.1"), |event| heard.push(event)).await.unwrap();
    assert_eq!(
        heard,
        [
            ChatEvent::Start { conversation_id: "c1".into(), user_message_id: "u1".into(), assistant_message_id: "a1".into() },
            ChatEvent::Delta { text: "Modèle openai/gpt-4.1 : ".into() },
            ChatEvent::Delta { text: "révision prévue.".into() },
            ChatEvent::Done { title: "Révision".into() },
        ]
    );

    // A refusal is an error, not an empty answer.
    let refused = assistant::stream_chat(&api, &request("refuse", "openai/gpt-4.1"), |_| panic!("nothing is written")).await.unwrap_err();
    assert_eq!((refused.status, refused.message.as_str()), (400, "Aucune clé OpenRouter n'est enregistrée."));
    let _ = std::fs::remove_dir_all(&dir);
}
