//! ipsec-client core.
//!
//! All the business logic lives here; the GUI (`src-tauri`) and the CLI
//! (`ipsec-client-cli`) are thin layers over this crate (ADR-0009).
//!
//! Modules (see `docs/architecture.md`):
//! - `component`: network component (state, installation through pkexec);
//! - `engine`: embedded engine (helper) or container engine;
//! - `vici`: client for charon's control protocol;
//! - `profiles`: model, validation and configuration rendering;
//! - `secrets`: secrets in memory and in the system keyring;
//! - `session`: connection state machine;
//! - `diagnostics`: environment checks;
//! - `i18n`: user-facing language (English or Portuguese).

pub mod component;
pub mod diagnostics;
pub mod engine;
pub mod i18n;
pub mod profiles;
pub mod secrets;
pub mod session;
pub mod vici;

/// Versão do núcleo (a mesma do workspace).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    #[test]
    fn version_is_set() {
        assert!(!super::VERSION.is_empty());
    }
}
