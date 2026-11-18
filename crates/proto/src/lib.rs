//! Protocol between the app (user) and `ipsec-client-helper` (root), ADR-0014.
//!
//! One request and one response per connection, each on a JSON line.
//!
//! **Security principle:** the client never sends paths, commands or secrets.
//! It only sends validated numbers and the profile id (a label). The helper
//! alone decides what to run, where and with which sandbox. The same
//! validation rules apply on both sides (this crate).

use std::net::IpAddr;

use serde::{Deserialize, Serialize};

/// Helper socket (created by systemd, socket activation).
pub const HELPER_SOCKET: &str = "/run/ipsec-client-helper.sock";
/// polkit action authorized to connect and disconnect.
pub const POLKIT_ACTION: &str = "io.github.lucastelso.ipsecclient.connect";
/// Transient systemd unit that runs the engine.
pub const ENGINE_UNIT: &str = "ipsec-client-engine";
/// Root of the per-user VICI socket directories (`<root>/<uid>/charon.vici`).
pub const SOCKET_ROOT: &str = "/run/ipsec-client";
/// File name of the VICI socket.
pub const VICI_SOCKET_FILE: &str = "charon.vici";
/// Maximum length of a protocol line.
pub const MAX_LINE: usize = 64 * 1024;
/// Maximum log lines returned.
pub const MAX_LOG_LINES: u32 = 2000;
/// Maximum fixed DNS servers (the same limit as the profile).
pub const MAX_DNS_SERVERS: usize = 8;
/// Maximum DNS domains (the same limit as the profile).
pub const MAX_DNS_DOMAINS: usize = 16;

/// A user's VICI socket directory.
#[must_use]
pub fn socket_dir(uid: u32) -> String {
    format!("{SOCKET_ROOT}/{uid}")
}

/// Path of a user's VICI socket.
#[must_use]
pub fn vici_socket(uid: u32) -> String {
    format!("{}/{VICI_SOCKET_FILE}", socket_dir(uid))
}

/// Request to the helper.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Request {
    /// Is the helper alive? Returns the version.
    Ping,
    /// Starts the engine (stopping a previous one of the same user, if any).
    Start(StartParams),
    /// Stops the engine.
    Stop,
    /// Engine state.
    Status,
    /// Last lines of the engine's log.
    Logs {
        /// How many lines (limited to [`MAX_LOG_LINES`]).
        tail: u32,
    },
}

/// Non-sensitive engine parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartParams {
    /// XFRM interface.
    pub iface: String,
    /// `if_id` of the SAs and policies (≠ 0). Kept on the wire for compatibility:
    /// since 0.3.0 the helper ignores it and uses its own value (audit P10).
    pub if_id: u32,
    /// Interface MTU.
    pub mtu: u16,
    /// charon log level (0..=2).
    pub log_level: u8,
    /// Profile id (the session's label).
    pub profile_id: String,
    /// Session DNS, applied by `updown` to the VPN interface. It goes through the
    /// helper, not VICI, because the `updown` command is fixed by root (ADR-0019).
    #[serde(default)]
    pub dns: DnsParams,
}

/// Session DNS (validated data; it never becomes shell text).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DnsParams {
    /// Fixed servers.
    pub servers: Vec<IpAddr>,
    /// Domains (`domain`, `~domain` or `~.`).
    pub domains: Vec<String>,
    /// Also use the DNS servers sent by the gateway.
    pub use_received: bool,
    /// Use these DNS servers outside the domains too.
    pub default_route: bool,
}

impl DnsParams {
    /// Validates with the same rules as the profile and `updown`.
    ///
    /// # Errors
    /// Description of the first problem.
    pub fn validate(&self) -> Result<(), String> {
        if self.servers.len() > MAX_DNS_SERVERS {
            return Err(format!("at most {MAX_DNS_SERVERS} DNS servers"));
        }
        if self.domains.len() > MAX_DNS_DOMAINS {
            return Err(format!("at most {MAX_DNS_DOMAINS} DNS domains"));
        }
        if !self.domains.iter().all(|d| valid_dns_domain(d)) {
            return Err("invalid DNS domain".to_owned());
        }
        Ok(())
    }
}

impl StartParams {
    /// Validates with the same rules as `entrypoint.sh`.
    ///
    /// # Errors
    /// Description of the first invalid field.
    pub fn validate(&self) -> Result<(), String> {
        if !valid_iface(&self.iface) {
            return Err("invalid interface name".to_owned());
        }
        if self.if_id == 0 {
            return Err("if_id cannot be 0".to_owned());
        }
        if !(576..=9000).contains(&self.mtu) {
            return Err("MTU outside 576..=9000".to_owned());
        }
        if self.log_level > 2 {
            return Err("log level above 2".to_owned());
        }
        if !valid_profile_id(&self.profile_id) {
            return Err("invalid profile id".to_owned());
        }
        self.dns.validate()
    }
}

/// Engine state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineStatus {
    /// Is the unit active?
    pub running: bool,
    /// Session profile.
    pub profile_id: Option<String>,
    /// uid of whoever started it.
    pub owner_uid: Option<u32>,
}

/// Helper response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Response {
    /// Response to `Ping`.
    Pong {
        /// Helper version.
        version: String,
    },
    /// Engine ready; the VICI socket accepts connections.
    Started {
        /// Path of the VICI socket.
        socket: String,
    },
    /// Engine stopped.
    Stopped {
        /// Was an engine running?
        was_running: bool,
    },
    /// State.
    Status(EngineStatus),
    /// Log.
    Logs {
        /// Lines.
        lines: Vec<String>,
    },
    /// Failure (a message for the user).
    Error {
        /// Message.
        message: String,
    },
}

/// (De)coding error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeError(pub String);

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DecodeError {}

/// Serializes a message into one line (with `\n`).
///
/// # Errors
/// Only in pathological serde cases.
pub fn encode<T: Serialize>(message: &T) -> Result<String, DecodeError> {
    let mut line = serde_json::to_string(message).map_err(|e| DecodeError(e.to_string()))?;
    line.push('\n');
    Ok(line)
}

/// Reads a one-line message (limit [`MAX_LINE`]).
///
/// # Errors
/// Line too long, or invalid or unknown JSON.
pub fn decode<T: for<'de> Deserialize<'de>>(line: &str) -> Result<T, DecodeError> {
    if line.len() > MAX_LINE {
        return Err(DecodeError("message too large".to_owned()));
    }
    serde_json::from_str(line.trim_end()).map_err(|e| DecodeError(format!("invalid message: {e}")))
}

/// Prefix of the session interface. The engine deletes and recreates the
/// interface it is given, as root: it only ever gets a name of its own.
pub const IFACE_PREFIX: &str = "ipsc";

/// `^ipsc[0-9]{1,4}$`: the session interface (`ipsc0`). Any other name is
/// refused: with a free name, a client could make root delete another
/// program's interface (`docker0`, `wg0`, a bridge) or reset its DNS.
#[must_use]
pub fn valid_iface(name: &str) -> bool {
    name.strip_prefix(IFACE_PREFIX)
        .is_some_and(|n| (1..=4).contains(&n.len()) && n.bytes().all(|b| b.is_ascii_digit()))
}

/// `^[a-z0-9][a-z0-9-]{0,62}$`.
#[must_use]
pub fn valid_profile_id(id: &str) -> bool {
    let mut chars = id.chars();
    matches!(chars.next(), Some('a'..='z' | '0'..='9'))
        && id.len() <= 63
        && chars.all(|c| matches!(c, 'a'..='z' | '0'..='9' | '-'))
}

/// Domain for systemd-resolved: `~.`, `domain` or `~domain` (labels
/// `[A-Za-z0-9-]`, no hyphen at either end, up to 253 characters).
#[must_use]
pub fn valid_dns_domain(s: &str) -> bool {
    if s == "~." {
        return true;
    }
    let name = s.strip_prefix('~').unwrap_or(s);
    let name = name.strip_suffix('.').unwrap_or(name);
    let label = |l: &str| {
        !l.is_empty()
            && l.len() <= 63
            && !l.starts_with('-')
            && !l.ends_with('-')
            && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    };
    !name.is_empty() && name.len() <= 253 && name.split('.').all(label)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> StartParams {
        StartParams {
            iface: "ipsc0".into(),
            if_id: 42,
            mtu: 1400,
            log_level: 1,
            profile_id: "trabalho".into(),
            dns: DnsParams {
                servers: vec!["192.0.2.53".parse().expect("ip")],
                domains: vec!["~corp.example.com".into(), "~.".into()],
                use_received: true,
                default_route: false,
            },
        }
    }

    #[test]
    fn roundtrip_requests_and_responses() {
        for req in [
            Request::Ping,
            Request::Start(params()),
            Request::Stop,
            Request::Status,
            Request::Logs { tail: 10 },
        ] {
            let line = encode(&req).expect("encode");
            assert!(line.ends_with('\n') && !line[..line.len() - 1].contains('\n'));
            assert_eq!(decode::<Request>(&line).expect("decode"), req);
        }
        let resp = Response::Started {
            socket: vici_socket(1000),
        };
        assert_eq!(
            decode::<Response>(&encode(&resp).expect("enc")).expect("dec"),
            resp
        );
        assert_eq!(vici_socket(1000), "/run/ipsec-client/1000/charon.vici");
    }

    #[test]
    fn dns_is_optional_on_the_wire() {
        // Client without the `dns` field (before 0.1.1): empty DNS.
        let old =
            r#"{"op":"start","iface":"ipsc0","if_id":1,"mtu":1400,"log_level":1,"profile_id":"x"}"#;
        let Ok(Request::Start(p)) = decode::<Request>(old) else {
            panic!("start sem dns deveria decodificar");
        };
        assert_eq!(p.dns, DnsParams::default());
        // A server that is not an IP is refused while decoding.
        let bad = r#"{"op":"start","iface":"ipsc0","if_id":1,"mtu":1400,"log_level":1,"profile_id":"x","dns":{"servers":["a;b"],"domains":[],"use_received":false,"default_route":false}}"#;
        assert!(decode::<Request>(bad).is_err());
    }

    #[test]
    fn dns_domains() {
        for ok in [
            "~.",
            "corp.example.com",
            "~corp.example.com",
            "~corp",
            "corp.example.com.",
        ] {
            assert!(valid_dns_domain(ok), "{ok}");
        }
        for bad in [
            "",
            "~",
            "~~corp",
            "corp..x",
            "co rp",
            "corp'; rm -rf /",
            "-x",
            "a\nb",
        ] {
            assert!(!valid_dns_domain(bad), "{bad}");
        }
    }

    #[test]
    fn rejects_unknown_fields_and_ops() {
        // An extra field (e.g. trying to pass a path or command) is refused.
        let sneaky = r#"{"op":"start","iface":"ipsc0","if_id":1,"mtu":1400,"log_level":1,"profile_id":"x","charon":"/tmp/x"}"#;
        assert!(decode::<Request>(sneaky).is_err());
        assert!(decode::<Request>(r#"{"op":"exec","cmd":"id"}"#).is_err());
        assert!(decode::<Request>("não é json").is_err());
        assert!(decode::<Request>(&"x".repeat(MAX_LINE + 1)).is_err());
    }

    #[test]
    fn validation_mirrors_entrypoint_rules() {
        assert!(params().validate().is_ok());
        let cases: &[fn(&mut StartParams)] = &[
            |p| p.iface = "Ipsc0".into(),
            |p| p.iface = "ipsc0;id".into(),
            |p| p.iface = "a-very-long-iface-name".into(),
            // Interfaces of other programs: root must never delete them.
            |p| p.iface = "eth0".into(),
            |p| p.iface = "docker0".into(),
            |p| p.iface = "wg0".into(),
            |p| p.iface = "lo".into(),
            |p| p.iface = "ipsc".into(),
            |p| p.iface = "ipsc0x".into(),
            |p| p.iface = "ipsc00000".into(),
            |p| p.if_id = 0,
            |p| p.mtu = 100,
            |p| p.log_level = 3,
            |p| p.profile_id = "../etc".into(),
            |p| p.profile_id = "Perfil".into(),
            |p| p.dns.domains = vec!["x'; touch /tmp/p; '".into()],
            |p| p.dns.domains = vec!["--iface".into()],
            |p| p.dns.domains = vec!["corp..example".into()],
            |p| p.dns.domains = vec!["a.example".into(); MAX_DNS_DOMAINS + 1],
            |p| p.dns.servers = vec!["192.0.2.1".parse().expect("ip"); MAX_DNS_SERVERS + 1],
        ];
        for (i, mutate) in cases.iter().enumerate() {
            let mut p = params();
            mutate(&mut p);
            assert!(p.validate().is_err(), "caso {i}");
        }
    }
}
