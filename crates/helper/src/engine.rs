//! Operações do helper sobre o engine: iniciar, parar, estado e log.

use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ipsec_client_proto::{
    EngineStatus, MAX_LOG_LINES, Request, Response, SOCKET_ROOT, StartParams, socket_dir,
    vici_socket,
};

use crate::authz::Peer;
use crate::systemd::{Launch, engine_logs, engine_run_args, engine_status, run, stop_engine};

/// Tempo máximo para o engine abrir o socket VICI.
const READY_TIMEOUT: Duration = Duration::from_secs(25);

/// Onde estão os arquivos do engine (fixos na instalação; o cliente não muda).
#[derive(Debug, Clone)]
pub struct Paths {
    /// strongSwan embutido.
    pub engine_dir: PathBuf,
    /// `entrypoint.sh` e `updown`.
    pub libexec_dir: PathBuf,
}

/// Atende uma requisição já autorizada.
pub fn handle(request: Request, peer: &Peer, paths: &Paths) -> Response {
    let result = match request {
        Request::Ping => Ok(Response::Pong {
            version: env!("CARGO_PKG_VERSION").to_owned(),
        }),
        Request::Start(params) => start(&params, peer, paths),
        Request::Stop => stop(),
        Request::Status => Ok(Response::Status(engine_status())),
        Request::Logs { tail } => Ok(Response::Logs {
            lines: engine_logs(tail.clamp(1, MAX_LOG_LINES)),
        }),
    };
    result.unwrap_or_else(|message| Response::Error { message })
}

fn stop() -> Result<Response, String> {
    let was_running = engine_status().running;
    if was_running {
        stop_engine()?;
    }
    Ok(Response::Stopped { was_running })
}

fn start(params: &StartParams, peer: &Peer, paths: &Paths) -> Result<Response, String> {
    params.validate()?;
    let current: EngineStatus = engine_status();
    if current.running {
        if current.owner_uid != Some(peer.uid) {
            return Err("a VPN já está em uso por outro usuário deste computador".to_owned());
        }
        // Mesmo usuário: substitui pelo caminho gracioso (ADR-0002).
        stop_engine()?;
    }
    let charon = paths.engine_dir.join("libexec/ipsec/charon");
    let entrypoint = paths.libexec_dir.join("entrypoint.sh");
    if !charon.is_file() || !entrypoint.is_file() {
        return Err("componente de rede incompleto; reinstale o IPsec Client".to_owned());
    }
    prepare_socket_dir(peer.uid, peer.gid)?;

    let args = engine_run_args(&Launch {
        params,
        uid: peer.uid,
        gid: peer.gid,
        engine_dir: &paths.engine_dir,
        libexec_dir: &paths.libexec_dir,
    });
    let (ok, _, err) = run("systemd-run", &args);
    if !ok {
        return Err(format!("falha ao iniciar o engine: {}", err.trim()));
    }
    wait_ready(peer.uid)
}

/// `/run/ipsec-client` (0755 root) e `/run/ipsec-client/<uid>` (0750
/// root:<gid>), sem symlinks, e sem o socket velho de uma sessão anterior.
fn prepare_socket_dir(uid: u32, gid: u32) -> Result<(), String> {
    let io = |e: std::io::Error| format!("diretório do socket: {e}");
    ensure_dir(Path::new(SOCKET_ROOT), 0o755).map_err(io)?;
    let dir = PathBuf::from(socket_dir(uid));
    ensure_dir(&dir, 0o750).map_err(io)?;
    rustix::fs::chown(
        &dir,
        Some(rustix::fs::Uid::ROOT),
        Some(rustix::fs::Gid::from_raw(gid)),
    )
    .map_err(|e| format!("diretório do socket: {e}"))?;
    match std::fs::remove_file(vici_socket(uid)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io(e)),
    }
}

fn ensure_dir(dir: &Path, mode: u32) -> std::io::Result<()> {
    match std::fs::DirBuilder::new().mode(mode).create(dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    let meta = std::fs::symlink_metadata(dir)?;
    if !meta.file_type().is_dir() {
        return Err(std::io::Error::other(format!(
            "{} não é um diretório",
            dir.display()
        )));
    }
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(mode))
}

fn wait_ready(uid: u32) -> Result<Response, String> {
    let socket = vici_socket(uid);
    let started = Instant::now();
    while started.elapsed() < READY_TIMEOUT {
        if UnixStream::connect(&socket).is_ok() {
            return Ok(Response::Started { socket });
        }
        if started.elapsed() > Duration::from_secs(1) && !engine_status().running {
            let logs = engine_logs(15).join("\n");
            return Err(format!(
                "o engine terminou durante a inicialização:\n{logs}"
            ));
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    let _ = stop_engine();
    Err("o engine não ficou pronto a tempo".to_owned())
}
