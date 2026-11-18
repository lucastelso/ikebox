//! The move from the name before 1.0 (ADR-0031). Up to 0.9 the project was
//! called IPsec Client: its configuration lived in `~/.config/ipsec-client`
//! and its credentials under the keyring service `ipsec-client`. On the first
//! start of 1.0 (app or CLI), both move to the new names, once.
//!
//! The configuration directory is renamed in one step. The credentials move
//! profile by profile, and a marker file in the new directory keeps the move
//! pending until it is complete: a locked keyring only postpones it.

use std::path::{Path, PathBuf};

use crate::profiles::{CredentialKind, ProfileStore, is_valid_id};
use crate::secrets::{KeyringStore, SecretStore, SecretsError};

/// Configuration directory name before 1.0.
pub const LEGACY_DIR: &str = "ipsec-client";

/// Keyring service before 1.0.
pub const LEGACY_KEYRING_SERVICE: &str = "ipsec-client";

/// Present while the credentials still have to move.
const KEYRING_PENDING: &str = ".keyring-from-ipsec-client";

/// Every credential a profile may have in the keyring.
const ALL_KINDS: [CredentialKind; 9] = [
    CredentialKind::Psk,
    CredentialKind::XauthUser,
    CredentialKind::XauthPassword,
    CredentialKind::EapIdentity,
    CredentialKind::EapPassword,
    CredentialKind::ClientCert,
    CredentialKind::ClientKey,
    CredentialKind::TokenKey,
    CredentialKind::TokenPin,
];

/// What moved.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Moved {
    /// The configuration directory (profiles and settings) moved.
    pub config: bool,
    /// Credentials moved to the new keyring service.
    pub credentials: usize,
}

/// The new configuration directory (`$XDG_CONFIG_HOME/ikebox`).
fn config_dir() -> Option<PathBuf> {
    ProfileStore::default_dir()
        .ok()?
        .parent()
        .map(Path::to_path_buf)
}

/// Renames `<config>/ipsec-client` to `config_dir` when only the old one
/// exists (a real directory, not a link), and marks the credentials as
/// pending.
///
/// # Errors
/// I/O error while renaming.
pub fn move_config_dir(config_dir: &Path) -> std::io::Result<bool> {
    let Some(parent) = config_dir.parent() else {
        return Ok(false);
    };
    let old = parent.join(LEGACY_DIR);
    let old_is_dir = std::fs::symlink_metadata(&old).is_ok_and(|m| m.is_dir());
    if !old_is_dir || std::fs::symlink_metadata(config_dir).is_ok() {
        return Ok(false);
    }
    std::fs::rename(&old, config_dir)?;
    std::fs::write(config_dir.join(KEYRING_PENDING), b"")?;
    Ok(true)
}

/// Do the credentials still have to move?
#[must_use]
pub fn keyring_pending(config_dir: &Path) -> bool {
    config_dir.join(KEYRING_PENDING).exists()
}

/// Moves the credentials of every profile in `config_dir` from `old` to
/// `new` (a value already in `new` wins), deletes them from `old`, and then
/// clears the pending mark.
///
/// # Errors
/// Keyring unavailable or locked: the move stays pending.
pub fn move_credentials(
    config_dir: &Path,
    old: &dyn SecretStore,
    new: &dyn SecretStore,
) -> Result<usize, SecretsError> {
    let mut moved = 0;
    for profile in profile_ids(&config_dir.join("profiles")) {
        for kind in ALL_KINDS {
            let Some(value) = old.get(&profile, kind)? else {
                continue;
            };
            if new.get(&profile, kind)?.is_none() {
                new.set(&profile, kind, &value)?;
                moved += 1;
            }
            old.delete(&profile, kind)?;
        }
    }
    let _ = std::fs::remove_file(config_dir.join(KEYRING_PENDING));
    Ok(moved)
}

/// Ids of the profile files in `dir`.
fn profile_ids(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let id = name.strip_suffix(".json")?;
            is_valid_id(id).then(|| id.to_owned())
        })
        .collect()
}

/// Moves the configuration directory (fast, on the file system). Call it
/// first thing, before reading profiles or settings.
#[must_use]
pub fn move_config() -> bool {
    let Some(dir) = config_dir() else {
        return false;
    };
    match move_config_dir(&dir) {
        Ok(moved) => moved,
        Err(e) => {
            tracing::warn!("could not move the IPsec Client configuration: {e}");
            false
        }
    }
}

/// Moves the pending credentials with the system keyring. Blocking (D-Bus,
/// and the keyring may ask to be unlocked): in the app, off the UI thread.
/// `None` when nothing was pending.
#[must_use]
pub fn move_keyring() -> Option<Result<usize, SecretsError>> {
    let dir = config_dir()?;
    if !keyring_pending(&dir) {
        return None;
    }
    Some(KeyringStore::legacy().and_then(|old| {
        let new = KeyringStore::new()?;
        move_credentials(&dir, &old, &new)
    }))
}

#[cfg(test)]
mod tests {
    use secrecy::{ExposeSecret, SecretString};

    use super::*;
    use crate::secrets::MemoryStore;

    fn secret(s: &str) -> SecretString {
        SecretString::from(s.to_owned())
    }

    #[test]
    fn the_old_directory_moves_once() {
        let base = tempfile::tempdir().expect("tempdir");
        let old = base.path().join(LEGACY_DIR);
        std::fs::create_dir_all(old.join("profiles")).expect("mkdir");
        std::fs::write(old.join("profiles/office.json"), "{}").expect("write");
        let new = base.path().join("ikebox");

        assert!(move_config_dir(&new).expect("move"));
        assert!(new.join("profiles/office.json").exists());
        assert!(!old.exists());
        assert!(keyring_pending(&new));
        // Nothing more to do the second time.
        assert!(!move_config_dir(&new).expect("again"));
    }

    #[test]
    fn an_existing_new_directory_is_never_replaced() {
        let base = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(base.path().join(LEGACY_DIR)).expect("old");
        let new = base.path().join("ikebox");
        std::fs::create_dir_all(&new).expect("new");
        assert!(!move_config_dir(&new).expect("move"));
        assert!(base.path().join(LEGACY_DIR).exists());
        assert!(!keyring_pending(&new));
    }

    #[test]
    fn a_link_in_place_of_the_old_directory_is_left_alone() {
        let base = tempfile::tempdir().expect("tempdir");
        let elsewhere = base.path().join("elsewhere");
        std::fs::create_dir_all(&elsewhere).expect("mkdir");
        std::os::unix::fs::symlink(&elsewhere, base.path().join(LEGACY_DIR)).expect("link");
        assert!(!move_config_dir(&base.path().join("ikebox")).expect("move"));
    }

    #[test]
    fn credentials_move_profile_by_profile() {
        let base = tempfile::tempdir().expect("tempdir");
        let dir = base.path().join("ikebox");
        std::fs::create_dir_all(dir.join("profiles")).expect("mkdir");
        std::fs::write(dir.join("profiles/office.json"), "{}").expect("write");
        std::fs::write(dir.join("profiles/home.json"), "{}").expect("write");
        std::fs::write(dir.join(KEYRING_PENDING), b"").expect("mark");

        let old = MemoryStore::new();
        let new = MemoryStore::new();
        old.set("office", CredentialKind::Psk, &secret("old-psk"))
            .expect("set");
        old.set("office", CredentialKind::XauthUser, &secret("user"))
            .expect("set");
        old.set("home", CredentialKind::Psk, &secret("stale"))
            .expect("set");
        // Already set under the new name: it wins.
        new.set("home", CredentialKind::Psk, &secret("newer"))
            .expect("set");

        assert_eq!(move_credentials(&dir, &old, &new).expect("move"), 2);
        let get = |s: &MemoryStore, p: &str, k| {
            s.get(p, k)
                .expect("get")
                .map(|v| v.expose_secret().to_owned())
        };
        assert_eq!(
            get(&new, "office", CredentialKind::Psk).as_deref(),
            Some("old-psk")
        );
        assert_eq!(
            get(&new, "office", CredentialKind::XauthUser).as_deref(),
            Some("user")
        );
        assert_eq!(
            get(&new, "home", CredentialKind::Psk).as_deref(),
            Some("newer")
        );
        assert_eq!(get(&old, "office", CredentialKind::Psk), None);
        assert_eq!(get(&old, "home", CredentialKind::Psk), None);
        assert!(!keyring_pending(&dir));
    }
}
