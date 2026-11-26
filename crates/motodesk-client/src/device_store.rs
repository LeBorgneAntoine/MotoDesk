//! What one PC keeps for itself between two runs, outside the shop's database: whether it is a
//! till, which model its assistant uses.
//!
//! Each is a small file in the data folder: one JSON object, one entry per key. The format has
//! not changed since the first version, so a PC keeps its choices from one version to the next.

use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{Map, Value};

#[derive(Debug, Clone)]
pub(crate) struct DeviceFile {
    path: PathBuf,
}

impl DeviceFile {
    /// `data_dir` is the app's data folder.
    pub(crate) fn in_dir(data_dir: &Path, file: &str) -> Self {
        Self { path: data_dir.join(file) }
    }

    fn read(&self) -> Map<String, Value> {
        std::fs::read(&self.path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
    }

    /// What is stored under `key`, when it is there and readable.
    pub(crate) fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.read().remove(key).and_then(|value| serde_json::from_value(value).ok())
    }

    /// Replace the file in one step, so a crash mid-write cannot leave half of it. The other
    /// keys of the file are kept.
    pub(crate) fn set<T: Serialize>(&self, key: &str, value: &T) -> std::io::Result<()> {
        let mut map = self.read();
        map.insert(key.into(), serde_json::to_value(value).unwrap_or(Value::Null));
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec(&map).unwrap_or_default())?;
        std::fs::rename(&tmp, &self.path)
    }
}
