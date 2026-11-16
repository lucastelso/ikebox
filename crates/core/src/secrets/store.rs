//! Onde as credenciais podem ficar guardadas ("lembrar credenciais").
//!
//! - [`KeyringStore`]: chaveiro do sistema via Secret Service (GNOME
//!   Keyring/KWallet), pela libdbus. Serviço `ipsec-client`, chave
//!   `<perfil>:<campo>`.
//! - [`MemoryStore`]: só em memória (testes e sessões sem "lembrar").
//!
//! As chamadas ao chaveiro são bloqueantes (D-Bus): em código async, use
//! `tokio::task::spawn_blocking`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use keyring_core::{CredentialStore, Entry, Error as KeyringError};
use secrecy::{ExposeSecret, SecretString};

use super::SecretsError;
use crate::profiles::CredentialKind;

/// Nome do serviço no chaveiro.
pub const KEYRING_SERVICE: &str = "ipsec-client";

/// Armazenamento de credenciais por perfil.
pub trait SecretStore: Send + Sync {
    /// Lê uma credencial (`None` se não houver).
    ///
    /// # Errors
    /// Chaveiro indisponível ou bloqueado.
    fn get(
        &self,
        profile: &str,
        kind: CredentialKind,
    ) -> Result<Option<SecretString>, SecretsError>;

    /// Grava (ou substitui) uma credencial.
    ///
    /// # Errors
    /// Chaveiro indisponível ou bloqueado.
    fn set(
        &self,
        profile: &str,
        kind: CredentialKind,
        value: &SecretString,
    ) -> Result<(), SecretsError>;

    /// Apaga uma credencial (não é erro se ela não existir).
    ///
    /// # Errors
    /// Chaveiro indisponível ou bloqueado.
    fn delete(&self, profile: &str, kind: CredentialKind) -> Result<(), SecretsError>;
}

fn check_profile_id(profile: &str) -> Result<(), SecretsError> {
    if crate::profiles::is_valid_id(profile) {
        Ok(())
    } else {
        Err(SecretsError::InvalidProfile(profile.to_owned()))
    }
}

/// Chave no chaveiro: `<perfil>:<campo>`.
#[must_use]
pub fn keyring_key(profile: &str, kind: CredentialKind) -> String {
    format!("{profile}:{}", kind.key())
}

/// Chaveiro do sistema (Secret Service).
pub struct KeyringStore {
    store: Arc<CredentialStore>,
}

impl std::fmt::Debug for KeyringStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("KeyringStore(Secret Service)")
    }
}

impl KeyringStore {
    /// Conecta ao Secret Service da sessão.
    ///
    /// # Errors
    /// [`SecretsError::Unavailable`] sem sessão D-Bus ou sem serviço de
    /// chaveiro (ex.: sessão SSH sem GNOME Keyring).
    pub fn new() -> Result<Self, SecretsError> {
        let store = dbus_secret_service_keyring_store::Store::new()
            .map_err(|e| SecretsError::Unavailable(e.to_string()))?;
        Ok(Self { store })
    }

    fn entry(&self, profile: &str, kind: CredentialKind) -> Result<Entry, SecretsError> {
        check_profile_id(profile)?;
        let label = format!("IPsec Client: {profile} ({})", kind.key());
        let modifiers = HashMap::from([("label", label.as_str())]);
        self.store
            .build(
                KEYRING_SERVICE,
                &keyring_key(profile, kind),
                Some(&modifiers),
            )
            .map_err(map_keyring_error)
    }
}

fn map_keyring_error(e: KeyringError) -> SecretsError {
    match e {
        KeyringError::NoStorageAccess(inner) => SecretsError::Locked(inner.to_string()),
        other => SecretsError::Unavailable(other.to_string()),
    }
}

impl SecretStore for KeyringStore {
    fn get(
        &self,
        profile: &str,
        kind: CredentialKind,
    ) -> Result<Option<SecretString>, SecretsError> {
        match self.entry(profile, kind)?.get_password() {
            Ok(value) => Ok(Some(SecretString::from(value))),
            Err(KeyringError::NoEntry) => Ok(None),
            Err(e) => Err(map_keyring_error(e)),
        }
    }

    fn set(
        &self,
        profile: &str,
        kind: CredentialKind,
        value: &SecretString,
    ) -> Result<(), SecretsError> {
        self.entry(profile, kind)?
            .set_password(value.expose_secret())
            .map_err(map_keyring_error)
    }

    fn delete(&self, profile: &str, kind: CredentialKind) -> Result<(), SecretsError> {
        match self.entry(profile, kind)?.delete_credential() {
            Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
            Err(e) => Err(map_keyring_error(e)),
        }
    }
}

/// Armazenamento só em memória (zerado no drop).
#[derive(Default)]
pub struct MemoryStore {
    map: Mutex<HashMap<String, SecretString>>,
}

impl std::fmt::Debug for MemoryStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let count = self.map.lock().map(|m| m.len()).unwrap_or(0);
        write!(f, "MemoryStore(<{count} credencial(is)>)")
    }
}

impl MemoryStore {
    /// Armazenamento vazio.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, HashMap<String, SecretString>>, SecretsError> {
        self.map.lock().map_err(|_| {
            SecretsError::Unavailable(
                crate::tr!(
                    "in-memory store corrupted",
                    "armazenamento em memória corrompido"
                )
                .to_owned(),
            )
        })
    }
}

impl SecretStore for MemoryStore {
    fn get(
        &self,
        profile: &str,
        kind: CredentialKind,
    ) -> Result<Option<SecretString>, SecretsError> {
        check_profile_id(profile)?;
        Ok(self.lock()?.get(&keyring_key(profile, kind)).cloned())
    }

    fn set(
        &self,
        profile: &str,
        kind: CredentialKind,
        value: &SecretString,
    ) -> Result<(), SecretsError> {
        check_profile_id(profile)?;
        self.lock()?
            .insert(keyring_key(profile, kind), value.clone());
        Ok(())
    }

    fn delete(&self, profile: &str, kind: CredentialKind) -> Result<(), SecretsError> {
        check_profile_id(profile)?;
        self.lock()?.remove(&keyring_key(profile, kind));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_store_roundtrip() {
        let store = MemoryStore::new();
        assert!(
            store
                .get("vpn", CredentialKind::Psk)
                .expect("get")
                .is_none()
        );
        store
            .set("vpn", CredentialKind::Psk, &SecretString::from("psk-falsa"))
            .expect("set");
        let value = store
            .get("vpn", CredentialKind::Psk)
            .expect("get")
            .expect("valor");
        assert_eq!(value.expose_secret(), "psk-falsa");
        assert!(!format!("{store:?}").contains("psk-falsa"));
        store.delete("vpn", CredentialKind::Psk).expect("delete");
        store
            .delete("vpn", CredentialKind::Psk)
            .expect("delete idempotente");
        assert!(
            store
                .get("vpn", CredentialKind::Psk)
                .expect("get")
                .is_none()
        );
    }

    #[test]
    fn invalid_profile_ids_are_rejected() {
        let store = MemoryStore::new();
        assert!(store.get("../x", CredentialKind::Psk).is_err());
        assert!(store.get("Vpn", CredentialKind::Psk).is_err());
    }

    #[test]
    fn keyring_keys_follow_the_convention() {
        assert_eq!(keyring_key("vpn", CredentialKind::Psk), "vpn:psk");
        assert_eq!(
            keyring_key("vpn", CredentialKind::XauthPassword),
            "vpn:xauth-password"
        );
    }

    /// Usa o chaveiro REAL da sessão (cria e apaga um item de teste).
    /// Rodar com: cargo test -p ipsec-client-core keyring_real -- --ignored
    #[test]
    #[ignore = "usa o chaveiro real da sessão"]
    fn keyring_real_roundtrip() {
        let store = KeyringStore::new().expect("Secret Service");
        let profile = "ipsec-client-teste-chaveiro";
        store
            .set(
                profile,
                CredentialKind::Psk,
                &SecretString::from("psk-de-teste"),
            )
            .expect("set");
        let value = store
            .get(profile, CredentialKind::Psk)
            .expect("get")
            .expect("valor");
        assert_eq!(value.expose_secret(), "psk-de-teste");
        store.delete(profile, CredentialKind::Psk).expect("delete");
        assert!(
            store
                .get(profile, CredentialKind::Psk)
                .expect("get")
                .is_none()
        );
    }
}
