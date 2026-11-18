//! Smartcards and tokens (ADR-0029): the certificate's private key stays on
//! the token, used through its PKCS#11 module in the person's own process
//! (the app or the CLI). charon never sees the key or the PIN: it asks this
//! process to sign over the ssh-agent protocol (`session::agent`).
//!
//! - [`TokenRef`]: which module and which key, chosen once and stored in the
//!   keyring like the certificate (public data);
//! - [`list_certificates`]: the certificates a token offers, to choose from;
//! - [`TokenSigner`]: a logged-in session on the token, which signs what the
//!   agent receives, in the format the ssh-agent protocol uses.

use std::fmt;
use std::path::{Path, PathBuf};

use cryptoki::context::{CInitializeArgs, CInitializeFlags, Pkcs11};
use cryptoki::error::{Error as P11Error, RvError};
use cryptoki::mechanism::Mechanism;
use cryptoki::mechanism::eddsa::{EddsaParams, EddsaSignatureScheme};
use cryptoki::object::{Attribute, AttributeType, CertificateType, ObjectClass, ObjectHandle};
use cryptoki::session::{Session, UserType};
use cryptoki::types::AuthPin;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha384, Sha512};
use x509_cert::Certificate;
use x509_cert::der::asn1::ObjectIdentifier;
use x509_cert::der::pem::LineEnding;
use x509_cert::der::{Decode, DecodePem, Encode, EncodePem};

use super::{ClientCertInfo, describe_pem};

const RSA: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.1");
const ED25519: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.101.112");
const EC: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.2.1");
const P256: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.3.1.7");
const P384: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.132.0.34");
const P521: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.132.0.35");

/// The ssh-agent signature flags strongSwan sends for RSA (agent plugin).
const FLAG_SHA2_256: u32 = 2;
const FLAG_SHA2_512: u32 = 4;
/// Private flag of Ikebox's agent (patch 0004): sign the data as it is,
/// PKCS#1 v1.5 over a hash charon already computed (IKEv1 RSA signatures).
const FLAG_RAW: u32 = 0x4000_0000;
/// Largest "hash" signed as it is: SHA-512.
const MAX_RAW: usize = 64;

/// Where PKCS#11 modules usually are: p11-kit's proxy first (it offers every
/// module the system configured, OpenSC and vendor ones included), then
/// OpenSC itself.
const MODULE_CANDIDATES: &[&str] = &[
    "/usr/lib64/p11-kit-proxy.so",
    "/usr/lib/x86_64-linux-gnu/p11-kit-proxy.so",
    "/usr/lib/p11-kit-proxy.so",
    "/usr/lib64/opensc-pkcs11.so",
    "/usr/lib64/pkcs11/opensc-pkcs11.so",
    "/usr/lib/x86_64-linux-gnu/opensc-pkcs11.so",
];

/// Errors of the token, said so the person knows what to do.
#[derive(Debug, thiserror::Error)]
pub enum TokenError {
    /// The PKCS#11 module could not be used.
    #[error("{}", crate::trf!(
        "the smartcard/token module {} could not be used: {}",
        "o módulo do smartcard/token {} não pôde ser usado: {}",
        .0.display(), .1
    ))]
    Module(PathBuf, String),
    /// No PKCS#11 module was found or given.
    #[error("{}", crate::tr!(
        "no PKCS#11 module found (install p11-kit or OpenSC, or choose the module file)",
        "nenhum módulo PKCS#11 encontrado (instale o p11-kit ou o OpenSC, ou escolha o arquivo do módulo)"
    ))]
    NoModule,
    /// The token with the key is not connected.
    #[error("{}", crate::tr!(
        "the smartcard or token with this certificate is not connected",
        "o smartcard ou token com este certificado não está conectado"
    ))]
    NotPresent,
    /// Wrong PIN.
    #[error("{}", crate::tr!(
        "wrong PIN for the smartcard or token",
        "PIN do smartcard ou token incorreto"
    ))]
    WrongPin,
    /// The PIN is locked after too many wrong attempts.
    #[error("{}", crate::tr!(
        "the smartcard or token PIN is locked (too many wrong attempts); unlock it with your IT team's tools",
        "o PIN do smartcard ou token está bloqueado (tentativas erradas demais); desbloqueie com as ferramentas da sua equipe de TI"
    ))]
    PinLocked,
    /// The certificate's key type cannot be used.
    #[error("{}", crate::trf!(
        "unsupported key on the token: {}",
        "chave não suportada no token: {}",
        .0
    ))]
    Unsupported(String),
    /// Another PKCS#11 failure.
    #[error("{}", crate::trf!(
        "smartcard/token error: {}",
        "erro do smartcard/token: {}",
        .0
    ))]
    Pkcs11(String),
    /// The stored reference to the token is not readable.
    #[error("{}", crate::tr!(
        "the stored choice of token certificate is invalid; choose the certificate again",
        "a escolha guardada do certificado do token é inválida; escolha o certificado de novo"
    ))]
    InvalidRef,
}

impl From<P11Error> for TokenError {
    fn from(e: P11Error) -> Self {
        match e {
            P11Error::Pkcs11(RvError::PinIncorrect | RvError::PinInvalid, _) => Self::WrongPin,
            P11Error::Pkcs11(RvError::PinLocked, _) => Self::PinLocked,
            P11Error::Pkcs11(
                RvError::TokenNotPresent | RvError::DeviceRemoved | RvError::SlotIdInvalid,
                _,
            ) => Self::NotPresent,
            other => Self::Pkcs11(other.to_string()),
        }
    }
}

/// Which module and which key: chosen once per profile, kept in the keyring
/// next to the certificate (public data, as JSON).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenRef {
    /// Path of the PKCS#11 module.
    pub module: PathBuf,
    /// `CKA_ID` of the certificate and its key, in hexadecimal.
    pub id: String,
    /// Serial number of the token, to find it among several.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub serial: Option<String>,
}

impl TokenRef {
    /// The value stored in the keyring.
    ///
    /// # Errors
    /// Never in practice (serializing plain strings).
    pub fn to_json(&self) -> Result<String, TokenError> {
        serde_json::to_string(self).map_err(|_| TokenError::InvalidRef)
    }

    /// Reads the value stored in the keyring.
    ///
    /// # Errors
    /// [`TokenError::InvalidRef`] if it is not a valid reference.
    pub fn from_json(text: &str) -> Result<Self, TokenError> {
        let r: Self = serde_json::from_str(text).map_err(|_| TokenError::InvalidRef)?;
        if !r.module.is_absolute() || hex::decode(&r.id).is_err() {
            return Err(TokenError::InvalidRef);
        }
        Ok(r)
    }

    fn key_id(&self) -> Vec<u8> {
        hex::decode(&self.id).unwrap_or_default()
    }
}

/// A certificate offered by a token.
#[derive(Debug, Clone)]
pub struct TokenCertificate {
    /// Label of the token (as the token reports it).
    pub token_label: String,
    /// Where it is: module, key id and token serial.
    pub reference: TokenRef,
    /// The certificate (PEM).
    pub cert_pem: String,
    /// What the app shows of it, or why it cannot be used.
    pub info: Result<ClientCertInfo, String>,
}

/// The first PKCS#11 module found on this computer.
#[must_use]
pub fn default_module() -> Option<PathBuf> {
    MODULE_CANDIDATES
        .iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
}

/// Opens a module. Only absolute paths to regular files: it is loaded into
/// this process (the person's own, never root's).
fn open_module(module: &Path) -> Result<Pkcs11, TokenError> {
    let usable =
        module.is_absolute() && std::fs::metadata(module).is_ok_and(|m| m.file_type().is_file());
    if !usable {
        return Err(TokenError::Module(
            module.to_owned(),
            crate::tr!("not a file", "não é um arquivo").to_owned(),
        ));
    }
    let p11 =
        Pkcs11::new(module).map_err(|e| TokenError::Module(module.to_owned(), e.to_string()))?;
    match p11.initialize(CInitializeArgs::new(CInitializeFlags::OS_LOCKING_OK)) {
        Ok(()) | Err(P11Error::Pkcs11(RvError::CryptokiAlreadyInitialized, _)) => Ok(p11),
        Err(e) => Err(TokenError::Module(module.to_owned(), e.to_string())),
    }
}

fn attribute_bytes(
    session: &Session,
    object: ObjectHandle,
    kind: AttributeType,
) -> Option<Vec<u8>> {
    session
        .get_attributes(object, &[kind])
        .ok()?
        .into_iter()
        .find_map(|a| match a {
            Attribute::Value(v) | Attribute::Id(v) | Attribute::Label(v) => Some(v),
            _ => None,
        })
}

/// The certificates the tokens of `module` offer (public objects: no PIN).
///
/// # Errors
/// The module cannot be loaded, or the token fails.
pub fn list_certificates(module: &Path) -> Result<Vec<TokenCertificate>, TokenError> {
    let p11 = open_module(module)?;
    let mut found = Vec::new();
    for slot in p11.get_slots_with_token()? {
        let Ok(info) = p11.get_token_info(slot) else {
            continue;
        };
        // A slot whose token cannot be opened (not initialized, or busy) is
        // not the one with the certificate: skip it.
        let Ok(session) = p11.open_ro_session(slot) else {
            continue;
        };
        let template = [
            Attribute::Class(ObjectClass::CERTIFICATE),
            Attribute::CertificateType(CertificateType::X_509),
        ];
        for object in session.find_objects(&template)? {
            let Some(der) = attribute_bytes(&session, object, AttributeType::Value) else {
                continue;
            };
            let Some(id) = attribute_bytes(&session, object, AttributeType::Id) else {
                continue;
            };
            let Ok(cert) = Certificate::from_der(&der) else {
                continue;
            };
            let Ok(cert_pem) = cert.to_pem(LineEnding::LF) else {
                continue;
            };
            found.push(TokenCertificate {
                token_label: info.label().trim().to_owned(),
                reference: TokenRef {
                    module: module.to_owned(),
                    id: hex::encode(&id),
                    serial: Some(info.serial_number().trim().to_owned()).filter(|s| !s.is_empty()),
                },
                info: describe_pem(&cert_pem),
                cert_pem,
            });
        }
    }
    Ok(found)
}

/// What kind of key, from the certificate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyKind {
    Rsa,
    Ecdsa(Curve),
    Ed25519,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Curve {
    P256,
    P384,
    P521,
}

impl Curve {
    fn name(self) -> &'static str {
        match self {
            Self::P256 => "nistp256",
            Self::P384 => "nistp384",
            Self::P521 => "nistp521",
        }
    }
}

/// The public key of a certificate in the two forms the agent path needs:
/// the SSH blob the agent announces and the PEM sent to charon by VICI.
#[derive(Debug, Clone)]
pub struct AgentPublicKey {
    kind: KeyKind,
    /// SSH wire format (what the agent lists and charon names when signing).
    pub ssh_blob: Vec<u8>,
    /// `-----BEGIN PUBLIC KEY-----` (SubjectPublicKeyInfo), for `load-key`.
    pub spki_pem: String,
}

impl AgentPublicKey {
    /// From the certificate (PEM).
    ///
    /// # Errors
    /// Invalid certificate or a key type the agent path does not carry.
    pub fn from_cert_pem(cert_pem: &str) -> Result<Self, TokenError> {
        let cert = Certificate::from_pem(cert_pem.trim().as_bytes())
            .map_err(|_| TokenError::Unsupported("invalid certificate".to_owned()))?;
        let spki = cert.tbs_certificate().subject_public_key_info();
        let key_bits = spki
            .subject_public_key
            .as_bytes()
            .ok_or_else(|| TokenError::Unsupported("public key".to_owned()))?;
        let oid = spki.algorithm.oid;
        let (kind, ssh_blob) = if oid == RSA {
            let rsa = pkcs1::RsaPublicKeyRef::from_der(key_bits)
                .map_err(|_| TokenError::Unsupported("RSA".to_owned()))?;
            let mut blob = Vec::new();
            put_string(&mut blob, b"ssh-rsa");
            put_mpint(&mut blob, rsa.public_exponent.as_bytes());
            put_mpint(&mut blob, rsa.modulus.as_bytes());
            (KeyKind::Rsa, blob)
        } else if oid == EC {
            let curve_oid = spki
                .algorithm
                .parameters
                .as_ref()
                .and_then(|p| p.decode_as::<ObjectIdentifier>().ok())
                .ok_or_else(|| TokenError::Unsupported("ECDSA curve".to_owned()))?;
            let curve = match curve_oid {
                c if c == P256 => Curve::P256,
                c if c == P384 => Curve::P384,
                c if c == P521 => Curve::P521,
                c => return Err(TokenError::Unsupported(format!("ECDSA curve {c}"))),
            };
            let mut blob = Vec::new();
            put_string(&mut blob, format!("ecdsa-sha2-{}", curve.name()).as_bytes());
            put_string(&mut blob, curve.name().as_bytes());
            put_string(&mut blob, key_bits);
            (KeyKind::Ecdsa(curve), blob)
        } else if oid == ED25519 {
            let mut blob = Vec::new();
            put_string(&mut blob, b"ssh-ed25519");
            put_string(&mut blob, key_bits);
            (KeyKind::Ed25519, blob)
        } else {
            return Err(TokenError::Unsupported(oid.to_string()));
        };
        let spki_der = spki
            .to_der()
            .map_err(|_| TokenError::Unsupported("public key".to_owned()))?;
        let spki_pem = x509_cert::der::pem::encode_string("PUBLIC KEY", LineEnding::LF, &spki_der)
            .map_err(|_| TokenError::Unsupported("public key".to_owned()))?;
        Ok(Self {
            kind,
            ssh_blob,
            spki_pem,
        })
    }
}

/// A logged-in session on the token, ready to sign with one key. The PIN is
/// used for the login and not kept; the session stays logged in while it
/// lives (the VPN session).
pub struct TokenSigner {
    session: Session,
    key: ObjectHandle,
    public: AgentPublicKey,
    // Keeps the module loaded as long as the session.
    _p11: Pkcs11,
}

impl fmt::Debug for TokenSigner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TokenSigner").finish_non_exhaustive()
    }
}

impl TokenSigner {
    /// Opens the token of `reference`, logs in with `pin` and finds the key
    /// of the certificate `cert_pem`.
    ///
    /// # Errors
    /// Module, token absent, wrong or locked PIN, key not found or of an
    /// unsupported type.
    pub fn open(
        reference: &TokenRef,
        pin: &SecretString,
        cert_pem: &str,
    ) -> Result<Self, TokenError> {
        let public = AgentPublicKey::from_cert_pem(cert_pem)?;
        let p11 = open_module(&reference.module)?;
        let id = reference.key_id();
        let mut slots = p11.get_slots_with_token()?;
        if let Some(serial) = &reference.serial {
            // The token with that serial first; the others still count (a
            // module may report it differently).
            slots.sort_by_key(|slot| {
                let ours = p11
                    .get_token_info(*slot)
                    .is_ok_and(|i| i.serial_number().trim() == serial);
                u8::from(!ours)
            });
        }
        let template = [
            Attribute::Class(ObjectClass::PRIVATE_KEY),
            Attribute::Id(id),
        ];
        for slot in slots {
            // A slot whose token cannot be opened (not initialized, or busy) is
            // not the one with the certificate: skip it.
            let Ok(session) = p11.open_ro_session(slot) else {
                continue;
            };
            // Public objects first: is this the token with that key id?
            let has_cert = !session
                .find_objects(&[
                    Attribute::Class(ObjectClass::CERTIFICATE),
                    Attribute::Id(reference.key_id()),
                ])?
                .is_empty();
            if !has_cert {
                continue;
            }
            let pin = AuthPin::new(pin.expose_secret().into());
            session.login(UserType::User, Some(&pin))?;
            if let Some(key) = session.find_objects(&template)?.into_iter().next() {
                return Ok(Self {
                    session,
                    key,
                    public,
                    _p11: p11,
                });
            }
        }
        Err(TokenError::NotPresent)
    }

    /// The public key this signer answers for.
    #[must_use]
    pub fn public_key(&self) -> &AgentPublicKey {
        &self.public
    }

    /// Signs `data` as the ssh-agent protocol asks (`flags` for RSA) and
    /// returns the signature blob of the `SIGN_RESPONSE`.
    ///
    /// # Errors
    /// Unsupported request or a token failure.
    pub fn sign(&self, data: &[u8], flags: u32) -> Result<Vec<u8>, TokenError> {
        let mut out = Vec::new();
        match self.public.kind {
            KeyKind::Rsa if flags & FLAG_RAW != 0 => {
                if data.is_empty() || data.len() > MAX_RAW {
                    return Err(TokenError::Pkcs11(
                        "invalid raw signature request".to_owned(),
                    ));
                }
                let sig = self.session.sign(&Mechanism::RsaPkcs, self.key, data)?;
                put_string(&mut out, b"ssh-rsa");
                put_string(&mut out, &sig);
            }
            KeyKind::Rsa => {
                let (name, digest_info) = rsa_digest_info(data, flags);
                let sig = self
                    .session
                    .sign(&Mechanism::RsaPkcs, self.key, &digest_info)?;
                put_string(&mut out, name.as_bytes());
                put_string(&mut out, &sig);
            }
            KeyKind::Ecdsa(curve) => {
                let digest = ecdsa_digest(curve, data);
                let raw = self.session.sign(&Mechanism::Ecdsa, self.key, &digest)?;
                put_string(&mut out, format!("ecdsa-sha2-{}", curve.name()).as_bytes());
                put_string(&mut out, &ecdsa_ssh_signature(&raw)?);
            }
            KeyKind::Ed25519 => {
                let params = EddsaParams::new(EddsaSignatureScheme::Pure);
                let sig = self
                    .session
                    .sign(&Mechanism::Eddsa(params), self.key, data)?;
                put_string(&mut out, b"ssh-ed25519");
                put_string(&mut out, &sig);
            }
        }
        Ok(out)
    }
}

/// PKCS#1 v1.5 input for `CKM_RSA_PKCS`: the DigestInfo of the hash the
/// flags ask for (none: SHA-1, the `ssh-rsa` default), and the signature
/// name the agent answers with.
fn rsa_digest_info(data: &[u8], flags: u32) -> (&'static str, Vec<u8>) {
    // DER prefixes of DigestInfo (RFC 8017, section 9.2, note 1).
    const SHA1: &[u8] = &[
        0x30, 0x21, 0x30, 0x09, 0x06, 0x05, 0x2b, 0x0e, 0x03, 0x02, 0x1a, 0x05, 0x00, 0x04, 0x14,
    ];
    const SHA256: &[u8] = &[
        0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01,
        0x05, 0x00, 0x04, 0x20,
    ];
    const SHA512: &[u8] = &[
        0x30, 0x51, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x03,
        0x05, 0x00, 0x04, 0x40,
    ];
    let (name, prefix, hash) = if flags & FLAG_SHA2_512 != 0 {
        ("rsa-sha2-512", SHA512, Sha512::digest(data).to_vec())
    } else if flags & FLAG_SHA2_256 != 0 {
        ("rsa-sha2-256", SHA256, Sha256::digest(data).to_vec())
    } else {
        ("ssh-rsa", SHA1, Sha1::digest(data).to_vec())
    };
    let mut info = prefix.to_vec();
    info.extend_from_slice(&hash);
    (name, info)
}

/// The hash each curve signs with in SSH (RFC 5656, section 6.2.1).
fn ecdsa_digest(curve: Curve, data: &[u8]) -> Vec<u8> {
    match curve {
        Curve::P256 => Sha256::digest(data).to_vec(),
        Curve::P384 => Sha384::digest(data).to_vec(),
        Curve::P521 => Sha512::digest(data).to_vec(),
    }
}

/// `CKM_ECDSA` returns r||s; SSH wants `mpint r, mpint s`.
fn ecdsa_ssh_signature(raw: &[u8]) -> Result<Vec<u8>, TokenError> {
    if raw.is_empty() || !raw.len().is_multiple_of(2) {
        return Err(TokenError::Pkcs11("invalid ECDSA signature".to_owned()));
    }
    let (r, s) = raw.split_at(raw.len() / 2);
    let mut out = Vec::new();
    put_mpint(&mut out, r);
    put_mpint(&mut out, s);
    Ok(out)
}

/// SSH `string`: length (u32, big-endian) and bytes.
pub(crate) fn put_string(out: &mut Vec<u8>, value: &[u8]) {
    let len = u32::try_from(value.len()).unwrap_or(u32::MAX);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(value);
}

/// SSH `mpint` of a non-negative big-endian number: no leading zeros, and one
/// zero byte in front when the high bit is set.
fn put_mpint(out: &mut Vec<u8>, value: &[u8]) {
    let trimmed = &value[value.iter().take_while(|b| **b == 0).count()..];
    if trimmed.first().is_some_and(|b| b & 0x80 != 0) {
        let mut v = Vec::with_capacity(trimmed.len() + 1);
        v.push(0);
        v.extend_from_slice(trimmed);
        put_string(out, &v);
    } else {
        put_string(out, trimmed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RSA_CA: &str = include_str!("../../tests/fixtures/ca/rsa-ca.pem");
    const ECDSA_CA: &str = include_str!("../../tests/fixtures/ca/ecdsa-ca.pem");
    const ED25519_CA: &str = include_str!("../../tests/fixtures/ca/ed25519-ca.pem");

    fn strings(mut blob: &[u8]) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        while blob.len() >= 4 {
            let len = u32::from_be_bytes([blob[0], blob[1], blob[2], blob[3]]) as usize;
            out.push(blob[4..4 + len].to_vec());
            blob = &blob[4 + len..];
        }
        out
    }

    #[test]
    fn ssh_public_keys_from_certificates() {
        let rsa = AgentPublicKey::from_cert_pem(RSA_CA).expect("rsa");
        let parts = strings(&rsa.ssh_blob);
        assert_eq!(parts[0], b"ssh-rsa");
        assert_eq!(parts.len(), 3, "type, e, n");
        assert!(rsa.spki_pem.starts_with("-----BEGIN PUBLIC KEY-----"));

        let ec = AgentPublicKey::from_cert_pem(ECDSA_CA).expect("ecdsa");
        let parts = strings(&ec.ssh_blob);
        assert!(parts[0].starts_with(b"ecdsa-sha2-nistp"), "{:?}", parts[0]);
        assert_eq!(parts[0][11..], parts[1][..]);
        assert_eq!(parts[2][0], 0x04, "uncompressed point");

        let ed = AgentPublicKey::from_cert_pem(ED25519_CA).expect("ed25519");
        let parts = strings(&ed.ssh_blob);
        assert_eq!(parts[0], b"ssh-ed25519");
        assert_eq!(parts[1].len(), 32);
    }

    #[test]
    fn mpint_encoding() {
        let mut out = Vec::new();
        put_mpint(&mut out, &[0x00, 0x00, 0x7f]);
        assert_eq!(out, [0, 0, 0, 1, 0x7f]);
        let mut out = Vec::new();
        put_mpint(&mut out, &[0x80, 0x01]);
        assert_eq!(out, [0, 0, 0, 3, 0x00, 0x80, 0x01]);
        let mut out = Vec::new();
        put_mpint(&mut out, &[0, 0]);
        assert_eq!(out, [0, 0, 0, 0]);
    }

    #[test]
    fn rsa_digest_info_follows_the_flags() {
        let (name, info) = rsa_digest_info(b"abc", 0);
        assert_eq!(name, "ssh-rsa");
        assert_eq!(info.len(), 15 + 20);
        let (name, info) = rsa_digest_info(b"abc", FLAG_SHA2_256);
        assert_eq!(name, "rsa-sha2-256");
        assert_eq!(info.len(), 19 + 32);
        assert_eq!(&info[19..], Sha256::digest(b"abc").as_slice());
        let (name, info) = rsa_digest_info(b"abc", FLAG_SHA2_512 | FLAG_SHA2_256);
        assert_eq!(name, "rsa-sha2-512");
        assert_eq!(info.len(), 19 + 64);
    }

    #[test]
    fn ecdsa_signature_becomes_two_mpints() {
        let sig = ecdsa_ssh_signature(&[0x80; 64]).expect("sig");
        let parts = strings(&sig);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].len(), 33, "leading zero for the high bit");
        assert!(ecdsa_ssh_signature(&[1, 2, 3]).is_err());
    }

    #[test]
    fn token_reference_round_trip_and_validation() {
        let r = TokenRef {
            module: PathBuf::from("/usr/lib64/p11-kit-proxy.so"),
            id: "01a2".to_owned(),
            serial: Some("1234".to_owned()),
        };
        assert_eq!(
            TokenRef::from_json(&r.to_json().expect("json")).expect("ref"),
            r
        );
        assert!(TokenRef::from_json(r#"{"module":"relative.so","id":"01"}"#).is_err());
        assert!(TokenRef::from_json(r#"{"module":"/m.so","id":"zz"}"#).is_err());
        assert!(TokenRef::from_json("not json").is_err());
    }
}
