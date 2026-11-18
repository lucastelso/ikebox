//! Profile validation: Rust is the source of truth (ADR-0008).
//!
//! The rules protect two boundaries: charon (`load-conn`) and `updown`, which
//! runs through `/bin/sh -c` and so only gets validated values (no single
//! quotes or control characters).

use std::fmt;
use std::net::IpAddr;

use super::ca::{MAX_CAS, parse_ca};
use super::model::{LocalMethod, Profile, RemoteMethod};

/// A problem found in a field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationIssue {
    /// Path of the field (e.g. `child.remote_ts[1]`).
    pub field: String,
    /// Description for the user.
    pub message: String,
}

impl fmt::Display for ValidationIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.field, self.message)
    }
}

#[derive(Default)]
struct Issues(Vec<ValidationIssue>);

impl Issues {
    fn add(&mut self, field: impl Into<String>, message: impl Into<String>) {
        self.0.push(ValidationIssue {
            field: field.into(),
            message: message.into(),
        });
    }

    fn check(&mut self, ok: bool, field: impl Into<String>, message: &str) {
        if !ok {
            self.add(field, message);
        }
    }
}

/// Validates the whole profile and returns every problem (empty = valid).
#[must_use]
pub fn validate(p: &Profile) -> Vec<ValidationIssue> {
    let mut v = Issues::default();

    v.check(
        is_slug(&p.id),
        "id",
        crate::tr!("use 1 to 63 characters: lowercase letters, digits and hyphens (not starting with a hyphen)", "use de 1 a 63 caracteres: letras minúsculas, dígitos e hífen (sem começar com hífen)"),
    );
    v.check(
        !p.name.trim().is_empty() && p.name.chars().count() <= 100 && !has_control(&p.name),
        "name",
        crate::tr!(
            "enter a name of up to 100 characters",
            "informe um nome de até 100 caracteres"
        ),
    );
    v.check(
        is_host(&p.gateway),
        "gateway",
        crate::tr!(
            "enter a valid DNS name or IP address",
            "informe um nome DNS ou um endereço IP válido"
        ),
    );

    // Phase 1.
    let ike = &p.ike;
    v.check(
        matches!(ike.version, 1 | 2),
        "ike.version",
        crate::tr!(
            "the IKE version must be 1 or 2",
            "a versão do IKE deve ser 1 ou 2"
        ),
    );
    if ike.aggressive && ike.version != 1 {
        v.add(
            "ike.aggressive",
            crate::tr!(
                "aggressive mode only exists in IKEv1",
                "o modo agressivo só existe no IKEv1"
            ),
        );
    }
    check_proposals(&mut v, "ike.proposals", &ike.proposals);
    check_duration(&mut v, "ike.rekey_time_secs", ike.rekey_time_secs, 60);
    if let Some(dpd) = ike.dpd_delay_secs {
        v.check(
            (1..=3600).contains(&dpd),
            "ike.dpd_delay_secs",
            crate::tr!("use 1 to 3600 seconds", "use de 1 a 3600 segundos"),
        );
    }

    // Authentication.
    check_auth(&mut v, p);

    // Phase 2.
    let child = &p.child;
    v.check(
        is_slug(&child.name),
        "child.name",
        crate::tr!(
            "use lowercase letters, digits and hyphens",
            "use letras minúsculas, dígitos e hífen"
        ),
    );
    check_proposals(&mut v, "child.esp_proposals", &child.esp_proposals);
    check_duration(&mut v, "child.rekey_time_secs", child.rekey_time_secs, 60);
    if child.remote_ts.is_empty() {
        v.add(
            "child.remote_ts",
            crate::tr!(
                "enter at least one remote network (e.g. 192.0.2.0/24)",
                "informe ao menos uma rede remota (ex.: 192.0.2.0/24)"
            ),
        );
    }
    for (name, list) in [
        ("child.remote_ts", &child.remote_ts),
        ("child.local_ts", &child.local_ts),
    ] {
        v.check(
            list.len() <= 32,
            name,
            crate::tr!("at most 32 networks", "no máximo 32 redes"),
        );
        for (i, ts) in list.iter().enumerate() {
            v.check(
                is_subnet(ts),
                format!("{name}[{i}]"),
                crate::tr!(
                    "invalid network (use CIDR, e.g. 192.0.2.0/24, or an IP)",
                    "rede inválida (use CIDR, ex.: 192.0.2.0/24, ou um IP)"
                ),
            );
        }
    }

    // Network and DNS.
    let net = &p.network;
    v.check(
        (576..=9000).contains(&net.mtu),
        "network.mtu",
        crate::tr!(
            "use an MTU between 576 and 9000",
            "use um MTU entre 576 e 9000"
        ),
    );
    v.check(
        net.dns.servers.len() <= ipsec_client_proto::MAX_DNS_SERVERS,
        "network.dns.servers",
        crate::tr!("at most 8 servers", "no máximo 8 servidores"),
    );
    v.check(
        net.dns.domains.len() <= ipsec_client_proto::MAX_DNS_DOMAINS,
        "network.dns.domains",
        crate::tr!("at most 16 domains", "no máximo 16 domínios"),
    );
    for (i, domain) in net.dns.domains.iter().enumerate() {
        v.check(
            is_dns_domain(domain),
            format!("network.dns.domains[{i}]"),
            crate::tr!(
                "invalid domain (e.g. corp.example.com, ~corp.example.com or ~.)",
                "domínio inválido (ex.: corp.example.com, ~corp.example.com ou ~.)"
            ),
        );
    }

    v.0
}

fn check_auth(v: &mut Issues, p: &Profile) {
    let local = &p.auth.local;
    if local.is_empty() || local.len() > 2 {
        v.add(
            "auth.local",
            crate::tr!(
                "use one or two authentication rounds",
                "use uma ou duas rodadas de autenticação"
            ),
        );
    }
    for (i, round) in local.iter().enumerate() {
        let field = format!("auth.local[{i}]");
        match (i, round.method) {
            (0, LocalMethod::Psk) => {}
            (0, LocalMethod::EapMschapv2) => {
                if p.ike.version != 2 {
                    v.add(
                        field.clone(),
                        crate::tr!("EAP only exists in IKEv2", "EAP só existe no IKEv2"),
                    );
                }
            }
            (0, LocalMethod::Certificate) => {
                if p.ike.version == 1 && p.ike.aggressive {
                    v.add(
                        field.clone(),
                        crate::tr!(
                            "with a certificate, use main mode (aggressive: false)",
                            "com certificado, use o modo principal (aggressive: false)"
                        ),
                    );
                }
            }
            (1, LocalMethod::Xauth)
                if matches!(local[0].method, LocalMethod::Psk | LocalMethod::Certificate) =>
            {
                if p.ike.version != 1 {
                    v.add(
                        field.clone(),
                        crate::tr!("XAuth only exists in IKEv1", "XAuth só existe no IKEv1"),
                    );
                }
            }
            (_, LocalMethod::Xauth) => v.add(
                field.clone(),
                crate::tr!(
                    "XAuth only comes after a PSK or certificate round",
                    "XAuth só vem depois de uma rodada de PSK ou certificado"
                ),
            ),
            (_, LocalMethod::Certificate) => v.add(
                field.clone(),
                crate::tr!(
                    "the certificate round must be the first",
                    "a rodada do certificado deve ser a primeira"
                ),
            ),
            (_, LocalMethod::Psk) => v.add(
                field.clone(),
                crate::tr!(
                    "the PSK round must be the first",
                    "a rodada PSK deve ser a primeira"
                ),
            ),
            (_, LocalMethod::EapMschapv2) => v.add(
                field.clone(),
                crate::tr!(
                    "EAP must be the only round",
                    "o EAP deve ser a única rodada"
                ),
            ),
        }
        if round.token && round.method != LocalMethod::Certificate {
            v.add(
                format!("{field}.token"),
                crate::tr!(
                    "only a certificate can be on a smartcard or token",
                    "só um certificado pode estar num smartcard ou token"
                ),
            );
        }
        if round.otp && round.method != LocalMethod::Xauth {
            v.add(
                format!("{field}.otp"),
                crate::tr!(
                    "only an XAuth password can be a one-time code",
                    "só a senha XAuth pode ser um código de uso único"
                ),
            );
        }
        if let Some(id) = &round.id {
            if round.method == LocalMethod::Xauth {
                v.add(
                    format!("{field}.id"),
                    crate::tr!(
                        "the XAuth username is a credential, not part of the profile",
                        "o usuário XAuth é uma credencial, não faz parte do perfil"
                    ),
                );
            } else {
                v.check(
                    is_identity(id),
                    format!("{field}.id"),
                    crate::tr!("invalid identity (1 to 255 characters, no leading/trailing spaces or control characters)", "identidade inválida (1 a 255 caracteres, sem espaços nas pontas nem controle)"),
                );
            }
        }
    }
    if let Some(id) = &p.auth.remote.id {
        v.check(
            is_identity(id),
            "auth.remote.id",
            crate::tr!("invalid identity (1 to 255 characters, no leading/trailing spaces or control characters)", "identidade inválida (1 a 255 caracteres, sem espaços nas pontas nem controle)"),
        );
    }
    check_remote(v, p);
}

/// The gateway side: with EAP it must prove itself by certificate, checked
/// against the profile's CAs and identity; never "accept any certificate".
fn check_remote(v: &mut Issues, p: &Profile) {
    let remote = &p.auth.remote;
    let eap = p.auth.uses_eap();
    let client_cert = p.auth.uses_client_certificate();
    match remote.method {
        RemoteMethod::Psk => {
            if client_cert {
                v.add(
                    "auth.remote.method",
                    crate::tr!(
                        "with a client certificate the gateway must authenticate with a certificate too",
                        "com certificado do cliente o gateway também deve se autenticar por certificado"
                    ),
                );
            }
            if eap {
                v.add(
                    "auth.remote.method",
                    crate::tr!(
                        "with EAP the gateway must authenticate with a certificate",
                        "com EAP o gateway deve se autenticar por certificado"
                    ),
                );
            }
            if !remote.ca.is_empty() {
                v.add(
                    "auth.remote.ca",
                    crate::tr!(
                        "CA certificates only apply to a gateway with a certificate",
                        "certificados de CA só valem para um gateway com certificado"
                    ),
                );
            }
        }
        RemoteMethod::Certificate => {
            if !eap && !client_cert {
                v.add(
                    "auth.remote.method",
                    crate::tr!(
                        "a gateway certificate is only supported with EAP or a client certificate",
                        "certificado no gateway só com EAP ou certificado do cliente"
                    ),
                );
            }
            if remote.id.is_none() {
                v.add(
                    "auth.remote.id",
                    crate::tr!(
                        "enter the name in the gateway's certificate (e.g. vpn.example.com)",
                        "informe o nome que está no certificado do gateway (ex.: vpn.example.com)"
                    ),
                );
            }
            if remote.ca.is_empty() {
                v.add(
                    "auth.remote.ca",
                    crate::tr!(
                        "add the CA that signed the gateway's certificate",
                        "adicione a CA que assinou o certificado do gateway"
                    ),
                );
            }
            v.check(
                remote.ca.len() <= MAX_CAS,
                "auth.remote.ca",
                crate::tr!(
                    "at most 8 CA certificates",
                    "no máximo 8 certificados de CA"
                ),
            );
            for (i, pem) in remote.ca.iter().enumerate() {
                if let Err(message) = parse_ca(pem) {
                    v.add(format!("auth.remote.ca[{i}]"), message);
                }
            }
        }
    }
}

fn check_proposals(v: &mut Issues, field: &str, proposals: &[String]) {
    if proposals.is_empty() || proposals.len() > 16 {
        v.add(
            field,
            crate::tr!("enter 1 to 16 proposals", "informe de 1 a 16 propostas"),
        );
    }
    for (i, proposal) in proposals.iter().enumerate() {
        v.check(
            is_proposal(proposal),
            format!("{field}[{i}]"),
            crate::tr!(
                "invalid proposal (e.g. aes256-sha1-modp1024)",
                "proposta inválida (ex.: aes256-sha1-modp1024)"
            ),
        );
    }
}

fn check_duration(v: &mut Issues, field: &str, value: Option<u32>, min: u32) {
    if let Some(secs) = value {
        v.check(
            (min..=31_536_000).contains(&secs),
            field,
            crate::tr!(
                "duration out of range (60 s to 1 year)",
                "duração fora do intervalo (60 s a 1 ano)"
            ),
        );
    }
}

fn has_control(s: &str) -> bool {
    s.chars().any(char::is_control)
}

/// `[a-z0-9][a-z0-9-]{0,62}`.
pub(crate) fn is_slug(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some('a'..='z' | '0'..='9'))
        && s.len() <= 63
        && chars.all(|c| matches!(c, 'a'..='z' | '0'..='9' | '-'))
}

fn is_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 63
        && !label.starts_with('-')
        && !label.ends_with('-')
        && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// DNS name (without the optional final dot) or IP.
pub(crate) fn is_host(s: &str) -> bool {
    if s.parse::<IpAddr>().is_ok() {
        return true;
    }
    let name = s.strip_suffix('.').unwrap_or(s);
    !name.is_empty() && name.len() <= 253 && name.split('.').all(is_label)
}

/// Domain for systemd-resolved: `~.`, `domain` or `~domain`. The rule lives in
/// `ipsec-client-proto`, because the helper validates the same data.
pub(crate) fn is_dns_domain(s: &str) -> bool {
    ipsec_client_proto::valid_dns_domain(s)
}

/// CIDR (`192.0.2.0/24`, `2001:db8::/32`) or a single IP.
pub(crate) fn is_subnet(s: &str) -> bool {
    let (addr, prefix) = match s.split_once('/') {
        Some((addr, prefix)) => (addr, Some(prefix)),
        None => (s, None),
    };
    let Ok(ip) = addr.parse::<IpAddr>() else {
        return false;
    };
    let max = if ip.is_ipv4() { 32 } else { 128 };
    match prefix {
        None => true,
        Some(p) => {
            !p.is_empty()
                && p.chars().all(|c| c.is_ascii_digit())
                && p.parse::<u8>().is_ok_and(|n| n <= max)
        }
    }
}

/// strongSwan identity (e.g. `keyid:group`, `vpn.example.com`, IP, e-mail).
pub(crate) fn is_identity(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 255
        && s.trim() == s
        && !has_control(s)
        && !s.contains('\'')
        && !s.contains('"')
}

/// Proposal: `default`, or algorithms separated by hyphens.
pub(crate) fn is_proposal(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.split('-').all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs() {
        for ok in ["a", "minha-vpn", "vpn2", "0vpn"] {
            assert!(is_slug(ok), "{ok}");
        }
        for bad in [
            "",
            "-vpn",
            "Vpn",
            "vpn_1",
            "vpn 1",
            "vpn;rm",
            &"a".repeat(64),
        ] {
            assert!(!is_slug(bad), "{bad}");
        }
    }

    #[test]
    fn hosts() {
        for ok in [
            "vpn.example.com",
            "203.0.113.10",
            "2001:db8::1",
            "gw",
            "vpn.example.com.",
        ] {
            assert!(is_host(ok), "{ok}");
        }
        for bad in [
            "",
            "vpn..example.com",
            "-vpn.example.com",
            "vpn example",
            "vpn.example.com;id",
        ] {
            assert!(!is_host(bad), "{bad}");
        }
    }

    #[test]
    fn subnets() {
        for ok in [
            "192.0.2.0/24",
            "0.0.0.0/0",
            "198.51.100.7",
            "2001:db8::/32",
            "10.0.0.0/8",
        ] {
            assert!(is_subnet(ok), "{ok}");
        }
        for bad in [
            "192.0.2.0/33",
            "192.0.2.0/",
            "192.0.2/24",
            "2001:db8::/129",
            "abc",
            "1.2.3.4/+8",
        ] {
            assert!(!is_subnet(bad), "{bad}");
        }
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
            assert!(is_dns_domain(ok), "{ok}");
        }
        for bad in ["", "~", "~~corp", "corp..x", "co rp", "corp'; rm -rf /"] {
            assert!(!is_dns_domain(bad), "{bad}");
        }
    }

    #[test]
    fn identities_and_proposals() {
        assert!(is_identity("keyid:grupo-teste"));
        assert!(is_identity("usuario@example.com"));
        assert!(!is_identity(" espaco"));
        assert!(!is_identity("aspas'"));
        assert!(!is_identity("linha\nnova"));
        assert!(is_proposal("aes256-sha1-modp1024"));
        assert!(is_proposal("aes256gcm16-prfsha256-ecp256"));
        assert!(is_proposal("default"));
        assert!(!is_proposal("aes256--sha1"));
        assert!(!is_proposal("AES256-SHA1"));
        assert!(!is_proposal("aes;rm"));
    }
}
