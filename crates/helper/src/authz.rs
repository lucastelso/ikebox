//! Who is asking, and whether they may (ADR-0014).
//!
//! - Identity: `SO_PEERCRED`, filled in by the kernel and impossible for the
//!   client to forge.
//! - Authorization: polkit, action [`POLKIT_ACTION`] (`allow_active=yes`: the
//!   user of the active local session may, without a password; remote or
//!   inactive sessions may not). Test mode replaces polkit with a list of
//!   uids, but only root starts the helper with that option.

use std::collections::HashMap;
use std::os::unix::net::UnixStream;
use std::time::Duration;

use ikebox_proto::POLKIT_ACTION;

/// Credentials of the client process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Peer {
    /// pid.
    pub pid: u32,
    /// uid.
    pub uid: u32,
    /// Effective gid (group of the VICI socket).
    pub gid: u32,
}

/// Reads the credentials of the other side of the socket.
///
/// # Errors
/// `getsockopt` failure.
pub fn peer_of(stream: &UnixStream) -> std::io::Result<Peer> {
    let cred = rustix::net::sockopt::socket_peercred(stream)?;
    Ok(Peer {
        pid: u32::try_from(rustix::process::Pid::as_raw(Some(cred.pid))).unwrap_or(0),
        uid: cred.uid.as_raw(),
        gid: cred.gid.as_raw(),
    })
}

/// `starttime` field (the 22nd) of `/proc/<pid>/stat`. The process name may
/// have spaces and parentheses, so counting starts after the last `)`.
#[must_use]
pub fn parse_start_time(stat: &str) -> Option<u64> {
    let after = &stat[stat.rfind(')')? + 1..];
    // After the ")" comes field 3 (state); starttime is field 22 → index 19.
    after.split_whitespace().nth(19)?.parse().ok()
}

fn start_time(pid: u32) -> Option<u64> {
    parse_start_time(&std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)
}

/// Effective uid in `/proc/<pid>/status` (`Uid:\treal\teffective\tsaved\tfs`).
#[must_use]
pub fn parse_euid(status: &str) -> Option<u32> {
    status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

/// Start time of the process that has this pid **now**, if it still belongs
/// to `uid`.
///
/// `SO_PEERCRED` gives the pid at connection time, and polkit looks the
/// session up by pid. If the client exits and the pid is reused by a process
/// of another user (one in the active session, for instance), polkit would
/// judge that other process. The uid is read between two reads of the start
/// time, so it is the uid of that very process.
fn same_user_process(pid: u32, uid: u32) -> Option<u64> {
    let before = start_time(pid)?;
    let euid = parse_euid(&std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?)?;
    let after = start_time(pid)?;
    (before == after && euid == uid).then_some(before)
}

/// Authorization policy.
#[derive(Debug, Clone)]
pub enum Authorizer {
    /// polkit (production).
    Polkit,
    /// Only these uids (test mode).
    AllowList(Vec<u32>),
}

impl Authorizer {
    /// Authorizes (or not) the client.
    ///
    /// # Errors
    /// Message for the user when not authorized.
    pub fn authorize(&self, peer: &Peer) -> Result<(), String> {
        match self {
            Self::AllowList(uids) => {
                if uids.contains(&peer.uid) {
                    Ok(())
                } else {
                    Err("not authorized".to_owned())
                }
            }
            Self::Polkit => match polkit_check(peer) {
                Ok(true) => Ok(()),
                Ok(false) => Err(
                    "not authorized: only the user of the active local session can control the VPN"
                        .to_owned(),
                ),
                Err(e) => Err(format!("could not query polkit: {e}")),
            },
        }
    }
}

fn polkit_check(peer: &Peer) -> Result<bool, dbus::Error> {
    use dbus::arg::{PropMap, Variant};

    let start = same_user_process(peer.pid, peer.uid)
        .ok_or_else(|| dbus::Error::new_failed("client process is gone"))?;
    let mut subject_details = PropMap::new();
    subject_details.insert("pid".to_owned(), Variant(Box::new(peer.pid)));
    subject_details.insert("start-time".to_owned(), Variant(Box::new(start)));
    subject_details.insert(
        "uid".to_owned(),
        Variant(Box::new(i32::try_from(peer.uid).unwrap_or(-1))),
    );
    let subject = ("unix-process".to_owned(), subject_details);
    let details: HashMap<String, String> = HashMap::new();

    let conn = dbus::blocking::Connection::new_system()?;
    let proxy = conn.with_proxy(
        "org.freedesktop.PolicyKit1",
        "/org/freedesktop/PolicyKit1/Authority",
        Duration::from_secs(10),
    );
    // flags = 0: no interaction (allow_active needs no password; the rest is
    // denied).
    let ((authorized, _challenge, _details),): ((bool, bool, HashMap<String, String>),) = proxy
        .method_call(
            "org.freedesktop.PolicyKit1.Authority",
            "CheckAuthorization",
            (subject, POLKIT_ACTION, details, 0u32, ""),
        )?;
    Ok(authorized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_proc_stat_start_time() {
        // A name with a space and parentheses, as the kernel allows.
        let stat = "1234 (ipsec (client) x) S 1 1234 1234 0 -1 4194560 100 0 0 0 1 2 0 0 20 0 4 0 987654 123456 789";
        assert_eq!(parse_start_time(stat), Some(987_654));
        assert_eq!(parse_start_time("lixo"), None);
        assert_eq!(parse_start_time("1 (x) S 1 2"), None);
    }

    #[test]
    fn allow_list_authorizer() {
        let peer = Peer {
            pid: 1,
            uid: 1000,
            gid: 1000,
        };
        assert!(Authorizer::AllowList(vec![1000]).authorize(&peer).is_ok());
        assert!(Authorizer::AllowList(vec![1001]).authorize(&peer).is_err());
    }

    #[test]
    fn reads_own_start_time() {
        assert!(start_time(std::process::id()).is_some());
    }

    #[test]
    fn parses_proc_status_euid() {
        let status = "Name:\tx\nUmask:\t0022\nUid:\t1000\t1001\t1000\t1000\nGid:\t5\t5\t5\t5\n";
        assert_eq!(parse_euid(status), Some(1001));
        assert_eq!(parse_euid("Name:\tx\n"), None);
        assert_eq!(parse_euid("Uid:\t1000\n"), None);
    }

    #[test]
    fn a_reused_pid_of_another_user_is_not_the_client() {
        let pid = std::process::id();
        let uid = rustix::process::geteuid().as_raw();
        assert_eq!(same_user_process(pid, uid), start_time(pid));
        assert_eq!(
            same_user_process(pid, uid + 1),
            None,
            "another user's process"
        );
        assert_eq!(same_user_process(u32::MAX, uid), None, "no such process");
    }
}
