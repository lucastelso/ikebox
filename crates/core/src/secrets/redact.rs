//! Mascaramento de identidades em texto exibido (ADR-0007).
//!
//! O charon, no nível de log 1, não registra PSK nem senha, mas registra
//! identidades, como o usuário XAuth (`XAuth authentication of '<usuário>'`) e
//! o id do grupo (`established between ...[grupo]`). Antes de exibir ou copiar
//! um log, o app troca essas identidades por versões mascaradas.

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

    #[test]
    fn ignores_short_identities() {
        let r = Redactor::new(["ab", "keyid:x"]);
        // "ab" e "x" são curtos demais; "keyid:x" (7 caracteres) vira "k***".
        assert_eq!(r.redact("ab x keyid:x"), "ab x k***");
    }
}
