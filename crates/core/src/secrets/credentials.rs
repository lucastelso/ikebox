//! A session's credentials: PSK, XAuth or EAP username and password (ADR-0007).
//!
//! They are kept in [`SecretString`] (zeroed on drop) and leave the app's
//! memory only for charon's memory, through `load-shared` ([`SharedSecret`]).

use std::fmt;

use secrecy::{ExposeSecret, SecretString};

use super::{ClientCertificate, SecretStore, SecretsError, mask};
use crate::profiles::{CredentialKind, Profile};
use crate::vici::{PrivateKey, SecretKind, SharedSecret};

/// A profile's credentials (some may be missing until the user enters them).
#[derive(Default, Clone)]
pub struct Credentials {
    psk: Option<SecretString>,
    xauth_user: Option<SecretString>,
    xauth_password: Option<SecretString>,
    eap_identity: Option<SecretString>,
    eap_password: Option<SecretString>,
    client_cert: Option<SecretString>,
    client_key: Option<SecretString>,
    token_key: Option<SecretString>,
    token_pin: Option<SecretString>,
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let present = |v: &Option<SecretString>| {
            if v.is_some() {
                "<definida>"
            } else {
                "<ausente>"
            }
        };
        f.debug_struct("Credentials")
            .field("psk", &present(&self.psk))
            .field(
                "xauth_user",
                &self.xauth_user.as_ref().map(|u| mask(u.expose_secret())),
            )
            .field("xauth_password", &present(&self.xauth_password))
            .field(
                "eap_identity",
                &self.eap_identity.as_ref().map(|u| mask(u.expose_secret())),
            )
            .field("eap_password", &present(&self.eap_password))
            .field("client_cert", &present(&self.client_cert))
            .field("client_key", &present(&self.client_key))
            .field("token_key", &present(&self.token_key))
            .field("token_pin", &present(&self.token_pin))
            .finish()
    }
}

impl Credentials {
    /// No credential at all.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn slot(&mut self, kind: CredentialKind) -> &mut Option<SecretString> {
        match kind {
            CredentialKind::Psk => &mut self.psk,
            CredentialKind::XauthUser => &mut self.xauth_user,
            CredentialKind::XauthPassword => &mut self.xauth_password,
            CredentialKind::EapIdentity => &mut self.eap_identity,
            CredentialKind::EapPassword => &mut self.eap_password,
            CredentialKind::ClientCert => &mut self.client_cert,
            CredentialKind::ClientKey => &mut self.client_key,
            CredentialKind::TokenKey => &mut self.token_key,
            CredentialKind::TokenPin => &mut self.token_pin,
        }
    }

    /// Reads a credential.
    #[must_use]
    pub fn get(&self, kind: CredentialKind) -> Option<&SecretString> {
        match kind {
            CredentialKind::Psk => self.psk.as_ref(),
            CredentialKind::XauthUser => self.xauth_user.as_ref(),
            CredentialKind::XauthPassword => self.xauth_password.as_ref(),
            CredentialKind::EapIdentity => self.eap_identity.as_ref(),
            CredentialKind::EapPassword => self.eap_password.as_ref(),
            CredentialKind::ClientCert => self.client_cert.as_ref(),
            CredentialKind::ClientKey => self.client_key.as_ref(),
            CredentialKind::TokenKey => self.token_key.as_ref(),
            CredentialKind::TokenPin => self.token_pin.as_ref(),
        }
    }

    /// Sets a credential. Empty values are treated as missing.
    pub fn set(&mut self, kind: CredentialKind, value: SecretString) {
        *self.slot(kind) = (!value.expose_secret().is_empty()).then_some(value);
    }

    /// XAuth username (not a secret, but personal: mask it when showing).
    #[must_use]
    pub fn xauth_user(&self) -> Option<&str> {
        self.xauth_user.as_ref().map(ExposeSecret::expose_secret)
    }

    /// EAP identity (not a secret, but personal: mask it when showing).
    #[must_use]
    pub fn eap_identity(&self) -> Option<&str> {
        self.eap_identity.as_ref().map(ExposeSecret::expose_secret)
    }

    /// Uses the certificate and key read from a `.p12` (see
    /// [`ClientCertificate`]).
    pub fn set_client_certificate(&mut self, cert: &ClientCertificate) {
        self.set(
            CredentialKind::ClientCert,
            SecretString::from(cert.cert_pem().to_owned()),
        );
        self.set(
            CredentialKind::ClientKey,
            SecretString::from(cert.key_pem().to_owned()),
        );
    }

    /// The client certificate, checked again against the key (what is in the
    /// keyring may have been swapped from outside).
    ///
    /// # Errors
    /// Incomplete or invalid pair.
    pub fn client_certificate(&self) -> Option<Result<ClientCertificate, String>> {
        let cert = self.get(CredentialKind::ClientCert)?;
        let key = self.get(CredentialKind::ClientKey)?;
        Some(ClientCertificate::from_pem(
            cert.expose_secret(),
            key.expose_secret(),
        ))
    }

    /// Client certificate in PEM (for `load-conn`).
    #[must_use]
    pub fn client_cert_pem(&self) -> Option<&str> {
        self.get(CredentialKind::ClientCert)
            .map(ExposeSecret::expose_secret)
    }

    /// The client's private key, for `load-key`.
    #[must_use]
    pub fn private_key(&self) -> Option<PrivateKey> {
        self.get(CredentialKind::ClientKey)
            .map(|key| PrivateKey::new(key.expose_secret().as_bytes()))
    }

    /// Credentials the profile requires that are still missing.
    #[must_use]
    pub fn missing(&self, profile: &Profile) -> Vec<CredentialKind> {
        profile
            .required_credentials()
            .into_iter()
            .filter(|kind| self.get(*kind).is_none())
            .collect()
    }

    /// Loads whatever is in the store (what is missing stays missing).
    ///
    /// # Errors
    /// Store error (e.g. a locked keyring).
    pub fn load(store: &dyn SecretStore, profile: &Profile) -> Result<Self, SecretsError> {
        let mut credentials = Self::new();
        for kind in profile.storable_credentials() {
            if let Some(value) = store.get(&profile.id, kind)? {
                credentials.set(kind, value);
            }
        }
        Ok(credentials)
    }

    /// Saves to the store ("remember credentials").
    ///
    /// # Errors
    /// Store error.
    pub fn save(&self, store: &dyn SecretStore, profile: &Profile) -> Result<(), SecretsError> {
        // A one-time password is never stored, and one stored before the profile
        // started using OTP is removed from the keyring.
        if profile.auth.otp() {
            store.delete(&profile.id, CredentialKind::XauthPassword)?;
        }
        for kind in profile.storable_credentials() {
            if let Some(value) = self.get(kind) {
                store.set(&profile.id, kind, value)?;
            }
        }
        Ok(())
    }

    /// Forgets everything stored for the profile.
    ///
    /// # Errors
    /// Store error.
    pub fn forget(store: &dyn SecretStore, profile: &Profile) -> Result<(), SecretsError> {
        for kind in [
            CredentialKind::Psk,
            CredentialKind::XauthUser,
            CredentialKind::XauthPassword,
            CredentialKind::EapIdentity,
            CredentialKind::EapPassword,
            CredentialKind::ClientCert,
            CredentialKind::ClientKey,
            CredentialKind::TokenKey,
        ] {
            store.delete(&profile.id, kind)?;
        }
        Ok(())
    }

    /// Secrets to load into charon (`load-shared`) for the profile.
    ///
    /// - PSK: owners = local identity of the PSK round (if any; without an owner,
    ///   it applies to any peer);
    /// - XAuth: owner = the XAuth username;
    /// - EAP: owner = the EAP identity.
    ///
    /// # Errors
    /// [`SecretsError::Missing`] if a required credential is missing.
    pub fn shared_secrets(&self, profile: &Profile) -> Result<Vec<SharedSecret>, SecretsError> {
        if let Some(kind) = self.missing(profile).first() {
            return Err(SecretsError::Missing(*kind));
        }
        let mut secrets = Vec::new();
        for kind in profile.required_credentials() {
            match kind {
                CredentialKind::Psk => {
                    let owners = profile
                        .auth
                        .psk_id()
                        .map(str::to_owned)
                        .into_iter()
                        .collect();
                    let psk = self.get(kind).ok_or(SecretsError::Missing(kind))?;
                    secrets.push(SharedSecret::new(
                        &format!("ike-{}", profile.id),
                        SecretKind::Ike,
                        owners,
                        psk.expose_secret().as_bytes(),
                    ));
                }
                CredentialKind::XauthPassword => {
                    let user = self
                        .xauth_user()
                        .ok_or(SecretsError::Missing(CredentialKind::XauthUser))?;
                    let password = self.get(kind).ok_or(SecretsError::Missing(kind))?;
                    secrets.push(SharedSecret::new(
                        &format!("xauth-{}", profile.id),
                        SecretKind::Xauth,
                        vec![user.to_owned()],
                        password.expose_secret().as_bytes(),
                    ));
                }
                CredentialKind::EapPassword => {
                    let identity = self
                        .eap_identity()
                        .ok_or(SecretsError::Missing(CredentialKind::EapIdentity))?;
                    let password = self.get(kind).ok_or(SecretsError::Missing(kind))?;
                    secrets.push(SharedSecret::new(
                        &format!("eap-{}", profile.id),
                        SecretKind::Eap,
                        vec![identity.to_owned()],
                        password.expose_secret().as_bytes(),
                    ));
                }
                // The key goes by load-key (private_key, or the token's key
                // agent) and the certificate in the connection; the PIN stays
                // in this process: none is a shared secret.
                CredentialKind::XauthUser
                | CredentialKind::EapIdentity
                | CredentialKind::ClientCert
                | CredentialKind::ClientKey
                | CredentialKind::TokenKey
                | CredentialKind::TokenPin => {}
            }
        }
        Ok(secrets)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::parse_profile;
    use crate::secrets::MemoryStore;

    fn profile() -> Profile {
        parse_profile(include_str!(
            "../../../../presets/example-ikev1-aggressive-xauth.json"
        ))
        .expect("preset")
    }

    fn complete() -> Credentials {
        let mut c = Credentials::new();
        c.set(CredentialKind::Psk, SecretString::from("psk-falsa"));
        c.set(
            CredentialKind::XauthUser,
            SecretString::from("usuario.exemplo"),
        );
        c.set(
            CredentialKind::XauthPassword,
            SecretString::from("senha-falsa"),
        );
        c
    }

    #[test]
    fn debug_never_shows_secrets_or_full_user() {
        let debug = format!("{:?}", complete());
        for secret in ["psk-falsa", "senha-falsa", "usuario.exemplo"] {
            assert!(!debug.contains(secret), "{debug}");
        }
        assert!(debug.contains("<definida>"));
    }

    #[test]
    fn missing_and_empty_values() {
        let p = profile();
        let mut c = Credentials::new();
        assert_eq!(c.missing(&p).len(), 3);
        c.set(CredentialKind::Psk, SecretString::from(""));
        assert!(c.get(CredentialKind::Psk).is_none(), "vazio = ausente");
        assert!(matches!(
            c.shared_secrets(&p),
            Err(SecretsError::Missing(CredentialKind::Psk))
        ));
        assert!(complete().missing(&p).is_empty());
    }

    #[test]
    fn shared_secrets_have_the_right_owners() {
        let secrets = complete().shared_secrets(&profile()).expect("segredos");
        assert_eq!(secrets.len(), 2);
        assert_eq!(secrets[0].id, "ike-example-legacy");
        assert_eq!(secrets[0].kind, SecretKind::Ike);
        assert_eq!(secrets[1].id, "xauth-example-legacy");
        assert_eq!(secrets[1].kind, SecretKind::Xauth);
        let debug = format!("{secrets:?}");
        assert!(!debug.contains("psk-falsa") && !debug.contains("senha-falsa"));
    }

    #[test]
    fn save_load_forget_through_store() {
        let store = MemoryStore::new();
        let p = profile();
        complete().save(&store, &p).expect("save");
        let loaded = Credentials::load(&store, &p).expect("load");
        assert!(loaded.missing(&p).is_empty());
        assert_eq!(loaded.xauth_user(), Some("usuario.exemplo"));
        Credentials::forget(&store, &p).expect("forget");
        assert_eq!(
            Credentials::load(&store, &p)
                .expect("load")
                .missing(&p)
                .len(),
            3
        );
    }

    #[test]
    fn a_one_time_password_is_never_stored() {
        let store = MemoryStore::new();
        // Stored while the profile did not use OTP...
        complete().save(&store, &profile()).expect("save");
        let mut otp = profile();
        otp.auth.local[1].otp = true;
        // ...it is not read back once the profile does, and the next save
        // removes it from the store.
        let loaded = Credentials::load(&store, &otp).expect("load");
        assert_eq!(loaded.missing(&otp), [CredentialKind::XauthPassword]);
        complete().save(&store, &otp).expect("save");
        assert!(
            store
                .get(&otp.id, CredentialKind::XauthPassword)
                .expect("get")
                .is_none()
        );
        assert!(
            store
                .get(&otp.id, CredentialKind::Psk)
                .expect("get")
                .is_some()
        );
    }
}
