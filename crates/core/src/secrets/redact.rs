//! Identity masking in displayed text (ADR-0007) and anonymization of logs
//! for sharing.
//!
//! At log level 1, charon does not log the PSK or the password, but it does
//! log identities, such as the XAuth username (`XAuth authentication of
//! '<user>'`) and the group id (`established between ...[group]`). Before
//! showing a log, the app replaces those identities with masked versions
//! ([`Redactor`]).
//!
//! The masking does not depend on knowing the identity. The known ones (from
//! the profile and the credentials) are replaced wherever they show up, and
//! then every identity is masked **by the shape of the charon line**: quoted
//! (`'%Y'`), in brackets after an address (`%H[%Y]`) and in the configuration
//! dump of log level 2 (`xauth_id = %Y`). Without that, the XAuth username
//! went out unmasked whenever the process reading the log did not have the
//! credentials (audit of 0.2.0).
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

/// Makes a log line safe to show: control characters (escape sequences, line
/// breaks, backspace) and bidirectional overrides become U+FFFD. Tabs stay.
///
/// The engine log carries text chosen by the other end of the tunnel (the
/// XAuth message of the gateway, for one, is logged as it came). Printed raw,
/// it could drive the user's terminal (move the cursor, rewrite lines, set
/// the window title) or fake lines of the log.
#[must_use]
pub fn printable(text: &str) -> String {
    let unsafe_char = |c: char| {
        (c.is_control() && c != '\t')
            || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
    };
    if !text.contains(unsafe_char) {
        return text.to_owned();
    }
    text.chars()
        .map(|c| if unsafe_char(c) { '\u{fffd}' } else { c })
        .collect()
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

    /// Text with the identities masked: the known ones wherever they are, and
    /// any other identity by the shape of the charon line.
    #[must_use]
    pub fn redact(&self, text: &str) -> String {
        mask_structural(&self.redact_known(text))
    }

    /// Replaces only the known identities.
    fn redact_known(&self, text: &str) -> String {
        let mut out = text.to_owned();
        for (needle, masked) in &self.needles {
            if out.contains(needle.as_str()) {
                out = out.replace(needle.as_str(), masked);
            }
        }
        out
    }
}

/// Words that come right before a quoted value that is **not** an identity
/// in charon messages (`plugin 'gmp'`, `config 'name'`). Any other quoted
/// value is treated as an identity: when in doubt, mask.
const QUOTED_NON_IDENTITY: &[&str] = &[
    "plugin",
    "plugins",
    "config",
    "configs",
    // "configure 'version = 2' in order to use IKEv2" (IKEv1 deprecation).
    "configure",
    "interface",
    "file",
    "script",
    "type",
    "policy",
    "label",
    "flag",
    "pool",
    "settings",
    "section",
    "sa",
];

/// Identity lines of the configuration dump (log level 2, `vici_config.c`).
const IDENTITY_DUMPS: &[&str] = &[
    " id = ",
    " xauth_id = ",
    " eap_id = ",
    " aaa_id = ",
    " ca_id = ",
    " ppk_id = ",
    " cert = ",
    " cacert = ",
    " group = ",
    " mediation_peer = ",
];

/// Longest quoted value treated as an identity (a certificate DN fits).
const MAX_QUOTED: usize = 256;

/// Values that stay as they are: already masked, a marker of the anonymizer
/// (`<gateway>`) or an IP address (shown like the other addresses, and
/// replaced by the anonymizer).
fn keeps_as_is(value: &str) -> bool {
    value.is_empty()
        || value.contains("***")
        || value.parse::<IpAddr>().is_ok()
        || (value.len() > 2
            && value.starts_with('<')
            && value.ends_with('>')
            && !value[1..value.len() - 1].contains(['<', '>']))
}

fn mask_unless_kept(value: &str) -> String {
    if keeps_as_is(value) {
        value.to_owned()
    } else {
        mask(value)
    }
}

/// Masks identities by the shape of the charon line, known or not.
fn mask_structural(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let (body, eol) = match line.strip_suffix('\n') {
            Some(body) => (body, "\n"),
            None => (line, ""),
        };
        out.push_str(&mask_delimited(&mask_identity_dump(body)));
        out.push_str(eol);
    }
    out
}

/// `   xauth_id = user` → `   xauth_id = us***er` (the value goes to the end
/// of the line).
fn mask_identity_dump(line: &str) -> String {
    for pattern in IDENTITY_DUMPS {
        if let Some(pos) = line.find(pattern) {
            let start = pos + pattern.len();
            let value = line[start..].trim_end();
            return format!(
                "{}{}{}",
                &line[..start],
                mask_unless_kept(value),
                &line[start + value.len()..]
            );
        }
    }
    line.to_owned()
}

/// Masks `'identity'`, `"identity"` and `address[identity]`.
fn mask_delimited(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(c) = rest.chars().next() {
        let at = line.len() - rest.len();
        let found = match c {
            '\'' | '"' => quoted_value(line, at, c),
            '[' => bracket_identity(line, at),
            _ => None,
        };
        if let Some((close, value)) = found {
            let closing = if c == '[' { ']' } else { c };
            out.push(c);
            out.push_str(&value);
            out.push(closing);
            rest = &line[close + closing.len_utf8()..];
        } else {
            out.push(c);
            rest = &rest[c.len_utf8()..];
        }
    }
    out
}

/// A quoted value starting at `open`: the position of the closing quote and
/// the value to show (masked, unless it is not an identity). `None` when the
/// quote is an apostrophe (`doesn't`) or has no closing pair.
fn quoted_value(line: &str, open: usize, quote: char) -> Option<(usize, String)> {
    let before = &line[..open];
    if before
        .chars()
        .next_back()
        .is_some_and(char::is_alphanumeric)
    {
        return None;
    }
    let inner = &line[open + quote.len_utf8()..];
    let (len, content) = inner.match_indices(quote).find_map(|(len, _)| {
        let content = &inner[..len];
        let after = inner[len + quote.len_utf8()..].chars().next();
        (!content.is_empty()
            && len <= MAX_QUOTED
            && !content.starts_with(char::is_whitespace)
            && !content.ends_with(char::is_whitespace)
            && !after.is_some_and(char::is_alphanumeric))
        .then_some((len, content))
    })?;
    let word: String = before
        .trim_end_matches(' ')
        .chars()
        .rev()
        .take_while(|c| c.is_alphabetic())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>()
        .to_lowercase();
    let value = if QUOTED_NON_IDENTITY.contains(&word.as_str()) {
        content.to_owned()
    } else {
        mask_unless_kept(content)
    };
    Some((open + quote.len_utf8() + len, value))
}

/// `address[identity]` (`%H[%Y]`): the position of the `]` and the identity
/// masked. `None` when the bracket does not follow an address (`05[IKE]`,
/// `name[1]`) or holds a number (`address[500]` is a port).
fn bracket_identity(line: &str, open: usize) -> Option<(usize, String)> {
    let before = &line[..open];
    let start = before
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_ip_char(*c))
        .last()
        .map(|(i, _)| i)?;
    let host = &before[start..];
    let is_address = host.parse::<IpAddr>().is_ok()
        || host
            .trim_start_matches(['.', ':'])
            .parse::<IpAddr>()
            .is_ok();
    if !is_address {
        return None;
    }
    let inner = &line[open + 1..];
    let len = inner.find(']')?;
    let content = &inner[..len];
    // A certificate subject (`O=Example Org, CN=Name`) has spaces; any other
    // spaced content is not an identity.
    let distinguished_name = content.contains('=');
    if content.is_empty()
        || (content.contains(char::is_whitespace) && !distinguished_name)
        || content.chars().all(|c| c.is_ascii_digit())
    {
        return None;
    }
    Some((open + 1 + len, mask_unless_kept(content)))
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
        // Known values first (identities, gateway and domains), then every
        // other identity by the shape of the line, and the addresses last
        // (the shapes need the addresses still in place).
        let mut out = self.redactor.redact_known(text);
        for (name, marker) in &self.names {
            out = replace_ascii_ci(&out, name, marker);
        }
        replace_ips(
            &mask_certificate_names(&mask_structural(&out)),
            &self.known_ips,
        )
    }
}

/// charon prints certificate names in double quotes (`received end entity
/// cert "O=..., CN=..."`, `using trusted ca certificate "..."`, `sending cert
/// request for "..."`): organization and people's names. In a line about
/// certificates, each quoted name becomes `<certificate>`.
fn mask_certificate_names(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        // Only lines about certificates, and only paired quotes.
        if !line.to_ascii_lowercase().contains("cert") || line.matches('"').count() % 2 != 0 {
            out.push_str(line);
            continue;
        }
        for (i, part) in line.split('"').enumerate() {
            out.push_str(if i % 2 == 1 {
                "\"<certificate>\""
            } else {
                part
            });
        }
    }
    out
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
    fn printable_neutralizes_terminal_control() {
        // Text a gateway can put in the log (XAuth message).
        let hostile =
            "XAuth message: ok\u{1b}[2J\u{1b}]0;title\u{7}\r\n[engine] ready\u{8}\u{9b}31m";
        let safe = printable(hostile);
        assert!(!safe.chars().any(char::is_control), "{safe:?}");
        assert!(safe.starts_with("XAuth message: ok\u{fffd}[2J"));
        assert_eq!(safe.chars().count(), hostile.chars().count());
        // Bidirectional overrides can reorder what is displayed.
        assert_eq!(printable("a\u{202e}b\u{2066}c"), "a\u{fffd}b\u{fffd}c");
        // Ordinary lines are untouched, tabs and non-ASCII text included.
        let plain = "09[IKE] <vpn|1>\tconexão estabelecida — ok";
        assert_eq!(printable(plain), plain);
    }

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

    /// The reader of the log may not have the credentials (CLI without the
    /// keyring, app reopened with the VPN up): the identities are masked by
    /// the shape of the line, with no known identity at all.
    #[test]
    fn masks_identities_it_does_not_know() {
        let r = Redactor::default();
        for (line, expected) in [
            (
                "09[IKE] XAuth authentication of 'usuario.exemplo' (myself) successful",
                "09[IKE] XAuth authentication of 'us***lo' (myself) successful",
            ),
            (
                "05[CFG] no XAuth secret found for 'grupo-exemplo' - 'usuario.exemplo'",
                "05[CFG] no XAuth secret found for 'gr***lo' - 'us***lo'",
            ),
            (
                "09[IKE] IKE_SA gw[1] established between 198.51.100.7[grupo-exemplo]...203.0.113.5[vpn.example.com]",
                "09[IKE] IKE_SA gw[1] established between 198.51.100.7[gr***lo]...203.0.113.5[vp***om]",
            ),
            (
                "05[CFG]    xauth_id = usuario.exemplo",
                "05[CFG]    xauth_id = us***lo",
            ),
            (
                "05[CFG]    id = keyid:grupo-exemplo",
                "05[CFG]    id = ke***lo",
            ),
            (
                "06[CFG]   using certificate \"C=BR, O=Exemplo, CN=vpn.example.com\"",
                "06[CFG]   using certificate \"C=***om\"",
            ),
            (
                "2001:db8::7[usuario.exemplo] is the peer",
                "2001:db8::7[us***lo] is the peer",
            ),
        ] {
            assert_eq!(r.redact(line), expected);
            assert_eq!(r.redact(expected), expected, "idempotent: {line}");
        }
    }

    #[test]
    fn structural_masking_leaves_the_rest_readable() {
        let r = Redactor::default();
        for line in [
            // Thread and group prefix, SA names with their number, ports.
            "15:30:14 05[IKE] <gw|1> initiating Aggressive Mode IKE_SA gw[1] to 203.0.113.5",
            "13[NET] sending packet: from 198.51.100.7[500] to 203.0.113.5[4500] (364 bytes)",
            "07[IKE] CHILD_SA rede{1} established with SPIs ce662fe9_i c7e6d166_o",
            // Quoted values that are not identities.
            "00[LIB] plugin 'gmp': failed to load",
            "15[CFG] WARNING: IKEv1 support is deprecated and will be removed soon, configure 'version = 2' in order to use IKEv2",
            "05[CFG] added vici connection: config 'gateway-teste'",
            "05[KNL] interface 'ipsc0' not found",
            // Apostrophes and addresses as identities.
            "09[IKE] peer didn't accept the proposal, it doesn't match",
            "09[IKE] authentication of '203.0.113.5' with pre-shared key successful",
            "09[IKE] established between 198.51.100.7[198.51.100.7]...203.0.113.5[203.0.113.5]",
            // Our own lines.
            "updown: DNS on ipsc0: servers [192.0.2.53] domains [~corp.example.com]",
        ] {
            assert_eq!(r.redact(line), line);
        }
    }

    #[test]
    fn masks_certificate_subjects_in_every_shape() {
        let r = Redactor::new([]);
        let line = "IKE_SA v2-cert[1] established between \
                    172.19.0.7[O=Example Org, CN=Alice Teste]...172.19.0.6[gw.example.com]\n\
                    authentication of 'O=Example Org, CN=Alice Teste' (myself) with RSA\n\
                    sending end entity cert \"O=Example Org, CN=Alice Teste\"\n";
        let out = r.redact(line);
        assert!(!out.contains("Alice"), "{out}");
        assert!(!out.contains("Example Org"), "{out}");
        // Other spaced brackets after an address stay as they are.
        assert_eq!(
            r.redact("from 192.0.2.1[not an id]"),
            "from 192.0.2.1[not an id]"
        );
    }

    #[test]
    fn anonymizes_certificate_names() {
        let a = anonymizer();
        let log = "received end entity cert \"O=Example Corp, CN=gw.example.com\"\n\
                   using trusted ca certificate \"O=Example Corp, CN=Example CA\"\n\
                   sending cert request for \"O=Example Corp, CN=Example CA\"\n\
                   EAP-MS-CHAPv2 succeeded: 'Welcome2strongSwan'\n";
        let out = a.anonymize(log);
        assert!(!out.contains("Example Corp"), "{out}");
        assert_eq!(out.matches("\"<certificate>\"").count(), 3, "{out}");
        assert!(out.ends_with('\n'));
        // A stray quote in a certificate line is left as it is.
        assert_eq!(mask_certificate_names("cert \"half"), "cert \"half");
    }

    #[test]
    fn anonymizes_identities_it_does_not_know() {
        // Gateway by address, identity by name: neither the username nor the
        // gateway identity are in the profile.
        let a = Anonymizer::new([], "203.0.113.5", &[], &[]);
        let out = a.anonymize(
            "XAuth authentication of 'usuario.exemplo' (myself) successful\n\
             IKE_SA gw[1] established between 198.51.100.7[grupo-exemplo]...203.0.113.5[vpn.interno.example]\n\
             authentication of '203.0.113.5' with pre-shared key successful",
        );
        for leaked in [
            "usuario.exemplo",
            "grupo-exemplo",
            "vpn.interno.example",
            "203.0.113.5",
            "198.51.100.7",
        ] {
            assert!(!out.contains(leaked), "leaked {leaked}:\n{out}");
        }
        assert!(
            out.contains("<ip-1>[gr***lo]...<gateway>[vp***le]"),
            "{out}"
        );
        assert!(out.contains("authentication of '<gateway>'"), "{out}");
    }

    #[test]
    fn ignores_short_identities() {
        let r = Redactor::new(["ab", "keyid:x"]);
        // "ab" e "x" são curtos demais; "keyid:x" (7 caracteres) vira "k***".
        assert_eq!(r.redact("ab x keyid:x"), "ab x k***");
    }
}
