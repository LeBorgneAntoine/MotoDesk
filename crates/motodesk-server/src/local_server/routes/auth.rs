use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use serde::Deserialize;

use crate::local_server::error::{LocalError, LocalResult};
use crate::local_server::state::SharedState;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/sync-session", post(sync_session))
        .route("/login", post(login))
        .route("/register", post(register))
        .route("/refresh", post(refresh))
        .route("/logout", post(logout))
        .route("/me", get(me))
        .route("/local/status", get(local_status))
        .route("/local/setup", post(local_setup))
        .route("/local/login", post(local_login))
        .route("/local/recover", post(local_recover))
        .route("/local/restore", post(local_restore))
        .route("/local/reconnect", post(local_reconnect))
        .route("/promote", post(promote))
}

#[derive(Debug, Deserialize)]
struct LoginInput {
    email: String,
    password: String,
}

#[derive(Debug, Deserialize)]
struct RegisterInput {
    email: String,
    password: String,
    full_name: String,
}

#[derive(Debug, Deserialize)]
struct RefreshInput {
    refresh_token: String,
}

async fn proxy_post(
    state: &SharedState,
    path: &str,
    body: serde_json::Value,
) -> LocalResult<(StatusCode, serde_json::Value)> {
    let base = crate::local_server::cloud::cloud_base_url(state).await;
    let url = format!("{base}{path}");
    tracing::debug!(%url, "proxying auth request to cloud");
    let res = state
        .http
        .post(&url)
        .json(&body)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await;
    match res {
        Ok(r) if r.status().is_success() || r.status().as_u16() == 201 => {
            let status = StatusCode::from_u16(r.status().as_u16()).unwrap_or(StatusCode::OK);
            let json: serde_json::Value = r.json().await.unwrap_or_default();
            Ok((status, json))
        }
        Ok(r) => {
            let code = r.status().as_u16();
            tracing::warn!(status = code, url = %url, "cloud auth proxy non-ok");
            if (500..600).contains(&code) {
                Err(LocalError::CloudRequired)
            } else {
                Err(LocalError::InvalidCredentials)
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, url = %url, "cloud auth proxy failed");
            Err(LocalError::CloudRequired)
        }
    }
}

async fn cache_session(state: &SharedState, bundle: &serde_json::Value) -> LocalResult<()> {
    let user = bundle.get("user").cloned().unwrap_or_default();
    // Before anything is stored: an account from another company does not get in.
    crate::local_server::company::ensure_same_company(
        &state.db,
        user.get("company_id").and_then(|v| v.as_str()),
    )
    .await?;
    // The session of a local network is this service's own record: what marks it as opened by
    // a password (`local_accounts::open_session`) is not in the bundle a window holds, so
    // storing a bundle over it ends it at the next check. Nothing handed over replaces it:
    // neither the local session itself coming back, nor a cloud one while this PC is still in
    // its network. Such a bundle is answered and left aside rather than refused, since the
    // windows that send it have nothing to do about a refusal. Going online leaves the network
    // first (`promote_inner`), so its cloud session is stored like any other.
    let local_bundle = user.get("auth_mode").and_then(|m| m.as_str()) == Some("local");
    if local_bundle || crate::local_server::local_accounts::is_standalone(&state.db).await {
        return Ok(());
    }
    // Tokens belong in the OS keychain. When it cannot keep them (no usable
    // credential store) they are kept in the local database instead: losing
    // them silently would switch cloud sync and bootstrap off without a word.
    let tokens_json = match crate::local_server::secrets::store_tokens(&bundle.to_string()) {
        Ok(()) => "{}".to_string(),
        Err(e) => {
            tracing::warn!(error = %e, "OS keychain unavailable; keeping the cloud session in the local database");
            bundle.to_string()
        }
    };
    sqlx::query(
        r#"
        INSERT INTO cached_session (id, user_json, tokens_json, cached_at)
        VALUES (1, $1, $3, $2)
        ON CONFLICT(id) DO UPDATE SET user_json = $1, tokens_json = $3, cached_at = $2
        "#,
    )
    .bind(user.to_string())
    .bind(Utc::now().to_rfc3339())
    .bind(tokens_json)
    .execute(&state.db)
    .await?;

    // Persist the company id so the mesh can advertise its fingerprint, then
    // refresh the company mesh secret + identity/logo cache in the background.
    if let Some(company_id) = user.get("company_id").and_then(|v| v.as_str()) {
        let _ = sqlx::query("UPDATE sync_state SET company_id = $1 WHERE id = 1")
            .bind(company_id)
            .execute(&state.db)
            .await;
    }
    state.reload_company().await;

    let refresh_state = state.clone();
    tokio::spawn(async move {
        if let Err(e) = crate::local_server::company::refresh_company(&refresh_state).await {
            tracing::debug!(error = %e, "company refresh after login failed");
        }
    });

    Ok(())
}

/// Store session from cloud login (frontend calls cloud directly, then syncs here).
/// A local session, or any session on a local network, is answered without being stored.
async fn sync_session(
    State(state): State<SharedState>,
    Json(bundle): Json<serde_json::Value>,
) -> LocalResult<Json<serde_json::Value>> {
    cache_session(&state, &bundle).await?;
    Ok(Json(bundle))
}

async fn login(
    State(state): State<SharedState>,
    Json(body): Json<LoginInput>,
) -> LocalResult<Json<serde_json::Value>> {
    let (_, json) = proxy_post(
        &state,
        "/api/auth/login",
        serde_json::json!({ "email": body.email, "password": body.password }),
    )
    .await?;
    cache_session(&state, &json).await?;
    Ok(Json(json))
}

async fn register(
    State(state): State<SharedState>,
    Json(body): Json<RegisterInput>,
) -> LocalResult<(StatusCode, Json<serde_json::Value>)> {
    let (status, json) = proxy_post(
        &state,
        "/api/auth/register",
        serde_json::json!({
            "email": body.email,
            "password": body.password,
            "full_name": body.full_name,
        }),
    )
    .await?;
    cache_session(&state, &json).await?;
    Ok((status, Json(json)))
}

async fn refresh(
    State(state): State<SharedState>,
    Json(body): Json<RefreshInput>,
) -> LocalResult<Json<serde_json::Value>> {
    let (_, json) = proxy_post(
        &state,
        "/api/auth/refresh",
        serde_json::json!({ "refresh_token": body.refresh_token }),
    )
    .await?;
    cache_session(&state, &json).await?;
    Ok(Json(json))
}

async fn logout(State(state): State<SharedState>) -> LocalResult<StatusCode> {
    let local_network = crate::local_server::local_accounts::is_standalone(&state.db).await;
    crate::local_server::secrets::clear_on_sign_out(local_network);
    let _ = sqlx::query("DELETE FROM cached_session WHERE id = 1")
        .execute(&state.db)
        .await;
    Ok(StatusCode::NO_CONTENT)
}

// ---- Standalone mode ------------------------------------------------------------------

/// What the sign-in screen needs to know before anyone is signed in.
async fn local_status(
    State(state): State<SharedState>,
) -> LocalResult<Json<crate::local_server::local_accounts::LocalStatus>> {
    Ok(Json(crate::local_server::local_accounts::status(&state.db).await?))
}

#[derive(Debug, Deserialize)]
struct LocalSetupInput {
    username: String,
    password: String,
    confirm: String,
    /// Create a new network although another one is visible on the LAN.
    #[serde(default)]
    force: bool,
}

/// The first run: create the shop's network by creating its first administrator, with no cloud.
async fn local_setup(
    State(state): State<SharedState>,
    Json(input): Json<LocalSetupInput>,
) -> LocalResult<(StatusCode, Json<serde_json::Value>)> {
    if !input.force {
        let seen = crate::local_server::mesh::join_candidates(&state).await;
        if !seen.is_empty() {
            return Err(LocalError::NetworkExists(format!(
                "Un réseau local Motodesk est visible ({} poste(s)). Si votre atelier en a déjà un, connectez-vous pour le rejoindre ; sinon confirmez la création d'un nouveau réseau.",
                seen.len()
            )));
        }
    }
    let bundle = crate::local_server::local_accounts::setup(
        &state,
        &input.username,
        &input.password,
        &input.confirm,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(bundle)))
}

#[derive(Debug, Deserialize)]
struct LocalLoginInput {
    username: String,
    password: String,
}

/// Sign in to the network. A PC that already belongs to it checks the account it holds; a PC
/// that has never joined looks for a PC of the network and joins it with the same credentials.
async fn local_login(
    State(state): State<SharedState>,
    Json(input): Json<LocalLoginInput>,
) -> LocalResult<Json<serde_json::Value>> {
    use crate::local_server::{local_accounts, mesh};
    if local_accounts::status(&state.db).await?.can_setup {
        let candidates = mesh::join_candidates(&state).await;
        let bundle =
            mesh::join::join_network(&state, &input.username, &input.password, &candidates).await?;
        return Ok(Json(bundle));
    }
    Ok(Json(local_accounts::login(&state, &input.username, &input.password).await?))
}

/// "Password lost": the network's recovery key gives one account a new password.
async fn local_recover(
    State(state): State<SharedState>,
    Json(input): Json<crate::local_server::recovery_key::Recover>,
) -> LocalResult<Json<serde_json::Value>> {
    crate::local_server::recovery_key::recover(&state, input).await.map(Json)
}

/// A PC left with an old network key: an administrator's password gets it the current one.
async fn local_reconnect(
    State(state): State<SharedState>,
    Json(input): Json<LocalLoginInput>,
) -> LocalResult<StatusCode> {
    let candidates = crate::local_server::mesh::join_candidates(&state).await;
    crate::local_server::network_devices::reconnect(&state, &input.username, &input.password, &candidates).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
struct FirstRunRestore {
    path: String,
    password: String,
}

/// Bring the shop back from a backup on a PC with nothing set up. No session to ask for: there
/// is none to be had, and nothing here to protect. Anywhere else, restoring is an
/// administrator's (`routes::backup`).
async fn local_restore(
    State(state): State<SharedState>,
    Json(input): Json<FirstRunRestore>,
) -> LocalResult<Json<crate::local_server::backup::RestoreResult>> {
    if !crate::local_server::local_accounts::status(&state.db).await?.can_setup {
        return Err(LocalError::Conflict(
            "Ce poste est déjà configuré : la restauration se fait dans Réglages › Sauvegardes.".into(),
        ));
    }
    let path = std::path::PathBuf::from(input.path);
    crate::local_server::backup::restore_path(&state, &path, &input.password).await.map(Json)
}

#[derive(Debug, Deserialize)]
pub(crate) struct PromoteInput {
    pub email: String,
    pub password: String,
    pub confirm: String,
    pub full_name: Option<String>,
    pub company_name: Option<String>,
}

/// Put a standalone installation online: register its company on the cloud under the ids
/// it already has, then let the normal sync upload everything.
async fn promote(
    State(state): State<SharedState>,
    Json(input): Json<PromoteInput>,
) -> LocalResult<Json<serde_json::Value>> {
    promote_inner(&state, input).await.map(Json)
}

pub(crate) async fn promote_inner(
    state: &SharedState,
    input: PromoteInput,
) -> LocalResult<serde_json::Value> {
    use crate::local_server::local_accounts;

    crate::local_server::auth_guard::require_admin(state).await?;
    if !local_accounts::is_standalone(&state.db).await {
        return Err(LocalError::Conflict("Cette installation est déjà en ligne.".into()));
    }
    let email = input.email.trim().to_string();
    if !email.contains('@') || email.len() < 5 {
        return Err(LocalError::BadRequest("Saisissez une adresse e-mail valide.".into()));
    }
    local_accounts::check_new_password(&input.password, &input.confirm)?;

    let (user_json,): (String,) = sqlx::query_as("SELECT user_json FROM cached_session WHERE id = 1")
        .fetch_one(&state.db)
        .await?;
    let user: serde_json::Value = serde_json::from_str(&user_json).unwrap_or_default();
    let (Some(user_id), Some(company_id)) = (
        user.get("id").and_then(|v| v.as_str()).map(str::to_string),
        user.get("company_id").and_then(|v| v.as_str()).map(str::to_string),
    ) else {
        return Err(LocalError::Internal(anyhow::anyhow!("the local session has no ids")));
    };
    let username = user.get("username").and_then(|v| v.as_str()).unwrap_or("");

    let cached_name: Option<String> = sqlx::query_scalar(
        "SELECT COALESCE(NULLIF(TRIM(name), ''), NULLIF(TRIM(legal_name), '')) FROM company_cache WHERE id = 1",
    )
    .fetch_optional(&state.db)
    .await?
    .flatten();
    let company_name = input
        .company_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or(cached_name)
        .unwrap_or_else(|| "Mon atelier".to_string());
    let full_name = input
        .full_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(username)
        .to_string();

    let base = crate::local_server::cloud::cloud_base_url(state).await;
    let res = state
        .http
        .post(format!("{base}/api/auth/register"))
        .json(&serde_json::json!({
            "email": email,
            "password": input.password,
            "full_name": full_name,
            "company_name": company_name,
            // The company and its admin keep the ids this PC's data already refers to.
            "company_id": company_id,
            "user_id": user_id,
        }))
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, url = %base, "promotion: cloud unreachable");
            LocalError::BadRequest(format!(
                "Impossible de joindre le serveur cloud ({base}). Vérifiez votre connexion internet et l'adresse du serveur."
            ))
        })?;
    if !res.status().is_success() {
        let status = res.status();
        let body: serde_json::Value = res.json().await.unwrap_or_default();
        let message = body
            .pointer("/error/message")
            .and_then(|m| m.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| format!("Le cloud a refusé la création (HTTP {status})."));
        tracing::warn!(%status, %message, "promotion refused by the cloud");
        return Err(LocalError::BadRequest(match status.as_u16() {
            403 => format!(
                "{message} Les inscriptions sont fermées sur ce cloud : demandez à son administrateur de les ouvrir le temps de la mise en ligne."
            ),
            _ => message,
        }));
    }
    let bundle: serde_json::Value = res.json().await.map_err(|e| LocalError::Internal(e.into()))?;

    // From here on the cloud knows this company: the installation is online, whatever
    // else goes wrong, and signing in with the new account always works. Leaving the network
    // comes before storing the session: `cache_session` stores none on a local network.
    sqlx::query("UPDATE sync_state SET standalone = 0 WHERE id = 1")
        .execute(&state.db)
        .await?;
    crate::local_server::routes::background::refresh(state).await;
    if let Err(e) = cache_session(state, &bundle).await {
        tracing::warn!(error = %e, "promotion: the cloud session could not be stored; sign in again");
    }
    if let Some(token) = bundle.get("access_token").and_then(|t| t.as_str()) {
        push_identity(state, &base, token).await;
    }
    state.notify_sync();
    Ok(bundle)
}

/// Send the company identity entered offline (legal name, logo) to the cloud.
/// Best-effort: it can be entered again in Réglages.
async fn push_identity(state: &SharedState, base: &str, token: &str) {
    type Identity = (Option<String>, Option<String>, Option<Vec<u8>>, Option<String>);
    let row: Option<Identity> = sqlx::query_as(
        "SELECT name, legal_name, logo_bytes, logo_content_type FROM company_cache WHERE id = 1",
    )
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    let Some((name, legal_name, logo, content_type)) = row else { return };
    let mut body = serde_json::Map::new();
    if let Some(n) = name.filter(|n| !n.trim().is_empty()) {
        body.insert("name".into(), n.into());
    }
    if let Some(l) = legal_name.filter(|l| !l.trim().is_empty()) {
        body.insert("legal_name".into(), l.into());
    }
    if !body.is_empty() {
        let _ = state
            .http
            .patch(format!("{base}/api/company"))
            .bearer_auth(token)
            .json(&body)
            .send()
            .await;
    }
    if let Some(bytes) = logo {
        let _ = state
            .http
            .put(format!("{base}/api/company/logo"))
            .bearer_auth(token)
            .header("Content-Type", content_type.unwrap_or_else(|| "image/png".into()))
            .body(bytes)
            .send()
            .await;
    }
}

async fn me(State(state): State<SharedState>) -> LocalResult<Json<serde_json::Value>> {
    crate::local_server::local_accounts::session_user(&state.db)
        .await?
        .map(Json)
        .ok_or(LocalError::Unauthorized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_server::local_accounts;
    use crate::local_server::test_support::shop;
    use axum::routing::{patch, put};
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    /// A stand-in for the cloud: records what registration it is sent and answers.
    async fn mock_cloud(register_status: u16) -> (String, Arc<Mutex<Vec<serde_json::Value>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let recorder = seen.clone();
        let app = Router::new()
            .route(
                "/api/auth/register",
                post(move |Json(body): Json<serde_json::Value>| {
                    let recorder = recorder.clone();
                    async move {
                        recorder.lock().unwrap().push(body.clone());
                        if register_status != 201 {
                            return (
                                StatusCode::from_u16(register_status).unwrap(),
                                Json(json!({"error": {"code": "registration_closed", "message": "Les inscriptions sont fermées."}})),
                            );
                        }
                        (
                            StatusCode::CREATED,
                            Json(json!({
                                "access_token": "cloud-token",
                                "access_expires_in": 900,
                                "refresh_token": "cloud-refresh",
                                "refresh_expires_in": 604800,
                                "user": {
                                    "id": body["user_id"],
                                    "company_id": body["company_id"],
                                    "email": body["email"],
                                    "full_name": body["full_name"],
                                    "role": "admin",
                                    "is_active": true,
                                    "created_at": "2026-11-21T10:00:00Z",
                                },
                            })),
                        )
                    }
                }),
            )
            .route("/api/company", patch(|| async { StatusCode::OK }))
            .route("/api/company/logo", put(|| async { StatusCode::OK }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{addr}"), seen)
    }

    async fn standalone_shop(cloud: &str) -> SharedState {
        let state = shop().await;
        local_accounts::setup(&state, "camille", "mot de passe", "mot de passe").await.unwrap();
        sqlx::query("UPDATE sync_state SET cloud_url = $1 WHERE id = 1")
            .bind(cloud)
            .execute(&state.db)
            .await
            .unwrap();
        state
    }

    fn promotion() -> PromoteInput {
        PromoteInput {
            email: "camille@example.fr".into(),
            password: "un autre mot de passe".into(),
            confirm: "un autre mot de passe".into(),
            full_name: Some("Camille Martin".into()),
            company_name: None,
        }
    }

    #[tokio::test]
    async fn going_online_registers_the_company_under_its_own_ids_and_switches_the_install() {
        let (cloud, seen) = mock_cloud(201).await;
        let state = standalone_shop(&cloud).await;
        let (session,): (String,) = sqlx::query_as("SELECT user_json FROM cached_session WHERE id = 1")
            .fetch_one(&state.db)
            .await
            .unwrap();
        let session: serde_json::Value = serde_json::from_str(&session).unwrap();

        let bundle = promote_inner(&state, promotion()).await.unwrap();
        assert_eq!(bundle["access_token"], "cloud-token");

        // What the cloud was asked to create: this PC's own ids, so the rows and the
        // fiscal journal already written keep pointing at something that exists.
        let sent = seen.lock().unwrap()[0].clone();
        assert_eq!(sent["company_id"], session["company_id"]);
        assert_eq!(sent["user_id"], session["id"]);
        assert_eq!(sent["company_name"], "Garage Dupont", "the name entered offline travels");

        // The install is online now: no local sign-in, a cloud token, a cloud session.
        let status = local_accounts::status(&state.db).await.unwrap();
        assert!(!status.standalone);
        assert!(status.has_account, "the local account is kept, but no longer used");
        let again = local_accounts::login(&state, "camille", "mot de passe").await;
        assert!(matches!(again, Err(LocalError::Conflict(_))), "{again:?}");
        let token = crate::local_server::secrets::session_access_token(&state.db).await.unwrap();
        assert_eq!(token.as_deref(), Some("cloud-token"));
        let (now,): (String,) = sqlx::query_as("SELECT user_json FROM cached_session WHERE id = 1")
            .fetch_one(&state.db)
            .await
            .unwrap();
        let now: serde_json::Value = serde_json::from_str(&now).unwrap();
        assert_eq!(now["email"], "camille@example.fr");
        assert_eq!(now["id"], session["id"], "the same person, for the journal's sake");
        assert!(now.get("auth_mode").is_none());
    }

    #[tokio::test]
    async fn a_cloud_that_refuses_leaves_the_install_standalone() {
        let (cloud, _) = mock_cloud(403).await;
        let state = standalone_shop(&cloud).await;
        let refused = promote_inner(&state, promotion()).await.unwrap_err();
        match refused {
            LocalError::BadRequest(message) => {
                assert!(message.contains("fermées"), "{message}");
                assert!(message.contains("administrateur"), "tells what to do: {message}");
            }
            other => panic!("{other:?}"),
        }
        assert!(local_accounts::status(&state.db).await.unwrap().standalone);
        assert!(crate::local_server::auth_guard::is_admin(&state).await, "still signed in locally");
    }

    #[tokio::test]
    async fn an_unreachable_cloud_leaves_the_install_standalone() {
        let state = standalone_shop("http://127.0.0.1:9").await;
        let refused = promote_inner(&state, promotion()).await.unwrap_err();
        match refused {
            LocalError::BadRequest(message) => assert!(message.contains("joindre"), "{message}"),
            other => panic!("{other:?}"),
        }
        assert!(local_accounts::status(&state.db).await.unwrap().standalone);
    }

    #[tokio::test]
    async fn signing_out_ends_the_session_and_leaves_the_pc_in_its_network() {
        let state = standalone_shop("http://127.0.0.1:9").await;
        let secret = state.mesh_secret.read().await.clone();
        assert!(secret.is_some());

        logout(State(state.clone())).await.unwrap();

        assert!(me(State(state.clone())).await.is_err(), "nobody is signed in");
        assert_eq!(*state.mesh_secret.read().await, secret);
        let stored = crate::local_server::db::load_mesh_secret(&state.db).await.unwrap();
        assert_eq!(stored, secret, "and the next start finds it");
        assert!(local_accounts::status(&state.db).await.unwrap().standalone);
    }

    #[tokio::test]
    async fn handing_a_local_session_back_does_not_end_it() {
        let state = standalone_shop("http://127.0.0.1:9").await;
        let bundle = local_accounts::login(&state, "camille", "mot de passe").await.unwrap();
        assert_eq!(bundle["user"]["auth_mode"], "local");

        // What "Synchroniser" does on a local network: the window posts the session it holds.
        let Json(answer) = sync_session(State(state.clone()), Json(bundle.clone())).await.unwrap();
        assert_eq!(answer, bundle, "answered as before, for the windows that still send it");

        let user = local_accounts::session_user(&state.db).await.unwrap().expect("still signed in");
        assert_eq!(user["username"], "camille");
        assert!(me(State(state.clone())).await.is_ok());
        assert!(crate::local_server::auth_guard::is_admin(&state).await);
    }

    #[tokio::test]
    async fn a_cloud_session_does_not_replace_the_session_of_a_local_network() {
        let state = standalone_shop("http://127.0.0.1:9").await;
        let local = local_accounts::login(&state, "camille", "mot de passe").await.unwrap();
        let cloud = |company: &serde_json::Value| {
            json!({
                "access_token": "cloud-token",
                "access_expires_in": 900,
                "refresh_token": "cloud-refresh",
                "refresh_expires_in": 604800,
                "user": {
                    "id": local["user"]["id"],
                    "company_id": company,
                    "email": "camille@example.fr",
                    "full_name": "Camille Martin",
                    "role": "admin",
                    "is_active": true,
                },
            })
        };

        let _ = sync_session(State(state.clone()), Json(cloud(&local["user"]["company_id"]))).await.unwrap();
        let user = local_accounts::session_user(&state.db).await.unwrap().expect("still signed in");
        assert_eq!(user["auth_mode"], "local");
        let token = crate::local_server::secrets::session_access_token(&state.db).await.unwrap();
        assert_eq!(token, None, "a local network holds no cloud token");

        // Another company's account is refused, as on any PC: the window closes that session.
        let other = json!(uuid::Uuid::new_v4().to_string());
        let refused = sync_session(State(state.clone()), Json(cloud(&other))).await;
        assert!(matches!(refused, Err(LocalError::WrongCompany(_))), "{refused:?}");
        assert!(local_accounts::session_user(&state.db).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn a_local_session_left_in_a_window_does_not_replace_the_cloud_one() {
        let (cloud, _) = mock_cloud(201).await;
        let state = standalone_shop(&cloud).await;
        let local = local_accounts::login(&state, "camille", "mot de passe").await.unwrap();
        promote_inner(&state, promotion()).await.unwrap();

        let _ = sync_session(State(state.clone()), Json(local)).await.unwrap();

        let token = crate::local_server::secrets::session_access_token(&state.db).await.unwrap();
        assert_eq!(token.as_deref(), Some("cloud-token"), "cloud sync keeps its token");
        let user = local_accounts::session_user(&state.db).await.unwrap().unwrap();
        assert_eq!(user["email"], "camille@example.fr");
        assert!(user.get("auth_mode").is_none());
    }

    #[tokio::test]
    async fn only_a_pc_with_nothing_set_up_restores_without_a_session() {
        let old = standalone_shop("http://127.0.0.1:9").await;
        let file = crate::local_server::backup::export(&old, "un-mot-de-passe-solide").await.unwrap();
        let ask = |path: &str| {
            Json(FirstRunRestore { path: path.into(), password: "un-mot-de-passe-solide".into() })
        };

        let refused = local_restore(State(old.clone()), ask(&file.path)).await;
        assert!(matches!(refused, Err(LocalError::Conflict(_))), "a set-up PC asks for an administrator");

        let fresh = shop().await;
        let Json(done) = local_restore(State(fresh.clone()), ask(&file.path)).await.unwrap();
        assert!(done.restart_required);
    }

    #[tokio::test]
    async fn going_online_needs_a_matching_password_and_a_standalone_admin() {
        let (cloud, seen) = mock_cloud(201).await;
        let state = standalone_shop(&cloud).await;

        let mut mismatch = promotion();
        mismatch.confirm = "autre chose".into();
        assert!(matches!(promote_inner(&state, mismatch).await, Err(LocalError::BadRequest(_))));
        let mut no_email = promotion();
        no_email.email = "pas un email".into();
        assert!(matches!(promote_inner(&state, no_email).await, Err(LocalError::BadRequest(_))));
        assert!(seen.lock().unwrap().is_empty(), "nothing reached the cloud");

        promote_inner(&state, promotion()).await.unwrap();
        let twice = promote_inner(&state, promotion()).await;
        assert!(matches!(twice, Err(LocalError::Conflict(_))), "{twice:?}");
    }
}
