//! The assistant's routes from end to end, with a stand-in for OpenRouter on this PC.
//!
//! The service is the real one: the loopback router on a socket, an administrator signed in, a
//! key stored through the route. Only OpenRouter is replaced, by a small server on 127.0.0.1
//! that the state is pointed at (`shop_with_openrouter`). No test here reaches another host.

use std::collections::VecDeque;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::local_server::assistant::{self, OPENROUTER_API, SYSTEM_PROMPT};
use crate::local_server::local_accounts;
use crate::local_server::local_auth::LOCAL_TOKEN_HEADER;
use crate::local_server::loopback_api;
use crate::local_server::state::SharedState;
use crate::local_server::test_support::{shop, shop_with_openrouter};

const KEY: &str = "sk-or-v1-abcdefghijkl";
const REFUSED_KEY: &str = "sk-or-v1-revoked-key";
const MODEL: &str = "anthropic/claude-sonnet-4.5";

/// What the stand-in answers to one chat completion call.
enum Answer {
    /// HTTP 200 and a body written in these pieces, each sent on its own.
    Stream(Vec<Vec<u8>>),
    /// A refusal instead of a stream.
    Refuse(StatusCode, String),
}

/// One call the stand-in received.
#[derive(Clone)]
struct Call {
    path: &'static str,
    authorization: String,
    body: Value,
}

#[derive(Clone, Default)]
struct StandIn {
    answers: Arc<Mutex<VecDeque<Answer>>>,
    calls: Arc<Mutex<Vec<Call>>>,
}

impl StandIn {
    fn will_answer(&self, answer: Answer) {
        self.answers.lock().unwrap().push_back(answer);
    }

    fn calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap().clone()
    }

    fn record(&self, path: &'static str, headers: &HeaderMap, body: Value) {
        let authorization = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        self.calls.lock().unwrap().push(Call { path, authorization, body });
    }
}

async fn stand_in_chat(
    State(stand_in): State<StandIn>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    stand_in.record("/chat/completions", &headers, body);
    let answer = stand_in.answers.lock().unwrap().pop_front();
    match answer.expect("the test said what the model answers") {
        Answer::Refuse(status, body) => (status, body).into_response(),
        Answer::Stream(pieces) => {
            let (tx, rx) = mpsc::channel::<Result<Bytes, Infallible>>(1);
            tokio::spawn(async move {
                for piece in pieces {
                    if tx.send(Ok(Bytes::from(piece))).await.is_err() {
                        return;
                    }
                    // Long enough for the piece to leave and be read before the next one.
                    tokio::time::sleep(Duration::from_millis(40)).await;
                }
            });
            (
                [(header::CONTENT_TYPE, "text/event-stream")],
                Body::from_stream(ReceiverStream::new(rx)),
            )
                .into_response()
        }
    }
}

async fn stand_in_models(State(stand_in): State<StandIn>, headers: HeaderMap) -> Response {
    stand_in.record("/models", &headers, Value::Null);
    let refused = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v == format!("Bearer {REFUSED_KEY}"));
    if refused {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": { "message": "User not found.", "code": 401 } })),
        )
            .into_response();
    }
    Json(json!({ "data": [
        { "id": "anthropic/claude-sonnet-4.5" },
        { "id": "openai/gpt-4o" },
        { "id": "mistralai/mistral-large" },
    ] }))
    .into_response()
}

/// Start the stand-in on a free port of this PC; returns the address the service is given in
/// place of OpenRouter's.
async fn stand_in() -> (String, StandIn) {
    let stand_in = StandIn::default();
    let app = Router::new()
        .route("/chat/completions", post(stand_in_chat))
        .route("/models", get(stand_in_models))
        .with_state(stand_in.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        // Without the delay of small writes, so that a piece is not held back with the next.
        axum::serve(listener, app).tcp_nodelay(true).await.unwrap();
    });
    (format!("http://{addr}"), stand_in)
}

/// The service as the interface reaches it: the loopback router on a socket, its token, an
/// administrator signed in.
struct Desk {
    base: String,
    token: String,
    http: reqwest::Client,
    state: SharedState,
}

impl Desk {
    async fn open(openrouter: &str) -> Self {
        let state = shop_with_openrouter(openrouter).await;
        local_accounts::setup(&state, "camille", "mot de passe", "mot de passe")
            .await
            .expect("the first administrator creates the network");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = loopback_api(state.clone());
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self {
            base: format!("http://{addr}"),
            token: state.local_token.clone(),
            http: reqwest::Client::new(),
            state,
        }
    }

    /// The same, with a key stored the way the settings screen stores it.
    async fn with_key(openrouter: &str) -> Self {
        let desk = Self::open(openrouter).await;
        desk.store_key(KEY).await;
        desk
    }

    fn call(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{}{path}", self.base))
            .header(LOCAL_TOKEN_HEADER.as_str(), &self.token)
    }

    async fn store_key(&self, key: &str) {
        let res = self
            .call(reqwest::Method::PUT, "/api/assistant/credentials")
            .json(&json!({ "api_key": key }))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 200);
        // Beside the database of this test, never in the developer's keychain.
        assert_eq!(
            std::fs::read_to_string(self.state.data_dir.join("openrouter.key")).unwrap(),
            key
        );
    }

    async fn post_chat(&self, conversation: Option<&str>, message: &str, model: &str) -> reqwest::Response {
        self.call(reqwest::Method::POST, "/api/assistant/chat")
            .json(&json!({ "conversation_id": conversation, "message": message, "model": model }))
            .send()
            .await
            .unwrap()
    }

    /// Send a message and read the stream to its end: the events, in the order they came.
    async fn chat(&self, conversation: Option<&str>, message: &str) -> Vec<Value> {
        let res = self.post_chat(conversation, message, MODEL).await;
        assert_eq!(res.status(), 200);
        let body = res.text().await.unwrap();
        body.lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .map(|data| serde_json::from_str(data.trim()).expect("an event is JSON"))
            .collect()
    }

    async fn conversation(&self, id: &str) -> Value {
        let res = self
            .call(reqwest::Method::GET, &format!("/api/assistant/conversations/{id}"))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 200);
        res.json().await.unwrap()
    }

    async fn conversations(&self) -> Vec<Value> {
        self.call(reqwest::Method::GET, "/api/assistant/conversations")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap()
    }

    fn close(self) {
        let _ = std::fs::remove_dir_all(&self.state.data_dir);
    }
}

/// One line of the model's stream carrying a piece of text.
fn content(text: &str) -> String {
    format!("data: {}\n\n", json!({ "choices": [{ "delta": { "content": text } }] }))
}

/// A whole streamed answer: OpenRouter's keep-alive comment, the role, the text, the end.
fn stream_of(texts: &[&str]) -> String {
    let mut out = String::from(": OPENROUTER PROCESSING\n\n");
    out.push_str(&format!(
        "data: {}\n\n",
        json!({ "choices": [{ "delta": { "role": "assistant", "content": "" } }] })
    ));
    for text in texts {
        out.push_str(&content(text));
    }
    out.push_str("data: [DONE]\n\n");
    out
}

/// Cut a body into pieces at these byte offsets.
fn cut(body: &str, at: &[usize]) -> Vec<Vec<u8>> {
    let bytes = body.as_bytes();
    let mut pieces = Vec::new();
    let mut from = 0;
    for &to in at {
        assert!(from < to && to < bytes.len(), "cuts go forward, inside the body");
        pieces.push(bytes[from..to].to_vec());
        from = to;
    }
    pieces.push(bytes[from..].to_vec());
    pieces
}

fn kinds(events: &[Value]) -> Vec<&str> {
    events.iter().map(|e| e["type"].as_str().unwrap()).collect()
}

fn deltas(events: &[Value]) -> Vec<&str> {
    events
        .iter()
        .filter(|e| e["type"] == "delta")
        .map(|e| e["text"].as_str().unwrap())
        .collect()
}

fn stored(conversation: &Value) -> Vec<(&str, &str)> {
    conversation["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| (m["role"].as_str().unwrap(), m["content"].as_str().unwrap()))
        .collect()
}

#[tokio::test]
async fn the_app_talks_to_openrouter_and_nothing_else_changes_that() {
    let state = shop().await;
    assert_eq!(OPENROUTER_API, "https://openrouter.ai/api/v1");
    assert_eq!(state.openrouter_url, OPENROUTER_API);
    assert_eq!(
        assistant::chat_completions_url(&state.openrouter_url),
        "https://openrouter.ai/api/v1/chat/completions"
    );
    assert_eq!(
        assistant::models_url(&state.openrouter_url),
        "https://openrouter.ai/api/v1/models"
    );
    let _ = std::fs::remove_dir_all(&state.data_dir);
}

#[tokio::test]
async fn an_answer_streamed_in_pieces_arrives_whole_and_is_kept() {
    let (openrouter, model) = stand_in().await;
    let desk = Desk::with_key(&openrouter).await;

    let texts = ["Bonjour", ", la facture est ", "réglée", " — à bientôt."];
    let body = stream_of(&texts);
    // Pieces that end where the network pleases: inside the first line of text, between the two
    // bytes of the « é » of « réglée », and inside the line that ends the stream.
    let in_a_line = body.find("Bonjour").unwrap() + 3;
    let in_a_character = body.find("réglée").unwrap() + 2;
    assert!(!body.is_char_boundary(in_a_character));
    let in_the_end = body.find("[DONE]").unwrap() + 2;
    model.will_answer(Answer::Stream(cut(&body, &[in_a_line, in_a_character, in_the_end])));

    let events = desk.chat(None, "  Où en est la facture   Dupont ?  ").await;
    assert_eq!(kinds(&events), ["start", "delta", "delta", "delta", "delta", "done"]);
    assert_eq!(deltas(&events), texts);
    assert_eq!(events.last().unwrap()["title"], "Où en est la facture Dupont ?");

    let start = &events[0];
    let id = start["conversation_id"].as_str().unwrap();
    let kept = desk.conversation(id).await;
    let answer = "Bonjour, la facture est réglée — à bientôt.";
    assert_eq!(kept["title"], "Où en est la facture Dupont ?");
    assert_eq!(
        stored(&kept),
        [("user", "Où en est la facture   Dupont ?"), ("assistant", answer)]
    );
    assert_eq!(kept["messages"][0]["id"], start["user_message_id"]);
    assert_eq!(kept["messages"][1]["id"], start["assistant_message_id"]);

    let artifacts = kept["artifacts"].as_array().unwrap();
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0]["title"], "Réponse");
    assert_eq!(artifacts[0]["kind"], "markdown");
    assert_eq!(artifacts[0]["payload"]["body"], answer);
    assert_eq!(kept["active_artifact_id"], artifacts[0]["id"]);

    // What left for the model: the key, the model, the prompt of the house, the message.
    let calls = model.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].path, "/chat/completions");
    assert_eq!(calls[0].authorization, format!("Bearer {KEY}"));
    assert_eq!(calls[0].body["model"], MODEL);
    assert_eq!(calls[0].body["stream"], true);
    assert_eq!(
        calls[0].body["messages"],
        json!([
            { "role": "system", "content": SYSTEM_PROMPT },
            { "role": "user", "content": "Où en est la facture   Dupont ?" },
        ])
    );
    desk.close();
}

#[tokio::test]
async fn a_second_message_keeps_the_title_and_sends_what_was_said_before() {
    let (openrouter, model) = stand_in().await;
    let desk = Desk::with_key(&openrouter).await;

    model.will_answer(Answer::Stream(vec![stream_of(&["Trois motos ", "sont en atelier."]).into_bytes()]));
    let first = desk.chat(None, "Combien de motos en atelier ?").await;
    assert_eq!(kinds(&first), ["start", "delta", "delta", "done"]);
    let id = first[0]["conversation_id"].as_str().unwrap().to_string();

    model.will_answer(Answer::Stream(vec![stream_of(&["La Tracer de Mme Martin."]).into_bytes()]));
    let second = desk.chat(Some(&id), "Laquelle sort en premier ?").await;
    assert_eq!(kinds(&second), ["start", "delta", "done"]);
    assert_eq!(second[0]["conversation_id"], id.as_str());
    // The conversation keeps the name its first message gave it.
    assert_eq!(second.last().unwrap()["title"], "Combien de motos en atelier ?");

    let calls = model.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(
        calls[1].body["messages"],
        json!([
            { "role": "system", "content": SYSTEM_PROMPT },
            { "role": "user", "content": "Combien de motos en atelier ?" },
            { "role": "assistant", "content": "Trois motos sont en atelier." },
            { "role": "user", "content": "Laquelle sort en premier ?" },
        ])
    );

    let kept = desk.conversation(&id).await;
    assert_eq!(kept["title"], "Combien de motos en atelier ?");
    assert_eq!(
        stored(&kept),
        [
            ("user", "Combien de motos en atelier ?"),
            ("assistant", "Trois motos sont en atelier."),
            ("user", "Laquelle sort en premier ?"),
            ("assistant", "La Tracer de Mme Martin."),
        ]
    );
    // One document per conversation: it now holds the last answer.
    let artifacts = kept["artifacts"].as_array().unwrap();
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0]["title"], "Réponse");
    assert_eq!(artifacts[0]["payload"]["body"], "La Tracer de Mme Martin.");

    let listed = desk.conversations().await;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["title"], "Combien de motos en atelier ?");
    desk.close();
}

#[tokio::test]
async fn a_key_openrouter_refuses_ends_the_answer_with_its_message() {
    let (openrouter, model) = stand_in().await;
    let desk = Desk::with_key(&openrouter).await;
    model.will_answer(Answer::Refuse(
        StatusCode::UNAUTHORIZED,
        json!({ "error": { "message": "No auth credentials found", "code": 401 } }).to_string(),
    ));

    let events = desk.chat(None, "Bonjour").await;
    assert_eq!(kinds(&events), ["start", "error"]);
    assert_eq!(events[1]["message"], "No auth credentials found");

    let kept = desk.conversation(events[0]["conversation_id"].as_str().unwrap()).await;
    assert_eq!(
        stored(&kept),
        [("user", "Bonjour"), ("assistant", "No auth credentials found")]
    );
    assert_eq!(kept["artifacts"], json!([]));
    assert_eq!(kept["active_artifact_id"], Value::Null);
    desk.close();
}

#[tokio::test]
async fn a_refusal_without_a_message_says_which_status_came_back() {
    let (openrouter, model) = stand_in().await;
    let desk = Desk::with_key(&openrouter).await;
    model.will_answer(Answer::Refuse(StatusCode::BAD_GATEWAY, "<html>Bad gateway</html>".into()));

    let events = desk.chat(None, "Bonjour").await;
    assert_eq!(kinds(&events), ["start", "error"]);
    assert_eq!(events[1]["message"], "OpenRouter a répondu HTTP 502 Bad Gateway.");
    desk.close();
}

#[tokio::test]
async fn an_error_in_the_middle_of_the_stream_leaves_no_half_answer() {
    let (openrouter, model) = stand_in().await;
    let desk = Desk::with_key(&openrouter).await;

    let mut body = String::new();
    body.push_str(&content("La marge du mois "));
    body.push_str(&content("est de "));
    body.push_str(&format!(
        "data: {}\n\n",
        json!({ "error": { "message": "Provider returned error", "code": 502 } })
    ));
    // Whatever follows the error is not part of an answer.
    body.push_str(&content("12 %."));
    body.push_str("data: [DONE]\n\n");
    let after_the_first_line = content("La marge du mois ").len();
    model.will_answer(Answer::Stream(cut(&body, &[after_the_first_line])));

    let events = desk.chat(None, "Quelle est la marge du mois ?").await;
    assert_eq!(kinds(&events), ["start", "delta", "delta", "error"]);
    assert_eq!(deltas(&events), ["La marge du mois ", "est de "]);
    assert_eq!(events[3]["message"], "Provider returned error");

    // The message says what went wrong; the cut sentence is not kept as if it were an answer,
    // and no document is made of it.
    let kept = desk.conversation(events[0]["conversation_id"].as_str().unwrap()).await;
    assert_eq!(
        stored(&kept),
        [("user", "Quelle est la marge du mois ?"), ("assistant", "Provider returned error")]
    );
    assert_eq!(kept["artifacts"], json!([]));
    assert_eq!(kept["active_artifact_id"], Value::Null);
    desk.close();
}

#[tokio::test]
async fn a_model_that_says_nothing_is_reported_as_such() {
    let (openrouter, model) = stand_in().await;
    let desk = Desk::with_key(&openrouter).await;
    model.will_answer(Answer::Stream(vec![stream_of(&[]).into_bytes()]));

    let events = desk.chat(None, "Bonjour").await;
    assert_eq!(kinds(&events), ["start", "done"]);
    let kept = desk.conversation(events[0]["conversation_id"].as_str().unwrap()).await;
    assert_eq!(
        stored(&kept),
        [("user", "Bonjour"), ("assistant", "Le modèle n'a rien renvoyé.")]
    );
    desk.close();
}

#[tokio::test]
async fn what_is_refused_here_never_reaches_the_model() {
    let (openrouter, model) = stand_in().await;
    let desk = Desk::open(&openrouter).await;

    // No key yet.
    let res = desk.post_chat(None, "Bonjour", MODEL).await;
    assert_eq!(res.status(), 400);
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["error"]["message"], "Aucune clé OpenRouter n'est enregistrée.");

    desk.store_key(KEY).await;
    let res = desk.post_chat(None, "   ", MODEL).await;
    assert_eq!(res.status(), 400);
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["error"]["message"], "Le message est vide.");

    let res = desk.post_chat(None, "Bonjour", "claude").await;
    assert_eq!(res.status(), 400);

    let res = desk.post_chat(Some("no-such-conversation"), "Bonjour", MODEL).await;
    assert_eq!(res.status(), 404);

    // Without the service's token the route is not reached at all.
    let res = reqwest::Client::new()
        .post(format!("{}/api/assistant/chat", desk.base))
        .json(&json!({ "message": "Bonjour", "model": MODEL }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 401);

    assert!(model.calls().is_empty());
    assert!(desk.conversations().await.is_empty());
    desk.close();
}

#[tokio::test]
async fn the_connection_test_counts_the_models_or_says_the_key_is_refused() {
    let (openrouter, model) = stand_in().await;
    let desk = Desk::open(&openrouter).await;

    // Nothing to test without a key, and nothing is asked.
    let res = desk.call(reqwest::Method::POST, "/api/assistant/test").send().await.unwrap();
    assert_eq!(res.status(), 400);
    assert!(model.calls().is_empty());

    desk.store_key(KEY).await;
    let res = desk.call(reqwest::Method::POST, "/api/assistant/test").send().await.unwrap();
    assert_eq!(res.status(), 200);
    let body: Value = res.json().await.unwrap();
    assert_eq!(body, json!({ "ok": true, "model_count": 3 }));

    desk.store_key(REFUSED_KEY).await;
    let res = desk.call(reqwest::Method::POST, "/api/assistant/test").send().await.unwrap();
    assert_eq!(res.status(), 400);
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["error"]["message"], "Clé OpenRouter refusée.");

    let calls = model.calls();
    assert_eq!(calls.len(), 2);
    assert!(calls.iter().all(|c| c.path == "/models"));
    assert_eq!(calls[0].authorization, format!("Bearer {KEY}"));
    assert_eq!(calls[1].authorization, format!("Bearer {REFUSED_KEY}"));
    desk.close();
}
