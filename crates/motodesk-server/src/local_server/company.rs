//! Company identity + logo offline cache.
//!
//! After login the desktop fetches the company's mesh secret (LAN trust root),
//! identity (name/legal name), and logo from the cloud, caching them in SQLite
//! so the UI can display them even while offline. The frontend always reads the
//! logo through the local API (`GET /api/company/logo`) which serves the cached
//! bytes.

use chrono::Utc;
use serde_json::Value;

use crate::local_server::error::LocalResult;
use crate::local_server::state::SharedState;

/// The company this PC's data belongs to, once it has been signed in to one.
async fn install_company(db: &sqlx::SqlitePool) -> LocalResult<Option<String>> {
    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT company_id FROM sync_state WHERE id = 1")
            .fetch_optional(db)
            .await?;
    Ok(row.and_then(|(c,)| c).filter(|c| !c.is_empty()))
}

/// Refuse an account from another company.
///
/// The local database is one company's books. Signing in with another company's
/// account used to keep that data on screen and, worse, send the first company's
/// queued changes to the second one's cloud with the new token. A PC that has no
/// company yet (first sign-in, or data entered offline) simply adopts the account's.
pub async fn ensure_same_company(
    db: &sqlx::SqlitePool,
    incoming_company: Option<&str>,
) -> LocalResult<()> {
    let (Some(mine), Some(theirs)) = (install_company(db).await?, incoming_company) else {
        return Ok(());
    };
    if mine.eq_ignore_ascii_case(theirs) {
        return Ok(());
    }
    Err(crate::local_server::error::LocalError::WrongCompany(
        "Ce poste contient les données d'une autre entreprise. Connectez-vous avec un compte de \
         cette entreprise. Pour utiliser une autre entreprise sur ce poste, sauvegardez puis \
         effacez d'abord les données locales."
            .into(),
    ))
}

/// True when the signed-in account belongs to another company than this PC's data.
/// Nothing may be sent to the cloud, or imported from it, under such a session.
pub async fn session_mismatches_install(db: &sqlx::SqlitePool) -> bool {
    let row: Option<(Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT json_extract(c.user_json, '$.company_id'), s.company_id
           FROM cached_session c, sync_state s WHERE c.id = 1 AND s.id = 1",
    )
    .fetch_optional(db)
    .await
    .ok()
    .flatten();
    match row {
        Some((Some(session), Some(install))) if !session.is_empty() && !install.is_empty() => {
            !session.eq_ignore_ascii_case(&install)
        }
        _ => false,
    }
}

/// Fetch the mesh secret, company identity, and logo from the cloud and cache
/// them locally. Best-effort: any individual step may fail offline.
pub async fn refresh_company(state: &SharedState) -> LocalResult<()> {
    let Some(token) = crate::local_server::secrets::session_access_token(&state.db).await? else {
        return Ok(());
    };
    let base = crate::local_server::cloud::cloud_base_url(state).await;

    // 1. Mesh secret (LAN trust root) + company id.
    if let Ok(res) = state
        .http
        .get(format!("{base}/api/company/mesh-secret"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
    {
        if res.status().is_success() {
            if let Ok(v) = res.json::<Value>().await {
                if let Some(secret) = v.get("mesh_secret").and_then(|s| s.as_str()) {
                    match crate::local_server::secrets::store_mesh_secret(secret) {
                        Ok(()) => {
                            let _ = sqlx::query("UPDATE sync_state SET mesh_secret = NULL WHERE id = 1")
                                .execute(&state.db)
                                .await;
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "OS keychain unavailable; keeping the mesh secret in the local database");
                            let _ = sqlx::query("UPDATE sync_state SET mesh_secret = $1 WHERE id = 1")
                                .bind(secret)
                                .execute(&state.db)
                                .await;
                        }
                    }
                }
                if let Some(cid) = v.get("company_id").and_then(|s| s.as_str()) {
                    let _ = sqlx::query("UPDATE sync_state SET company_id = $1 WHERE id = 1")
                        .bind(cid)
                        .execute(&state.db)
                        .await;
                }
            }
        }
    }

    // 2. Company identity.
    if let Ok(res) = state
        .http
        .get(format!("{base}/api/company"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
    {
        if res.status().is_success() {
            if let Ok(v) = res.json::<Value>().await {
                let id = v.get("id").and_then(|s| s.as_str());
                let name = v.get("name").and_then(|s| s.as_str());
                let legal = v.get("legal_name").and_then(|s| s.as_str());
                let _ = sqlx::query(
                    r#"
                    INSERT INTO company_cache (id, company_id, name, legal_name, cached_at)
                    VALUES (1, $1, $2, $3, $4)
                    ON CONFLICT(id) DO UPDATE SET
                        company_id = $1, name = $2, legal_name = $3, cached_at = $4
                    "#,
                )
                .bind(id)
                .bind(name)
                .bind(legal)
                .bind(Utc::now().to_rfc3339())
                .execute(&state.db)
                .await;
            }
        }
    }

    // 3. Logo (ETag-aware: skip the download if unchanged).
    refresh_logo(state, &base, &token).await;

    state.reload_company().await;
    Ok(())
}

async fn refresh_logo(state: &SharedState, base: &str, token: &str) {
    let current_etag: Option<String> =
        sqlx::query_scalar("SELECT logo_etag FROM company_cache WHERE id = 1")
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();

    let mut req = state
        .http
        .get(format!("{base}/api/company/logo"))
        .header("Authorization", format!("Bearer {token}"));
    if let Some(etag) = current_etag.as_ref() {
        req = req.header("If-None-Match", etag);
    }

    let res = match req.send().await {
        Ok(r) => r,
        Err(_) => return,
    };

    if res.status().as_u16() == 304 {
        return; // unchanged
    }
    if !res.status().is_success() {
        return;
    }

    let etag = res
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let content_type = res
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("image/png")
        .to_string();

    let bytes = match res.bytes().await {
        Ok(b) => b.to_vec(),
        Err(_) => return,
    };

    let _ = sqlx::query(
        r#"
        INSERT INTO company_cache (id, logo_bytes, logo_content_type, logo_etag, logo_updated_at, cached_at)
        VALUES (1, $1, $2, $3, $4, $4)
        ON CONFLICT(id) DO UPDATE SET
            logo_bytes = $1, logo_content_type = $2, logo_etag = $3,
            logo_updated_at = $4, cached_at = $4
        "#,
    )
    .bind(bytes)
    .bind(content_type)
    .bind(etag)
    .bind(Utc::now().to_rfc3339())
    .execute(&state.db)
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_server::error::LocalError;
    use crate::local_server::test_support::shop;
    use sqlx::SqlitePool;

    const A: &str = "11111111-1111-1111-1111-111111111111";
    const B: &str = "22222222-2222-2222-2222-222222222222";

    async fn install_for(db: &SqlitePool, company: Option<&str>) {
        sqlx::query("UPDATE sync_state SET company_id = $1 WHERE id = 1")
            .bind(company)
            .execute(db)
            .await
            .unwrap();
    }

    async fn signed_in_as(db: &SqlitePool, company: &str) {
        sqlx::query(
            "INSERT INTO cached_session (id, user_json, tokens_json, cached_at)
             VALUES (1, $1, $2, '2026-11-15T00:00:00Z')
             ON CONFLICT(id) DO UPDATE SET user_json = $1, tokens_json = $2",
        )
        .bind(format!(r#"{{"company_id":"{company}"}}"#))
        .bind(r#"{"access_token":"token-of-someone-else"}"#)
        .execute(db)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn an_account_from_another_company_does_not_get_in() {
        let state = shop().await;
        install_for(&state.db, Some(A)).await;

        let refused = ensure_same_company(&state.db, Some(B)).await.unwrap_err();
        assert!(matches!(refused, LocalError::WrongCompany(_)), "{refused:?}");
        assert!(ensure_same_company(&state.db, Some(A)).await.is_ok());
        assert!(
            ensure_same_company(&state.db, Some(&A.to_uppercase())).await.is_ok(),
            "ids are compared as ids, not as spelled"
        );

        // A PC with no company yet adopts the first account that signs in.
        install_for(&state.db, None).await;
        assert!(ensure_same_company(&state.db, Some(B)).await.is_ok());
    }

    #[tokio::test]
    async fn a_session_for_another_company_gets_no_cloud_access() {
        let state = shop().await;
        install_for(&state.db, Some(A)).await;
        signed_in_as(&state.db, B).await;

        assert!(session_mismatches_install(&state.db).await);
        // The token is there, and is still not handed out: this is what stops the
        // first company's queued changes being pushed to the second one's cloud.
        let token = crate::local_server::secrets::session_access_token(&state.db)
            .await
            .unwrap();
        assert_eq!(token, None);

        signed_in_as(&state.db, A).await;
        assert!(!session_mismatches_install(&state.db).await);
        install_for(&state.db, None).await;
        signed_in_as(&state.db, B).await;
        assert!(!session_mismatches_install(&state.db).await, "nothing to mismatch yet");
    }
}
