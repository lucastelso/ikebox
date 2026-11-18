//! Our own client for strongSwan's VICI protocol (ADR-0003, ADR-0011).
//!
//! - [`Message`]: model, encoding and decoding (values zeroed on drop, `Debug`
//!   without values);
//! - [`ViciClient`] and [`ViciEvents`]: transport over a Unix socket (one
//!   connection for commands, another for events);
//! - [`Charon`]: typed operations used by the session (mockable).
//!
//! Protocol reference: `src/libcharon/plugins/vici/README.md` in strongSwan's
//! source code.

mod charon;
mod client;
mod message;

pub use charon::{
    Charon, CharonEvent, ChildSa, IkeSa, PrivateKey, SecretKind, SharedSecret, VersionInfo,
};
pub use client::{MAX_PACKET, ViciClient, ViciEvents};
pub use message::{Message, Value};

/// VICI errors. The messages are for the end user.
#[derive(Debug, thiserror::Error)]
pub enum ViciError {
    /// I/O error on the socket.
    #[error("{}", crate::trf!("I/O error on the VICI socket: {}", "erro de E/S no socket VICI: {}", .0))]
    Io(#[from] std::io::Error),
    /// Bytes that do not follow the protocol.
    #[error("{}", crate::trf!("VICI protocol: {}", "protocolo VICI: {}", .0))]
    Protocol(String),
    /// Packet above the limit.
    #[error("{}", crate::trf!("VICI packet too large ({} bytes)", "pacote VICI grande demais ({} bytes)", .0))]
    TooLarge(usize),
    /// charon closed the connection.
    #[error("{}", crate::tr!("charon closed the VICI connection", "o charon fechou a conexão VICI"))]
    Closed,
    /// A command charon does not know.
    #[error("{}", crate::trf!("unknown VICI command: {}", "comando VICI desconhecido: {}", .0))]
    UnknownCommand(String),
    /// An event charon does not know.
    #[error("{}", crate::trf!("unknown VICI event: {}", "evento VICI desconhecido: {}", .0))]
    UnknownEvent(String),
    /// charon ran the command and answered `success = no`.
    #[error("{}", crate::trf!("charon rejected '{}': {}", "o charon recusou '{}': {}", .command, .message))]
    CommandFailed {
        /// Command.
        command: String,
        /// charon's `errmsg`.
        message: String,
    },
}
