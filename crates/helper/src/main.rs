//! `ipsec-client-helper`: o único componente do IPsec Client que roda como
//! root (ADR-0014). Sobe e derruba o engine embutido sob sandbox do systemd.
//!
//! Em produção é ativado pelo systemd (`ipsec-client-helper.socket`). Para
//! testes, o root pode iniciá-lo à mão:
//!
//! ```text
//! sudo ipsec-client-helper --socket /run/teste.sock --allow-uid 1000 \
//!     --engine-dir /caminho/engine --libexec-dir /caminho/libexec
//! ```
//!
//! Protocolo e validação: crate `ipsec-client-proto`. O helper nunca recebe
//! segredos (eles vão do app direto ao charon, pelo socket VICI).

mod authz;
mod engine;
mod systemd;

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use ipsec_client_proto::{MAX_LINE, Request, Response, decode, encode};

use crate::authz::{Authorizer, peer_of};
use crate::engine::Paths;

const DEFAULT_ENGINE_DIR: &str = "/opt/ipsec-client/engine";
const DEFAULT_LIBEXEC_DIR: &str = "/opt/ipsec-client/libexec";
/// Conexões atendidas ao mesmo tempo. O socket aceita qualquer usuário: sem
/// limite (e com uma conexão por vez), um cliente parado travaria o helper.
const MAX_CLIENTS: usize = 8;
/// Tempo para o cliente mandar a linha do pedido.
const READ_TIMEOUT: Duration = Duration::from_secs(3);

static ACTIVE_CLIENTS: AtomicUsize = AtomicUsize::new(0);

/// Libera a vaga da conexão mesmo se a thread entrar em pânico.
struct ClientSlot;

impl ClientSlot {
    fn take() -> Option<Self> {
        if ACTIVE_CLIENTS.fetch_add(1, Ordering::SeqCst) >= MAX_CLIENTS {
            ACTIVE_CLIENTS.fetch_sub(1, Ordering::SeqCst);
            None
        } else {
            Some(Self)
        }
    }
}

impl Drop for ClientSlot {
    fn drop(&mut self) {
        ACTIVE_CLIENTS.fetch_sub(1, Ordering::SeqCst);
    }
}

#[derive(Debug)]
struct Options {
    socket: Option<PathBuf>,
    paths: Paths,
    authorizer: Authorizer,
}

fn parse_args() -> Result<Options, String> {
    let mut socket = None;
    let mut engine_dir = PathBuf::from(DEFAULT_ENGINE_DIR);
    let mut libexec_dir = PathBuf::from(DEFAULT_LIBEXEC_DIR);
    let mut allow = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(format!("{arg} requires a value"));
        match arg.as_str() {
            "--socket" => socket = Some(PathBuf::from(value()?)),
            "--engine-dir" => engine_dir = PathBuf::from(value()?),
            "--libexec-dir" => libexec_dir = PathBuf::from(value()?),
            "--allow-uid" => allow.push(value()?.parse().map_err(|_| "invalid uid".to_owned())?),
            "--version" => {
                println!("ipsec-client-helper {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(Options {
        socket,
        paths: Paths {
            engine_dir,
            libexec_dir,
        },
        authorizer: if allow.is_empty() {
            Authorizer::Polkit
        } else {
            Authorizer::AllowList(allow)
        },
    })
}

/// Socket herdado do systemd (ativação por socket: descritor 3).
fn inherited_listener() -> Option<UnixListener> {
    let pid_matches = std::env::var("LISTEN_PID")
        .ok()
        .and_then(|p| p.parse::<u32>().ok())
        == Some(std::process::id());
    let fds: u32 = std::env::var("LISTEN_FDS").ok()?.parse().ok()?;
    if !pid_matches || fds < 1 {
        return None;
    }
    use std::os::fd::FromRawFd;
    // SAFETY: com LISTEN_PID igual ao nosso pid e LISTEN_FDS >= 1, o protocolo
    // de ativação do systemd garante que o descritor 3 é um socket aberto e que
    // pertence a este processo. Ele é adotado uma única vez, aqui.
    #[allow(unsafe_code)]
    let listener = unsafe { UnixListener::from_raw_fd(3) };
    Some(listener)
}

fn bind(path: &PathBuf) -> std::io::Result<UnixListener> {
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path)?;
    // Qualquer usuário conecta; quem pode agir é decidido por SO_PEERCRED + polkit.
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o666))?;
    Ok(listener)
}

fn serve(mut stream: UnixStream, options: &Options) -> std::io::Result<()> {
    stream.set_read_timeout(Some(READ_TIMEOUT))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    let peer = peer_of(&stream)?;
    let mut line = String::new();
    BufReader::new((&stream).take(MAX_LINE as u64 + 1)).read_line(&mut line)?;

    let response = match decode::<Request>(&line) {
        Err(e) => Response::Error {
            message: e.to_string(),
        },
        Ok(Request::Ping) => engine::handle(Request::Ping, &peer, &options.paths),
        Ok(request) => match options.authorizer.authorize(&peer) {
            Ok(()) => {
                eprintln!("uid {}: {}", peer.uid, describe(&request));
                engine::handle(request, &peer, &options.paths)
            }
            Err(message) => {
                eprintln!("uid {}: refused ({message})", peer.uid);
                Response::Error { message }
            }
        },
    };
    let encoded = encode(&response)
        .unwrap_or_else(|_| "{\"result\":\"error\",\"message\":\"internal failure\"}\n".to_owned());
    stream.write_all(encoded.as_bytes())
}

/// Descrição curta para o log (sem nada sensível: não há segredos aqui).
fn describe(request: &Request) -> String {
    match request {
        Request::Ping => "ping".to_owned(),
        Request::Start(p) => format!("start (profile {})", p.profile_id),
        Request::Stop => "stop".to_owned(),
        Request::Status => "status".to_owned(),
        Request::Logs { tail } => format!("logs ({tail})"),
    }
}

fn main() {
    let options = match parse_args() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("ipsec-client-helper: {e}");
            std::process::exit(2);
        }
    };
    if !rustix::process::geteuid().is_root() {
        eprintln!("ipsec-client-helper: must run as root (from systemd)");
        std::process::exit(1);
    }
    let listener = match (&options.socket, inherited_listener()) {
        (Some(path), _) => bind(path),
        (None, Some(listener)) => Ok(listener),
        (None, None) => Err(std::io::Error::other(
            "no socket: use systemd socket activation or --socket",
        )),
    };
    let listener = match listener {
        Ok(l) => l,
        Err(e) => {
            eprintln!("ipsec-client-helper: {e}");
            std::process::exit(1);
        }
    };
    eprintln!(
        "ipsec-client-helper {} ready (authorization: {})",
        env!("CARGO_PKG_VERSION"),
        match options.authorizer {
            Authorizer::Polkit => "polkit",
            Authorizer::AllowList(_) => "uid list (test)",
        }
    );
    let options = Arc::new(options);
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let Some(slot) = ClientSlot::take() else {
                    eprintln!("connection refused: {MAX_CLIENTS} already being served");
                    continue;
                };
                let options = Arc::clone(&options);
                std::thread::spawn(move || {
                    let _slot = slot;
                    if let Err(e) = serve(stream, &options) {
                        eprintln!("connection closed with an error: {e}");
                    }
                });
            }
            Err(e) => eprintln!("failed to accept a connection: {e}"),
        }
    }
}
