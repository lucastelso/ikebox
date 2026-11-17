//! Trust anchors of a gateway authenticated by certificate (ADR-0022,
//! ADR-0023): the CA certificates a profile carries, in PEM.
//!
//! They are public data, but they decide whom the client trusts, so they are
//! checked in Rust before they reach charon (as data, never as a path):
//! one certificate per entry, a CA, valid today, with a key the embedded
//! engine can verify. The engine has no OpenSSL: RSA and Ed25519 only, and an
//! ECDSA CA is refused here with a clear message instead of failing at
//! connect time (decision of phase 8).

use std::time::{SystemTime, UNIX_EPOCH};

use x509_cert::Certificate;
use x509_cert::der::DecodePem;
use x509_cert::der::oid::ObjectIdentifier;
use x509_cert::ext::pkix::BasicConstraints;

/// Most CA certificates a profile may carry.
pub const MAX_CAS: usize = 8;
/// Largest CA certificate accepted, in PEM bytes.
pub const MAX_CA_BYTES: usize = 16 * 1024;

const RSA: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.1");
const ED25519: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.101.112");
const EC: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.2.1");

const BEGIN: &str = "-----BEGIN CERTIFICATE-----";
const END: &str = "-----END CERTIFICATE-----";

/// What the editor shows of a CA.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaInfo {
    /// Subject (RFC 4514).
    pub subject: String,
    /// Last day of validity (`YYYY-MM-DD`, UTC).
    pub not_after: String,
    /// Key algorithm (`RSA` or `Ed25519`).
    pub key: &'static str,
}

/// Reads and checks one CA certificate in PEM.
///
/// # Errors
/// A message for the person: not one PEM certificate, not a CA, not valid
/// today, or a key the engine cannot verify.
pub fn parse_ca(pem: &str) -> Result<CaInfo, String> {
    if pem.len() > MAX_CA_BYTES {
        return Err(crate::tr!(
            "certificate too large (16 KiB at most)",
            "certificado grande demais (no máximo 16 KiB)"
        )
        .to_owned());
    }
    if pem.matches(BEGIN).count() != 1 {
        return Err(crate::tr!(
            "expected exactly one PEM certificate (-----BEGIN CERTIFICATE-----)",
            "esperado exatamente um certificado PEM (-----BEGIN CERTIFICATE-----)"
        )
        .to_owned());
    }
    let cert = Certificate::from_pem(pem.trim().as_bytes()).map_err(|_| {
        crate::tr!(
            "this is not a valid X.509 certificate",
            "isto não é um certificado X.509 válido"
        )
        .to_owned()
    })?;
    let tbs = cert.tbs_certificate();

    let is_ca = matches!(
        tbs.get_extension::<BasicConstraints>(),
        Ok(Some((_, BasicConstraints { ca: true, .. })))
    );
    if !is_ca {
        return Err(crate::tr!(
            "this certificate is not a CA: use the certificate of the authority that signed the gateway's",
            "este certificado não é de uma CA: use o da autoridade que assinou o do gateway"
        )
        .to_owned());
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let validity = tbs.validity();
    if validity.not_before.to_unix_duration() > now {
        return Err(crate::tr!(
            "the CA is not valid yet (check the computer's date)",
            "a CA ainda não é válida (confira a data do computador)"
        )
        .to_owned());
    }
    if validity.not_after.to_unix_duration() < now {
        return Err(crate::tr!("the CA has expired", "a CA está vencida").to_owned());
    }

    let algorithm = tbs.subject_public_key_info().algorithm.oid;
    let key = if algorithm == RSA {
        "RSA"
    } else if algorithm == ED25519 {
        "Ed25519"
    } else if algorithm == EC {
        return Err(crate::tr!(
            "ECDSA certificates are not supported yet (the engine verifies RSA and Ed25519)",
            "certificados ECDSA ainda não são suportados (o motor verifica RSA e Ed25519)"
        )
        .to_owned());
    } else {
        return Err(crate::tr!(
            "unsupported key type (the engine verifies RSA and Ed25519)",
            "tipo de chave não suportado (o motor verifica RSA e Ed25519)"
        )
        .to_owned());
    };

    let end = validity.not_after.to_date_time();
    Ok(CaInfo {
        subject: tbs.subject().to_string(),
        not_after: format!("{:04}-{:02}-{:02}", end.year(), end.month(), end.day()),
        key,
    })
}

/// Splits pasted text (or a file) into one PEM block per certificate, so a
/// chain becomes several entries. Text outside the blocks is ignored.
#[must_use]
pub fn split_pem(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find(BEGIN) {
        let Some(len) = rest[start..].find(END) else {
            break;
        };
        let end = start + len + END.len();
        blocks.push(format!("{}\n", rest[start..end].trim()));
        rest = &rest[end..];
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    const RSA_CA: &str = include_str!("../../tests/fixtures/ca/rsa-ca.pem");
    const ED25519_CA: &str = include_str!("../../tests/fixtures/ca/ed25519-ca.pem");
    const ECDSA_CA: &str = include_str!("../../tests/fixtures/ca/ecdsa-ca.pem");
    const LEAF: &str = include_str!("../../tests/fixtures/ca/rsa-leaf.pem");
    const EXPIRED: &str = include_str!("../../tests/fixtures/ca/expired-ca.pem");

    #[test]
    fn rsa_and_ed25519_cas_are_accepted() {
        let rsa = parse_ca(RSA_CA).expect("rsa");
        assert_eq!(rsa.key, "RSA");
        assert!(
            rsa.subject.contains("IPsec Client Test CA (RSA)"),
            "{}",
            rsa.subject
        );
        assert!(rsa.not_after.starts_with("20"), "{}", rsa.not_after);
        assert_eq!(parse_ca(ED25519_CA).expect("ed25519").key, "Ed25519");
    }

    #[test]
    fn what_the_engine_cannot_trust_is_refused_with_the_reason() {
        assert!(parse_ca(ECDSA_CA).expect_err("ecdsa").contains("ECDSA"));
        assert!(parse_ca(LEAF).expect_err("leaf").contains("not a CA"));
        assert!(parse_ca(EXPIRED).expect_err("expired").contains("expired"));
        assert!(parse_ca("not a certificate").is_err());
        let two = format!("{RSA_CA}{ED25519_CA}");
        assert!(parse_ca(&two).expect_err("two").contains("exactly one"));
        let garbled = RSA_CA.replacen("MII", "XXX", 1);
        assert!(parse_ca(&garbled).is_err());
        assert!(parse_ca(&"A".repeat(MAX_CA_BYTES + 1)).is_err());
    }

    #[test]
    fn the_fixtures_are_public_certificates_only() {
        // The .gitignore lets these PEM files in: they must never carry a key.
        for pem in [RSA_CA, ED25519_CA, ECDSA_CA, LEAF, EXPIRED] {
            assert!(!pem.contains("PRIVATE KEY"));
            assert_eq!(split_pem(pem).len(), 1);
        }
    }

    #[test]
    fn a_pasted_chain_becomes_one_entry_per_certificate() {
        let pasted = format!("Subject: something\n{RSA_CA}\nnoise\n{ED25519_CA}");
        let blocks = split_pem(&pasted);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0], RSA_CA);
        assert_eq!(blocks[1], ED25519_CA);
        assert!(split_pem("nothing here").is_empty());
    }
}
