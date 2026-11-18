//! `ipsec-client-helper`: the only IPsec Client component that runs as root
//! (ADR-0014). It starts and stops the embedded engine under a systemd sandbox.
//!
//! In production it is activated by systemd (`ipsec-client-helper.socket`).
//! For tests, root can start it by hand:
//!
//! ```text
//! sudo ipsec-client-helper --socket /run/test.sock --allow-uid 1000 \
//!     --engine-dir /path/engine --libexec-dir /path/libexec
//! ```
//!
//! Protocol and validation: the `ipsec-client-proto` crate. The helper never
//! receives secrets (they go from the app straight to charon, over the VICI
//! socket).

mod authz;
mod engine;
mod systemd;

use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use ipsec_client_proto::{MAX_LINE, Request, Response, decode, encode};

use crate::authz::{Authorizer, Peer, peer_of};
use crate::engine::Paths;

const DEFAULT_ENGINE_DIR: &str = "/opt/ipsec-client/engine";
const DEFAULT_LIBEXEC_DIR: &str = "/opt/ipsec-client/libexec";
/// Connections served at the same time. The socket accepts any user: without
/// a limit (and with one connection at a time), a stalled client would block
/// the helper.
const MAX_CLIENTS: usize = 8;
/// Connections served at the same time for one uid. Lower than
/// [`MAX_CLIENTS`]: one user who opens connections in a loop cannot take
/// every slot and keep the others from controlling their VPN.
const MAX_CLIENTS_PER_UID: usize = 4;
/// Time for the client to send the **whole** request line. It is a deadline,
/// not a per-read timeout: a client that drips one byte at a time does not
/// hold its slot any longer.
const REQUEST_DEADLINE: Duration = Duration::from_secs(3);

/// Connections being served, in total and per uid.
#[derive(Debug, Default)]
struct Slots {
    total: usize,
    per_uid: HashMap<u32, usize>,
}

impl Slots {
    fn try_take(&mut self, uid: u32) -> bool {
        let mine = self.per_uid.get(&uid).copied().unwrap_or(0);
        if self.total >= MAX_CLIENTS || mine >= MAX_CLIENTS_PER_UID {
            return false;
        }
        self.total += 1;
        self.per_uid.insert(uid, mine + 1);
        true
    }

    fn release(&mut self, uid: u32) {
        self.total = self.total.saturating_sub(1);
        if let Some(mine) = self.per_uid.get_mut(&uid) {
            *mine = mine.saturating_sub(1);
            if *mine == 0 {
                self.per_uid.remove(&uid);
            }
        }
    }
}

static SLOTS: Mutex<Option<Slots>> = Mutex::new(None);

/// Frees the connection slot even if the thread panics.
struct ClientSlot {
    uid: u32,
}

impl ClientSlot {
    fn take(uid: u32) -> Option<Self> {
        let mut slots = SLOTS.lock().unwrap_or_else(PoisonError::into_inner);
        let taken = slots.get_or_insert_with(Slots::default).try_take(uid);
        drop(slots);
        // Built only when a slot was taken (and with the lock released):
        // dropping a `ClientSlot` gives a slot back.
        taken.then(|| Self { uid })
    }
}

impl Drop for ClientSlot {
    fn drop(&mut self) {
        if let Some(slots) = SLOTS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_mut()
        {
            slots.release(self.uid);
        }
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

/// Socket inherited from systemd (socket activation: descriptor 3).
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
    // SAFETY: with LISTEN_PID equal to our pid and LISTEN_FDS >= 1, systemd's
    // activation protocol guarantees that descriptor 3 is an open socket that
    // belongs to this process. It is adopted only once, here.
    #[allow(unsafe_code)]
    let listener = unsafe { UnixListener::from_raw_fd(3) };
    Some(listener)
}

fn bind(path: &PathBuf) -> std::io::Result<UnixListener> {
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path)?;
    // Any user connects; who may act is decided by SO_PEERCRED + polkit.
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o666))?;
    Ok(listener)
}

/// Reads the request line, up to `limit` in total and [`MAX_LINE`] bytes.
/// A longer line comes back as it is, and decoding refuses it.
fn read_request(stream: &UnixStream, limit: Duration) -> std::io::Result<String> {
    let deadline = Instant::now() + limit;
    let mut line = Vec::with_capacity(256);
    let mut chunk = [0_u8; 4096];
    while line.len() <= MAX_LINE {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|left| !left.is_zero())
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "the request did not arrive in time",
                )
            })?;
        stream.set_read_timeout(Some(remaining))?;
        let mut reader = stream;
        let n = reader.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        line.extend_from_slice(&chunk[..n]);
        if let Some(end) = line.iter().position(|byte| *byte == b'\n') {
            line.truncate(end);
            break;
        }
    }
    String::from_utf8(line)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "request is not UTF-8"))
}

fn serve(mut stream: UnixStream, peer: &Peer, options: &Options) -> std::io::Result<()> {
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    let line = read_request(&stream, REQUEST_DEADLINE)?;

    let response = match decode::<Request>(&line) {
        Err(e) => Response::Error {
            message: e.to_string(),
        },
        Ok(Request::Ping) => engine::handle(Request::Ping, peer, &options.paths),
        Ok(request) => match options.authorizer.authorize(peer) {
            Ok(()) => {
                eprintln!("uid {}: {}", peer.uid, describe(&request));
                engine::handle(request, peer, &options.paths)
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

/// Short description for the log (nothing sensitive: there are no secrets
/// here).
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
                let peer = match peer_of(&stream) {
                    Ok(peer) => peer,
                    Err(e) => {
                        eprintln!("connection without credentials: {e}");
                        continue;
                    }
                };
                let Some(slot) = ClientSlot::take(peer.uid) else {
                    eprintln!(
                        "uid {}: connection refused (too many being served)",
                        peer.uid
                    );
                    continue;
                };
                let options = Arc::clone(&options);
                std::thread::spawn(move || {
                    let _slot = slot;
                    if let Err(e) = serve(stream, &peer, &options) {
                        eprintln!("connection closed with an error: {e}");
                    }
                });
            }
            Err(e) => eprintln!("failed to accept a connection: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_line_has_a_total_deadline() {
        // A client that drips one byte at a time, each well inside a per-read
        // timeout, used to hold its slot for hours.
        let (server, mut client) = UnixStream::pair().expect("pair");
        let drip = std::thread::spawn(move || {
            for _ in 0..40 {
                if client.write_all(b"x").is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        });
        let started = Instant::now();
        let err = read_request(&server, Duration::from_millis(400)).expect_err("deadline");
        assert!(
            matches!(
                err.kind(),
                std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
            ),
            "{err}"
        );
        assert!(started.elapsed() < Duration::from_millis(1500));
        drop(server);
        drip.join().expect("drip");
    }

    #[test]
    fn request_line_is_read_up_to_the_newline_and_the_limit() {
        let (server, mut client) = UnixStream::pair().expect("pair");
        client.write_all(b"{\"op\":\"ping\"}\nrest").expect("write");
        let line = read_request(&server, Duration::from_secs(2)).expect("line");
        assert_eq!(line, "{\"op\":\"ping\"}");
        assert_eq!(decode::<Request>(&line).expect("decode"), Request::Ping);

        // End of stream without a newline: what arrived is the line.
        let (server, mut client) = UnixStream::pair().expect("pair");
        client.write_all(b"{\"op\":\"stop\"}").expect("write");
        drop(client);
        let line = read_request(&server, Duration::from_secs(2)).expect("line");
        assert_eq!(decode::<Request>(&line).expect("decode"), Request::Stop);

        // Too long: reading stops, and decoding refuses it.
        let (server, client) = UnixStream::pair().expect("pair");
        let writer = std::thread::spawn(move || {
            let mut client = client;
            let _ = client.write_all(&vec![b'x'; MAX_LINE * 2]);
        });
        let line = read_request(&server, Duration::from_secs(5)).expect("line");
        assert!(line.len() > MAX_LINE && line.len() <= MAX_LINE + 4096);
        assert!(decode::<Request>(&line).is_err());
        drop(server);
        writer.join().expect("writer");
    }

    #[test]
    fn refused_connections_do_not_block_or_free_slots() {
        // uid that no other test uses: the counters are global.
        let uid = 4_000_000_001;
        let held: Vec<ClientSlot> = (0..MAX_CLIENTS_PER_UID)
            .map(|_| ClientSlot::take(uid).expect("slot"))
            .collect();
        // A refusal must neither deadlock nor give back a slot it never had.
        assert!(ClientSlot::take(uid).is_none());
        assert!(ClientSlot::take(uid).is_none());
        drop(held);
        assert!(ClientSlot::take(uid).is_some());
    }

    #[test]
    fn one_uid_cannot_take_every_slot() {
        let mut slots = Slots::default();
        for _ in 0..MAX_CLIENTS_PER_UID {
            assert!(slots.try_take(1001));
        }
        assert!(!slots.try_take(1001), "per-uid limit");
        assert!(slots.try_take(1000), "another user still gets in");
        slots.release(1001);
        assert!(slots.try_take(1001));
        // The total limit still holds.
        let mut slots = Slots::default();
        let mut taken = 0;
        for uid in 0..100 {
            if slots.try_take(uid) {
                taken += 1;
            }
        }
        assert_eq!(taken, MAX_CLIENTS);
        slots.release(0);
        assert!(slots.try_take(500));
        assert!(!slots.per_uid.contains_key(&0), "empty entries are dropped");
    }
}
