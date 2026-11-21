//! Local (offline-capable) company identity + logo, served from the SQLite
//! cache populated after login by `local_server::company::refresh_company`.
//! On an online installation writes are not handled here: the frontend sends them straight
//! to the cloud (admin-only) and then triggers a refresh. A standalone installation has no
//! cloud, so its identity is written here, into the same cache the cloud would fill.

use axum::{
    body::Bytes,
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};

use crate::local_server::error::{LocalError, LocalResult};
use crate::local_server::state::SharedState;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/api/company", get(get_company).patch(update_company))
        .route("/api/company/logo", get(get_logo).put(put_logo))
        .route("/api/company/refresh", post(refresh))
}

async fn get_company(State(state): State<SharedState>) -> LocalResult<Json<Value>> {
    company_json(&state).await.map(Json)
}

const MAX_LOGO_BYTES: usize = 1024 * 1024; // the cloud's limit too

/// Only a standalone installation's administrator writes the identity here.
async fn require_standalone_admin(state: &SharedState) -> LocalResult<()> {
    crate::local_server::auth_guard::require_admin(state).await?;
    if !crate::local_server::local_accounts::is_standalone(&state.db).await {
        return Err(LocalError::Conflict(
            "L'identité de l'entreprise est gérée par le cloud.".into(),
        ));
    }
    Ok(())
}

#[derive(Debug, serde::Deserialize)]
struct UpdateCompany {
    name: Option<String>,
    legal_name: Option<String>,
}

async fn update_company(
    State(state): State<SharedState>,
    Json(body): Json<UpdateCompany>,
) -> LocalResult<Json<Value>> {
    require_standalone_admin(&state).await?;
    let name = match body.name.as_deref().map(str::trim) {
        Some("") => return Err(LocalError::BadRequest("Le nom de l'entreprise est obligatoire.".into())),
        Some(n) if n.chars().count() > 200 => {
            return Err(LocalError::BadRequest("Le nom est limité à 200 caractères.".into()))
        }
        other => other.map(str::to_string),
    };
    let legal = body.legal_name.as_deref().map(str::trim).map(str::to_string);
    if legal.as_deref().is_some_and(|l| l.chars().count() > 200) {
        return Err(LocalError::BadRequest("La dénomination est limitée à 200 caractères.".into()));
    }
    let company_id: Option<String> = sqlx::query_scalar("SELECT company_id FROM sync_state WHERE id = 1")
        .fetch_optional(&state.db)
        .await?
        .flatten();
    sqlx::query(
        r#"
        INSERT INTO company_cache (id, company_id, name, legal_name, cached_at)
        VALUES (1, $1, $2, NULLIF($3, ''), $4)
        ON CONFLICT(id) DO UPDATE SET
            company_id = COALESCE(company_id, $1),
            name       = COALESCE($2, name),
            legal_name = CASE WHEN $5 THEN NULLIF($3, '') ELSE legal_name END,
            cached_at  = $4
        "#,
    )
    .bind(company_id)
    .bind(name)
    .bind(legal.clone().unwrap_or_default())
    .bind(chrono::Utc::now().to_rfc3339())
    .bind(legal.is_some())
    .execute(&state.db)
    .await?;
    company_json(&state).await.map(Json)
}

async fn put_logo(
    State(state): State<SharedState>,
    headers: HeaderMap,
    body: Bytes,
) -> LocalResult<Json<Value>> {
    require_standalone_admin(&state).await?;
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .filter(|ct| ct.starts_with("image/"))
        .ok_or_else(|| LocalError::BadRequest("Le logo doit être une image.".into()))?
        .to_string();
    if body.is_empty() || body.len() > MAX_LOGO_BYTES {
        return Err(LocalError::BadRequest("Le logo doit faire au plus 1 Mo.".into()));
    }
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        r#"
        INSERT INTO company_cache (id, logo_bytes, logo_content_type, logo_etag, logo_updated_at, cached_at)
        VALUES (1, $1, $2, $3, $4, $4)
        ON CONFLICT(id) DO UPDATE SET
            logo_bytes = $1, logo_content_type = $2, logo_etag = $3,
            logo_updated_at = $4, cached_at = $4
        "#,
    )
    .bind(body.to_vec())
    .bind(content_type)
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(now)
    .execute(&state.db)
    .await?;
    company_json(&state).await.map(Json)
}

async fn company_json(state: &SharedState) -> LocalResult<Value> {
    let row: Option<(
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    )> = sqlx::query_as(
        "SELECT company_id, name, legal_name, logo_updated_at FROM company_cache WHERE id = 1",
    )
    .fetch_optional(&state.db)
    .await?;

    match row {
        Some((id, name, legal_name, logo_updated_at)) => Ok(json!({
            "id": id,
            "name": name,
            "legal_name": legal_name,
            "has_logo": logo_updated_at.is_some(),
            "logo_updated_at": logo_updated_at,
        })),
        None => Ok(json!({
            "id": null,
            "name": null,
            "legal_name": null,
            "has_logo": false,
            "logo_updated_at": null,
        })),
    }
}

async fn get_logo(State(state): State<SharedState>) -> LocalResult<Response> {
    let row: Option<(Option<Vec<u8>>, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT logo_bytes, logo_content_type, logo_updated_at FROM company_cache WHERE id = 1",
    )
    .fetch_optional(&state.db)
    .await?;

    let Some((Some(bytes), content_type, updated_at)) = row else {
        return Err(LocalError::NotFound);
    };

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        content_type
            .as_deref()
            .unwrap_or("image/png")
            .parse()
            .unwrap_or_else(|_| "image/png".parse().unwrap()),
    );
    if let Some(etag) = updated_at.and_then(|s| format!("\"{s}\"").parse().ok()) {
        headers.insert(header::ETAG, etag);
    }
    headers.insert(header::CACHE_CONTROL, "private, no-cache".parse().unwrap());

    Ok((StatusCode::OK, headers, bytes).into_response())
}

/// Force a re-fetch of the company identity, mesh secret, and logo from cloud.
async fn refresh(State(state): State<SharedState>) -> LocalResult<Json<Value>> {
    crate::local_server::company::refresh_company(&state).await?;
    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_server::local_accounts;
    use crate::local_server::test_support::shop;

    async fn standalone_shop() -> SharedState {
        let state = shop().await;
        local_accounts::setup(&state, "camille", "mot de passe", "mot de passe").await.unwrap();
        state
    }

    #[tokio::test]
    async fn a_standalone_admin_writes_the_company_identity_here() {
        let state = standalone_shop().await;
        let body = update_company(
            State(state.clone()),
            Json(UpdateCompany { name: Some("Atelier Exemple".into()), legal_name: Some("Atelier Exemple SARL".into()) }),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(body["name"], "Atelier Exemple");
        assert_eq!(body["legal_name"], "Atelier Exemple SARL");
        assert!(body["id"].is_string(), "the company id of the install");

        // An empty legal name clears it; an empty trade name is refused.
        let cleared = update_company(State(state.clone()), Json(UpdateCompany { name: None, legal_name: Some("".into()) }))
            .await
            .unwrap()
            .0;
        assert!(cleared["legal_name"].is_null());
        assert_eq!(cleared["name"], "Atelier Exemple", "an absent field is left alone");
        let blank = update_company(State(state), Json(UpdateCompany { name: Some("  ".into()), legal_name: None })).await;
        assert!(matches!(blank, Err(LocalError::BadRequest(_))));
    }

    #[tokio::test]
    async fn the_logo_is_stored_and_served_back() {
        let state = standalone_shop().await;
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_TYPE, "image/png".parse().unwrap());
        let body = put_logo(State(state.clone()), headers.clone(), Bytes::from_static(b"\x89PNGfake"))
            .await
            .unwrap()
            .0;
        assert_eq!(body["has_logo"], true);

        let served = get_logo(State(state.clone())).await.unwrap();
        assert_eq!(served.status(), StatusCode::OK);

        let mut not_image = HeaderMap::new();
        not_image.insert(header::CONTENT_TYPE, "text/html".parse().unwrap());
        assert!(put_logo(State(state.clone()), not_image, Bytes::from_static(b"<b>x</b>")).await.is_err());
        let too_big = put_logo(State(state), headers, Bytes::from(vec![0u8; MAX_LOGO_BYTES + 1])).await;
        assert!(matches!(too_big, Err(LocalError::BadRequest(_))));
    }

    #[tokio::test]
    async fn an_online_install_leaves_the_identity_to_the_cloud() {
        let state = standalone_shop().await;
        sqlx::query("UPDATE sync_state SET standalone = 0 WHERE id = 1").execute(&state.db).await.unwrap();
        let refused = update_company(State(state), Json(UpdateCompany { name: Some("X".into()), legal_name: None })).await;
        assert!(matches!(refused, Err(LocalError::Conflict(_))), "{refused:?}");
    }
}
