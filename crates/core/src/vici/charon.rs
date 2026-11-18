//! Typed operations on charon (ADR-0003): the [`Charon`] trait, the response
//! types and the event parsing.
//!
//! The session (`core::session`) depends on the trait only, so its tests can
//! use a fake charon.

use std::fmt;
use std::time::Duration;

use zeroize::Zeroizing;

use super::{Message, ViciClient, ViciError};

/// Kind of secret for `load-shared`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretKind {
    /// IKE PSK (e.g. the group PSK).
    Ike,
    /// XAuth password (IKEv1).
    Xauth,
    /// EAP password (IKEv2).
    Eap,
}

impl SecretKind {
    fn as_vici(self) -> &'static str {
        match self {
            Self::Ike => "IKE",
            Self::Xauth => "XAUTH",
            Self::Eap => "EAP",
        }
    }
}

/// A private key to load into charon (`load-key`), as data (PEM, PKCS#8):
/// never a path (ADR-0019, ADR-0024). `Debug` shows nothing of it, and the
/// buffer is zeroed on drop.
#[derive(Clone)]
pub struct PrivateKey {
    data: Zeroizing<Vec<u8>>,
}

impl PrivateKey {
    /// Copies `data` into a buffer zeroed on drop.
    #[must_use]
    pub fn new(data: &[u8]) -> Self {
        Self {
            data: Zeroizing::new(data.to_vec()),
        }
    }

    fn to_message(&self) -> Message {
        // "any": charon finds the type (RSA, ECDSA or Ed25519) in the PKCS#8.
        Message::new()
            .kv("type", "any")
            .kv("data", self.data.as_slice())
    }
}

impl fmt::Debug for PrivateKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PrivateKey(<redigida>)")
    }
}

/// A secret to load into charon. `Debug` shows neither the data nor the owners
/// (identities such as the group id and the XAuth username).
#[derive(Clone)]
pub struct SharedSecret {
    /// Unique identifier of the secret in charon (for `unload-shared`).
    pub id: String,
    /// Kind of secret.
    pub kind: SecretKind,
    owners: Vec<String>,
    data: Zeroizing<Vec<u8>>,
}

impl SharedSecret {
    /// Creates a secret. `data` is copied into a buffer zeroed on drop.
    #[must_use]
    pub fn new(id: &str, kind: SecretKind, owners: Vec<String>, data: &[u8]) -> Self {
        Self {
            id: id.to_owned(),
            kind,
            owners,
            data: Zeroizing::new(data.to_vec()),
        }
    }

    /// Matching `load-shared` message.
    fn to_message(&self) -> Message {
        Message::new()
            .kv("id", &self.id)
            .kv("type", self.kind.as_vici())
            .kv("data", self.data.as_slice())
            .list("owners", &self.owners)
    }
}

impl fmt::Debug for SharedSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SharedSecret")
            .field("id", &self.id)
            .field("kind", &self.kind)
            .field(
                "owners",
                &format_args!("<{} identidade(s)>", self.owners.len()),
            )
            .field("data", &format_args!("<redigido>"))
            .finish()
    }
}

/// Response to `version`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VersionInfo {
    /// Daemon name (`charon`).
    pub daemon: String,
    /// strongSwan version.
    pub version: String,
    /// System (`Linux`).
    pub sysname: String,
    /// Kernel version.
    pub release: String,
    /// Architecture.
    pub machine: String,
}

/// A CHILD_SA (phase 2) from `list-sas`. No identities.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChildSa {
    /// Name of the child in the configuration.
    pub name: String,
    /// State (`INSTALLED`, `REKEYING`...).
    pub state: String,
    /// Bytes received.
    pub bytes_in: u64,
    /// Bytes sent.
    pub bytes_out: u64,
    /// Seconds since it was installed.
    pub install_time: Option<u64>,
    /// Local traffic selectors.
    pub local_ts: Vec<String>,
    /// Remote traffic selectors.
    pub remote_ts: Vec<String>,
}

/// An IKE_SA (phase 1) from `list-sas`. No identities (group id, XAuth
/// username), so they do not leak through the UI or logs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IkeSa {
    /// Connection name.
    pub name: String,
    /// Unique id of the IKE_SA in charon.
    pub uniqueid: String,
    /// State (`ESTABLISHED`, `CONNECTING`...).
    pub state: String,
    /// Gateway address.
    pub remote_host: String,
    /// Virtual IPs received.
    pub local_vips: Vec<String>,
    /// Seconds since it was established.
    pub established_secs: Option<u64>,
    /// CHILD_SAs.
    pub children: Vec<ChildSa>,
}

impl IkeSa {
    fn from_message(name: &str, msg: &Message) -> Self {
        let text = |key: &str| msg.get_str(key).unwrap_or_default().to_owned();
        let children = msg
            .get_section("child-sas")
            .map(|c| {
                c.sections()
                    .map(|(_, m)| ChildSa::from_message(m))
                    .collect()
            })
            .unwrap_or_default();
        Self {
            name: name.to_owned(),
            uniqueid: text("uniqueid"),
            state: text("state"),
            remote_host: text("remote-host"),
            local_vips: msg
                .get_list("local-vips")
                .into_iter()
                .map(str::to_owned)
                .collect(),
            established_secs: msg.get_str("established").and_then(|v| v.parse().ok()),
            children,
        }
    }
}

impl ChildSa {
    fn from_message(msg: &Message) -> Self {
        let number = |key: &str| msg.get_str(key).and_then(|v| v.parse().ok());
        let list = |key: &str| msg.get_list(key).into_iter().map(str::to_owned).collect();
        Self {
            name: msg.get_str("name").unwrap_or_default().to_owned(),
            state: msg.get_str("state").unwrap_or_default().to_owned(),
            bytes_in: number("bytes-in").unwrap_or(0),
            bytes_out: number("bytes-out").unwrap_or(0),
            install_time: number("install-time"),
            local_ts: list("local-ts"),
            remote_ts: list("remote-ts"),
        }
    }
}

/// A charon event, already interpreted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CharonEvent {
    /// IKE_SA went up (`up = true`) or down.
    IkeUpDown {
        /// Up?
        up: bool,
        /// Connection name.
        ike: String,
    },
    /// CHILD_SA went up or down.
    ChildUpDown {
        /// Up?
        up: bool,
        /// Connection name.
        ike: String,
        /// Names of the affected children.
        children: Vec<String>,
    },
    /// A charon log line (level ≤ 1). It may contain identities: whoever shows it
    /// must mask them (ADR-0007).
    Log {
        /// Level (0 = basic, 1 = control).
        level: i32,
        /// Subsystem (`IKE`, `CFG`, `NET`...).
        group: String,
        /// Related connection, if any.
        ike: Option<String>,
        /// Text.
        message: String,
    },
    /// Another event.
    Other(String),
}

impl CharonEvent {
    /// Interprets a VICI event.
    #[must_use]
    pub fn parse(name: &str, msg: &Message) -> Self {
        let up = msg.get_str("up") == Some("yes");
        match name {
            "ike-updown" => Self::IkeUpDown {
                up,
                ike: first_section_name(msg),
            },
            "child-updown" => {
                let (ike, children) = msg
                    .sections()
                    .next()
                    .map(|(ike, sa)| {
                        let children = sa
                            .get_section("child-sas")
                            .map(|c| {
                                c.sections()
                                    .map(|(key, m)| m.get_str("name").unwrap_or(key).to_owned())
                                    .collect()
                            })
                            .unwrap_or_default();
                        (ike.to_owned(), children)
                    })
                    .unwrap_or_default();
                Self::ChildUpDown { up, ike, children }
            }
            "log" => Self::Log {
                level: msg
                    .get_str("level")
                    .and_then(|l| l.parse().ok())
                    .unwrap_or(0),
                group: msg.get_str("group").unwrap_or_default().to_owned(),
                ike: msg.get_str("ikesa-name").map(str::to_owned),
                message: msg.get_str("msg").unwrap_or_default().to_owned(),
            },
            other => Self::Other(other.to_owned()),
        }
    }
}

fn first_section_name(msg: &Message) -> String {
    msg.sections()
        .next()
        .map(|(name, _)| name.to_owned())
        .unwrap_or_default()
}

/// Operations used by the app (ADR-0003).
#[allow(async_fn_in_trait)] // internal use: always with concrete or generic types
pub trait Charon {
    /// `version`.
    async fn version(&mut self) -> Result<VersionInfo, ViciError>;
    /// `load-shared`: loads a secret into charon's memory.
    async fn load_shared(&mut self, secret: &SharedSecret) -> Result<(), ViciError>;
    /// `unload-shared`.
    async fn unload_shared(&mut self, id: &str) -> Result<(), ViciError>;
    /// `load-key`: loads the client's private key into charon's memory.
    async fn load_key(&mut self, key: &PrivateKey) -> Result<(), ViciError>;
    /// `load-key` of type `agent` (patch 0004, ADR-0029): a key held by the
    /// person's key agent, named by its public key (SubjectPublicKeyInfo,
    /// PEM). charon uses the agent socket root configured, never one named
    /// here.
    async fn load_agent_key(&mut self, public_key_pem: &str) -> Result<(), ViciError>;
    /// `load-conn` with the configuration of the connection `name`.
    async fn load_conn(&mut self, name: &str, conn: &Message) -> Result<(), ViciError>;
    /// `unload-conn`.
    async fn unload_conn(&mut self, name: &str) -> Result<(), ViciError>;
    /// `list-conns`: names of the loaded connections.
    async fn list_conns(&mut self) -> Result<Vec<String>, ViciError>;
    /// `initiate` of the child `child` of the connection `ike`, waiting up to
    /// `timeout`.
    async fn initiate(
        &mut self,
        ike: &str,
        child: &str,
        timeout: Duration,
    ) -> Result<(), ViciError>;
    /// `terminate` of the IKE_SA `ike` (sends DELETE to the gateway).
    async fn terminate(&mut self, ike: &str, timeout: Duration) -> Result<(), ViciError>;
    /// `list-sas`.
    async fn list_sas(&mut self) -> Result<Vec<IkeSa>, ViciError>;
}

/// Checks `success = yes`; otherwise, returns charon's `errmsg`.
fn check(command: &str, response: &Message) -> Result<(), ViciError> {
    if response.get_str("success") == Some("yes") {
        return Ok(());
    }
    Err(ViciError::CommandFailed {
        command: command.to_owned(),
        message: response
            .get_str("errmsg")
            .unwrap_or(crate::tr!(
                "failure without a message",
                "falha sem mensagem"
            ))
            .to_owned(),
    })
}

fn millis(timeout: Duration) -> String {
    timeout.as_millis().to_string()
}

impl Charon for ViciClient {
    async fn version(&mut self) -> Result<VersionInfo, ViciError> {
        let r = self.request("version", &Message::new()).await?;
        let text = |key: &str| r.get_str(key).unwrap_or_default().to_owned();
        Ok(VersionInfo {
            daemon: text("daemon"),
            version: text("version"),
            sysname: text("sysname"),
            release: text("release"),
            machine: text("machine"),
        })
    }

    async fn load_shared(&mut self, secret: &SharedSecret) -> Result<(), ViciError> {
        let response = self.request("load-shared", &secret.to_message()).await?;
        check("load-shared", &response)
    }

    async fn unload_shared(&mut self, id: &str) -> Result<(), ViciError> {
        let response = self
            .request("unload-shared", &Message::new().kv("id", id))
            .await?;
        check("unload-shared", &response)
    }

    async fn load_key(&mut self, key: &PrivateKey) -> Result<(), ViciError> {
        let response = self.request("load-key", &key.to_message()).await?;
        check("load-key", &response)
    }

    async fn load_agent_key(&mut self, public_key_pem: &str) -> Result<(), ViciError> {
        let message = Message::new()
            .kv("type", "agent")
            .kv("data", public_key_pem.as_bytes());
        let response = self.request("load-key", &message).await?;
        check("load-key", &response)
    }

    async fn load_conn(&mut self, name: &str, conn: &Message) -> Result<(), ViciError> {
        let message = Message::new().section(name, conn.clone());
        let response = self.request("load-conn", &message).await?;
        check("load-conn", &response)
    }

    async fn unload_conn(&mut self, name: &str) -> Result<(), ViciError> {
        let response = self
            .request("unload-conn", &Message::new().kv("name", name))
            .await?;
        check("unload-conn", &response)
    }

    async fn list_conns(&mut self) -> Result<Vec<String>, ViciError> {
        let (events, _) = self
            .streamed_request("list-conns", "list-conn", &Message::new())
            .await?;
        Ok(events
            .iter()
            .flat_map(|e| e.sections().map(|(name, _)| name.to_owned()))
            .collect())
    }

    async fn initiate(
        &mut self,
        ike: &str,
        child: &str,
        timeout: Duration,
    ) -> Result<(), ViciError> {
        let message = Message::new()
            .kv("child", child)
            .kv("ike", ike)
            .kv("timeout", millis(timeout))
            .kv("init-limits", "no");
        let response = self.request("initiate", &message).await?;
        check("initiate", &response)
    }

    async fn terminate(&mut self, ike: &str, timeout: Duration) -> Result<(), ViciError> {
        let message = Message::new().kv("ike", ike).kv("timeout", millis(timeout));
        let response = self.request("terminate", &message).await?;
        check("terminate", &response)
    }

    async fn list_sas(&mut self) -> Result<Vec<IkeSa>, ViciError> {
        let (events, _) = self
            .streamed_request("list-sas", "list-sa", &Message::new())
            .await?;
        Ok(events
            .iter()
            .flat_map(|e| e.sections().map(|(name, m)| IkeSa::from_message(name, m)))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_secret_message_and_debug() {
        let secret = SharedSecret::new(
            "ike-perfil",
            SecretKind::Ike,
            vec!["keyid:grupo-teste".to_owned()],
            b"psk-falsa",
        );
        let msg = secret.to_message();
        assert_eq!(msg.get_str("type"), Some("IKE"));
        assert_eq!(msg.get_str("data"), Some("psk-falsa"));
        assert_eq!(msg.get_list("owners"), ["keyid:grupo-teste"]);
        let debug = format!("{secret:?}");
        assert!(!debug.contains("psk-falsa"), "{debug}");
        assert!(!debug.contains("grupo-teste"), "{debug}");
    }

    #[test]
    fn check_reports_errmsg() {
        assert!(check("x", &Message::new().kv("success", "yes")).is_ok());
        let err = check(
            "initiate",
            &Message::new().kv("success", "no").kv("errmsg", "timeout"),
        )
        .expect_err("falha");
        assert_eq!(err.to_string(), "charon rejected 'initiate': timeout");
    }

    #[test]
    fn parses_list_sa() {
        let child = Message::new()
            .kv("name", "rede")
            .kv("state", "INSTALLED")
            .kv("bytes-in", "120")
            .kv("bytes-out", "80")
            .kv("install-time", "5")
            .list("local-ts", ["198.51.100.10/32"])
            .list("remote-ts", ["192.0.2.0/24"]);
        let sa = Message::new()
            .kv("uniqueid", "1")
            .kv("state", "ESTABLISHED")
            .kv("remote-host", "203.0.113.1")
            .kv("established", "42")
            .kv("remote-xauth-id", "usuario.teste")
            .list("local-vips", ["198.51.100.10"])
            .section("child-sas", Message::new().section("rede-1", child));
        let ike = IkeSa::from_message("teste", &sa);
        assert_eq!(ike.state, "ESTABLISHED");
        assert_eq!(ike.local_vips, ["198.51.100.10"]);
        assert_eq!(ike.established_secs, Some(42));
        assert_eq!(ike.children.len(), 1);
        assert_eq!(ike.children[0].bytes_in, 120);
        assert_eq!(ike.children[0].install_time, Some(5));
        assert!(!format!("{ike:?}").contains("usuario"));
    }

    #[test]
    fn parses_events() {
        let child_up = Message::new().kv("up", "yes").section(
            "teste",
            Message::new().section(
                "child-sas",
                Message::new().section("rede-1", Message::new().kv("name", "rede")),
            ),
        );
        assert_eq!(
            CharonEvent::parse("child-updown", &child_up),
            CharonEvent::ChildUpDown {
                up: true,
                ike: "teste".into(),
                children: vec!["rede".into()]
            }
        );
        let ike_down = Message::new().section("teste", Message::new());
        assert_eq!(
            CharonEvent::parse("ike-updown", &ike_down),
            CharonEvent::IkeUpDown {
                up: false,
                ike: "teste".into()
            }
        );
        let log = Message::new()
            .kv("group", "IKE")
            .kv("level", "1")
            .kv("ikesa-name", "teste")
            .kv("msg", "IKE_SA established");
        assert!(matches!(
            CharonEvent::parse("log", &log),
            CharonEvent::Log { level: 1, ref group, .. } if group == "IKE"
        ));
        assert_eq!(
            CharonEvent::parse("x", &Message::new()),
            CharonEvent::Other("x".into())
        );
    }
}
