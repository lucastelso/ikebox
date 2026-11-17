//! Engines: the embedded one, through the network component (ADRs 0013, 0014
//! and 0019), and, with the `docker` feature only, the container with the
//! strongSwan of the distribution (ADRs 0001, 0002, 0005 and 0006), for
//! development and tests. Release builds never have the second.
//!
//! - [`Backend`]: which engine this computer uses;
//! - [`NativeEngine`]: client of the network component;
//! - `context`, `EngineSpec`, `DockerEngine` (feature `docker`): the files of
//!   `engine/` embedded for the image build, the container configuration and
//!   its lifecycle.

mod backend;
#[cfg(feature = "docker")]
pub mod context;
#[cfg(feature = "docker")]
mod docker;
mod native;
#[cfg(feature = "docker")]
mod spec;

use std::path::PathBuf;

pub use backend::{Backend, BuildProgress, RunningEngine};
#[cfg(feature = "docker")]
pub use docker::{DockerEngine, READY_LINE, READY_TIMEOUT, STOP_TIMEOUT_SECS};
pub use native::NativeEngine;
#[cfg(feature = "docker")]
pub use spec::{
    CONTAINER_NAME, CONTAINER_SOCKET_DIR, EngineSpec, HostSecurity, MANAGED_LABEL, PROFILE_LABEL,
    SOCKET_FILE,
};

/// Nome padrão da interface XFRM.
pub const DEFAULT_IFACE: &str = "ipsc0";
/// MTU padrão da interface XFRM (evita blackhole em Wi-Fi, ADR-0004).
pub const DEFAULT_MTU: u16 = 1400;
/// Caminho do socket D-Bus do sistema no host.
pub const DBUS_SYSTEM_SOCKET: &str = "/run/dbus/system_bus_socket";

/// Whether this build carries the Docker engine (the `docker` feature).
pub const HAS_DOCKER_ENGINE: bool = cfg!(feature = "docker");

/// Engine errors. Messages are for the end user (English or Portuguese, see
/// [`crate::i18n`]).
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    /// The Docker daemon is not reachable.
    #[error("{}", crate::trf!(
        "Docker is not reachable ({}). Check that the service is running \
         (systemctl status docker) and that your user is in the docker group \
         (sudo usermod -aG docker $USER, then log out and back in)",
        "o Docker não está acessível ({}). Verifique se o serviço está ativo \
         (systemctl status docker) e se o seu usuário está no grupo docker \
         (sudo usermod -aG docker $USER, depois saia e entre na sessão)",
        .0
    ))]
    DockerUnavailable(String),

    /// Invalid configuration.
    #[error("{}", crate::trf!("invalid engine configuration: {}", "configuração inválida do engine: {}", .0))]
    InvalidSpec(String),

    /// The image build failed.
    #[error("{}", crate::trf!("failed to build the engine image: {}", "falha ao construir a imagem do engine: {}", .0))]
    Build(String),

    /// The engine did not become ready.
    #[error("{}", crate::trf!("the engine did not become ready: {}", "o engine não ficou pronto: {}", .reason))]
    NotReady {
        /// Short reason.
        reason: String,
        /// Last lines of the container log (no secrets).
        recent_logs: Vec<String>,
    },

    /// Network component (helper of the embedded engine).
    #[error("{0}")]
    Helper(String),

    /// The installed network component is older than the app and does not
    /// understand the request (e.g. the DNS that moved to the helper in 0.2.0).
    #[error("{}", crate::tr!(
        "the installed network component is from an older version. Update it \
         (System tab, Network component, Update) and connect again",
        "o componente de rede instalado é de uma versão anterior. Atualize-o \
         (aba Sistema, Componente de rede, Atualizar) e conecte de novo"
    ))]
    HelperOutdated,

    /// Problem with the VICI socket directory.
    #[error("{}", crate::trf!("runtime directory: {}", "diretório de runtime: {}", .0))]
    RuntimeDir(String),

    /// Error returned by the Docker API.
    #[error("{}", crate::trf!("Docker error: {}", "erro do Docker: {}", .0))]
    Docker(String),

    /// This build has no Docker engine (release builds), and it was asked
    /// for (`IPSEC_CLIENT_BACKEND=docker`).
    #[error("{}", crate::tr!(
        "this build of IPsec Client has no Docker engine; install the network \
         component (System tab)",
        "esta versão do IPsec Client não tem o engine em Docker; instale o \
         componente de rede (aba Sistema)"
    ))]
    NoDockerEngine,

    /// Local I/O error.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[cfg(feature = "docker")]
impl From<bollard::errors::Error> for EngineError {
    fn from(e: bollard::errors::Error) -> Self {
        Self::Docker(e.to_string())
    }
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
                crate::tr!(
                    "XDG_RUNTIME_DIR is not set (session without systemd-logind?)",
                    "XDG_RUNTIME_DIR não definido (sessão sem systemd-logind?)"
                )
                .to_owned(),
            )
        })
}

/// gid primário do usuário atual (dono do socket VICI).
#[must_use]
pub fn current_gid() -> u32 {
    rustix::process::getgid().as_raw()
}
