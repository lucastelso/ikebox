//! Security level of a profile (phase 6 of the post-0.2.0 program).
//!
//! It **warns, never blocks**: legacy VPNs still need aggressive mode, small
//! DH groups or SHA-1, and the person cannot change the gateway. Each finding
//! says what is weak and why, in the interface language.
//!
//! Levels:
//! - [`SecurityLevel::Weak`]: a practical attack is known (aggressive mode
//!   with a PSK, DH under 2048 bits, DES, MD5, NULL encryption);
//! - [`SecurityLevel::Legacy`]: obsolete but without a cheap attack (IKEv1,
//!   group PSK + XAuth, 3DES and 64-bit block ciphers, SHA-1, modp1536);
//! - [`SecurityLevel::Good`]: nothing to point out.

use std::collections::BTreeSet;

use serde::Serialize;

use super::model::{LocalMethod, Profile};

/// Overall level of a profile: the worst of its findings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SecurityLevel {
    /// Nothing to point out.
    Good,
    /// Obsolete, without a cheap attack.
    Legacy,
    /// A practical attack is known.
    Weak,
}

impl SecurityLevel {
    /// Name for the person, in the interface language.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Good => crate::tr!("good", "boa"),
            Self::Legacy => crate::tr!("legacy", "legada"),
            Self::Weak => crate::tr!("weak", "fraca"),
        }
    }
}

/// What a finding is about (stable, for tests and the interface).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FindingKind {
    /// IKEv1 aggressive mode with a PSK.
    AggressivePsk,
    /// Group PSK + XAuth (audit P13).
    GroupPskXauth,
    /// IKEv1 itself.
    Ikev1,
    /// DH group under 2048 bits.
    WeakDh,
    /// DH group of 1536 bits or a small subgroup.
    LegacyDh,
    /// DES or NULL encryption.
    BrokenCipher,
    /// 3DES, Blowfish, CAST: 64-bit blocks.
    LegacyCipher,
    /// MD5.
    BrokenHash,
    /// SHA-1.
    LegacyHash,
}

/// One weak point of the profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SecurityFinding {
    /// What it is about.
    pub kind: FindingKind,
    /// How bad it is.
    pub level: SecurityLevel,
    /// Profile field (e.g. `ike.proposals`).
    pub field: String,
    /// Explanation for the person, in the interface language.
    pub message: String,
}

/// The security level of a profile and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SecurityReport {
    /// The worst level among the findings ([`SecurityLevel::Good`] if none).
    pub level: SecurityLevel,
    /// Findings, the worst first.
    pub findings: Vec<SecurityFinding>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Class {
    WeakDh,
    LegacyDh,
    BrokenCipher,
    LegacyCipher,
    BrokenHash,
    LegacyHash,
}

/// Class of one algorithm keyword of a strongSwan proposal.
fn classify(token: &str) -> Option<Class> {
    Some(match token {
        "modp768" | "modp1024" | "modp1024s160" | "ecp192" => Class::WeakDh,
        "modp1536" | "modp2048s224" | "ecp224" | "ecp224bp" => Class::LegacyDh,
        "des" | "null" => Class::BrokenCipher,
        "3des" | "cast128" | "blowfish" | "blowfish128" | "blowfish192" | "blowfish256" => {
            Class::LegacyCipher
        }
        "md5" | "md5_128" | "prfmd5" => Class::BrokenHash,
        "sha1" | "sha" | "sha1_160" | "prfsha1" => Class::LegacyHash,
        _ => return None,
    })
}

impl Profile {
    /// Security level of the profile, with the explanation of each finding.
    #[must_use]
    pub fn security(&self) -> SecurityReport {
        let mut findings = Vec::new();
        let ike = &self.ike;
        let uses_psk = self.auth.local.iter().any(|r| r.method == LocalMethod::Psk);

        if ike.version == 1 && ike.aggressive && uses_psk {
            findings.push(SecurityFinding {
                kind: FindingKind::AggressivePsk,
                level: SecurityLevel::Weak,
                field: "ike.aggressive".to_owned(),
                message: crate::tr!(
                    "Aggressive mode with a PSK: the gateway answers anyone who knows the \
                     group id with a hash of the PSK, which can be guessed offline. Only a \
                     long random PSK makes that impractical; main mode or IKEv2 avoid it.",
                    "Modo agressivo com PSK: o gateway responde a quem souber o id do grupo \
                     com um hash da PSK, que pode ser adivinhada offline. Só uma PSK longa e \
                     aleatória torna isso impraticável; o modo principal ou o IKEv2 evitam o \
                     problema."
                )
                .to_owned(),
            });
        }
        if uses_psk && self.auth.uses_xauth() {
            findings.push(SecurityFinding {
                kind: FindingKind::GroupPskXauth,
                level: SecurityLevel::Legacy,
                field: "auth.method".to_owned(),
                message: crate::tr!(
                    "Group PSK + XAuth: everyone who has the group PSK can pose as the \
                     gateway and receive your username and password. Use it on networks \
                     you trust, and prefer IKEv2 with a gateway certificate when available.",
                    "PSK de grupo + XAuth: todos que têm a PSK do grupo podem se passar pelo \
                     gateway e receber o seu usuário e senha. Use em redes de confiança e \
                     prefira IKEv2 com certificado no gateway quando houver."
                )
                .to_owned(),
            });
        }
        if ike.version == 1 {
            findings.push(SecurityFinding {
                kind: FindingKind::Ikev1,
                level: SecurityLevel::Legacy,
                field: "ike.version".to_owned(),
                message: crate::tr!(
                    "IKEv1 is obsolete (RFC 9395): use IKEv2 when the gateway offers it.",
                    "O IKEv1 está obsoleto (RFC 9395): use IKEv2 quando o gateway oferecer."
                )
                .to_owned(),
            });
        }
        check_proposals(&mut findings, "ike.proposals", &ike.proposals, true);
        check_proposals(
            &mut findings,
            "child.esp_proposals",
            &self.child.esp_proposals,
            false,
        );

        findings.sort_by_key(|f| std::cmp::Reverse(f.level));
        let level = findings
            .iter()
            .map(|f| f.level)
            .max()
            .unwrap_or(SecurityLevel::Good);
        SecurityReport { level, findings }
    }
}

fn check_proposals(
    findings: &mut Vec<SecurityFinding>,
    field: &str,
    proposals: &[String],
    phase1: bool,
) {
    let mut found: Vec<(Class, BTreeSet<&str>)> = Vec::new();
    for token in proposals.iter().flat_map(|p| p.split('-')) {
        if let Some(class) = classify(token) {
            match found.iter_mut().find(|(c, _)| *c == class) {
                Some((_, algos)) => {
                    algos.insert(token);
                }
                None => found.push((class, BTreeSet::from([token]))),
            }
        }
    }
    found.sort_by_key(|(class, _)| *class);
    let phase = if phase1 {
        crate::tr!("Phase 1 (IKE)", "Fase 1 (IKE)")
    } else {
        crate::tr!("Phase 2 (ESP)", "Fase 2 (ESP)")
    };
    for (class, algos) in found {
        let algos = algos.into_iter().collect::<Vec<_>>().join(", ");
        let (kind, level, message) = match class {
            Class::WeakDh => (
                FindingKind::WeakDh,
                SecurityLevel::Weak,
                crate::trf!(
                    "{phase} offers DH groups under 2048 bits ({algos}): an attacker with \
                     enough resources can recover the keys of recorded sessions (Logjam). \
                     Prefer modp2048 or curve25519.",
                    "{phase} oferece grupos DH com menos de 2048 bits ({algos}): um atacante \
                     com recursos suficientes consegue recuperar as chaves de sessões \
                     gravadas (Logjam). Prefira modp2048 ou curve25519."
                ),
            ),
            Class::LegacyDh => (
                FindingKind::LegacyDh,
                SecurityLevel::Legacy,
                crate::trf!(
                    "{phase} offers DH groups below today's 2048 bits ({algos}).",
                    "{phase} oferece grupos DH abaixo dos 2048 bits recomendados hoje ({algos})."
                ),
            ),
            Class::BrokenCipher => (
                FindingKind::BrokenCipher,
                SecurityLevel::Weak,
                crate::trf!(
                    "{phase} offers broken or no encryption ({algos}). Use AES.",
                    "{phase} oferece criptografia quebrada ou nenhuma ({algos}). Use AES."
                ),
            ),
            Class::LegacyCipher => (
                FindingKind::LegacyCipher,
                SecurityLevel::Legacy,
                crate::trf!(
                    "{phase} offers obsolete encryption with 64-bit blocks ({algos}), which \
                     leaks data in long sessions (Sweet32). Prefer AES.",
                    "{phase} oferece criptografia obsoleta de blocos de 64 bits ({algos}), \
                     que vaza dados em sessões longas (Sweet32). Prefira AES."
                ),
            ),
            Class::BrokenHash => (
                FindingKind::BrokenHash,
                SecurityLevel::Weak,
                crate::trf!(
                    "{phase} offers MD5 ({algos}), which is broken. Prefer SHA-256.",
                    "{phase} oferece MD5 ({algos}), que está quebrado. Prefira SHA-256."
                ),
            ),
            Class::LegacyHash => (
                FindingKind::LegacyHash,
                SecurityLevel::Legacy,
                crate::trf!(
                    "{phase} offers SHA-1 ({algos}): still sound as an HMAC, but obsolete. \
                     Prefer SHA-256.",
                    "{phase} oferece SHA-1 ({algos}): ainda seguro como HMAC, mas obsoleto. \
                     Prefira SHA-256."
                ),
            ),
        };
        findings.push(SecurityFinding {
            kind,
            level,
            field: field.to_owned(),
            message,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::parse_profile;

    fn profile(version: u8, aggressive: bool, method: &str, ike: &str, esp: &str) -> Profile {
        parse_profile(&format!(
            r#"{{"schema_version":1,"id":"t","name":"T","gateway":"vpn.example.com",
               "ike":{{"version":{version},"aggressive":{aggressive},"proposals":[{ike}]}},
               "auth":{{"method":"{method}"}},
               "child":{{"esp_proposals":[{esp}],"remote_ts":["192.0.2.0/24"]}}}}"#
        ))
        .expect("valid test profile")
    }

    fn kinds(report: &SecurityReport) -> Vec<FindingKind> {
        report.findings.iter().map(|f| f.kind).collect()
    }

    #[test]
    fn legacy_vpn_is_weak_and_says_why() {
        // The shape of the test gateway: aggressive + group PSK + XAuth.
        let p = profile(
            1,
            true,
            "psk-xauth",
            r#""aes256-sha1-modp1024","aes128-sha1-modp1024""#,
            r#""aes256-sha1-modp1024","aes256-sha1-modp768""#,
        );
        let report = p.security();
        assert_eq!(report.level, SecurityLevel::Weak);
        let kinds = kinds(&report);
        for kind in [
            FindingKind::AggressivePsk,
            FindingKind::GroupPskXauth,
            FindingKind::Ikev1,
            FindingKind::WeakDh,
            FindingKind::LegacyHash,
        ] {
            assert!(kinds.contains(&kind), "{kind:?} missing: {kinds:?}");
        }
        // One finding per class and phase, listing every algorithm once.
        let esp_dh: Vec<_> = report
            .findings
            .iter()
            .filter(|f| f.kind == FindingKind::WeakDh && f.field == "child.esp_proposals")
            .collect();
        assert_eq!(esp_dh.len(), 1);
        assert!(esp_dh[0].message.contains("modp1024, modp768"));
        // The worst first.
        assert!(report.findings.windows(2).all(|w| w[0].level >= w[1].level));
    }

    #[test]
    fn modern_ikev2_is_good() {
        let p = profile(
            2,
            false,
            "psk",
            r#""aes256gcm16-prfsha256-curve25519""#,
            r#""aes256gcm16-curve25519","aes256gcm16""#,
        );
        let report = p.security();
        assert_eq!(report.level, SecurityLevel::Good);
        assert!(report.findings.is_empty());
    }

    #[test]
    fn ikev1_main_mode_with_good_algorithms_is_only_legacy() {
        let p = profile(
            1,
            false,
            "psk",
            r#""aes256-sha256-modp2048""#,
            r#""aes256-sha256-modp2048""#,
        );
        let report = p.security();
        assert_eq!(report.level, SecurityLevel::Legacy);
        assert_eq!(kinds(&report), [FindingKind::Ikev1]);
    }

    #[test]
    fn ciphers_hashes_and_groups() {
        let p = profile(
            2,
            false,
            "psk",
            r#""3des-md5-modp1536""#,
            r#""des-sha256","default""#,
        );
        let report = p.security();
        assert_eq!(report.level, SecurityLevel::Weak);
        let kinds = kinds(&report);
        for kind in [
            FindingKind::LegacyCipher,
            FindingKind::BrokenHash,
            FindingKind::LegacyDh,
            FindingKind::BrokenCipher,
        ] {
            assert!(kinds.contains(&kind), "{kind:?} missing: {kinds:?}");
        }
        assert!(!kinds.contains(&FindingKind::Ikev1));
    }
}
