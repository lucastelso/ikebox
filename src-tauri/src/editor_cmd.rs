//! Profile editor (phase 7): the UI edits a flat draft, and Rust turns it into
//! a profile, validates it and rates its security. Rust stays the source of
//! truth (ADR-0008): the UI never builds a profile by itself.
//!
//! A draft carries only what the editor shows. When an existing profile is
//! edited, everything else (rekey times, child name, local networks, DPD and
//! close actions) comes from the saved profile; a new one takes it from its
//! template.

use std::net::IpAddr;

use ipsec_client_core::profiles::{
    AuthSettings, LocalMethod, Profile, ProfileStore, ProposalPreset, Template, proposals_level,
    schema_of, validate,
};
use ipsec_client_core::{tr, trf};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::commands::blocking;
use crate::dto::{ProfileSummary, Security, SecurityNote, UiError, UiErrorKind};

/// Vendor template of a new profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ProfileTemplate {
    /// FortiGate: IKEv1 aggressive, group PSK + XAuth.
    Fortigate,
    /// Cisco Easy VPN: IKEv1 aggressive, group PSK + XAuth, older algorithms.
    CiscoEzvpn,
    /// Any gateway with IKEv2 and a PSK.
    Ikev2Psk,
    /// Nothing suggested.
    Blank,
}

impl From<ProfileTemplate> for Template {
    fn from(t: ProfileTemplate) -> Self {
        match t {
            ProfileTemplate::Fortigate => Self::Fortigate,
            ProfileTemplate::CiscoEzvpn => Self::CiscoEzvpn,
            ProfileTemplate::Ikev2Psk => Self::Ikev2Psk,
            ProfileTemplate::Blank => Self::Blank,
        }
    }
}

/// A named set of proposals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Proposals {
    /// AES-GCM and Curve25519 (IKEv2).
    Modern,
    /// AES and SHA-256 with DH 2048.
    Compatible,
    /// SHA-1 and DH 1024: old gateways.
    Legacy,
}

impl From<ProposalPreset> for Proposals {
    fn from(p: ProposalPreset) -> Self {
        match p {
            ProposalPreset::Modern => Self::Modern,
            ProposalPreset::Compatible => Self::Compatible,
            ProposalPreset::Legacy => Self::Legacy,
        }
    }
}

impl From<Proposals> for ProposalPreset {
    fn from(p: Proposals) -> Self {
        match p {
            Proposals::Modern => Self::Modern,
            Proposals::Compatible => Self::Compatible,
            Proposals::Legacy => Self::Legacy,
        }
    }
}

/// A preset as the editor offers it.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProposalPresetInfo {
    /// Which preset.
    pub preset: Proposals,
    /// Phase 1 proposals.
    pub ike: Vec<String>,
    /// Phase 2 proposals.
    pub esp: Vec<String>,
    /// Security level of these proposals.
    pub security: Security,
}

/// What the editor shows of a profile. No credentials: profiles never have
/// them (ADR-0007).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProfileDraft {
    /// Id of the profile being edited; absent for a new profile.
    pub original_id: Option<String>,
    /// Template a new profile starts from.
    pub template: ProfileTemplate,
    /// Identifier (file name and keyring key; fixed once saved).
    pub id: String,
    /// Display name.
    pub name: String,
    /// Gateway: DNS name or IP.
    pub gateway: String,
    /// 1 or 2.
    pub ike_version: u8,
    /// IKEv1 aggressive mode.
    pub aggressive: bool,
    /// Forces NAT-T.
    pub encap: bool,
    /// Phase 1 proposals.
    pub ike_proposals: Vec<String>,
    /// Phase 2 proposals.
    pub esp_proposals: Vec<String>,
    /// A second round with username and password (IKEv1).
    pub xauth: bool,
    /// The XAuth password is a one-time code.
    pub otp: bool,
    /// Identity of the PSK round (group id); empty = the computer's address.
    pub local_id: String,
    /// Identity the gateway must present; empty = any.
    pub remote_id: String,
    /// Networks reached through the VPN.
    pub remote_ts: Vec<String>,
    /// Asks the gateway for a virtual IP.
    pub request_vip: bool,
    /// Static DNS servers.
    pub dns_servers: Vec<String>,
    /// DNS domains (`~x` routes only).
    pub dns_domains: Vec<String>,
    /// Also uses the DNS servers sent by the gateway.
    pub use_received_dns: bool,
    /// These DNS servers answer every name.
    pub dns_default_route: bool,
    /// Tunnel MTU.
    pub mtu: u16,
    /// DPD interval in seconds; absent = off.
    pub dpd_delay_secs: Option<u32>,
}

/// A problem in a field of the draft.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DraftIssue {
    /// Draft field (`gateway`, `remoteTs`...; `general` when none fits).
    pub field: String,
    /// What is wrong, in the interface language.
    pub message: String,
}

/// The draft as Rust sees it: problems, security and file schema.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DraftCheck {
    /// Problems; empty = can be saved.
    pub issues: Vec<DraftIssue>,
    /// Security level of the profile.
    pub security: Security,
    /// Why, the worst first.
    pub notes: Vec<SecurityNote>,
    /// The preset the proposals are, if any.
    pub preset: Option<Proposals>,
    /// Schema the file will be written in (1 unless something needs 2).
    pub schema: u32,
}

impl ProfileDraft {
    fn from_profile(p: &Profile, original_id: Option<String>, template: ProfileTemplate) -> Self {
        Self {
            original_id,
            template,
            id: p.id.clone(),
            name: p.name.clone(),
            gateway: p.gateway.clone(),
            ike_version: p.ike.version,
            aggressive: p.ike.aggressive,
            encap: p.ike.encap,
            ike_proposals: p.ike.proposals.clone(),
            esp_proposals: p.child.esp_proposals.clone(),
            xauth: p.auth.uses_xauth(),
            otp: p.auth.otp(),
            local_id: p.auth.psk_id().unwrap_or_default().to_owned(),
            remote_id: p.auth.remote_id().unwrap_or_default().to_owned(),
            remote_ts: p.child.remote_ts.clone(),
            request_vip: p.network.request_vip,
            dns_servers: p
                .network
                .dns
                .servers
                .iter()
                .map(ToString::to_string)
                .collect(),
            dns_domains: p.network.dns.domains.clone(),
            use_received_dns: p.network.dns.use_received,
            dns_default_route: p.network.dns.default_route,
            mtu: p.network.mtu,
            dpd_delay_secs: p.ike.dpd_delay_secs,
        }
    }

    /// The profile this draft describes, on top of `base`, and the problems
    /// found on the way (validation included).
    fn to_profile(&self, mut p: Profile) -> (Profile, Vec<DraftIssue>) {
        let mut issues = Vec::new();
        let text = |s: &str| s.trim().to_owned();
        let optional = |s: &str| Some(s.trim().to_owned()).filter(|s| !s.is_empty());
        let list = |items: &[String]| -> Vec<String> {
            items
                .iter()
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
                .collect()
        };
        p.id = text(&self.id);
        p.name = text(&self.name);
        p.gateway = text(&self.gateway);
        p.ike.version = self.ike_version;
        p.ike.aggressive = self.aggressive;
        p.ike.encap = self.encap;
        p.ike.proposals = list(&self.ike_proposals);
        p.ike.dpd_delay_secs = self.dpd_delay_secs;
        p.auth = AuthSettings::psk(
            optional(&self.local_id),
            self.xauth,
            optional(&self.remote_id),
        );
        if let Some(round) = p
            .auth
            .local
            .iter_mut()
            .find(|r| r.method == LocalMethod::Xauth)
        {
            round.otp = self.otp;
        }
        p.child.esp_proposals = list(&self.esp_proposals);
        p.child.remote_ts = list(&self.remote_ts);
        p.network.request_vip = self.request_vip;
        p.network.mtu = self.mtu;
        p.network.dns.servers = Vec::new();
        for server in list(&self.dns_servers) {
            match server.parse::<IpAddr>() {
                Ok(ip) => p.network.dns.servers.push(ip),
                Err(_) => issues.push(DraftIssue {
                    field: "dnsServers".to_owned(),
                    message: trf!(
                        "{server}: not an IP address",
                        "{server}: não é um endereço IP"
                    ),
                }),
            }
        }
        p.network.dns.domains = list(&self.dns_domains);
        p.network.dns.use_received = self.use_received_dns;
        p.network.dns.default_route = self.dns_default_route;

        for issue in validate(&p) {
            let (field, value) = draft_field(&issue.field, &p);
            issues.push(DraftIssue {
                field: field.to_owned(),
                message: match value {
                    Some(value) => format!("{value}: {}", issue.message),
                    None => issue.message,
                },
            });
        }
        (p, issues)
    }
}

/// The draft field of a profile validation path, and the offending item of a
/// list (`child.remote_ts[1]` → `remoteTs`, its second network).
fn draft_field(path: &str, p: &Profile) -> (&'static str, Option<String>) {
    let index = path
        .rsplit_once('[')
        .and_then(|(_, rest)| rest.trim_end_matches(']').parse::<usize>().ok());
    let item = |items: &[String]| index.and_then(|i| items.get(i).cloned());
    match path {
        "id" | "name" | "gateway" => (
            match path {
                "id" => "id",
                "name" => "name",
                _ => "gateway",
            },
            None,
        ),
        "ike.version" => ("ikeVersion", None),
        "ike.aggressive" => ("aggressive", None),
        "ike.dpd_delay_secs" => ("dpdDelaySecs", None),
        "auth.local[0].id" => ("localId", None),
        "auth.remote.id" => ("remoteId", None),
        "network.mtu" => ("mtu", None),
        _ if path.starts_with("ike.proposals") => ("ikeProposals", item(&p.ike.proposals)),
        _ if path.starts_with("auth.local") => ("xauth", None),
        _ if path.starts_with("child.esp_proposals") => {
            ("espProposals", item(&p.child.esp_proposals))
        }
        _ if path.starts_with("child.remote_ts") => ("remoteTs", item(&p.child.remote_ts)),
        _ if path.starts_with("network.dns.servers") => ("dnsServers", None),
        _ if path.starts_with("network.dns.domains") => {
            ("dnsDomains", item(&p.network.dns.domains))
        }
        _ => ("general", None),
    }
}

fn store() -> Result<ProfileStore, UiError> {
    Ok(ProfileStore::new(ProfileStore::default_dir()?))
}

/// The profile a draft starts from: the saved one, or the template.
fn base_of(draft: &ProfileDraft, store: &ProfileStore) -> Result<Profile, UiError> {
    match &draft.original_id {
        Some(id) => Ok(store.load(id)?),
        None => Ok(Template::from(draft.template).profile()),
    }
}

fn check(draft: &ProfileDraft, base: Profile) -> (Profile, DraftCheck) {
    let (profile, issues) = draft.to_profile(base);
    let report = profile.security();
    let check = DraftCheck {
        issues,
        security: report.level.into(),
        notes: report.findings.iter().map(SecurityNote::from).collect(),
        preset: ProposalPreset::detect(
            profile.ike.version,
            &profile.ike.proposals,
            &profile.child.esp_proposals,
        )
        .map(Proposals::from),
        schema: schema_of(&profile),
    };
    (profile, check)
}

/// The presets the editor offers for an IKE version, with their security.
#[tauri::command]
#[specta::specta]
pub fn proposal_presets(ike_version: u8) -> Vec<ProposalPresetInfo> {
    ProposalPreset::for_version(ike_version)
        .iter()
        .map(|p| {
            let (ike, esp) = (p.ike(ike_version), p.esp(ike_version));
            ProposalPresetInfo {
                preset: (*p).into(),
                security: proposals_level(&ike, &esp).into(),
                ike,
                esp,
            }
        })
        .collect()
}

/// A new draft from a template.
#[tauri::command]
#[specta::specta]
pub fn new_profile_draft(template: ProfileTemplate) -> ProfileDraft {
    ProfileDraft::from_profile(&Template::from(template).profile(), None, template)
}

/// A draft of a saved profile.
#[tauri::command]
#[specta::specta]
pub async fn edit_profile_draft(profile_id: String) -> Result<ProfileDraft, UiError> {
    blocking(move || {
        let profile = store()?.load(&profile_id)?;
        Ok(ProfileDraft::from_profile(
            &profile,
            Some(profile_id),
            ProfileTemplate::Blank,
        ))
    })
    .await
}

/// Problems, security and schema of a draft (called while the person types).
#[tauri::command]
#[specta::specta]
pub async fn check_profile_draft(draft: ProfileDraft) -> Result<DraftCheck, UiError> {
    blocking(move || {
        let base = base_of(&draft, &store()?)?;
        Ok(check(&draft, base).1)
    })
    .await
}

/// Saves the draft: a new profile (its id must be free) or the edited one
/// (its id does not change: it is also the keyring key).
#[tauri::command]
#[specta::specta]
pub async fn save_profile_draft(draft: ProfileDraft) -> Result<ProfileSummary, UiError> {
    blocking(move || {
        let store = store()?;
        let base = base_of(&draft, &store)?;
        let (profile, result) = check(&draft, base);
        if let Some(issue) = result.issues.first() {
            return Err(UiError::new(
                UiErrorKind::Profile,
                trf!(
                    "The profile has problems to fix: {}",
                    "O perfil tem problemas a corrigir: {}",
                    issue.message
                ),
            ));
        }
        match &draft.original_id {
            Some(original) if *original != profile.id => {
                return Err(UiError::new(
                    UiErrorKind::Profile,
                    tr!(
                        "The identifier of a saved profile cannot change.",
                        "O identificador de um perfil salvo não pode mudar."
                    ),
                ));
            }
            None if store.load(&profile.id).is_ok() => {
                return Err(UiError::new(
                    UiErrorKind::Profile,
                    trf!(
                        "A profile with id \"{}\" already exists. Choose another id.",
                        "Já existe um perfil com o id \"{}\". Escolha outro id.",
                        profile.id
                    ),
                ));
            }
            _ => {}
        }
        store.save(&profile)?;
        Ok(ProfileSummary::from(&profile))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filled(template: ProfileTemplate) -> ProfileDraft {
        let mut d = new_profile_draft(template);
        d.id = "vpn".to_owned();
        d.name = " VPN ".to_owned();
        d.gateway = "vpn.example.com".to_owned();
        d.remote_ts = vec!["192.0.2.0/24".to_owned(), String::new()];
        d
    }

    fn checked(d: &ProfileDraft) -> (Profile, DraftCheck) {
        check(d, Template::from(d.template).profile())
    }

    #[test]
    fn a_filled_template_is_valid_and_trimmed() {
        for t in [
            ProfileTemplate::Fortigate,
            ProfileTemplate::CiscoEzvpn,
            ProfileTemplate::Ikev2Psk,
            ProfileTemplate::Blank,
        ] {
            let (p, c) = checked(&filled(t));
            assert!(c.issues.is_empty(), "{t:?}: {:?}", c.issues);
            assert_eq!(p.name, "VPN");
            assert_eq!(p.child.remote_ts, ["192.0.2.0/24"]);
            assert_eq!(c.schema, 1);
        }
        let (_, c) = checked(&filled(ProfileTemplate::Fortigate));
        assert_eq!(c.preset, Some(Proposals::Compatible));
        assert_eq!(c.security, Security::Weak); // aggressive + PSK
    }

    #[test]
    fn issues_name_the_draft_field_and_the_bad_item() {
        let mut d = filled(ProfileTemplate::Ikev2Psk);
        d.gateway = "vpn..example".to_owned();
        d.remote_ts = vec!["192.0.2.0/24".to_owned(), "192.0.2.0/99".to_owned()];
        d.dns_servers = vec!["192.0.2.53".to_owned(), "dns.example".to_owned()];
        d.xauth = true; // IKEv2
        d.local_id = " espaço".to_owned();
        let (_, c) = checked(&d);
        let fields: Vec<_> = c.issues.iter().map(|i| i.field.as_str()).collect();
        for expected in ["gateway", "remoteTs", "dnsServers", "xauth"] {
            assert!(fields.contains(&expected), "{expected}: {:?}", c.issues);
        }
        let ts = c.issues.iter().find(|i| i.field == "remoteTs").expect("ts");
        assert!(ts.message.starts_with("192.0.2.0/99: "), "{}", ts.message);
        let dns = c
            .issues
            .iter()
            .find(|i| i.field == "dnsServers")
            .expect("dns");
        assert!(dns.message.starts_with("dns.example: "), "{}", dns.message);
    }

    #[test]
    fn otp_makes_it_a_schema_2_profile() {
        let mut d = filled(ProfileTemplate::Fortigate);
        d.otp = true;
        let (p, c) = checked(&d);
        assert!(c.issues.is_empty(), "{:?}", c.issues);
        assert!(p.auth.otp());
        assert_eq!(c.schema, 2);
        // otp without XAuth has nothing to apply to.
        d.xauth = false;
        let (p, _) = checked(&d);
        assert!(!p.auth.otp());
    }

    #[test]
    fn a_saved_profile_round_trips_through_the_draft() {
        let mut d = filled(ProfileTemplate::CiscoEzvpn);
        d.local_id = "keyid:grupo".to_owned();
        d.dns_servers = vec!["192.0.2.53".to_owned()];
        d.dns_domains = vec!["~corp.example.com".to_owned()];
        let (p, _) = checked(&d);
        let again = ProfileDraft::from_profile(&p, Some(p.id.clone()), ProfileTemplate::Blank);
        let (p2, c) = check(&again, p.clone());
        assert!(c.issues.is_empty(), "{:?}", c.issues);
        assert_eq!(p2, p);
    }

    #[test]
    fn presets_carry_their_security() {
        let v1 = proposal_presets(1);
        assert_eq!(
            v1.iter().map(|p| p.preset).collect::<Vec<_>>(),
            [Proposals::Compatible, Proposals::Legacy]
        );
        assert_eq!(v1[1].security, Security::Weak);
        assert_eq!(proposal_presets(2)[0].security, Security::Good);
    }
}
