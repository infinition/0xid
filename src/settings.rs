// Persistent user settings, stored as JSON in `<data_dir>/settings.json`.
// Single source of truth — the mute flag, conda env, trigger hotkey, apps
// directory and custom environment variables all live here.

use serde::{Deserialize, Serialize};
use std::sync::Mutex;

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Settings {
    /// Mute all sound effects.
    pub muted: bool,
    /// Conda environment used to run `.py` scripts. Empty = skip conda and use
    /// a plain `python` interpreter from PATH.
    pub conda_env: String,
    /// Global trigger hotkey, e.g. "Ctrl+Shift+Space".
    pub hotkey: String,
    /// Folder scanned for plugins. Empty = default `<data_dir>/apps`.
    pub apps_dir: String,
    /// Extra environment variables injected into launched apps/scripts,
    /// each formatted as "KEY=VALUE".
    pub env_vars: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            muted: false,
            conda_env: "0xid".to_string(),
            hotkey: "Ctrl+Shift+Space".to_string(),
            apps_dir: String::new(),
            env_vars: Vec::new(),
        }
    }
}

static SETTINGS: Mutex<Option<Settings>> = Mutex::new(None);

fn file() -> std::path::PathBuf {
    crate::plugins::data_dir().join("settings.json")
}

fn read_from_disk() -> Settings {
    std::fs::read_to_string(file())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Load settings from disk into the global cache. Safe to call once at startup;
/// `get()` will lazily load too if this wasn't called.
pub fn load() -> Settings {
    let s = read_from_disk();
    *SETTINGS.lock().unwrap() = Some(s.clone());
    s
}

/// Current settings (a clone). Lazily loads from disk on first use.
pub fn get() -> Settings {
    let mut guard = SETTINGS.lock().unwrap();
    if guard.is_none() {
        *guard = Some(read_from_disk());
    }
    guard.clone().unwrap_or_default()
}

/// Persist settings to disk and update the global cache.
pub fn save(s: &Settings) {
    *SETTINGS.lock().unwrap() = Some(s.clone());
    let _ = std::fs::write(
        file(),
        serde_json::to_string_pretty(s).unwrap_or_default(),
    );
}
