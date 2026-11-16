//! Segredos e credenciais (ADR-0007).
//!
//! - [`Credentials`]: PSK, usuário e senha XAuth de uma sessão, em memória
//!   zerada no drop, com `Debug` redigido;
//! - [`SecretStore`]: onde lembrar as credenciais ([`KeyringStore`] = chaveiro
//!   do sistema, [`MemoryStore`] = só memória);
//! - [`Redactor`]/[`mask`]: mascaramento de identidades em logs exibidos.

mod credentials;
mod redact;
mod store;

pub use credentials::Credentials;
pub use redact::{Anonymizer, Redactor, mask};
pub use store::{KEYRING_SERVICE, KeyringStore, MemoryStore, SecretStore, keyring_key};

use crate::profiles::CredentialKind;

/// Credential errors. Messages are for the end user (see [`crate::i18n`]).
#[derive(Debug, thiserror::Error)]
pub enum SecretsError {
    /// No Secret Service (e.g. a session without GNOME Keyring/KWallet).
    #[error("{}", crate::trf!(
        "the system keyring is not available ({}); credentials will be asked \
         on every connection",
        "o chaveiro do sistema não está disponível ({}); as credenciais \
         serão pedidas a cada conexão",
        .0
    ))]
    Unavailable(String),
    /// Keyring locked or access denied.
    #[error("{}", crate::trf!(
        "the system keyring is locked or denied access ({})",
        "o chaveiro do sistema está bloqueado ou negou acesso ({})",
        .0
    ))]
    Locked(String),
    /// A required credential was not provided.
    #[error("{}", crate::trf!("credential not provided: {}", "credencial não informada: {}", describe(*.0)))]
    Missing(CredentialKind),
    /// Profile id not valid as a keyring key.
    #[error("{}", crate::trf!("invalid profile id: {}", "id de perfil inválido: {}", .0))]
    InvalidProfile(String),
}

/// Name of the credential for the user.
#[must_use]
pub fn describe(kind: CredentialKind) -> &'static str {
    match kind {
        CredentialKind::Psk => crate::tr!("PSK (group key)", "PSK (chave do grupo)"),
        CredentialKind::XauthUser => crate::tr!("XAuth username", "usuário XAuth"),
        CredentialKind::XauthPassword => crate::tr!("XAuth password", "senha XAuth"),
    }
}
