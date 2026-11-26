//! What the kit embeds in the binary: the interface font, the logo, and the icons.

use std::borrow::Cow;

use gpui_kit::assets::AllAssets;
use gpui_kit::{AssetSource, Result, SharedString};

/// The kit's own files (`crates/motodesk-ui/assets`).
#[derive(rust_embed::RustEmbed)]
#[folder = "assets"]
struct Embedded;

/// The asset source of the application: the kit's files first, then the Lucide icon catalog.
#[derive(Clone, Copy, Debug, Default)]
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path.is_empty() {
            return Ok(None);
        }
        match Embedded::get(path) {
            Some(file) => Ok(Some(file.data)),
            None => AllAssets.load(path),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut found: Vec<SharedString> =
            Embedded::iter().filter(|name| name.starts_with(path)).map(|name| name.to_string().into()).collect();
        found.extend(AllAssets.list(path)?);
        Ok(found)
    }
}

/// Inter (variable weight, Latin), under the SIL Open Font License: see
/// `assets/fonts/Inter-LICENSE.txt`.
pub fn fonts() -> Vec<Cow<'static, [u8]>> {
    Embedded::iter()
        .filter(|name| name.starts_with("fonts/") && name.ends_with(".ttf"))
        .filter_map(|name| Embedded::get(&name).map(|file| file.data))
        .collect()
}
