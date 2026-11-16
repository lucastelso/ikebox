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
pub use redact::{Redactor, mask};
pub use store::{KEYRING_SERVICE, KeyringStore, MemoryStore, SecretStore, keyring_key};

use crate::profiles::CredentialKind;

/// Erros de credenciais. As mensagens são para o usuário final (pt-BR).
#[derive(Debug, thiserror::Error)]
pub enum SecretsError {
    /// Sem Secret Service (ex.: sessão sem GNOME Keyring/KWallet).
    #[error(
        "o chaveiro do sistema não está disponível ({0}); as credenciais \
         serão pedidas a cada conexão"
    )]
    Unavailable(String),
    /// Chaveiro bloqueado ou acesso negado.
    #[error("o chaveiro do sistema está bloqueado ou negou acesso ({0})")]
    Locked(String),
    /// Credencial exigida que não foi informada.
    #[error("credencial não informada: {}", describe(*.0))]
    Missing(CredentialKind),
    /// Id de perfil inválido para chave do chaveiro.
    #[error("id de perfil inválido: {0}")]
    InvalidProfile(String),
}

/// Nome da credencial para o usuário.
#[must_use]
pub fn describe(kind: CredentialKind) -> &'static str {
    match kind {
        CredentialKind::Psk => "PSK (chave do grupo)",
        CredentialKind::XauthUser => "usuário XAuth",
        CredentialKind::XauthPassword => "senha XAuth",
    }
}
