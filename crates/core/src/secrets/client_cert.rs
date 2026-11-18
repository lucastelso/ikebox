//! Client certificates (ADR-0024): a PKCS#12 file (`.p12`, `.pfx`) is read in
//! memory with its password, checked, and turned into the person's
//! certificate and private key in PEM. The key then lives only in the keyring
//! (or, from the CLI, only in memory for one connection) and reaches charon
//! by VICI as data, never as a path.
//!
//! Accepted: modern files (PBES2/AES) and old ones (3DES/RC2 with SHA-1, as
//! older Windows and firewalls export; decided at the start of phase 9), with
//! one private key, RSA, ECDSA (through OpenSSL in the engine, ADR-0027) or
//! Ed25519, and its certificate valid today. The `.p12` password is never stored.

use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use p12_keystore::Pkcs12Archive;
use pkcs1::{RsaPrivateKeyRef, RsaPublicKeyRef};
use pkcs8::PrivateKeyInfoRef;
use x509_cert::Certificate;
use x509_cert::der::oid::ObjectIdentifier;
use x509_cert::der::pem::LineEnding;
use x509_cert::der::{Decode, DecodePem, EncodePem};
use x509_cert::ext::pkix::name::GeneralName;
use x509_cert::ext::pkix::{BasicConstraints, SubjectAltName};
use zeroize::Zeroizing;

const RSA: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.1");
const ED25519: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.101.112");
const EC: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.2.1");

/// Largest PKCS#12 file read.
pub const MAX_PKCS12_BYTES: usize = 64 * 1024;

/// What the app shows of a client certificate (no secret).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientCertInfo {
    /// Subject (RFC 4514).
    pub subject: String,
    /// Issuer (RFC 4514).
    pub issuer: String,
    /// Last day of validity (`YYYY-MM-DD`, UTC).
    pub not_after: String,
    /// Seconds since the epoch of the end of validity.
    pub not_after_unix: u64,
    /// `RSA`, `ECDSA` or `Ed25519`.
    pub key: &'static str,
    /// Values that identify the person or the organization (subject
    /// attributes, alternative names): masked in the logs.
    pub names: Vec<String>,
}

impl ClientCertInfo {
    /// Whole days until the certificate expires (negative: expired).
    #[must_use]
    pub fn days_left(&self) -> i64 {
        let now = now().as_secs();
        let secs = i128::from(self.not_after_unix) - i128::from(now);
        i64::try_from(secs.div_euclid(86_400)).unwrap_or(i64::MIN)
    }
}

/// A client certificate and its private key, in PEM. The `Debug` shows only
/// the certificate's subject; the key is zeroed on drop.
#[derive(Clone)]
pub struct ClientCertificate {
    cert_pem: String,
    key_pem: Zeroizing<String>,
    info: ClientCertInfo,
}

impl fmt::Debug for ClientCertificate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientCertificate")
            .field("key", &self.info.key)
            .field("not_after", &self.info.not_after)
            .field("private_key", &format_args!("<redigida>"))
            .finish_non_exhaustive()
    }
}

fn now() -> std::time::Duration {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
}

fn err(en: &str, pt: &str) -> String {
    match crate::i18n::lang() {
        crate::i18n::Lang::En => en.to_owned(),
        crate::i18n::Lang::Pt => pt.to_owned(),
    }
}

impl ClientCertificate {
    /// Reads a PKCS#12 file with its password.
    ///
    /// # Errors
    /// A message for the person: wrong password or not a PKCS#12 file, no
    /// key or more than one, an ECDSA or other unsupported key, no
    /// certificate for the key, or a certificate not valid today.
    pub fn from_pkcs12(data: &[u8], password: &str) -> Result<Self, String> {
        if data.len() > MAX_PKCS12_BYTES {
            return Err(err(
                "the file is too large for a certificate (.p12)",
                "o arquivo é grande demais para um certificado (.p12)",
            ));
        }
        let archive = Pkcs12Archive::from_pkcs12(data, password).map_err(|_| {
            err(
                "could not open the file: wrong password, or it is not a .p12/.pfx certificate",
                "não foi possível abrir o arquivo: senha errada, ou não é um certificado .p12/.pfx",
            )
        })?;
        let [key_bag] = archive.keys.as_slice() else {
            return Err(if archive.keys.is_empty() {
                err(
                    "the file has no private key: export the certificate with its key",
                    "o arquivo não tem chave privada: exporte o certificado com a chave",
                )
            } else {
                err(
                    "the file has more than one private key: export only yours",
                    "o arquivo tem mais de uma chave privada: exporte só a sua",
                )
            });
        };
        let key_der = Zeroizing::new(key_bag.key.as_der().to_vec());
        let certs: Vec<Certificate> = archive
            .certs
            .iter()
            .filter_map(|bag| Certificate::from_der(bag.cert.as_der()).ok())
            .collect();
        let cert = pick_certificate(
            &key_der,
            &certs,
            key_bag.local_key_id.as_ref().map(|id| id.0.as_slice()),
            &archive,
        )?;
        let cert_pem = cert
            .to_pem(LineEnding::LF)
            .map_err(|_| err("invalid certificate", "certificado inválido"))?;
        let key_pem = Zeroizing::new(
            pkcs8::der::pem::encode_string("PRIVATE KEY", LineEnding::LF, &key_der)
                .map_err(|_| err("invalid private key", "chave privada inválida"))?,
        );
        Self::from_pem(&cert_pem, &key_pem)
    }

    /// Rebuilds the pair from its PEM texts (as stored in the keyring), and
    /// checks them again.
    ///
    /// # Errors
    /// As [`ClientCertificate::from_pkcs12`].
    pub fn from_pem(cert_pem: &str, key_pem: &str) -> Result<Self, String> {
        let cert = Certificate::from_pem(cert_pem.trim().as_bytes())
            .map_err(|_| err("invalid certificate", "certificado inválido"))?;
        let (label, key_der) = pkcs8::der::pem::decode_vec(key_pem.trim().as_bytes())
            .map_err(|_| err("invalid private key", "chave privada inválida"))?;
        let key_der = Zeroizing::new(key_der);
        if label != "PRIVATE KEY" {
            return Err(err("invalid private key", "chave privada inválida"));
        }
        if !key_matches(&key_der, &cert)? {
            return Err(err(
                "the private key does not belong to the certificate",
                "a chave privada não é a do certificado",
            ));
        }
        let info = describe(&cert)?;
        Ok(Self {
            cert_pem: cert_pem.trim().to_owned() + "\n",
            key_pem: Zeroizing::new(key_pem.trim().to_owned() + "\n"),
            info,
        })
    }

    /// The certificate, in PEM (public, but personal).
    #[must_use]
    pub fn cert_pem(&self) -> &str {
        &self.cert_pem
    }

    /// The private key, in PEM (PKCS#8). Only for the keyring and charon.
    #[must_use]
    pub fn key_pem(&self) -> &str {
        &self.key_pem
    }

    /// What can be shown.
    #[must_use]
    pub fn info(&self) -> &ClientCertInfo {
        &self.info
    }
}

/// Describes a certificate in PEM without its key (for the status shown to
/// the person).
///
/// # Errors
/// Not a certificate the app can use.
pub fn describe_pem(cert_pem: &str) -> Result<ClientCertInfo, String> {
    let cert = Certificate::from_pem(cert_pem.trim().as_bytes())
        .map_err(|_| err("invalid certificate", "certificado inválido"))?;
    describe(&cert)
}

/// The certificate of the key: the one whose public key is the key's, then
/// the one with the same local key id, then the only one that is not a CA.
fn pick_certificate(
    key_der: &[u8],
    certs: &[Certificate],
    key_id: Option<&[u8]>,
    archive: &Pkcs12Archive,
) -> Result<Certificate, String> {
    for cert in certs {
        if key_matches(key_der, cert).unwrap_or(false) {
            return Ok(cert.clone());
        }
    }
    if let Some(id) = key_id {
        for bag in &archive.certs {
            if bag.local_key_id.as_deref() == Some(id)
                && let Ok(cert) = Certificate::from_der(bag.cert.as_der())
            {
                return Ok(cert);
            }
        }
    }
    let leaves: Vec<_> = certs.iter().filter(|c| !is_ca(c)).collect();
    if let [leaf] = leaves.as_slice() {
        return Ok((*leaf).clone());
    }
    // An unsupported key type says so before "no certificate".
    key_kind(key_der)?;
    Err(err(
        "the file has no certificate for its private key",
        "o arquivo não tem o certificado da chave privada",
    ))
}

fn is_ca(cert: &Certificate) -> bool {
    matches!(
        cert.tbs_certificate().get_extension::<BasicConstraints>(),
        Ok(Some((_, BasicConstraints { ca: true, .. })))
    )
}

/// `RSA`, `ECDSA` or `Ed25519`; anything else is refused with the reason.
fn key_kind(key_der: &[u8]) -> Result<&'static str, String> {
    let info = PrivateKeyInfoRef::from_der(key_der)
        .map_err(|_| err("invalid private key", "chave privada inválida"))?;
    let oid = info.algorithm.oid;
    if oid == RSA {
        Ok("RSA")
    } else if oid == ED25519 {
        Ok("Ed25519")
    } else if oid == EC {
        Ok("ECDSA")
    } else {
        Err(err(
            "unsupported key type (the engine signs with RSA, ECDSA and Ed25519)",
            "tipo de chave não suportado (o motor assina com RSA, ECDSA e Ed25519)",
        ))
    }
}

/// Does the private key belong to the certificate? RSA: same modulus.
/// Ed25519: same public key when the PKCS#8 carries it. ECDSA keeps its
/// public key inside the SEC1 structure, which is not parsed here: a
/// mismatch is reported by charon at connect time.
fn key_matches(key_der: &[u8], cert: &Certificate) -> Result<bool, String> {
    let kind = key_kind(key_der)?;
    let info = PrivateKeyInfoRef::from_der(key_der)
        .map_err(|_| err("invalid private key", "chave privada inválida"))?;
    let spki = cert.tbs_certificate().subject_public_key_info();
    if spki.algorithm.oid != info.algorithm.oid {
        return Ok(false);
    }
    let Some(cert_key) = spki.subject_public_key.as_bytes() else {
        return Ok(false);
    };
    match kind {
        "RSA" => {
            let private = RsaPrivateKeyRef::from_der(info.private_key.as_bytes())
                .map_err(|_| err("invalid private key", "chave privada inválida"))?;
            let public = RsaPublicKeyRef::from_der(cert_key)
                .map_err(|_| err("invalid certificate", "certificado inválido"))?;
            Ok(private.modulus.as_bytes() == public.modulus.as_bytes())
        }
        _ => Ok(info
            .public_key
            .as_ref()
            .and_then(|k| k.as_bytes())
            .is_none_or(|k| k == cert_key)),
    }
}

fn describe(cert: &Certificate) -> Result<ClientCertInfo, String> {
    let tbs = cert.tbs_certificate();
    let validity = tbs.validity();
    let now = now();
    if validity.not_before.to_unix_duration() > now {
        return Err(err(
            "the certificate is not valid yet (check the computer's date)",
            "o certificado ainda não é válido (confira a data do computador)",
        ));
    }
    let end = validity.not_after.to_date_time();
    let not_after = format!("{:04}-{:02}-{:02}", end.year(), end.month(), end.day());
    if validity.not_after.to_unix_duration() < now {
        return Err(match crate::i18n::lang() {
            crate::i18n::Lang::En => format!("the certificate expired on {not_after}"),
            crate::i18n::Lang::Pt => format!("o certificado venceu em {not_after}"),
        });
    }
    let key = match tbs.subject_public_key_info().algorithm.oid {
        oid if oid == RSA => "RSA",
        oid if oid == ED25519 => "Ed25519",
        oid if oid == EC => "ECDSA",
        _ => {
            return Err(err(
                "unsupported key type (RSA, ECDSA or Ed25519)",
                "tipo de chave não suportado (RSA, ECDSA ou Ed25519)",
            ));
        }
    };
    let mut names: Vec<String> = tbs
        .subject()
        .iter()
        .filter_map(|atv| std::str::from_utf8(atv.value.value()).ok())
        .map(|v| v.trim().to_owned())
        .filter(|v| v.chars().count() >= 3)
        .collect();
    if let Ok(Some((_, SubjectAltName(alt)))) = tbs.get_extension::<SubjectAltName>() {
        for name in alt {
            match name {
                GeneralName::Rfc822Name(v) | GeneralName::DnsName(v) => names.push(v.to_string()),
                _ => {}
            }
        }
    }
    names.sort();
    names.dedup();
    Ok(ClientCertInfo {
        subject: tbs.subject().to_string(),
        issuer: tbs.issuer().to_string(),
        not_after,
        not_after_unix: validity.not_after.to_unix_duration().as_secs(),
        key,
        names,
    })
}

#[cfg(test)]
mod tests {
    //! The PKCS#12 files are made at test time with the `openssl` command
    //! (fake keys, in a temporary directory): no private key is kept in the
    //! repository. Without `openssl`, these tests say so and pass.

    use std::path::Path;
    use std::process::Command;

    use super::*;

    const PASSWORD: &str = "senha-de-teste-p12";

    fn openssl(dir: &Path, args: &[&str]) -> bool {
        Command::new("openssl")
            .args(args)
            .current_dir(dir)
            .output()
            .is_ok_and(|o| o.status.success())
    }

    /// CA, a client certificate signed by it with `key` (`rsa:2048`,
    /// `ed25519` or `ec`), and its PKCS#12 with `extra` export options.
    fn make_p12(dir: &Path, key: &str, extra: &[&str]) -> Option<Vec<u8>> {
        let ca_ext = [
            "-addext",
            "basicConstraints=critical,CA:TRUE",
            "-addext",
            "keyUsage=critical,keyCertSign,cRLSign",
        ];
        let mut ca = vec![
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-days",
            "2",
            "-keyout",
            "ca.key",
            "-out",
            "ca.pem",
            "-subj",
            "/O=Example/CN=Test CA",
        ];
        ca.extend(ca_ext);
        let newkey: Vec<&str> = match key {
            "ec" => vec!["-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:P-256"],
            other => vec!["-newkey", other],
        };
        let mut req = vec!["req"];
        req.extend(&newkey);
        req.extend([
            "-nodes",
            "-keyout",
            "client.key",
            "-out",
            "client.csr",
            "-subj",
            "/O=Example Org/CN=Fulana de Teste",
        ]);
        let ext = dir.join("client.ext");
        std::fs::write(&ext, "subjectAltName=email:fulana@example.com\n").ok()?;
        let sign = [
            "x509",
            "-req",
            "-in",
            "client.csr",
            "-CA",
            "ca.pem",
            "-CAkey",
            "ca.key",
            "-CAcreateserial",
            "-days",
            "2",
            "-extfile",
            "client.ext",
            "-out",
            "client.pem",
        ];
        let pass = format!("pass:{PASSWORD}");
        let mut export = vec![
            "pkcs12",
            "-export",
            "-inkey",
            "client.key",
            "-in",
            "client.pem",
            "-certfile",
            "ca.pem",
            "-out",
            "client.p12",
            "-passout",
            &pass,
        ];
        export.extend(extra);
        if !(openssl(dir, &ca)
            && openssl(dir, &req)
            && openssl(dir, &sign)
            && openssl(dir, &export))
        {
            return None;
        }
        std::fs::read(dir.join("client.p12")).ok()
    }

    fn has_openssl() -> bool {
        let ok = Command::new("openssl").arg("version").output().is_ok();
        if !ok {
            eprintln!("openssl not found: PKCS#12 tests skipped");
        }
        ok
    }

    #[test]
    fn modern_and_old_files_give_the_certificate_and_its_key() {
        if !has_openssl() {
            return;
        }
        let old_3des = [
            "-certpbe",
            "PBE-SHA1-3DES",
            "-keypbe",
            "PBE-SHA1-3DES",
            "-macalg",
            "sha1",
        ];
        for (name, key, extra) in [
            ("modern RSA", "rsa:2048", &[][..]),
            ("old 3DES/SHA-1", "rsa:2048", &old_3des[..]),
            ("Ed25519", "ed25519", &[][..]),
            ("ECDSA P-256", "ec", &[][..]),
        ] {
            let dir = tempfile::tempdir().expect("tempdir");
            let p12 = make_p12(dir.path(), key, extra).expect(name);
            let cert = ClientCertificate::from_pkcs12(&p12, PASSWORD).expect(name);
            let info = cert.info();
            assert!(
                info.subject.contains("Fulana de Teste"),
                "{name}: {}",
                info.subject
            );
            assert!(info.issuer.contains("Test CA"), "{name}: {}", info.issuer);
            assert!(info.names.contains(&"Fulana de Teste".to_owned()), "{name}");
            assert!(
                info.names.contains(&"fulana@example.com".to_owned()),
                "{name}"
            );
            assert!(
                (0..=2).contains(&info.days_left()),
                "{name}: {}",
                info.days_left()
            );
            assert!(cert.key_pem().starts_with("-----BEGIN PRIVATE KEY-----"));
            assert!(cert.cert_pem().starts_with("-----BEGIN CERTIFICATE-----"));
            // Back from the keyring's two PEM texts.
            let again = ClientCertificate::from_pem(cert.cert_pem(), cert.key_pem()).expect(name);
            assert_eq!(again.info(), info);
            assert!(!format!("{cert:?}").contains("PRIVATE"));
        }
    }

    #[test]
    fn rc2_files_from_older_windows_are_read_too() {
        if !has_openssl() {
            return;
        }
        let dir = tempfile::tempdir().expect("tempdir");
        // `-legacy` needs OpenSSL's legacy provider; without it, nothing to test.
        let Some(p12) = make_p12(dir.path(), "rsa:2048", &["-legacy"]) else {
            eprintln!("openssl without the legacy provider: RC2 test skipped");
            return;
        };
        assert!(ClientCertificate::from_pkcs12(&p12, PASSWORD).is_ok());
    }

    #[test]
    fn what_cannot_be_used_is_refused_with_the_reason() {
        if !has_openssl() {
            return;
        }
        let dir = tempfile::tempdir().expect("tempdir");
        let p12 = make_p12(dir.path(), "rsa:2048", &[]).expect("rsa");
        let wrong = ClientCertificate::from_pkcs12(&p12, "errada").expect_err("password");
        assert!(wrong.contains("wrong password"), "{wrong}");
        assert!(ClientCertificate::from_pkcs12(b"not a p12", PASSWORD).is_err());

        // A key that is not the certificate's.
        let other = tempfile::tempdir().expect("tempdir");
        make_p12(other.path(), "rsa:2048", &[]).expect("other");
        let cert = std::fs::read_to_string(dir.path().join("ca.pem")).expect("ca");
        let key = std::fs::read_to_string(other.path().join("client.key")).expect("key");
        let mismatch = ClientCertificate::from_pem(&cert, &key).expect_err("mismatch");
        assert!(mismatch.contains("does not belong"), "{mismatch}");
    }
}
