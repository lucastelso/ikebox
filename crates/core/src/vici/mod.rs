//! Cliente próprio do protocolo VICI do strongSwan (ADR-0003, ADR-0011).
//!
//! - [`Message`]: modelo, codificação e decodificação (valores zerados no drop,
//!   `Debug` sem valores);
//! - [`ViciClient`] e [`ViciEvents`]: transporte por socket Unix (uma conexão
//!   para comandos, outra para eventos);
//! - [`Charon`]: operações tipadas usadas pela sessão (mockável).
//!
//! Referência do protocolo: `src/libcharon/plugins/vici/README.md` no
//! código-fonte do strongSwan.

mod charon;
mod client;
mod message;

pub use charon::{
    Charon, CharonEvent, ChildSa, IkeSa, PrivateKey, SecretKind, SharedSecret, VersionInfo,
};
pub use client::{MAX_PACKET, ViciClient, ViciEvents};
pub use message::{Message, Value};

/// Erros do VICI. As mensagens são para o usuário final (pt-BR).
#[derive(Debug, thiserror::Error)]
pub enum ViciError {
    /// Erro de E/S no socket.
    #[error("{}", crate::trf!("I/O error on the VICI socket: {}", "erro de E/S no socket VICI: {}", .0))]
    Io(#[from] std::io::Error),
    /// Bytes que não seguem o protocolo.
    #[error("{}", crate::trf!("VICI protocol: {}", "protocolo VICI: {}", .0))]
    Protocol(String),
    /// Pacote acima do limite.
    #[error("{}", crate::trf!("VICI packet too large ({} bytes)", "pacote VICI grande demais ({} bytes)", .0))]
    TooLarge(usize),
    /// O charon fechou a conexão.
    #[error("{}", crate::tr!("charon closed the VICI connection", "o charon fechou a conexão VICI"))]
    Closed,
    /// Comando que o charon não conhece.
    #[error("{}", crate::trf!("unknown VICI command: {}", "comando VICI desconhecido: {}", .0))]
    UnknownCommand(String),
    /// Evento que o charon não conhece.
    #[error("{}", crate::trf!("unknown VICI event: {}", "evento VICI desconhecido: {}", .0))]
    UnknownEvent(String),
    /// O charon executou o comando e respondeu `success = no`.
    #[error("{}", crate::trf!("charon rejected '{}': {}", "o charon recusou '{}': {}", .command, .message))]
    CommandFailed {
        /// Comando.
        command: String,
        /// `errmsg` do charon.
        message: String,
    },
}
