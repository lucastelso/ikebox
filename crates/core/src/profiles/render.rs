//! Generates charon's configuration from the profile: the `load-conn` message
//! (VICI) and the `updown` command (ADR-0004, ADR-0008).

use sha2::{Digest, Sha256};

use super::ProfileError;
use super::model::{ChildAction, LocalMethod, Profile, RemoteMethod};
use super::validate::{is_identity, validate};
use crate::vici::Message;

/// Path of `updown` in the engine's Docker image. In the embedded engine, the
/// `updown` command is fixed by root and whatever comes over VICI is ignored
/// (ADR-0019): there, `load-conn` goes without `updown`.
pub const UPDOWN_PATH: &str = "/usr/local/libexec/ikebox/updown";

/// Session data needed to build the connection.
#[derive(Debug, Clone, Copy)]
pub struct ConnParams<'a> {
    /// The engine's XFRM interface.
    pub iface: &'a str,
    /// Path of `updown` to send in `load-conn` ([`UPDOWN_PATH`] in the Docker
    /// engine; `None` in the embedded one, where root sets it).
    pub updown: Option<&'a str>,
    /// `if_id` of the SAs and policies.
    pub if_id: u32,
    /// XAuth username (required with `psk-xauth`). Not a secret, but a personal
    /// identifier: it comes from the credentials, never from the profile.
    pub xauth_user: Option<&'a str>,
    /// EAP identity (required with EAP): it also comes from the credentials.
    pub eap_identity: Option<&'a str>,
    /// Client certificate in PEM (required with a certificate): it comes from the
    /// keyring; the private key goes through `load-key`, not here.
    pub client_cert: Option<&'a str>,
}

impl Profile {
    /// Connection name in charon.
    #[must_use]
    pub fn conn_name(&self) -> &str {
        &self.id
    }

    /// Stable `if_id` derived from the profile id (1..=2³¹-1, never 0).
    #[must_use]
    pub fn if_id(&self) -> u32 {
        let digest = Sha256::digest(self.id.as_bytes());
        let raw = u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]) & 0x7fff_ffff;
        raw.max(1)
    }

    /// `updown` command (run by charon through `/bin/sh -c`): each argument goes
    /// in single quotes, and the values were already validated.
    ///
    /// # Errors
    /// [`ProfileError::Unsafe`] if some value cannot go in single quotes (extra
    /// defense; validation already refuses such values).
    pub fn updown_command(&self, updown: &str, iface: &str) -> Result<String, ProfileError> {
        let dns = &self.network.dns;
        if !updown.starts_with('/') || updown.chars().any(|c| c.is_whitespace() || c == '\'') {
            return Err(ProfileError::Unsafe(
                crate::tr!("invalid updown path", "caminho do updown inválido").to_owned(),
            ));
        }
        let mut cmd = format!("{updown} --iface {}", shell_quote(iface)?);
        for server in &dns.servers {
            cmd.push_str(&format!(" --dns {}", shell_quote(&server.to_string())?));
        }
        for domain in &dns.domains {
            cmd.push_str(&format!(" --domain {}", shell_quote(domain)?));
        }
        if dns.use_received {
            cmd.push_str(" --use-received-dns");
        }
        if dns.default_route {
            cmd.push_str(" --dns-default-route");
        }
        Ok(cmd)
    }

    /// `load-conn` message (the content of the `<conn_name>` section).
    ///
    /// # Errors
    /// Invalid profile ([`ProfileError::Invalid`]), XAuth username missing or
    /// invalid ([`ProfileError::MissingCredential`]).
    pub fn to_load_conn(&self, params: &ConnParams<'_>) -> Result<Message, ProfileError> {
        let issues = validate(self);
        if !issues.is_empty() {
            return Err(ProfileError::Invalid(issues));
        }
        let ike = &self.ike;
        let mut conn = Message::new()
            .list("remote_addrs", [&self.gateway])
            .kv("version", ike.version.to_string())
            .kv("aggressive", yes_no(ike.aggressive))
            .kv("encap", yes_no(ike.encap))
            .list("proposals", &ike.proposals);
        if let Some(secs) = ike.rekey_time_secs {
            conn = conn.kv("rekey_time", format!("{secs}s"));
        }
        if let Some(secs) = ike.dpd_delay_secs {
            conn = conn.kv("dpd_delay", format!("{secs}s"));
        }
        if self.network.request_vip {
            conn = conn.list("vips", ["0.0.0.0"]);
        }

        // One section per round, in the profile's order (local-1, local-2).
        for (i, round) in self.auth.local.iter().enumerate() {
            let mut section = match round.method {
                LocalMethod::Psk => Message::new().kv("auth", "psk"),
                LocalMethod::Xauth => {
                    let user = params.xauth_user.filter(|u| is_identity(u)).ok_or(
                        ProfileError::MissingCredential(crate::tr!("XAuth user", "usuário XAuth")),
                    )?;
                    Message::new().kv("auth", "xauth").kv("xauth_id", user)
                }
                LocalMethod::EapMschapv2 => {
                    let identity = params.eap_identity.filter(|u| is_identity(u)).ok_or(
                        ProfileError::MissingCredential(crate::tr!("EAP username", "usuário EAP")),
                    )?;
                    Message::new()
                        .kv("auth", "eap-mschapv2")
                        .kv("eap_id", identity)
                }
                LocalMethod::Certificate => {
                    let cert =
                        params
                            .client_cert
                            .ok_or(ProfileError::MissingCredential(crate::tr!(
                                "client certificate",
                                "certificado do cliente"
                            )))?;
                    // As data: the certificate goes in the connection itself.
                    Message::new().kv("auth", "pubkey").list("certs", [cert])
                }
            };
            if let Some(id) = &round.id {
                section = section.kv("id", id);
            }
            conn = conn.section(&format!("local-{}", i + 1), section);
        }
        // The gateway: the same PSK, or a certificate checked against the profile's
        // CAs, sent as data (patch 0002 refuses paths).
        let mut remote = match self.auth.remote.method {
            RemoteMethod::Psk => Message::new().kv("auth", "psk"),
            RemoteMethod::Certificate => Message::new()
                .kv("auth", "pubkey")
                .list("cacerts", &self.auth.remote.ca),
        };
        if let Some(id) = self.auth.remote_id() {
            remote = remote.kv("id", id);
        }
        conn = conn.section("remote", remote);

        let c = &self.child;
        let mut child = Message::new()
            .list("remote_ts", &c.remote_ts)
            .list("esp_proposals", &c.esp_proposals)
            .kv("start_action", "none")
            .kv("dpd_action", dpd_action(c.dpd_action))
            .kv("close_action", close_action(c.close_action))
            .kv("if_id_in", params.if_id.to_string())
            .kv("if_id_out", params.if_id.to_string());
        if let Some(updown) = params.updown {
            child = child.kv("updown", self.updown_command(updown, params.iface)?);
        }
        if !c.local_ts.is_empty() {
            child = child.list("local_ts", &c.local_ts);
        }
        if let Some(secs) = c.rekey_time_secs {
            child = child.kv("rekey_time", format!("{secs}s"));
        }
        Ok(conn.section("children", Message::new().section(&c.name, child)))
    }
}

fn yes_no(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

fn dpd_action(a: ChildAction) -> &'static str {
    match a {
        ChildAction::None => "clear",
        ChildAction::Trap => "trap",
        ChildAction::Restart => "restart",
    }
}

fn close_action(a: ChildAction) -> &'static str {
    match a {
        ChildAction::None => "none",
        ChildAction::Trap => "trap",
        ChildAction::Restart => "restart",
    }
}

/// Single quotes for `/bin/sh`. Refuses single quotes and control characters
/// instead of escaping: such values should never get here.
fn shell_quote(value: &str) -> Result<String, ProfileError> {
    if value.is_empty() || value.contains('\'') || value.chars().any(char::is_control) {
        return Err(ProfileError::Unsafe(
            crate::tr!(
                "value not allowed in the updown command",
                "valor impróprio para o comando do updown"
            )
            .to_owned(),
        ));
    }
    Ok(format!("'{value}'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_quote_rejects_breakouts() {
        assert_eq!(shell_quote("~corp").ok().as_deref(), Some("'~corp'"));
        for bad in ["", "a'b", "a\nb", "'; rm -rf / #"] {
            assert!(shell_quote(bad).is_err(), "{bad:?}");
        }
    }
}
