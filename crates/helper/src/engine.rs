//! Operações do helper sobre o engine: iniciar, parar, estado e log.

use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use ipsec_client_proto::{
    EngineStatus, MAX_LOG_LINES, Request, Response, SOCKET_ROOT, StartParams, socket_dir,
    vici_socket,
};

use crate::authz::Peer;
use crate::systemd::{Launch, engine_logs, engine_run_args, engine_status, run, stop_engine};

/// Tempo máximo para o engine abrir o socket VICI.
const READY_TIMEOUT: Duration = Duration::from_secs(25);

/// `Start` e `Stop` mexem na mesma unidade do systemd: um de cada vez (o
/// helper atende conexões em paralelo).
static ENGINE_LOCK: Mutex<()> = Mutex::new(());

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
        Request::Start(params) => {
            let _lock = ENGINE_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
            start(&params, peer, paths)
        }
        Request::Stop => {
            let _lock = ENGINE_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
            stop(peer)
        }
        Request::Status => Ok(Response::Status(status_for(engine_status(), peer))),
        Request::Logs { tail } => logs(peer, tail),
    };
    result.unwrap_or_else(|message| Response::Error { message })
}

/// Só o dono da sessão (ou o root) mexe nela ou lê o log dela: o log tem o
/// gateway, o id do grupo e o login da VPN de quem conectou.
fn owns(status: &EngineStatus, peer: &Peer) -> bool {
    peer.uid == 0 || status.owner_uid == Some(peer.uid)
}

const NOT_OWNER: &str = "the VPN in use belongs to another user of this computer";

/// Para outro usuário, só "há uma VPN rodando", sem o perfil.
fn status_for(status: EngineStatus, peer: &Peer) -> EngineStatus {
    if !status.running || owns(&status, peer) {
        status
    } else {
        EngineStatus {
            running: true,
            profile_id: None,
            owner_uid: status.owner_uid,
        }
    }
}

fn stop(peer: &Peer) -> Result<Response, String> {
    let status = engine_status();
    if !status.running {
        return Ok(Response::Stopped { was_running: false });
    }
    if !owns(&status, peer) {
        return Err(NOT_OWNER.to_owned());
    }
    stop_engine()?;
    Ok(Response::Stopped { was_running: true })
}

fn logs(peer: &Peer, tail: u32) -> Result<Response, String> {
    let status = engine_status();
    // Sem sessão ativa, o log é da última sessão, que pode ser de outro
    // usuário: só o root lê.
    if !(status.running && owns(&status, peer)) && peer.uid != 0 {
        return Ok(Response::Logs { lines: Vec::new() });
    }
    Ok(Response::Logs {
        lines: engine_logs(tail.clamp(1, MAX_LOG_LINES)),
    })
}

fn start(params: &StartParams, peer: &Peer, paths: &Paths) -> Result<Response, String> {
    params.validate()?;
    let current: EngineStatus = engine_status();
    if current.running {
        if current.owner_uid != Some(peer.uid) {
            return Err(NOT_OWNER.to_owned());
        }
        // Mesmo usuário: substitui pelo caminho gracioso (ADR-0002).
        stop_engine()?;
    }
    let charon = paths.engine_dir.join("libexec/ipsec/charon");
    let entrypoint = paths.libexec_dir.join("entrypoint.sh");
    if !charon.is_file() || !entrypoint.is_file() {
        return Err("incomplete network component; reinstall IPsec Client".to_owned());
    }
    prepare_socket_dir(peer.uid)?;

    let args = engine_run_args(&Launch {
        params,
        uid: peer.uid,
        engine_dir: &paths.engine_dir,
        libexec_dir: &paths.libexec_dir,
    });
    let (ok, _, err) = run("systemd-run", &args);
    if !ok {
        return Err(format!("failed to start the engine: {}", err.trim()));
    }
    wait_ready(peer.uid)
}

/// `/run/ipsec-client` (0755 root) e `/run/ipsec-client/<uid>` (0711
/// root:root), sem symlinks, e sem o socket velho de uma sessão anterior.
/// Quem decide o acesso é o socket (0600, dono = uid): o usuário não grava no
/// diretório, então não consegue trocar o socket por um link.
fn prepare_socket_dir(uid: u32) -> Result<(), String> {
    let io = |e: std::io::Error| format!("socket directory: {e}");
    ensure_dir(Path::new(SOCKET_ROOT), 0o755).map_err(io)?;
    let dir = PathBuf::from(socket_dir(uid));
    ensure_dir(&dir, 0o711).map_err(io)?;
    rustix::fs::chown(
        &dir,
        Some(rustix::fs::Uid::ROOT),
        Some(rustix::fs::Gid::ROOT),
    )
    .map_err(|e| format!("socket directory: {e}"))?;
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
            "{} is not a directory",
            dir.display()
        )));
    }
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(mode))
}

/// O socket só está pronto para o usuário depois que o entrypoint o entrega
/// a ele (dono = uid, 0600), o que só acontece depois que o charon o abriu.
/// "O root conectou" não serve como sinal: antes da entrega o app levaria
/// EACCES (bug do passo 26), e depois dela o root do helper, sem
/// CAP_DAC_OVERRIDE, nem consegue conectar num socket 0600 de outro uid.
fn socket_handed_over(socket: &str, uid: u32) -> bool {
    std::fs::symlink_metadata(socket).is_ok_and(|m| {
        m.file_type().is_socket() && m.uid() == uid && m.permissions().mode() & 0o777 == 0o600
    })
}

fn wait_ready(uid: u32) -> Result<Response, String> {
    let socket = vici_socket(uid);
    let started = Instant::now();
    while started.elapsed() < READY_TIMEOUT {
        if socket_handed_over(&socket, uid) {
            return Ok(Response::Started { socket });
        }
        if started.elapsed() > Duration::from_secs(1) && !engine_status().running {
            let logs = engine_logs(15).join("\n");
            return Err(format!("the engine exited during startup:\n{logs}"));
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    let _ = stop_engine();
    Err("the engine did not become ready in time".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    #[test]
    fn socket_is_ready_only_after_handover() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("charon.vici");
        let _listener = UnixListener::bind(&path).expect("bind");
        let socket = path.to_string_lossy().into_owned();
        let uid = rustix::process::getuid().as_raw();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o660)).expect("chmod");
        assert!(
            !socket_handed_over(&socket, uid),
            "0660 (grupo) não é mais aceito"
        );
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("chmod");
        assert!(socket_handed_over(&socket, uid));
        assert!(!socket_handed_over(&socket, uid + 1), "outro uid");
        assert!(!socket_handed_over("/nao/existe", uid));
        // Um arquivo comum com o mesmo nome não é o socket.
        let file = dir.path().join("arquivo");
        std::fs::write(&file, "").expect("write");
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).expect("chmod");
        assert!(!socket_handed_over(&file.to_string_lossy(), uid));
    }

    fn peer(uid: u32) -> Peer {
        Peer {
            pid: 1,
            uid,
            gid: uid,
        }
    }

    #[test]
    fn only_the_owner_sees_the_session() {
        let running = EngineStatus {
            running: true,
            profile_id: Some("trabalho".into()),
            owner_uid: Some(1000),
        };
        assert!(owns(&running, &peer(1000)));
        assert!(owns(&running, &peer(0)), "o root pode tudo");
        assert!(!owns(&running, &peer(1001)));
        assert_eq!(status_for(running.clone(), &peer(1000)), running);
        let other = status_for(running, &peer(1001));
        assert!(other.running);
        assert_eq!(
            other.profile_id, None,
            "o perfil de outro usuário não aparece"
        );
    }
}
