//! The key agent of a smartcard or token (ADR-0029): while the VPN is up,
//! this process listens on the agent socket of the session
//! (`/run/ikebox/<uid>/agent/agent.sock`) and signs, with the token,
//! what charon asks over the ssh-agent protocol. Only the subset strongSwan's
//! agent plugin uses: list the identities, sign.
//!
//! Only this user and charon's group (the group of the agent directory, set
//! by root) can reach it, and `SO_PEERCRED` is checked on every connection.
//! Requests are size-limited, and every answer goes out in one write
//! (strongSwan reads it with a single `read`).

use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::task::JoinHandle;
use tracing::{debug, warn};

use crate::secrets::TokenSigner;
use crate::secrets::token::put_string;

/// ssh-agent message types (draft-miller-ssh-agent).
const FAILURE: u8 = 5;
const REQUEST_IDENTITIES: u8 = 11;
const IDENTITIES_ANSWER: u8 = 12;
const SIGN_REQUEST: u8 = 13;
const SIGN_RESPONSE: u8 = 14;

/// Largest request read: a key blob and the IKE data to sign fit easily.
const MAX_REQUEST: usize = 64 * 1024;

/// The running agent. Dropping it stops the agent and removes its socket.
#[derive(Debug)]
pub struct AgentHandle {
    task: JoinHandle<()>,
    path: PathBuf,
}

impl Drop for AgentHandle {
    fn drop(&mut self) {
        self.task.abort();
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Starts the agent at `path` with `signer`. The directory must exist (root
/// makes it for the session, owned by this user, group charon's).
///
/// # Errors
/// The directory is not usable or the socket cannot be created.
pub fn start(path: &Path, signer: TokenSigner) -> std::io::Result<AgentHandle> {
    let dir = path
        .parent()
        .ok_or_else(|| std::io::Error::other("agent socket without a directory"))?;
    let meta = std::fs::symlink_metadata(dir)?;
    if !meta.is_dir() || meta.uid() != rustix::process::getuid().as_raw() {
        return Err(std::io::Error::other(format!(
            "{} is not this user's agent directory",
            dir.display()
        )));
    }
    let engine_gid = meta.gid();
    // A leftover socket of an earlier session (ours: it is our directory).
    match std::fs::symlink_metadata(path) {
        Ok(_) => std::fs::remove_file(path)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let listener = UnixListener::bind(path)?;
    // charon must be able to connect. Only this user and charon's group can
    // enter the directory (0750, set by root), and every connection is
    // checked below, so the socket itself may be open to all.
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o666))?;
    let signer = Arc::new(Mutex::new(signer));
    let task = tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            let signer = Arc::clone(&signer);
            tokio::spawn(async move {
                if let Err(e) = serve(stream, engine_gid, signer).await {
                    debug!("key agent connection ended: {e}");
                }
            });
        }
    });
    Ok(AgentHandle {
        task,
        path: path.to_owned(),
    })
}

async fn serve(
    mut stream: UnixStream,
    engine_gid: u32,
    signer: Arc<Mutex<TokenSigner>>,
) -> std::io::Result<()> {
    let peer = stream.peer_cred()?;
    let own = rustix::process::getuid().as_raw();
    if peer.uid() != own && peer.gid() != engine_gid {
        warn!("key agent: connection from uid {} refused", peer.uid());
        return Ok(());
    }
    loop {
        let mut len = [0u8; 4];
        if stream.read_exact(&mut len).await.is_err() {
            return Ok(()); // closed
        }
        let len = u32::from_be_bytes(len) as usize;
        if len == 0 || len > MAX_REQUEST {
            return Err(std::io::Error::other("invalid request length"));
        }
        let mut body = vec![0u8; len];
        stream.read_exact(&mut body).await?;
        let reply = {
            let signer = Arc::clone(&signer);
            tokio::task::spawn_blocking(move || answer(&body, &signer))
                .await
                .unwrap_or_else(|_| vec![FAILURE])
        };
        let mut frame = Vec::with_capacity(reply.len() + 4);
        put_string(&mut frame, &reply);
        stream.write_all(&frame).await?;
    }
}

/// The answer (without its length) to one request.
fn answer(body: &[u8], signer: &Mutex<TokenSigner>) -> Vec<u8> {
    let Ok(signer) = signer.lock() else {
        return vec![FAILURE];
    };
    let blob = &signer.public_key().ssh_blob;
    match body.split_first() {
        Some((&REQUEST_IDENTITIES, _)) => {
            let mut out = vec![IDENTITIES_ANSWER];
            out.extend_from_slice(&1u32.to_be_bytes());
            put_string(&mut out, blob);
            put_string(&mut out, b"ikebox token");
            out
        }
        Some((&SIGN_REQUEST, rest)) => {
            let Some((key, data, flags)) = parse_sign_request(rest) else {
                return vec![FAILURE];
            };
            if key != blob.as_slice() {
                return vec![FAILURE];
            }
            match signer.sign(data, flags) {
                Ok(signature) => {
                    let mut out = vec![SIGN_RESPONSE];
                    put_string(&mut out, &signature);
                    out
                }
                Err(e) => {
                    warn!("key agent: signing failed: {e}");
                    vec![FAILURE]
                }
            }
        }
        _ => vec![FAILURE],
    }
}

/// `string key_blob, string data, uint32 flags`.
fn parse_sign_request(mut rest: &[u8]) -> Option<(&[u8], &[u8], u32)> {
    let key = take_string(&mut rest)?;
    let data = take_string(&mut rest)?;
    let flags = rest.get(..4)?;
    let flags = u32::from_be_bytes([flags[0], flags[1], flags[2], flags[3]]);
    Some((key, data, flags))
}

fn take_string<'a>(rest: &mut &'a [u8]) -> Option<&'a [u8]> {
    let len = rest.get(..4)?;
    let len = u32::from_be_bytes([len[0], len[1], len[2], len[3]]) as usize;
    let value = rest.get(4..4 + len)?;
    *rest = &rest[4 + len..];
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_request_parsing_is_bounded() {
        let mut req = Vec::new();
        put_string(&mut req, b"key");
        put_string(&mut req, b"data to sign");
        req.extend_from_slice(&2u32.to_be_bytes());
        let (key, data, flags) = parse_sign_request(&req).expect("request");
        assert_eq!((key, data, flags), (&b"key"[..], &b"data to sign"[..], 2));
        // Truncated or with lengths past the end: refused, never a panic.
        assert!(parse_sign_request(&req[..req.len() - 1]).is_none());
        assert!(parse_sign_request(&[0xff, 0xff, 0xff, 0xff, 1]).is_none());
        assert!(parse_sign_request(&[]).is_none());
    }
}
