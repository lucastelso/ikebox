//! Validação do perfil: o Rust é a fonte da verdade (ADR-0008).
//!
//! As regras protegem duas fronteiras: o charon (`load-conn`) e o `updown`,
//! que roda via `/bin/sh -c` e por isso recebe só valores validados (sem aspas
//! simples nem caracteres de controle).

use std::fmt;
use std::net::IpAddr;

use super::model::{AuthMethod, Profile, SCHEMA_VERSION};

/// Um problema encontrado num campo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationIssue {
    /// Caminho do campo (ex.: `child.remote_ts[1]`).
    pub field: String,
    /// Descrição para o usuário.
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

/// Valida o perfil inteiro e devolve todos os problemas (vazio = válido).
#[must_use]
pub fn validate(p: &Profile) -> Vec<ValidationIssue> {
    let mut v = Issues::default();

    v.check(
        p.schema_version == SCHEMA_VERSION,
        "schema_version",
        "versão de formato não suportada",
    );
    v.check(
        is_slug(&p.id),
        "id",
        "use de 1 a 63 caracteres: letras minúsculas, dígitos e hífen (sem começar com hífen)",
    );
    v.check(
        !p.name.trim().is_empty() && p.name.chars().count() <= 100 && !has_control(&p.name),
        "name",
        "informe um nome de até 100 caracteres",
    );
    v.check(
        is_host(&p.gateway),
        "gateway",
        "informe um nome DNS ou um endereço IP válido",
    );

    // Fase 1.
    let ike = &p.ike;
    v.check(
        matches!(ike.version, 1 | 2),
        "ike.version",
        "a versão do IKE deve ser 1 ou 2",
    );
    if ike.aggressive && ike.version != 1 {
        v.add("ike.aggressive", "o modo agressivo só existe no IKEv1");
    }
    check_proposals(&mut v, "ike.proposals", &ike.proposals);
    check_duration(&mut v, "ike.rekey_time_secs", ike.rekey_time_secs, 60);
    if let Some(dpd) = ike.dpd_delay_secs {
        v.check(
            (1..=3600).contains(&dpd),
            "ike.dpd_delay_secs",
            "use de 1 a 3600 segundos",
        );
    }

    // Autenticação.
    if p.auth.method == AuthMethod::PskXauth && ike.version != 1 {
        v.add("auth.method", "XAuth só existe no IKEv1");
    }
    for (field, id) in [
        ("auth.local_id", &p.auth.local_id),
        ("auth.remote_id", &p.auth.remote_id),
    ] {
        if let Some(id) = id {
            v.check(
                is_identity(id),
                field,
                "identidade inválida (1 a 255 caracteres, sem espaços nas pontas nem controle)",
            );
        }
    }

    // Fase 2.
    let child = &p.child;
    v.check(
        is_slug(&child.name),
        "child.name",
        "use letras minúsculas, dígitos e hífen",
    );
    check_proposals(&mut v, "child.esp_proposals", &child.esp_proposals);
    check_duration(&mut v, "child.rekey_time_secs", child.rekey_time_secs, 60);
    if child.remote_ts.is_empty() {
        v.add(
            "child.remote_ts",
            "informe ao menos uma rede remota (ex.: 192.0.2.0/24)",
        );
    }
    for (name, list) in [
        ("child.remote_ts", &child.remote_ts),
        ("child.local_ts", &child.local_ts),
    ] {
        v.check(list.len() <= 32, name, "no máximo 32 redes");
        for (i, ts) in list.iter().enumerate() {
            v.check(
                is_subnet(ts),
                format!("{name}[{i}]"),
                "rede inválida (use CIDR, ex.: 192.0.2.0/24, ou um IP)",
            );
        }
    }

    // Rede e DNS.
    let net = &p.network;
    v.check(
        (576..=9000).contains(&net.mtu),
        "network.mtu",
        "use um MTU entre 576 e 9000",
    );
    v.check(
        net.dns.servers.len() <= 8,
        "network.dns.servers",
        "no máximo 8 servidores",
    );
    v.check(
        net.dns.domains.len() <= 16,
        "network.dns.domains",
        "no máximo 16 domínios",
    );
    for (i, domain) in net.dns.domains.iter().enumerate() {
        v.check(
            is_dns_domain(domain),
            format!("network.dns.domains[{i}]"),
            "domínio inválido (ex.: corp.example.com, ~corp.example.com ou ~.)",
        );
    }

    v.0
}

fn check_proposals(v: &mut Issues, field: &str, proposals: &[String]) {
    if proposals.is_empty() || proposals.len() > 16 {
        v.add(field, "informe de 1 a 16 propostas");
    }
    for (i, proposal) in proposals.iter().enumerate() {
        v.check(
            is_proposal(proposal),
            format!("{field}[{i}]"),
            "proposta inválida (ex.: aes256-sha1-modp1024)",
        );
    }
}

fn check_duration(v: &mut Issues, field: &str, value: Option<u32>, min: u32) {
    if let Some(secs) = value {
        v.check(
            (min..=31_536_000).contains(&secs),
            field,
            "duração fora do intervalo (60 s a 1 ano)",
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

/// Nome DNS (sem ponto final opcional) ou IP.
pub(crate) fn is_host(s: &str) -> bool {
    if s.parse::<IpAddr>().is_ok() {
        return true;
    }
    let name = s.strip_suffix('.').unwrap_or(s);
    !name.is_empty() && name.len() <= 253 && name.split('.').all(is_label)
}

/// Domínio para o systemd-resolved: `~.`, `dominio` ou `~dominio`.
pub(crate) fn is_dns_domain(s: &str) -> bool {
    if s == "~." {
        return true;
    }
    let name = s.strip_prefix('~').unwrap_or(s);
    let name = name.strip_suffix('.').unwrap_or(name);
    !name.is_empty() && name.len() <= 253 && name.split('.').all(is_label)
}

/// CIDR (`192.0.2.0/24`, `2001:db8::/32`) ou IP único.
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

/// Identidade do strongSwan (ex.: `keyid:grupo`, `vpn.example.com`, IP, e-mail).
pub(crate) fn is_identity(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 255
        && s.trim() == s
        && !has_control(s)
        && !s.contains('\'')
        && !s.contains('"')
}

/// Proposta: `default` ou algoritmos separados por hífen.
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
