//! Identity masking in displayed text (ADR-0007) and anonymization of logs
//! for sharing.
//!
//! At log level 1, charon does not log the PSK or the password, but it does
//! log identities, such as the XAuth username (`XAuth authentication of
//! '<user>'`) and the group id (`established between ...[group]`). Before
//! showing a log, the app replaces those identities with masked versions
//! ([`Redactor`]).
//!
//! The log also shows the gateway address, the virtual IP, the internal
//! networks and the organization's domains. That is useful on screen, but must
//! not go into a bug report. **Copied** text goes through the [`Anonymizer`],
//! which replaces that data with stable placeholders (`<gateway>`, `<ip-1>`,
//! `<domain-1>`).

use std::net::IpAddr;

/// Mascara um valor: `usuario.exemplo` → `us***lo`. Valores curtos viram `***`.
#[must_use]
pub fn mask(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    match chars.len() {
        0..=3 => "***".to_owned(),
        4..=7 => format!("{}***", chars[0]),
        n => format!(
            "{}{}***{}{}",
            chars[0],
            chars[1],
            chars[n - 2],
            chars[n - 1]
        ),
    }
}

/// Troca identidades conhecidas por versões mascaradas.
#[derive(Debug, Clone, Default)]
pub struct Redactor {
    /// (original, mascarado), do mais longo para o mais curto.
    needles: Vec<(String, String)>,
}

/// Identidades com menos caracteres que isto não são mascaradas, para não
/// destruir o texto do log trocando sílabas comuns.
const MIN_NEEDLE: usize = 3;

impl Redactor {
    /// Cria um mascarador para as identidades dadas. Para identidades com
    /// prefixo de tipo (`keyid:grupo`, `fqdn:host`), mascara também o valor sem
    /// o prefixo, que é como o charon costuma registrar.
    pub fn new<'a>(identities: impl IntoIterator<Item = &'a str>) -> Self {
        let mut needles: Vec<(String, String)> = Vec::new();
        for identity in identities {
            let bare = identity
                .split_once(':')
                .filter(|(prefix, _)| {
                    matches!(
                        *prefix,
                        "keyid" | "fqdn" | "email" | "rfc822" | "ipv4" | "ipv6" | "asn1dn"
                    )
                })
                .map(|(_, rest)| rest);
            for value in std::iter::once(identity).chain(bare) {
                if value.chars().count() >= MIN_NEEDLE && !needles.iter().any(|(n, _)| n == value) {
                    needles.push((value.to_owned(), mask(value)));
                }
            }
        }
        // Os mais longos primeiro: `keyid:grupo` antes de `grupo`.
        needles.sort_by_key(|(n, _)| std::cmp::Reverse(n.len()));
        Self { needles }
    }

    /// Texto com as identidades mascaradas.
    #[must_use]
    pub fn redact(&self, text: &str) -> String {
        let mut out = text.to_owned();
        for (needle, masked) in &self.needles {
            if out.contains(needle.as_str()) {
                out = out.replace(needle.as_str(), masked);
            }
        }
        out
    }
}

/// Anonimiza um log para compartilhar: identidades mascaradas, gateway,
/// domínios do perfil e **todo** endereço IP trocados por marcadores. O mesmo
/// endereço recebe sempre o mesmo marcador no texto, para o log continuar
/// legível.
#[derive(Debug, Clone, Default)]
pub struct Anonymizer {
    redactor: Redactor,
    /// (nome em minúsculas, marcador), do mais longo para o mais curto.
    names: Vec<(String, String)>,
    /// Endereços com marcador fixo (gateway, DNS do perfil).
    known_ips: Vec<(IpAddr, String)>,
}

impl Anonymizer {
    /// Cria o anonimizador a partir do que o perfil e as credenciais sabem.
    pub fn new<'a>(
        identities: impl IntoIterator<Item = &'a str>,
        gateway: &str,
        dns_servers: &[IpAddr],
        domains: &[String],
    ) -> Self {
        let mut names: Vec<(String, String)> = Vec::new();
        let mut known_ips: Vec<(IpAddr, String)> = Vec::new();
        let gateway = gateway.trim().trim_end_matches('.');
        match gateway.parse::<IpAddr>() {
            Ok(ip) => known_ips.push((ip, "<gateway>".to_owned())),
            Err(_) if gateway.len() >= 3 => {
                names.push((gateway.to_ascii_lowercase(), "<gateway>".to_owned()));
            }
            Err(_) => {}
        }
        for (i, ip) in dns_servers.iter().enumerate() {
            if !known_ips.iter().any(|(known, _)| known == ip) {
                known_ips.push((*ip, format!("<dns-{}>", i + 1)));
            }
        }
        let mut n = 0;
        for domain in domains {
            let name = domain
                .trim_start_matches('~')
                .trim_end_matches('.')
                .to_ascii_lowercase();
            if name.len() < 3 || names.iter().any(|(known, _)| *known == name) {
                continue;
            }
            n += 1;
            names.push((name, format!("<domain-{n}>")));
        }
        names.sort_by_key(|(name, _)| std::cmp::Reverse(name.len()));
        Self {
            redactor: Redactor::new(identities),
            names,
            known_ips,
        }
    }

    /// Texto anonimizado.
    #[must_use]
    pub fn anonymize(&self, text: &str) -> String {
        let mut out = self.redactor.redact(text);
        for (name, marker) in &self.names {
            out = replace_ascii_ci(&out, name, marker);
        }
        replace_ips(&out, &self.known_ips)
    }
}

/// Troca `needle` (ASCII, em minúsculas) por `with`, sem diferenciar
/// maiúsculas. `to_ascii_lowercase` mantém as posições dos bytes.
fn replace_ascii_ci(haystack: &str, needle: &str, with: &str) -> String {
    let lower = haystack.to_ascii_lowercase();
    let mut out = String::with_capacity(haystack.len());
    let mut last = 0;
    for (pos, _) in lower.match_indices(needle) {
        out.push_str(&haystack[last..pos]);
        out.push_str(with);
        last = pos + needle.len();
    }
    out.push_str(&haystack[last..]);
    out
}

/// Endereços que não identificam nada e ajudam a ler o log (rota padrão,
/// loopback, broadcast).
fn keep_ip(ip: &IpAddr) -> bool {
    ip.is_unspecified() || ip.is_loopback() || matches!(ip, IpAddr::V4(v4) if v4.is_broadcast())
}

fn is_ip_char(c: char) -> bool {
    c.is_ascii_hexdigit() || c == '.' || c == ':'
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Troca cada IPv4/IPv6 do texto por um marcador (`<ip-N>`, ou o fixo de
/// `known`). Um candidato é uma sequência máxima de `[0-9A-Fa-f.:]` que não
/// encosta em letras ou dígitos de uma palavra maior.
fn replace_ips(text: &str, known: &[(IpAddr, String)]) -> String {
    let mut markers: Vec<(IpAddr, String)> = known.to_vec();
    let mut next = 1;
    let mut out = String::with_capacity(text.len());
    let mut prev: Option<char> = None;
    let mut chars = text.char_indices().peekable();
    while let Some((start, c)) = chars.next() {
        if !is_ip_char(c) || prev.is_some_and(is_word_char) {
            out.push(c);
            prev = Some(c);
            continue;
        }
        let mut end = start + c.len_utf8();
        while let Some(&(i, d)) = chars.peek() {
            if !is_ip_char(d) {
                break;
            }
            end = i + d.len_utf8();
            chars.next();
        }
        let token = &text[start..end];
        let after = chars.peek().map(|&(_, d)| d);
        // Ponto final de frase não faz parte do endereço, nem as reticências
        // ou os dois-pontos antes dele (`...198.51.100.7`, `dns::192.0.2.1`).
        let core = token.trim_end_matches('.');
        let candidate = if core.parse::<IpAddr>().is_ok() {
            core
        } else {
            core.trim_start_matches(['.', ':'])
        };
        let prefix = &core[..core.len() - candidate.len()];
        let ip = candidate
            .parse::<IpAddr>()
            .ok()
            .filter(|ip| !keep_ip(ip) && !after.is_some_and(is_word_char));
        if let Some(ip) = ip {
            out.push_str(prefix);
            let marker = if let Some((_, m)) = markers.iter().find(|(known, _)| *known == ip) {
                m.clone()
            } else {
                let m = format!("<ip-{next}>");
                next += 1;
                markers.push((ip, m.clone()));
                m
            };
            out.push_str(&marker);
            out.push_str(&token[core.len()..]);
        } else {
            out.push_str(token);
        }
        prev = token.chars().last();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_by_length() {
        assert_eq!(mask(""), "***");
        assert_eq!(mask("abc"), "***");
        assert_eq!(mask("joana"), "j***");
        assert_eq!(mask("usuario.exemplo"), "us***lo");
        assert_eq!(mask("ção-ãé"), "ç***");
    }

    #[test]
    fn redacts_charon_log_lines() {
        let r = Redactor::new(["usuario.teste", "keyid:grupo-teste"]);
        let line = "XAuth authentication of 'usuario.teste' (myself) successful";
        assert_eq!(
            r.redact(line),
            "XAuth authentication of 'us***te' (myself) successful"
        );
        let line = "IKE_SA teste[1] established between 172.19.0.1[grupo-teste]...172.19.0.2";
        let redacted = r.redact(line);
        assert!(!redacted.contains("grupo-teste"), "{redacted}");
        assert!(redacted.contains("[gr***te]"), "{redacted}");
    }

    fn anonymizer() -> Anonymizer {
        Anonymizer::new(
            ["usuario.teste", "keyid:grupo-teste"],
            "vpn.example.com",
            &["192.0.2.53".parse().expect("ip")],
            &["~corp.example.com".to_owned(), "~.".to_owned()],
        )
    }

    #[test]
    fn anonymizes_a_charon_log() {
        let a = anonymizer();
        let log = "\
2026-11-16 15:30:14 05[IKE] <teste|1> initiating Aggressive Mode IKE_SA teste[1] to 198.51.100.7
2026-11-16 15:30:14 09[IKE] <teste|1> XAuth authentication of 'usuario.teste' (myself) successful
2026-11-16 15:30:14 09[IKE] IKE_SA teste[1] established between 172.19.0.3[grupo-teste]...198.51.100.7[VPN.Example.com]
2026-11-16 15:30:14 15[IKE] <teste|1> installing new virtual IP 10.20.30.40
2026-11-16 15:30:14 07[IKE] CHILD_SA rede{1} established with SPIs ce662fe9_i c7e6d166_o and TS 10.20.30.40/32 === 10.0.0.0/8
updown: DNS em ipsc0: servidores [192.0.2.53] domínios [~corp.example.com] host srv.corp.example.com.
rota 0.0.0.0/1 via ipsc0; fe80::1 e 2001:db8::7 vistos; versão 6.1.0; hora 15:30:14";
        let out = a.anonymize(log);
        for leaked in [
            "usuario.teste",
            "grupo-teste",
            "198.51.100.7",
            "172.19.0.3",
            "10.20.30.40",
            "10.0.0.0",
            "192.0.2.53",
            "corp.example.com",
            "vpn.example.com",
            "VPN.Example.com",
            "fe80::1",
            "2001:db8::7",
        ] {
            assert!(!out.contains(leaked), "vazou {leaked}:\n{out}");
        }
        // O mesmo endereço recebe o mesmo marcador; o resto do log fica legível.
        assert!(out.contains("to <ip-1>"), "{out}");
        assert!(out.contains("<ip-2>[gr***te]...<ip-1>[<gateway>]"), "{out}");
        assert!(out.contains("virtual IP <ip-3>"), "{out}");
        assert!(out.contains("TS <ip-3>/32 === <ip-4>/8"), "{out}");
        assert!(
            out.contains("servidores [<dns-1>] domínios [~<domain-1>]"),
            "{out}"
        );
        assert!(out.contains("host srv.<domain-1>."), "{out}");
        assert!(
            out.contains("rota 0.0.0.0/1"),
            "a rota padrão ajuda a ler o log: {out}"
        );
        assert!(out.contains("SPIs ce662fe9_i c7e6d166_o"), "{out}");
        assert!(out.contains("versão 6.1.0; hora 15:30:14"), "{out}");
        assert!(out.starts_with("2026-11-16 15:30:14 05[IKE]"), "{out}");
    }

    #[test]
    fn gateway_given_as_ip_gets_its_own_marker() {
        let a = Anonymizer::new([], "203.0.113.9", &[], &[]);
        assert_eq!(
            a.anonymize("from 192.0.2.1[500] to 203.0.113.9[500]"),
            "from <ip-1>[500] to <gateway>[500]"
        );
    }

    #[test]
    fn leaves_non_addresses_alone() {
        let a = Anonymizer::default();
        for text in [
            "aes256-sha1-modp1024",
            "Linux 7.3.3-300.fc45.x86_64",
            "deadbeef cafe:babe 1.2.3 12:34:56",
            "MAC aa:bb:cc:dd:ee:ff",
            "ação ç ü 漢字",
        ] {
            assert_eq!(a.anonymize(text), text);
        }
    }

    #[test]
    fn ignores_short_identities() {
        let r = Redactor::new(["ab", "keyid:x"]);
        // "ab" e "x" são curtos demais; "keyid:x" (7 caracteres) vira "k***".
        assert_eq!(r.redact("ab x keyid:x"), "ab x k***");
    }
}
