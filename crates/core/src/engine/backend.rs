//! Engine choice: embedded (helper, the default) or, only in builds with the
//! `docker` feature, Docker (development and tests). ADR-0013.

use std::path::PathBuf;

use ipsec_client_proto::{DnsParams, StartParams, vici_socket};

#[cfg(feature = "docker")]
use super::{CONTAINER_NAME, DockerEngine, EngineSpec, current_gid, default_runtime_dir};
use super::{DEFAULT_IFACE, EngineError, NativeEngine};
use crate::profiles::Profile;
#[cfg(feature = "docker")]
use crate::profiles::UPDOWN_PATH;

/// Progress of getting the engine ready (image build, in Docker), to show the
/// user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildProgress {
    /// The image already existed; nothing to build.
    UpToDate,
    /// A line of build output.
    Step(String),
}

/// Running engine: what the session needs to negotiate.
#[derive(Debug, Clone)]
pub struct RunningEngine {
    /// VICI socket.
    pub socket: PathBuf,
    /// XFRM interface.
    pub iface: String,
    /// `if_id` of the SAs and policies.
    pub if_id: u32,
    /// `updown` to send in `load-conn`: the Docker engine's, or `None` in the
    /// embedded one, where root fixes the command (ADR-0019).
    pub updown: Option<&'static str>,
}

/// Engine available on this computer.
#[derive(Debug, Clone)]
pub enum Backend {
    /// Embedded engine, through `ipsec-client-helper`.
    Native(NativeEngine),
    /// Engine in a Docker container (the `docker` feature).
    #[cfg(feature = "docker")]
    Docker(DockerEngine),
}

impl Backend {
    /// Chooses the engine: `IPSEC_CLIENT_BACKEND=native|docker` forces one;
    /// otherwise, the embedded one. Only builds with the `docker` feature fall back
    /// to Docker when the network component is not installed; release builds say
    /// to install it (diagnostics).
    ///
    /// # Errors
    /// Docker unavailable when it is the only option, or asked for in a build
    /// without it.
    pub fn detect() -> Result<Self, EngineError> {
        let native = NativeEngine::default();
        match std::env::var("IPSEC_CLIENT_BACKEND").as_deref() {
            Ok("native") => return Ok(Self::Native(native)),
            #[cfg(feature = "docker")]
            Ok("docker") => return Ok(Self::Docker(DockerEngine::connect()?)),
            #[cfg(not(feature = "docker"))]
            Ok("docker") => return Err(EngineError::NoDockerEngine),
            _ => {}
        }
        #[cfg(feature = "docker")]
        if !native.installed() {
            return Ok(Self::Docker(DockerEngine::connect()?));
        }
        Ok(Self::Native(native))
    }

    /// Is it the Docker engine?
    #[must_use]
    pub fn is_docker(&self) -> bool {
        match self {
            Self::Native(_) => false,
            #[cfg(feature = "docker")]
            Self::Docker(_) => true,
        }
    }

    /// Name for diagnostics.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Native(_) => "embutido",
            #[cfg(feature = "docker")]
            Self::Docker(_) => "Docker",
        }
    }

    /// Gets the engine ready to start: image (Docker) or helper answering.
    ///
    /// # Errors
    /// Image build failed or helper unavailable.
    pub async fn prepare(&self, progress: impl FnMut(BuildProgress)) -> Result<(), EngineError> {
        #[cfg(not(feature = "docker"))]
        let _ = progress;
        match self {
            #[cfg(feature = "docker")]
            Self::Docker(docker) => docker.ensure_image(progress).await.map(|_| ()),
            Self::Native(native) => {
                let native = native.clone();
                blocking(move || native.ping().map(|_| ())).await
            }
        }
    }

    /// Starts the engine for the profile.
    ///
    /// # Errors
    /// Failed to start or to become ready.
    pub async fn start(
        &self,
        profile: &Profile,
        privileged: bool,
    ) -> Result<RunningEngine, EngineError> {
        let if_id = profile.if_id();
        #[cfg(not(feature = "docker"))]
        let _ = privileged;
        match self {
            #[cfg(feature = "docker")]
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

    /// Stops the engine. Returns whether one was running.
    ///
    /// # Errors
    /// Docker or helper unavailable.
    pub async fn stop(&self) -> Result<bool, EngineError> {
        match self {
            #[cfg(feature = "docker")]
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

    /// Profile of the running engine (`None` = stopped).
    ///
    /// # Errors
    /// Docker or helper unavailable.
    pub async fn running_profile(&self) -> Result<Option<String>, EngineError> {
        match self {
            #[cfg(feature = "docker")]
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

    /// VICI socket of this user's engine.
    ///
    /// # Errors
    /// No `XDG_RUNTIME_DIR` (Docker).
    pub fn socket_path(&self) -> Result<PathBuf, EngineError> {
        match self {
            #[cfg(feature = "docker")]
            Self::Docker(_) => Ok(default_runtime_dir()?.join(super::SOCKET_FILE)),
            Self::Native(_) => Ok(PathBuf::from(vici_socket(
                rustix::process::getuid().as_raw(),
            ))),
        }
    }

    /// Last lines of the engine's log (identities not masked yet).
    ///
    /// # Errors
    /// Docker or helper unavailable.
    pub async fn logs(&self, tail: usize) -> Result<Vec<String>, EngineError> {
        match self {
            #[cfg(feature = "docker")]
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
    tokio::task::spawn_blocking(f).await.map_err(|e| {
        EngineError::Helper(crate::trf!("internal failure: {e}", "falha interna: {e}"))
    })?
}

/// The profile's DNS for the helper (the embedded engine's `updown` applies
/// it).
fn dns_params(profile: &Profile) -> DnsParams {
    let dns = &profile.network.dns;
    DnsParams {
        servers: dns.servers.clone(),
        domains: dns.domains.clone(),
        use_received: dns.use_received,
        default_route: dns.default_route,
    }
}
