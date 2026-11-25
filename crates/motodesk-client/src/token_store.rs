//! Where the session is kept between two runs (port of `desktop/src/lib/api/tokenStore.ts`).
//!
//! The same file and key as the Svelte interface (`auth.store.json` in the app's data folder,
//! written there by Tauri's store plugin), so a PC that was signed in stays signed in when it
//! moves to the native interface, and back.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::types::TokenBundle;

const FILE: &str = "auth.store.json";
const KEY: &str = "auth.tokens.v1";

#[derive(Debug, Clone)]
pub struct TokenStore {
    path: PathBuf,
}

impl TokenStore {
    /// `data_dir` is the app's data folder (`%APPDATA%\com.motodesk.app`).
    pub fn in_dir(data_dir: &Path) -> Self {
        Self { path: data_dir.join(FILE) }
    }

    fn read(&self) -> Map<String, Value> {
        std::fs::read(&self.path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Map<String, Value>>(&bytes).ok())
            .unwrap_or_default()
    }

    /// Replace the file in one step, so a crash mid-write cannot leave half a session.
    fn write(&self, map: &Map<String, Value>) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec(map).unwrap_or_default())?;
        std::fs::rename(&tmp, &self.path)
    }

    pub fn load(&self) -> Option<TokenBundle> {
        self.read().remove(KEY).and_then(|v| serde_json::from_value(v).ok())
    }

    pub fn save(&self, tokens: &TokenBundle) {
        let mut map = self.read();
        map.insert(KEY.into(), serde_json::to_value(tokens).unwrap_or(Value::Null));
        if let Err(e) = self.write(&map) {
            // The session is in memory; only the next start will ask to sign in again.
            tracing::warn!(error = %e, "could not save the session");
        }
    }

    pub fn clear(&self) {
        let mut map = self.read();
        if map.remove(KEY).is_some() {
            if let Err(e) = self.write(&map) {
                tracing::warn!(error = %e, "could not forget the session");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundle() -> TokenBundle {
        serde_json::from_value(serde_json::json!({
            "access_token": "a", "access_expires_in": 900, "refresh_token": "r", "refresh_expires_in": 60,
            "user": { "id": "u", "email": "", "full_name": "Léa", "role": "admin", "created_at": "" }
        }))
        .unwrap()
    }

    #[test]
    fn a_session_is_found_again_and_other_keys_of_the_file_are_left_alone() {
        let dir = std::env::temp_dir().join(format!("motodesk_tokens_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(FILE), br#"{"something.else":42}"#).unwrap();

        let store = TokenStore::in_dir(&dir);
        assert!(store.load().is_none());
        store.save(&bundle());
        assert_eq!(store.load(), Some(bundle()));

        store.clear();
        assert!(store.load().is_none());
        let left: Value = serde_json::from_slice(&std::fs::read(dir.join(FILE)).unwrap()).unwrap();
        assert_eq!(left, serde_json::json!({ "something.else": 42 }));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
