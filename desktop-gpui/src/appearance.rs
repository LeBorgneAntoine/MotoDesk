//! The look the shop chose (port of `applyAppearance` in `desktop/src/lib/theme.ts`): light
//! or dark, the accent colour, the compact layout. Read from the same settings as the Svelte
//! interface, so a PC looks the same in both.
//!
//! The corner setting (`system.radius`) is not read: corners are square.

use motodesk_client::settings::AppSetting;
use motodesk_ui::gpui::{App, WindowAppearance};
use motodesk_ui::{DEFAULT_ACCENT, Density, Mode, UiTheme, parse_hex};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Appearance {
    pub mode: Mode,
    pub accent: u32,
    pub density: Density,
}

/// `system_is_dark` settles the "Système" choice.
pub fn from_settings(settings: &[AppSetting], system_is_dark: bool) -> Appearance {
    let value = |key: &str| settings.iter().find(|s| s.key == key).map(|s| &s.value);
    let theme = value("system.theme").and_then(|v| v.as_str()).unwrap_or("light");
    let dark = theme == "dark" || (theme == "auto" && system_is_dark);
    Appearance {
        mode: if dark { Mode::Dark } else { Mode::Light },
        accent: value("system.accent_color").and_then(|v| v.as_str()).and_then(parse_hex).unwrap_or(DEFAULT_ACCENT),
        density: if value("system.compact_mode").and_then(|v| v.as_bool()).unwrap_or(false) {
            Density::Compact
        } else {
            Density::Regular
        },
    }
}

/// Give the whole app the look these settings ask for.
pub fn apply(settings: &[AppSetting], cx: &mut App) {
    let system_is_dark = matches!(cx.window_appearance(), WindowAppearance::Dark | WindowAppearance::VibrantDark);
    let look = from_settings(settings, system_is_dark);
    UiTheme::update(cx, |theme| {
        theme.mode = look.mode;
        theme.accent = look.accent;
        theme.density = look.density;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setting(key: &str, value: serde_json::Value) -> AppSetting {
        AppSetting { key: key.into(), value, updated_at: String::new() }
    }

    #[test]
    fn with_no_setting_the_app_is_light_blue_and_regular() {
        assert_eq!(
            from_settings(&[], true),
            Appearance { mode: Mode::Light, accent: DEFAULT_ACCENT, density: Density::Regular }
        );
    }

    #[test]
    fn the_shops_choices_are_followed() {
        let chosen = [
            setting("system.theme", "dark".into()),
            setting("system.accent_color", "#c2410c".into()),
            setting("system.compact_mode", true.into()),
        ];
        assert_eq!(
            from_settings(&chosen, false),
            Appearance { mode: Mode::Dark, accent: 0xc2410c, density: Density::Compact }
        );
    }

    #[test]
    fn auto_follows_windows_and_a_bad_colour_falls_back() {
        let auto = [setting("system.theme", "auto".into()), setting("system.accent_color", "bleu".into())];
        assert_eq!(from_settings(&auto, true).mode, Mode::Dark);
        assert_eq!(from_settings(&auto, false).mode, Mode::Light);
        assert_eq!(from_settings(&auto, false).accent, DEFAULT_ACCENT);
    }
}
