//! The helper's operations on the engine: start, stop, state and log.

use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use ipsec_client_proto::{
    EngineStatus, MAX_LOG_LINES, Request, Response, SOCKET_ROOT, StartParams, socket_dir,
    vici_socket,
};

use crate::authz::Peer;
use crate::systemd::{
    Launch, LogScope, engine_logs, engine_run_args, engine_status, engine_unit, run, stop_engine,
};

/// Maximum time for the engine to open the VICI socket.
const READY_TIMEOUT: Duration = Duration::from_secs(25);

/// `Start` and `Stop` touch the same systemd unit: one at a time (the helper
/// serves connections in parallel).
static ENGINE_LOCK: Mutex<()> = Mutex::new(());

/// Where the engine's files are (fixed at installation; the client does not
/// change them).
#[derive(Debug, Clone)]
pub struct Paths {
    /// Embedded strongSwan.
    pub engine_dir: PathBuf,
    /// `entrypoint.sh` e `updown`.
    pub libexec_dir: PathBuf,
}

/// Serves an already authorized request.
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

/// Only the session's owner (or root) touches it or reads its log: the log has
/// the gateway, the group id and the VPN login of whoever connected.
fn owns(status: &EngineStatus, peer: &Peer) -> bool {
    peer.uid == 0 || status.owner_uid == Some(peer.uid)
}

const NOT_OWNER: &str = "the VPN in use belongs to another user of this computer";

/// For another user, only "a VPN is running", without the profile.
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
    // State, owner and invocation ID come from a single read: the session
    // cannot change between the ownership check and the choice of the lines.
    let unit = engine_unit();
    let scope = match unit.invocation {
        // The unit name is the same for every session of every user: only
        // the lines of the running session, never those of an earlier one.
        Some(id) if unit.status.running && owns(&unit.status, peer) => LogScope::Invocation(id),
        // Without a session of their own, only root reads (the last sessions
        // may be other users').
        _ if peer.uid == 0 => LogScope::Unit,
        _ => return Ok(Response::Logs { lines: Vec::new() }),
    };
    Ok(Response::Logs {
        lines: engine_logs(&scope, tail.clamp(1, MAX_LOG_LINES)),
    })
}

fn start(params: &StartParams, peer: &Peer, paths: &Paths) -> Result<Response, String> {
    params.validate()?;
    let current: EngineStatus = engine_status();
    if current.running {
        if current.owner_uid != Some(peer.uid) {
            return Err(NOT_OWNER.to_owned());
        }
        // Same user: replace it the graceful way (ADR-0002).
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
    let launched = SystemTime::now();
    let (ok, _, err) = run("systemd-run", &args);
    if !ok {
        return Err(format!("failed to start the engine: {}", err.trim()));
    }
    wait_ready(peer.uid, launched)
}

/// `/run/ipsec-client` (0755 root) and `/run/ipsec-client/<uid>` (0711
/// root:root), without symlinks, and without the old socket of a previous
/// session. The socket decides the access (0600, owner = uid): the user cannot
/// write to the directory, so they cannot swap the socket for a link.
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

/// The socket is only ready for the user after the entrypoint hands it over
/// (owner = uid, 0600), which only happens after charon opened it. "Root
/// connected" is no signal: before the handover the app would get EACCES (the
/// bug of step 26), and after it the helper's root, without
/// CAP_DAC_OVERRIDE, cannot even connect to a 0600 socket of another uid.
fn socket_handed_over(socket: &str, uid: u32) -> bool {
    std::fs::symlink_metadata(socket).is_ok_and(|m| {
        m.file_type().is_socket() && m.uid() == uid && m.permissions().mode() & 0o777 == 0o600
    })
}

fn wait_ready(uid: u32, launched: SystemTime) -> Result<Response, String> {
    let socket = vici_socket(uid);
    let started = Instant::now();
    while started.elapsed() < READY_TIMEOUT {
        if socket_handed_over(&socket, uid) {
            return Ok(Response::Started { socket });
        }
        if started.elapsed() > Duration::from_secs(1) && !engine_status().running {
            // Only what this attempt logged: the unit is gone, and so is its
            // invocation ID.
            let logs = engine_logs(&LogScope::Since(launched), 15).join("\n");
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
        // A regular file with the same name is not the socket.
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
