//! Engine lifecycle through the Docker API (bollard), local Unix socket only.
//!
//! Follows the engine contract (docs/architecture.md):
//! - the container is always recreated (ADR-0002); a previous engine is stopped
//!   with `docker stop` (graceful, so the entrypoint cleans up the host) and
//!   removed;
//! - the old socket is deleted before creating the container, and readiness is
//!   the `[engine] ready` line in the log **and** the socket accepting
//!   connections.

use std::collections::{HashMap, VecDeque};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::Path;
use std::time::Duration;

use bollard::Docker;
use bollard::errors::Error as BollardError;
use bollard::query_parameters::{
    BuildImageOptionsBuilder, CreateContainerOptionsBuilder, InspectContainerOptions,
    LogsOptionsBuilder, RemoveContainerOptionsBuilder, StartContainerOptions,
    StopContainerOptionsBuilder,
};
use futures_util::StreamExt;
use tracing::{debug, info};

use super::{EngineError, EngineSpec, context};

/// Linha que o entrypoint escreve quando o socket VICI está liberado.
pub const READY_LINE: &str = "[engine] ready";
/// Tempo máximo para o engine ficar pronto.
pub const READY_TIMEOUT: Duration = Duration::from_secs(20);
/// Tempo do `docker stop` antes do SIGKILL (o entrypoint precisa limpar).
pub const STOP_TIMEOUT_SECS: i32 = 10;
const RECENT_LOG_LINES: usize = 40;

/// Progresso do build da imagem, para mostrar ao usuário.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildProgress {
    /// A imagem já existia; nada a construir.
    UpToDate,
    /// Uma linha de saída do build.
    Step(String),
}

/// Cliente do engine sobre a Docker API.
#[derive(Debug, Clone)]
pub struct DockerEngine {
    docker: Docker,
}

impl DockerEngine {
    /// Conecta ao Docker pelo socket Unix padrão (`/var/run/docker.sock`).
    ///
    /// # Errors
    /// [`EngineError::DockerUnavailable`] se o cliente não puder ser criado.
    pub fn connect() -> Result<Self, EngineError> {
        Docker::connect_with_unix_defaults()
            .map(|docker| Self { docker })
            .map_err(|e| EngineError::DockerUnavailable(e.to_string()))
    }

    /// Verifica se o daemon responde (e se o usuário tem permissão).
    ///
    /// # Errors
    /// [`EngineError::DockerUnavailable`] com a causa.
    pub async fn ping(&self) -> Result<(), EngineError> {
        self.docker
            .ping()
            .await
            .map(|_| ())
            .map_err(|e| EngineError::DockerUnavailable(e.to_string()))
    }

    /// A imagem existe localmente?
    ///
    /// # Errors
    /// Erros do Docker que não sejam "não encontrada".
    pub async fn image_present(&self, tag: &str) -> Result<bool, EngineError> {
        match self.docker.inspect_image(tag).await {
            Ok(_) => Ok(true),
            Err(e) if is_not_found(&e) => Ok(false),
            Err(e) => Err(e.into()),
        }
    }

    /// Garante a imagem do engine embutida neste binário, construindo-a a
    /// partir do contexto em memória se ela não existir (ADR-0006).
    ///
    /// # Errors
    /// [`EngineError::Build`] se o build falhar.
    pub async fn ensure_image(
        &self,
        mut progress: impl FnMut(BuildProgress),
    ) -> Result<String, EngineError> {
        let tag = context::image_tag();
        if self.image_present(&tag).await? {
            progress(BuildProgress::UpToDate);
            return Ok(tag);
        }
        info!(%tag, "building the engine image");
        let labels = HashMap::from([(context::HASH_LABEL, context::content_hash())]);
        let options = BuildImageOptionsBuilder::default()
            .t(&tag)
            .rm(true)
            .forcerm(true)
            .labels(&labels)
            .build();
        let tar = context::tar_archive()?;
        let mut stream =
            self.docker
                .build_image(options, None, Some(bollard::body_full(tar.into())));
        while let Some(item) = stream.next().await {
            let info = item.map_err(|e| EngineError::Build(e.to_string()))?;
            if let Some(detail) = info.error_detail {
                return Err(EngineError::Build(detail.message.unwrap_or_else(|| {
                    crate::tr!("unknown error", "erro desconhecido").to_owned()
                })));
            }
            if let Some(raw) = info.stream.as_deref() {
                let line = strip_ansi(raw);
                let line = line.trim();
                if !line.is_empty() {
                    progress(BuildProgress::Step(line.to_owned()));
                }
            }
        }
        if !self.image_present(&tag).await? {
            return Err(EngineError::Build(
                crate::tr!(
                    "the image did not appear after the build",
                    "a imagem não apareceu após o build"
                )
                .to_owned(),
            ));
        }
        Ok(tag)
    }

    /// Sobe uma instância nova do engine e espera ficar pronta.
    ///
    /// # Errors
    /// Spec inválida, problema no diretório de runtime, erro do Docker ou
    /// [`EngineError::NotReady`] (com as últimas linhas do log).
    pub async fn start(&self, spec: &EngineSpec) -> Result<(), EngineError> {
        spec.validate()?;
        prepare_runtime_dir(&spec.runtime_dir)?;
        // Um engine anterior sai pelo caminho gracioso (o entrypoint limpa o host).
        self.stop(&spec.container_name).await?;
        remove_stale_socket(&spec.socket_path())?;

        let options = CreateContainerOptionsBuilder::default()
            .name(&spec.container_name)
            .build();
        self.docker
            .create_container(Some(options), spec.create_body())
            .await?;
        self.docker
            .start_container(&spec.container_name, None::<StartContainerOptions>)
            .await?;
        info!(container = %spec.container_name, iface = %spec.iface, "engine started");
        self.wait_ready(spec).await
    }

    /// Para (graciosamente) e remove o container. Idempotente.
    ///
    /// # Errors
    /// Erros do Docker que não sejam "não existe" / "já parado".
    pub async fn stop(&self, name: &str) -> Result<(), EngineError> {
        let options = StopContainerOptionsBuilder::default()
            .t(STOP_TIMEOUT_SECS)
            .build();
        match self.docker.stop_container(name, Some(options)).await {
            Ok(()) => debug!(name, "engine stopped"),
            Err(e) if is_not_found(&e) || is_not_modified(&e) => {}
            Err(e) => return Err(e.into()),
        }
        let options = RemoveContainerOptionsBuilder::default().force(true).build();
        match self.docker.remove_container(name, Some(options)).await {
            Ok(()) => Ok(()),
            Err(e) if is_not_found(&e) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    /// O container existe e está rodando?
    ///
    /// # Errors
    /// Erros do Docker que não sejam "não existe".
    pub async fn is_running(&self, name: &str) -> Result<bool, EngineError> {
        match self
            .docker
            .inspect_container(name, None::<InspectContainerOptions>)
            .await
        {
            Ok(info) => Ok(info.state.and_then(|s| s.running).unwrap_or(false)),
            Err(e) if is_not_found(&e) => Ok(false),
            Err(e) => Err(e.into()),
        }
    }

    /// Id do perfil do engine em execução (label do container), se houver.
    ///
    /// # Errors
    /// Erros do Docker que não sejam "não existe".
    pub async fn running_profile(&self, name: &str) -> Result<Option<String>, EngineError> {
        match self
            .docker
            .inspect_container(name, None::<InspectContainerOptions>)
            .await
        {
            Ok(info) => {
                let running = info.state.as_ref().and_then(|s| s.running).unwrap_or(false);
                Ok(running
                    .then(|| info.config.and_then(|c| c.labels))
                    .flatten()
                    .and_then(|mut labels| labels.remove(super::PROFILE_LABEL)))
            }
            Err(e) if is_not_found(&e) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Linhas de log do container (as últimas `tail`; com `follow`, continua
    /// até o container parar). Os logs do engine não contêm segredos, mas
    /// contêm identidades: mascarar antes de exibir.
    pub fn log_lines(
        &self,
        name: &str,
        tail: usize,
        follow: bool,
    ) -> impl futures_util::Stream<Item = Result<String, EngineError>> + '_ {
        let options = LogsOptionsBuilder::default()
            .follow(follow)
            .stdout(true)
            .stderr(true)
            .tail(&tail.to_string())
            .build();
        self.docker
            .logs(name, Some(options))
            .map(|chunk| {
                chunk
                    .map(|c| String::from_utf8_lossy(&c.into_bytes()).into_owned())
                    .map_err(EngineError::from)
            })
            .flat_map(|chunk| {
                let lines: Vec<Result<String, EngineError>> = match chunk {
                    Ok(text) => text.lines().map(|l| Ok(l.trim_end().to_owned())).collect(),
                    Err(e) => vec![Err(e)],
                };
                futures_util::stream::iter(lines)
            })
    }

    async fn wait_ready(&self, spec: &EngineSpec) -> Result<(), EngineError> {
        let options = LogsOptionsBuilder::default()
            .follow(true)
            .stdout(true)
            .stderr(true)
            .build();
        let mut logs = self.docker.logs(&spec.container_name, Some(options));
        let mut recent: VecDeque<String> = VecDeque::with_capacity(RECENT_LOG_LINES);

        let outcome = tokio::time::timeout(READY_TIMEOUT, async {
            let mut pending = String::new();
            while let Some(chunk) = logs.next().await {
                pending.push_str(&String::from_utf8_lossy(&chunk?.into_bytes()));
                while let Some(pos) = pending.find('\n') {
                    let line: String = pending.drain(..=pos).collect();
                    let line = line.trim_end().to_owned();
                    debug!(target: "ipsec_client::engine::log", "{line}");
                    let ready = line.starts_with(READY_LINE);
                    if recent.len() == RECENT_LOG_LINES {
                        recent.pop_front();
                    }
                    recent.push_back(line);
                    if ready {
                        return Ok(true);
                    }
                }
            }
            Ok::<bool, EngineError>(false)
        })
        .await;

        let not_ready = |reason: &str, recent: VecDeque<String>| EngineError::NotReady {
            reason: reason.to_owned(),
            recent_logs: recent.into(),
        };
        match outcome {
            Ok(Ok(true)) => {
                tokio::net::UnixStream::connect(spec.socket_path())
                    .await
                    .map_err(|e| {
                        not_ready(
                            &crate::trf!(
                                "the VICI socket does not accept connections: {e}",
                                "o socket VICI não aceita conexão: {e}"
                            ),
                            recent.clone(),
                        )
                    })?;
                info!("engine ready");
                Ok(())
            }
            Ok(Ok(false)) => Err(not_ready(
                crate::tr!(
                    "the engine exited during startup",
                    "o engine terminou durante a inicialização"
                ),
                recent,
            )),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(not_ready(
                crate::tr!(
                    "timed out waiting for the engine",
                    "tempo esgotado esperando o engine"
                ),
                recent,
            )),
        }
    }
}

/// Cria (ou confere) o diretório do socket: do usuário atual, 0700, sem symlink.
fn prepare_runtime_dir(dir: &Path) -> Result<(), EngineError> {
    match std::fs::DirBuilder::new().mode(0o700).create(dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(EngineError::RuntimeDir(format!("{}: {e}", dir.display()))),
    }
    let meta = std::fs::symlink_metadata(dir)?;
    if !meta.file_type().is_dir() {
        return Err(EngineError::RuntimeDir(crate::trf!(
            "{} is not a directory",
            "{} não é um diretório",
            dir.display()
        )));
    }
    if meta.uid() != rustix::process::getuid().as_raw() {
        return Err(EngineError::RuntimeDir(crate::trf!(
            "{} belongs to another user",
            "{} pertence a outro usuário",
            dir.display()
        )));
    }
    if meta.permissions().mode() & 0o777 != 0o700 {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Apaga um socket velho: um arquivo de socket sozinho não indica prontidão.
fn remove_stale_socket(path: &Path) -> Result<(), EngineError> {
    match std::fs::remove_file(path) {
        Ok(()) => {
            debug!(path = %path.display(), "stale socket removed");
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Remove sequências de escape ANSI (cores) da saída do build.
fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                // Parâmetros e intermediários até o byte final (0x40..=0x7e).
                for c in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&c) {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

fn is_not_found(e: &BollardError) -> bool {
    matches!(
        e,
        BollardError::DockerResponseServerError {
            status_code: 404,
            ..
        }
    )
}

fn is_not_modified(e: &BollardError) -> bool {
    matches!(
        e,
        BollardError::DockerResponseServerError {
            status_code: 304,
            ..
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_dir_is_created_private() {
        let base = tempfile::tempdir().expect("tempdir");
        let dir = base.path().join("ipsec-client");
        prepare_runtime_dir(&dir).expect("cria");
        let mode = std::fs::metadata(&dir).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);

        // Já existente com permissão aberta: corrige para 0700.
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        prepare_runtime_dir(&dir).expect("confere");
        let mode = std::fs::metadata(&dir).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }

    #[test]
    fn runtime_dir_rejects_symlink_and_files() {
        let base = tempfile::tempdir().expect("tempdir");
        let target = base.path().join("alvo");
        std::fs::create_dir(&target).expect("alvo");
        let link = base.path().join("link");
        std::os::unix::fs::symlink(&target, &link).expect("symlink");
        assert!(prepare_runtime_dir(&link).is_err());

        let file = base.path().join("arquivo");
        std::fs::write(&file, b"x").expect("arquivo");
        assert!(prepare_runtime_dir(&file).is_err());
    }

    #[test]
    fn ansi_sequences_are_stripped() {
        assert_eq!(
            strip_ansi("\u{1b}[91m+ chmod 0644 x\n\u{1b}[0m"),
            "+ chmod 0644 x\n"
        );
        assert_eq!(strip_ansi("sem cor"), "sem cor");
        assert_eq!(strip_ansi("\u{1b}[1;36m==>\u{1b}[0m ok"), "==> ok");
    }

    #[test]
    fn stale_socket_removal_is_idempotent() {
        let base = tempfile::tempdir().expect("tempdir");
        let sock = base.path().join("charon.vici");
        std::fs::write(&sock, b"").expect("socket falso");
        remove_stale_socket(&sock).expect("remove");
        assert!(!sock.exists());
        remove_stale_socket(&sock).expect("idempotente");
    }
}
