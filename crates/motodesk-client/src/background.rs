//! Keep Motodesk running when its window is closed, and start it with Windows, so that the PC
//! stays in the shop's network (port of `desktop/src/lib/api/background.ts`).
//!
//! The service holds the choice and decides what applies; carrying it out (hiding the window,
//! the icon near the clock, the start-up entry) is the app's part.

use serde::Deserialize;

use crate::error::ApiResult;
use crate::session::Client;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct BackgroundSetting {
    /// What applies now.
    pub enabled: bool,
    /// The owner's choice; `None` while they have made none.
    #[serde(default)]
    pub stored: Option<bool>,
    /// What applies without a choice: on for a PC of a local network.
    #[serde(default)]
    pub default_enabled: bool,
}

pub async fn get(client: &Client) -> ApiResult<BackgroundSetting> {
    client.get("/api/background").await
}

/// Turn the mode on or off. Administrators only. The answer is what applies from now on.
pub async fn set(client: &Client, enabled: bool) -> ApiResult<BackgroundSetting> {
    client.put("/api/background", &serde_json::json!({ "enabled": enabled })).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_setting_reads_as_the_service_writes_it() {
        let fresh: BackgroundSetting = serde_json::from_str(r#"{"enabled":true,"stored":null,"default_enabled":true}"#).unwrap();
        assert_eq!(fresh, BackgroundSetting { enabled: true, stored: None, default_enabled: true });
        let chosen: BackgroundSetting = serde_json::from_str(r#"{"enabled":false,"stored":false,"default_enabled":true}"#).unwrap();
        assert_eq!(chosen.stored, Some(false));
    }
}
