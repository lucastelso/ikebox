//! The app's own settings (phase F of the program to 1.0.0): what closing the
//! window does and the optional update notice. Kept by Rust, which needs them
//! when the window is closed and to check for updates, in
//! `~/.config/ikebox/settings.json` (0600, written atomically). The
//! interface's preferences (theme, language) stay in the webview.

use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::PathBuf;
use std::sync::Mutex;

use ikebox_core::profiles::ProfileStore;
use serde::{Deserialize, Serialize};

/// What is stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    /// Closing the window keeps the app running in the tray.
    pub close_to_tray: bool,
    /// The person was told once that the app keeps running.
    pub close_notice_shown: bool,
    /// Check once a day whether a newer version was released (off by
    /// default: it contacts GitHub).
    pub update_check: bool,
    /// When the last check ran (seconds since the epoch).
    pub last_update_check: u64,
    /// The newest version seen by the last check (validated `X.Y.Z`).
    pub latest_version: Option<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            close_to_tray: true,
            close_notice_shown: false,
            update_check: false,
            last_update_check: 0,
            latest_version: None,
        }
    }
}

/// The settings in memory, shared by the commands and the window handler.
#[derive(Debug)]
pub struct SettingsState(pub Mutex<AppSettings>);

impl Default for SettingsState {
    fn default() -> Self {
        Self(Mutex::new(load()))
    }
}

impl SettingsState {
    /// A copy of the current settings.
    pub fn get(&self) -> AppSettings {
        self.0.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// Changes the settings and stores them.
    pub fn update(&self, change: impl FnOnce(&mut AppSettings)) -> AppSettings {
        let Ok(mut settings) = self.0.lock() else {
            return AppSettings::default();
        };
        change(&mut settings);
        if let Err(e) = save(&settings) {
            tracing::warn!("could not store the app settings: {e}");
        }
        settings.clone()
    }
}

fn path() -> Option<PathBuf> {
    let profiles = ProfileStore::default_dir().ok()?;
    Some(profiles.parent()?.join("settings.json"))
}

/// The stored settings, or the defaults (a missing or unreadable file is not
/// an error: the app keeps working with the defaults).
fn load() -> AppSettings {
    path()
        .and_then(|p| std::fs::read(p).ok())
        .filter(|data| data.len() <= 64 * 1024)
        .and_then(|data| serde_json::from_slice(&data).ok())
        .unwrap_or_default()
}

fn save(settings: &AppSettings) -> std::io::Result<()> {
    let path = path().ok_or_else(|| std::io::Error::other("no configuration directory"))?;
    let dir = path
        .parent()
        .ok_or_else(|| std::io::Error::other("no configuration directory"))?;
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(".settings.json.tmp");
    let data = serde_json::to_vec_pretty(settings).map_err(std::io::Error::other)?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)?;
    file.write_all(&data)?;
    file.sync_all()?;
    std::fs::rename(&tmp, &path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_partial_files() {
        let d = AppSettings::default();
        assert!(
            d.close_to_tray,
            "closing keeps the app in the tray by default"
        );
        assert!(!d.update_check, "the update check is off by default");
        // A file from an older version, with fewer fields, still reads.
        let s: AppSettings = serde_json::from_str(r#"{"updateCheck":true}"#).expect("json");
        assert!(s.update_check && s.close_to_tray);
    }
}
