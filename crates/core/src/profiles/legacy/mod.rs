//! Importers for the configuration files of other VPN clients (phase 10):
//! Cisco VPN Client (`.pcf`), vpnc, NetworkManager, the old `ipsec.conf` of
//! strongSwan/libreswan, and FortiClient's XML.
//!
//! Security, as for `swanctl.conf`: **secrets are skipped without being
//! read**. A group password, plain or "encrypted" (the `.pcf` `enc_GroupPwd`
//! uses a known, weak algorithm), an XAuth username or password, a
//! `[vpn-secrets]` section: none of it is decrypted, copied or kept; the
//! person types the credentials when connecting (decided at the start of
//! phase 10). Paths inside a file (a CA, a key) are never opened either.
//! The profile comes out like any other: validated, with warnings for what
//! the person should review.

mod forticlient;
mod ipsecconf;
mod networkmanager;
mod pcf;
mod vpnc;

use super::ProfileError;
use super::model::{
    AuthSettings, ChildAction, ChildSettings, IkeSettings, NetworkSettings, Profile,
};
use super::swanctl::{SwanctlImport, SwanctlImportOptions, slugify};
use super::validate::validate;

/// A file format of another VPN client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyFormat {
    /// Cisco VPN Client profile (`.pcf`).
    CiscoPcf,
    /// vpnc configuration (`/etc/vpnc/*.conf`).
    Vpnc,
    /// NetworkManager connection (`.nmconnection`, keyfile format).
    NetworkManager,
    /// strongSwan/libreswan `ipsec.conf` (`conn` sections).
    IpsecConf,
    /// FortiClient configuration backup (XML).
    FortiClient,
}

impl LegacyFormat {
    /// Name shown to the person.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::CiscoPcf => "Cisco .pcf",
            Self::Vpnc => "vpnc",
            Self::NetworkManager => "NetworkManager",
            Self::IpsecConf => "ipsec.conf",
            Self::FortiClient => "FortiClient",
        }
    }
}

/// Recognizes the format by the file name and content. `None`: none of
/// these (a swanctl.conf or a profile is handled elsewhere).
#[must_use]
pub fn detect_legacy(file_name: &str, text: &str) -> Option<LegacyFormat> {
    let name = file_name.to_ascii_lowercase();
    let head: String = text.chars().take(4096).collect();
    let lower = head.to_ascii_lowercase();
    if name.ends_with(".pcf") || (lower.contains("[main]") && lower.contains("host=")) {
        return Some(LegacyFormat::CiscoPcf);
    }
    if name.ends_with(".nmconnection")
        || (lower.contains("[connection]") && lower.contains("service-type="))
    {
        return Some(LegacyFormat::NetworkManager);
    }
    if lower.contains("<forticlient_configuration") {
        return Some(LegacyFormat::FortiClient);
    }
    if lower.contains("ipsec gateway ") {
        return Some(LegacyFormat::Vpnc);
    }
    let conn_line = head.lines().any(|l| {
        let l = l.trim_start();
        l.starts_with("conn ") || l.starts_with("config setup")
    });
    if conn_line || name == "ipsec.conf" || name.ends_with(".ipsec.conf") {
        return Some(LegacyFormat::IpsecConf);
    }
    None
}

/// Converts a file of another VPN client into a profile.
///
/// # Errors
/// [`ProfileError::Legacy`] when the file cannot be read or uses something
/// the app cannot import (certificates by path, TCP tunneling, a non-IPsec
/// VPN); [`ProfileError::Invalid`] when the result is not a valid profile.
pub fn import_legacy(
    text: &str,
    format: LegacyFormat,
    options: &SwanctlImportOptions,
) -> Result<SwanctlImport, ProfileError> {
    let mut warnings = Vec::new();
    let mut profile = match format {
        LegacyFormat::CiscoPcf => pcf::import(text, &mut warnings)?,
        LegacyFormat::Vpnc => vpnc::import(text, &mut warnings)?,
        LegacyFormat::NetworkManager => networkmanager::import(text, &mut warnings)?,
        LegacyFormat::IpsecConf => ipsecconf::import(text, options, &mut warnings)?,
        LegacyFormat::FortiClient => forticlient::import(text, &mut warnings)?,
    };
    if let Some(name) = &options.name {
        profile.name.clone_from(name);
    }
    profile.id = options.id.clone().unwrap_or_else(|| slugify(&profile.name));
    // DNS given by the person wins over what the file says.
    let given = &options.dns;
    if !given.servers.is_empty() || !given.domains.is_empty() || given.default_route {
        profile.network.dns = given.clone();
    } else if given.use_received {
        profile.network.dns.use_received = true;
    }
    let issues = validate(&profile);
    if !issues.is_empty() {
        return Err(ProfileError::Invalid(issues));
    }
    Ok(SwanctlImport { profile, warnings })
}

fn err(format: LegacyFormat, message: impl Into<String>) -> ProfileError {
    ProfileError::Legacy(format.label(), message.into())
}

/// A Diffie-Hellman group number as a strongSwan keyword (`2` → `modp1024`).
fn dh_keyword(group: u32) -> Option<&'static str> {
    Some(match group {
        1 => "modp768",
        2 => "modp1024",
        5 => "modp1536",
        14 => "modp2048",
        15 => "modp3072",
        16 => "modp4096",
        17 => "modp6144",
        18 => "modp8192",
        19 => "ecp256",
        20 => "ecp384",
        21 => "ecp521",
        31 => "curve25519",
        32 => "curve448",
        _ => return None,
    })
}

/// `dh2`, `2`, `group2`, `modp1024` → the group number.
fn dh_group(value: &str) -> Option<u32> {
    let v = value.trim().to_ascii_lowercase();
    let digits = v
        .trim_start_matches("dh")
        .trim_start_matches("group")
        .trim();
    if let Ok(n) = digits.parse::<u32>() {
        return dh_keyword(n).map(|_| n);
    }
    (1..=32).find(|n| dh_keyword(*n) == Some(v.as_str()))
}

/// What a Cisco-style client (`.pcf`, vpnc) says: IKEv1 aggressive mode,
/// the group name as the key id, group PSK + XAuth.
struct CiscoStyle {
    name: String,
    gateway: String,
    group: Option<String>,
    dh: u32,
    pfs: Option<u32>,
    force_natt: bool,
    domain: Option<String>,
}

/// Cisco clients offer AES-256, AES-128 and 3DES with SHA-1 over the
/// configured DH group, and phase 2 without PFS unless set.
fn cisco_profile(c: CiscoStyle, warnings: &mut Vec<String>) -> Profile {
    let dh = dh_keyword(c.dh).unwrap_or("modp1024");
    let ike = ["aes256", "aes128", "3des"]
        .iter()
        .map(|cipher| format!("{cipher}-sha1-{dh}"))
        .collect();
    let esp = ["aes256", "aes128", "3des"]
        .iter()
        .map(|cipher| match c.pfs.and_then(dh_keyword) {
            Some(group) => format!("{cipher}-sha1-{group}"),
            None => format!("{cipher}-sha1"),
        })
        .collect();
    warnings.push(
        crate::tr!(
            "the networks reached through the VPN come from the gateway (Cisco sends them \
             after connecting): the profile asks for everything (0.0.0.0/0); if only some \
             networks should go through the VPN, set them in the editor",
            "as redes acessadas pela VPN vêm do gateway (o Cisco as envia depois de \
             conectar): o perfil pede tudo (0.0.0.0/0); se só algumas redes devem passar \
             pela VPN, defina-as no editor"
        )
        .to_owned(),
    );
    let mut network = NetworkSettings::default();
    if let Some(domain) = c.domain.filter(|d| !d.trim().is_empty()) {
        network.dns.domains = vec![format!("~{}", domain.trim().trim_start_matches('~'))];
    }
    Profile {
        id: String::new(),
        name: c.name,
        gateway: c.gateway,
        ike: IkeSettings {
            version: 1,
            aggressive: true,
            encap: c.force_natt,
            proposals: ike,
            rekey_time_secs: None,
            dpd_delay_secs: Some(30),
        },
        auth: AuthSettings::psk(c.group.map(|g| format!("keyid:{g}")), true, None),
        child: ChildSettings {
            name: "rede".to_owned(),
            esp_proposals: esp,
            rekey_time_secs: None,
            remote_ts: vec!["0.0.0.0/0".to_owned()],
            local_ts: Vec::new(),
            dpd_action: ChildAction::Restart,
            close_action: ChildAction::Restart,
        },
        network,
    }
}

/// `[section]` + `key=value` (INI, as `.pcf` and NetworkManager keyfiles).
/// Sections named in `skip` are passed over without keeping a value.
fn parse_ini(text: &str, skip: &[&str]) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    let mut section = String::new();
    let mut skipping = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = name.trim().to_ascii_lowercase();
            skipping = skip.contains(&section.as_str());
            continue;
        }
        if skipping {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            // A secret's value is never copied out of the file text.
            if is_secret_key(key.trim()) {
                continue;
            }
            out.push((
                section.clone(),
                key.trim().to_owned(),
                value.trim().to_owned(),
            ));
        }
    }
    out
}

fn ini_get<'a>(
    entries: &'a [(String, String, String)],
    section: &str,
    key: &str,
) -> Option<&'a str> {
    entries
        .iter()
        .find(|(s, k, _)| s == section && k.eq_ignore_ascii_case(key))
        .map(|(_, _, v)| v.as_str())
        .filter(|v| !v.is_empty())
}

/// Keys that hold secrets or personal credentials in these formats: never
/// read (the parsers drop them before anything else looks at the line).
fn is_secret_key(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    k.contains("pwd")
        || k.contains("password")
        || k.contains("secret")
        || k.contains("psk")
        || k.contains("username")
        || k == "user"
        || k.contains("xauth_identity")
        || k.contains("auth_key")
        || k.contains("preshared")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_are_recognized() {
        assert_eq!(
            detect_legacy("work.pcf", "[main]\nHost=x"),
            Some(LegacyFormat::CiscoPcf)
        );
        assert_eq!(
            detect_legacy("vpn.conf", "IPSec gateway vpn.example.com\n"),
            Some(LegacyFormat::Vpnc)
        );
        assert_eq!(
            detect_legacy("work.nmconnection", "[connection]\nid=x"),
            Some(LegacyFormat::NetworkManager)
        );
        assert_eq!(
            detect_legacy(
                "backup.conf",
                "<?xml version=\"1.0\"?>\n<forticlient_configuration>"
            ),
            Some(LegacyFormat::FortiClient)
        );
        assert_eq!(
            detect_legacy("ipsec.conf", "config setup\nconn work\n  right=x"),
            Some(LegacyFormat::IpsecConf)
        );
        // A swanctl.conf is not one of these.
        assert_eq!(detect_legacy("swanctl.conf", "connections {\n}"), None);
    }

    #[test]
    fn dh_groups_in_every_spelling() {
        for (spelling, group) in [
            ("dh2", 2),
            ("2", 2),
            ("group14", 14),
            ("modp1536", 5),
            ("DH5", 5),
        ] {
            assert_eq!(dh_group(spelling), Some(group), "{spelling}");
        }
        assert_eq!(dh_group("dh99"), None);
        assert_eq!(dh_keyword(31), Some("curve25519"));
    }
}
