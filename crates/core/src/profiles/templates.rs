//! Starting points for the profile editor (phase 7): vendor templates and
//! proposal presets.
//!
//! A template fills what a vendor's remote-access VPN usually looks like and
//! leaves blank what only the gateway's administrator knows (name, gateway,
//! networks, group id): the editor shows those as missing until filled. The
//! presets are sets of proposals the person picks by name, each with the
//! security level it gives; anything else goes in the editor's advanced mode.

use super::model::{
    AuthSettings, ChildAction, ChildSettings, IkeSettings, NetworkSettings, Profile,
};
use super::security::{SecurityLevel, proposals_level};

/// Vendor template for a new profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Template {
    /// FortiGate dial-up (FortiClient legacy style): IKEv1 aggressive, group
    /// PSK + XAuth.
    Fortigate,
    /// Cisco ASA/IOS Easy VPN: IKEv1 aggressive, group name as the key id,
    /// group PSK + XAuth, older algorithms.
    CiscoEzvpn,
    /// Any gateway with IKEv2 and a PSK (strongSwan, pfSense/OPNsense,
    /// MikroTik...).
    Ikev2Psk,
    /// IKEv2 with a username and password (EAP-MSCHAPv2), the gateway proven
    /// by its certificate: what Windows, macOS and Android call "IKEv2 with
    /// a username" (ADR-0023). The CA and the gateway's name stay blank.
    Ikev2Eap,
    /// Nothing suggested: strongSwan's default proposals.
    Blank,
}

impl Template {
    /// Every template, in the order the editor offers them.
    pub const ALL: [Self; 5] = [
        Self::Fortigate,
        Self::CiscoEzvpn,
        Self::Ikev2Psk,
        Self::Ikev2Eap,
        Self::Blank,
    ];

    /// The profile the editor starts from. Name, id, gateway and networks are
    /// empty: the person fills them.
    #[must_use]
    pub fn profile(self) -> Profile {
        let (version, aggressive, encap, xauth, preset, dpd) = match self {
            Self::Fortigate => (
                1,
                true,
                true,
                true,
                Some(ProposalPreset::Compatible),
                Some(30),
            ),
            Self::CiscoEzvpn => (1, true, true, true, Some(ProposalPreset::Legacy), Some(30)),
            Self::Ikev2Psk => (
                2,
                false,
                false,
                false,
                Some(ProposalPreset::Modern),
                Some(30),
            ),
            Self::Ikev2Eap => (
                2,
                false,
                false,
                false,
                Some(ProposalPreset::Compatible),
                Some(30),
            ),
            Self::Blank => (2, false, false, false, None, None),
        };
        let (ike, esp) = preset.map_or_else(
            || (vec!["default".to_owned()], vec!["default".to_owned()]),
            |p| (p.ike(version), p.esp(version)),
        );
        let action = if dpd.is_some() {
            ChildAction::Restart
        } else {
            ChildAction::None
        };
        Profile {
            id: String::new(),
            name: String::new(),
            gateway: String::new(),
            ike: IkeSettings {
                version,
                aggressive,
                encap,
                proposals: ike,
                rekey_time_secs: None,
                dpd_delay_secs: dpd,
            },
            auth: if self == Self::Ikev2Eap {
                let mut auth = AuthSettings::eap(None, String::new(), Vec::new());
                auth.remote.id = None;
                auth
            } else {
                AuthSettings::psk(None, xauth, None)
            },
            child: ChildSettings {
                name: "rede".to_owned(),
                esp_proposals: esp,
                rekey_time_secs: None,
                remote_ts: Vec::new(),
                local_ts: Vec::new(),
                dpd_action: action,
                close_action: action,
            },
            network: NetworkSettings::default(),
        }
    }
}

/// A named set of proposals for both phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProposalPreset {
    /// AES-GCM and Curve25519 (IKEv2 only).
    Modern,
    /// AES-CBC with SHA-256 and DH 2048: what most current gateways accept,
    /// IKEv1 included.
    Compatible,
    /// SHA-1 and DH 1024 (and 3DES as the last resort): old FortiGate/Cisco
    /// configurations. Weak, but sometimes the only thing the gateway takes.
    Legacy,
}

impl ProposalPreset {
    /// Presets that make sense for an IKE version.
    #[must_use]
    pub fn for_version(version: u8) -> &'static [Self] {
        if version == 1 {
            &[Self::Compatible, Self::Legacy]
        } else {
            &[Self::Modern, Self::Compatible, Self::Legacy]
        }
    }

    /// Phase 1 proposals.
    #[must_use]
    pub fn ike(self, version: u8) -> Vec<String> {
        let list: &[&str] = match (self, version) {
            (Self::Modern, _) => &["aes256gcm16-prfsha256-curve25519"],
            (Self::Compatible, _) => &["aes256-sha256-modp2048", "aes128-sha256-modp2048"],
            (Self::Legacy, _) => &[
                "aes256-sha1-modp1024",
                "aes128-sha1-modp1024",
                "3des-sha1-modp1024",
            ],
        };
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    /// Phase 2 proposals. In IKEv1 every proposal carries the same DH group
    /// (PFS): quick mode sends one key exchange, the one of the first
    /// proposal. In IKEv2 the gateway may also pick a proposal without PFS.
    #[must_use]
    pub fn esp(self, version: u8) -> Vec<String> {
        let list: &[&str] = match (self, version) {
            (Self::Modern, _) => &["aes256gcm16-curve25519", "aes256gcm16"],
            (Self::Compatible, 1) => &["aes256-sha256-modp2048", "aes128-sha256-modp2048"],
            (Self::Compatible, _) => &["aes256-sha256-modp2048", "aes256-sha256"],
            (Self::Legacy, 1) => &[
                "aes256-sha1-modp1024",
                "aes128-sha1-modp1024",
                "3des-sha1-modp1024",
            ],
            (Self::Legacy, _) => &["aes256-sha1", "aes128-sha1"],
        };
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    /// Security level of the proposals alone (not of the mode).
    #[must_use]
    pub fn level(self, version: u8) -> SecurityLevel {
        proposals_level(&self.ike(version), &self.esp(version))
    }

    /// The preset these proposals are, if any.
    #[must_use]
    pub fn detect(version: u8, ike: &[String], esp: &[String]) -> Option<Self> {
        Self::for_version(version)
            .iter()
            .copied()
            .find(|p| p.ike(version) == ike && p.esp(version) == esp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::validate;

    #[test]
    fn templates_become_valid_once_the_blanks_are_filled() {
        for template in Template::ALL {
            let mut p = template.profile();
            let fields: Vec<_> = validate(&p).into_iter().map(|i| i.field).collect();
            for blank in ["id", "name", "gateway", "child.remote_ts"] {
                assert!(
                    fields.contains(&blank.to_owned()),
                    "{template:?}: {fields:?}"
                );
            }
            p.id = "vpn".to_owned();
            p.name = "VPN".to_owned();
            p.gateway = "vpn.example.com".to_owned();
            p.child.remote_ts = vec!["192.0.2.0/24".to_owned()];
            if template == Template::Ikev2Eap {
                // Also blank there: the gateway's name and its CA.
                let fields: Vec<_> = validate(&p).into_iter().map(|i| i.field).collect();
                assert_eq!(fields, ["auth.remote.id", "auth.remote.ca"]);
                p.auth.remote.id = Some("vpn.example.com".to_owned());
                p.auth.remote.ca =
                    vec![include_str!("../../tests/fixtures/ca/rsa-ca.pem").to_owned()];
            }
            assert!(validate(&p).is_empty(), "{template:?}: {:?}", validate(&p));
        }
    }

    #[test]
    fn presets_are_detected_and_rated() {
        for version in [1, 2] {
            for preset in ProposalPreset::for_version(version) {
                let (ike, esp) = (preset.ike(version), preset.esp(version));
                assert_eq!(ProposalPreset::detect(version, &ike, &esp), Some(*preset));
            }
        }
        assert_eq!(ProposalPreset::Modern.level(2), SecurityLevel::Good);
        assert_eq!(ProposalPreset::Compatible.level(1), SecurityLevel::Good);
        assert_eq!(ProposalPreset::Legacy.level(1), SecurityLevel::Weak);
        assert_eq!(
            ProposalPreset::detect(2, &["default".to_owned()], &["default".to_owned()]),
            None
        );
        // IKEv1 has no Modern preset (no AEAD in IKEv1 phase 1).
        assert!(!ProposalPreset::for_version(1).contains(&ProposalPreset::Modern));
    }

    #[test]
    fn the_fortigate_template_is_what_its_name_says() {
        let p = Template::Fortigate.profile();
        assert_eq!((p.ike.version, p.ike.aggressive), (1, true));
        assert!(p.auth.uses_xauth());
        assert_eq!(
            ProposalPreset::detect(1, &p.ike.proposals, &p.child.esp_proposals),
            Some(ProposalPreset::Compatible)
        );
    }
}
