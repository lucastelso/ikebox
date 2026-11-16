//! Núcleo do ipsec-client.
//!
//! Toda a lógica de negócio fica aqui; a GUI (`src-tauri`) e o CLI
//! (`ipsec-client-cli`) são camadas finas sobre este crate (ADR-0009).
//!
//! Módulos (ver `docs/arquitetura.md`):
//! - `component`: componente de rede (estado, instalação via pkexec);
//! - `engine`: engine embutido (helper) ou em container;
//! - `vici`: cliente do protocolo de controle do charon;
//! - `profiles`: modelo, validação e geração da configuração;
//! - `secrets`: segredos em memória e no chaveiro do sistema;
//! - `session`: máquina de estados da conexão;
//! - `diagnostics`: verificações do ambiente.

pub mod component;
pub mod diagnostics;
pub mod engine;
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
