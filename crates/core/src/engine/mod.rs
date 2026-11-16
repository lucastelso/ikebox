//! Engine: o container com o strongSwan 5.8.2 (ADRs 0001, 0002, 0005 e 0006).
//!
//! - [`context`]: arquivos de `engine/` embutidos, hash e tar do build;
//! - [`EngineSpec`]: configuração de uma instância (pura, testável);
//! - [`DockerEngine`]: build da imagem e ciclo de vida do container.

mod backend;
pub mod context;
mod docker;
mod native;
mod spec;

use std::path::PathBuf;

pub use backend::{Backend, RunningEngine};
pub use docker::{BuildProgress, DockerEngine, READY_LINE, READY_TIMEOUT, STOP_TIMEOUT_SECS};
pub use native::NativeEngine;
pub use spec::{
    CONTAINER_NAME, CONTAINER_SOCKET_DIR, DBUS_SYSTEM_SOCKET, DEFAULT_IFACE, DEFAULT_MTU,
    EngineSpec, HostSecurity, MANAGED_LABEL, PROFILE_LABEL, SOCKET_FILE,
};

/// Erros do engine. As mensagens são para o usuário final (pt-BR).
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    /// O daemon Docker não está acessível.
    #[error(
        "o Docker não está acessível ({0}). Verifique se o serviço está ativo \
         (systemctl status docker) e se o seu usuário está no grupo docker \
         (sudo usermod -aG docker $USER, depois saia e entre na sessão)"
    )]
    DockerUnavailable(String),

    /// Configuração inválida.
    #[error("configuração inválida do engine: {0}")]
    InvalidSpec(String),

    /// O build da imagem falhou.
    #[error("falha ao construir a imagem do engine: {0}")]
    Build(String),

    /// O engine não ficou pronto.
    #[error("o engine não ficou pronto: {reason}")]
    NotReady {
        /// Motivo resumido.
        reason: String,
        /// Últimas linhas do log do container (não contêm segredos).
        recent_logs: Vec<String>,
    },

    /// Componente de rede (helper do engine embutido).
    #[error("{0}")]
    Helper(String),

    /// O componente de rede instalado é anterior ao app e não entende o
    /// pedido (ex.: o DNS que passou a ir pelo helper na 0.1.1).
    #[error(
        "o componente de rede instalado é de uma versão anterior. Atualize-o \
         (aba Sistema, Componente de rede, Atualizar) e conecte de novo"
    )]
    HelperOutdated,

    /// Problema no diretório do socket VICI.
    #[error("diretório de runtime: {0}")]
    RuntimeDir(String),

    /// Erro devolvido pela Docker API.
    #[error("erro do Docker: {0}")]
    Docker(#[from] bollard::errors::Error),

    /// Erro de E/S local.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Diretório padrão do socket VICI: `$XDG_RUNTIME_DIR/ipsec-client`.
///
/// # Errors
/// [`EngineError::RuntimeDir`] se `XDG_RUNTIME_DIR` não estiver definido.
pub fn default_runtime_dir() -> Result<PathBuf, EngineError> {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(|dir| PathBuf::from(dir).join("ipsec-client"))
        .filter(|dir| dir.is_absolute())
        .ok_or_else(|| {
            EngineError::RuntimeDir(
                "XDG_RUNTIME_DIR não definido (sessão sem systemd-logind?)".to_owned(),
            )
        })
}

/// gid primário do usuário atual (dono do socket VICI).
#[must_use]
pub fn current_gid() -> u32 {
    rustix::process::getgid().as_raw()
}
