//! Escolha do engine: embutido (helper, padrão) ou Docker (testes e quem
//! ainda não instalou o componente de rede). ADR-0013.

use std::path::PathBuf;

use ipsec_client_proto::{DnsParams, StartParams, vici_socket};

use super::{
    BuildProgress, CONTAINER_NAME, DEFAULT_IFACE, DockerEngine, EngineError, EngineSpec,
    NativeEngine, current_gid, default_runtime_dir,
};
use crate::profiles::{Profile, UPDOWN_PATH};

/// Engine em execução: o que a sessão precisa para negociar.
#[derive(Debug, Clone)]
pub struct RunningEngine {
    /// Socket VICI.
    pub socket: PathBuf,
    /// Interface XFRM.
    pub iface: String,
    /// `if_id` das SAs/políticas.
    pub if_id: u32,
    /// `updown` a mandar no `load-conn`: o do engine Docker, ou `None` no
    /// embutido, onde o root fixa o comando (ADR-0019).
    pub updown: Option<&'static str>,
}

/// Engine disponível neste computador.
#[derive(Debug, Clone)]
pub enum Backend {
    /// Engine embutido, via `ipsec-client-helper`.
    Native(NativeEngine),
    /// Engine em container Docker.
    Docker(DockerEngine),
}

impl Backend {
    /// Escolhe o engine: `IPSEC_CLIENT_BACKEND=native|docker` força; senão, o
    /// embutido quando o componente de rede está instalado, e o Docker como
    /// reserva.
    ///
    /// # Errors
    /// Docker indisponível quando ele é a única opção.
    pub fn detect() -> Result<Self, EngineError> {
        let native = NativeEngine::default();
        match std::env::var("IPSEC_CLIENT_BACKEND").as_deref() {
            Ok("native") => return Ok(Self::Native(native)),
            Ok("docker") => return Ok(Self::Docker(DockerEngine::connect()?)),
            _ => {}
        }
        if native.installed() {
            Ok(Self::Native(native))
        } else {
            Ok(Self::Docker(DockerEngine::connect()?))
        }
    }

    /// Nome para diagnóstico.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Native(_) => "embutido",
            Self::Docker(_) => "Docker",
        }
    }

    /// Deixa o engine pronto para subir: imagem (Docker) ou helper respondendo.
    ///
    /// # Errors
    /// Build da imagem ou helper indisponível.
    pub async fn prepare(&self, progress: impl FnMut(BuildProgress)) -> Result<(), EngineError> {
        match self {
            Self::Docker(docker) => docker.ensure_image(progress).await.map(|_| ()),
            Self::Native(native) => {
                let native = native.clone();
                blocking(move || native.ping().map(|_| ())).await
            }
        }
    }

    /// Sobe o engine para o perfil.
    ///
    /// # Errors
    /// Falha ao subir ou ficar pronto.
    pub async fn start(
        &self,
        profile: &Profile,
        privileged: bool,
    ) -> Result<RunningEngine, EngineError> {
        let if_id = profile.if_id();
        match self {
            Self::Docker(docker) => {
                let mut spec = EngineSpec::new(default_runtime_dir()?, current_gid(), if_id);
                spec.privileged = privileged;
                spec.mtu = profile.network.mtu;
                spec.profile_id = Some(profile.id.clone());
                docker.start(&spec).await?;
                Ok(RunningEngine {
                    socket: spec.socket_path(),
                    iface: spec.iface,
                    if_id,
                    updown: Some(UPDOWN_PATH),
                })
            }
            Self::Native(native) => {
                let params = StartParams {
                    iface: DEFAULT_IFACE.to_owned(),
                    if_id,
                    mtu: profile.network.mtu,
                    log_level: 1,
                    profile_id: profile.id.clone(),
                    dns: dns_params(profile),
                };
                let native = native.clone();
                let socket = blocking(move || native.start(&params)).await?;
                Ok(RunningEngine {
                    socket,
                    iface: DEFAULT_IFACE.to_owned(),
                    if_id,
                    updown: None,
                })
            }
        }
    }

    /// Para o engine. Devolve se havia um rodando.
    ///
    /// # Errors
    /// Docker/helper indisponível.
    pub async fn stop(&self) -> Result<bool, EngineError> {
        match self {
            Self::Docker(docker) => {
                let was_running = docker.is_running(CONTAINER_NAME).await?;
                docker.stop(CONTAINER_NAME).await?;
                Ok(was_running)
            }
            Self::Native(native) => {
                let native = native.clone();
                blocking(move || native.stop()).await
            }
        }
    }

    /// Perfil do engine em execução (`None` = parado).
    ///
    /// # Errors
    /// Docker/helper indisponível.
    pub async fn running_profile(&self) -> Result<Option<String>, EngineError> {
        match self {
            Self::Docker(docker) => {
                if !docker.is_running(CONTAINER_NAME).await? {
                    return Ok(None);
                }
                Ok(Some(
                    docker
                        .running_profile(CONTAINER_NAME)
                        .await?
                        .unwrap_or_else(|| "desconhecido".to_owned()),
                ))
            }
            Self::Native(native) => {
                let native = native.clone();
                let status = blocking(move || native.status()).await?;
                Ok(status.running.then(|| {
                    status
                        .profile_id
                        .unwrap_or_else(|| "desconhecido".to_owned())
                }))
            }
        }
    }

    /// Socket VICI do engine deste usuário.
    ///
    /// # Errors
    /// Sem `XDG_RUNTIME_DIR` (Docker).
    pub fn socket_path(&self) -> Result<PathBuf, EngineError> {
        match self {
            Self::Docker(_) => Ok(default_runtime_dir()?.join(super::SOCKET_FILE)),
            Self::Native(_) => Ok(PathBuf::from(vici_socket(
                rustix::process::getuid().as_raw(),
            ))),
        }
    }

    /// Últimas linhas do log do engine (identidades ainda não mascaradas).
    ///
    /// # Errors
    /// Docker/helper indisponível.
    pub async fn logs(&self, tail: usize) -> Result<Vec<String>, EngineError> {
        match self {
            Self::Docker(docker) => {
                use futures_util::StreamExt;
                let mut stream = Box::pin(docker.log_lines(CONTAINER_NAME, tail, false));
                let mut lines = Vec::new();
                while let Some(line) = stream.next().await {
                    lines.push(line?);
                }
                Ok(lines)
            }
            Self::Native(native) => {
                let native = native.clone();
                let tail = u32::try_from(tail).unwrap_or(u32::MAX);
                blocking(move || native.logs(tail)).await
            }
        }
    }
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, EngineError> + Send + 'static,
) -> Result<T, EngineError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| EngineError::Helper(format!("falha interna: {e}")))?
}

/// DNS do perfil para o helper (o `updown` do engine embutido o aplica).
fn dns_params(profile: &Profile) -> DnsParams {
    let dns = &profile.network.dns;
    DnsParams {
        servers: dns.servers.clone(),
        domains: dns.domains.clone(),
        use_received: dns.use_received,
        default_route: dns.default_route,
    }
}
