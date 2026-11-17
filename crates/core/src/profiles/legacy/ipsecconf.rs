//! The old `ipsec.conf` of strongSwan (stroke) and libreswan: `config setup`,
//! `conn name` sections with indented `key=value` lines, `conn %default`
//! and `also=` inheritance. NetworkManager's libreswan plugin stores the same
//! keys, so [`from_conn`] is shared.
//!
//! The client is the "left" side and the gateway the "right" one, as both
//! projects document. `ipsec.secrets` is another file and is never asked
//! for; secret-looking keys (`xauth_identity`, `leftxauthusername`) are
//! skipped without being read.

use std::collections::HashMap;

use super::{LegacyFormat, dh_group, dh_keyword, err, is_secret_key};
use crate::profiles::ProfileError;
use crate::profiles::model::{
    AuthSettings, ChildAction, ChildSettings, IkeSettings, NetworkSettings, Profile,
};
use crate::profiles::swanctl::{SwanctlImportOptions, duration};

const F: LegacyFormat = LegacyFormat::IpsecConf;

/// A profile with the defaults of these clients, to be filled in.
pub(super) fn base_profile(name: &str, gateway: &str) -> Profile {
    Profile {
        id: String::new(),
        name: name.to_owned(),
        gateway: gateway.to_owned(),
        ike: IkeSettings {
            version: 2,
            aggressive: false,
            encap: false,
            proposals: vec!["default".to_owned()],
            rekey_time_secs: None,
            dpd_delay_secs: None,
        },
        auth: AuthSettings::psk(None, false, None),
        child: ChildSettings {
            name: "rede".to_owned(),
            esp_proposals: vec!["default".to_owned()],
            rekey_time_secs: None,
            remote_ts: vec!["0.0.0.0/0".to_owned()],
            local_ts: Vec::new(),
            dpd_action: ChildAction::None,
            close_action: ChildAction::None,
        },
        network: NetworkSettings {
            request_vip: false,
            ..NetworkSettings::default()
        },
    }
}

type Conn = Vec<(String, String)>;

/// `conn` sections, with their own keys (secret keys already gone).
fn parse(text: &str, warnings: &mut Vec<String>) -> Vec<(String, Conn)> {
    let mut conns: Vec<(String, Conn)> = Vec::new();
    let mut current: Option<usize> = None;
    for raw in text.lines() {
        let without_comment = raw.split('#').next().unwrap_or("");
        if without_comment.trim().is_empty() {
            continue;
        }
        let indented = without_comment.starts_with([' ', '\t']);
        let line = without_comment.trim();
        if !indented {
            current = None;
            if let Some(name) = line.strip_prefix("conn ") {
                conns.push((name.trim().to_owned(), Vec::new()));
                current = Some(conns.len() - 1);
            } else if line.starts_with("include ") {
                warnings.push(
                    crate::tr!("'include' is not followed", "'include' não é seguido").to_owned(),
                );
            }
            continue;
        }
        let (Some(index), Some((key, value))) = (current, line.split_once('=')) else {
            continue;
        };
        let key = key.trim();
        if is_secret_key(key) {
            continue;
        }
        let value = value.trim().trim_matches('"').to_owned();
        conns[index].1.push((key.to_owned(), value));
    }
    conns
}

/// The connection's keys after `%default` and `also=` (depth-limited).
fn resolve(conns: &[(String, Conn)], name: &str) -> HashMap<String, String> {
    fn add(conns: &[(String, Conn)], name: &str, out: &mut HashMap<String, String>, depth: usize) {
        let Some((_, keys)) = conns.iter().find(|(n, _)| n == name) else {
            return;
        };
        if depth > 8 {
            return;
        }
        for (key, value) in keys {
            if key == "also" {
                add(conns, value, out, depth + 1);
            }
        }
        for (key, value) in keys {
            if key != "also" {
                out.insert(key.clone(), value.clone());
            }
        }
    }
    let mut out = HashMap::new();
    add(conns, "%default", &mut out, 0);
    add(conns, name, &mut out, 0);
    out
}

pub(super) fn import(
    text: &str,
    options: &SwanctlImportOptions,
    warnings: &mut Vec<String>,
) -> Result<Profile, ProfileError> {
    let conns = parse(text, warnings);
    let names: Vec<&str> = conns
        .iter()
        .map(|(n, _)| n.as_str())
        .filter(|n| *n != "%default")
        .collect();
    let name = match &options.connection {
        Some(wanted) => names
            .iter()
            .find(|n| **n == wanted.as_str())
            .copied()
            .ok_or_else(|| {
                err(
                    F,
                    crate::trf!(
                        "connection '{wanted}' not found",
                        "conexão '{wanted}' não encontrada"
                    ),
                )
            })?,
        None => {
            let first = *names.first().ok_or_else(|| {
                err(
                    F,
                    crate::tr!("there is no 'conn' section", "não há seção 'conn'"),
                )
            })?;
            if names.len() > 1 {
                warnings.push(crate::trf!(
                    "the file has {} connections; importing '{}' (pick another one with the connection option)",
                    "o arquivo tem {} conexões; importando '{}' (escolha outra com a opção de conexão)",
                    names.len(),
                    first
                ));
            }
            first
        }
    };
    let keys: Conn = resolve(&conns, name).into_iter().collect();
    from_conn(F, name, &keys, warnings)
}

/// strongSwan/libreswan proposal syntax to the profile's: no strict `!`,
/// libreswan's `;` before the DH group and `sha2_256` spelling.
fn proposals(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|p| {
            p.trim()
                .trim_end_matches('!')
                .replace(';', "-")
                .replace("sha2_", "sha")
                .replace("sha2-", "sha")
                .to_ascii_lowercase()
        })
        .filter(|p| !p.is_empty())
        .map(|p| {
            // libreswan "modp1024" works as is; "dh2"-style groups do not.
            p.split('-')
                .map(|part| match dh_group(part) {
                    Some(n) if part.starts_with("dh") => dh_keyword(n).unwrap_or(part).to_owned(),
                    _ => part.to_owned(),
                })
                .collect::<Vec<_>>()
                .join("-")
        })
        .collect()
}

fn id(value: &str) -> Option<String> {
    let v = value.trim();
    if v.is_empty() || v.starts_with('%') {
        return None;
    }
    Some(v.trim_start_matches('@').to_owned())
}

/// The profile from one connection's keys (an `ipsec.conf` `conn`, or a
/// NetworkManager libreswan connection).
pub(super) fn from_conn(
    format: LegacyFormat,
    name: &str,
    keys: &[(String, String)],
    warnings: &mut Vec<String>,
) -> Result<Profile, ProfileError> {
    let get = |key: &str| {
        keys.iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
            .filter(|v| !v.is_empty())
    };
    let gateway = get("right")
        .filter(|r| !r.starts_with('%'))
        .ok_or_else(|| {
            err(
                format,
                crate::tr!(
                    "there is no right= with the gateway (the client is the left side)",
                    "não há right= com o gateway (o cliente é o lado left)"
                ),
            )
        })?;
    let authby = get("authby").unwrap_or("");
    let certificates = matches!(
        authby,
        "rsasig" | "rsa" | "pubkey" | "xauthrsasig" | "ecdsa" | "rsa-sha2"
    ) || get("leftcert").is_some()
        || get("leftrsasigkey").is_some_and(|v| v != "%none")
        || get("leftauth").is_some_and(|v| v.starts_with("pubkey") || v.starts_with("eap"));
    if certificates {
        return Err(err(
            format,
            crate::tr!(
                "certificate or EAP authentication cannot be imported from this file: create the \
                 profile in the app (Profiles → New)",
                "a autenticação por certificado ou EAP não pode ser importada deste arquivo: crie \
                 o perfil no app (Perfis → Novo)"
            ),
        ));
    }
    let xauth = authby == "xauthpsk"
        || get("leftauth2") == Some("xauth")
        || get("xauth") == Some("client")
        || get("leftxauthclient") == Some("yes");
    let version = match (get("keyexchange"), get("ikev2")) {
        (Some("ikev1"), _) | (_, Some("no" | "never" | "permit")) => 1,
        _ => 2,
    };
    let mut profile = base_profile(name, gateway);
    profile.ike.version = version;
    profile.ike.aggressive =
        matches!(get("aggressive"), Some("yes")) || get("aggrmode") == Some("yes");
    profile.ike.encap = get("forceencaps") == Some("yes") || get("encapsulation") == Some("yes");
    if let Some(ike) = get("ike") {
        profile.ike.proposals = proposals(ike);
    }
    if let Some(esp) = get("esp").or_else(|| get("phase2alg")) {
        profile.child.esp_proposals = proposals(esp);
    }
    profile.ike.dpd_delay_secs = get("dpddelay").and_then(duration);
    profile.ike.rekey_time_secs = get("ikelifetime").and_then(duration);
    profile.child.rekey_time_secs = get("lifetime")
        .or_else(|| get("keylife"))
        .and_then(duration);
    let action = match get("dpdaction") {
        Some("restart") => ChildAction::Restart,
        Some("hold" | "trap") => ChildAction::Trap,
        _ => ChildAction::None,
    };
    profile.child.dpd_action = action;
    profile.child.close_action = action;
    let subnets: Vec<String> = get("rightsubnet")
        .or_else(|| get("rightsubnets"))
        .map(|v| {
            v.trim_matches(['{', '}'])
                .split([',', ' '])
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    if !subnets.is_empty() {
        profile.child.remote_ts = subnets;
    }
    profile.network.request_vip = get("leftsourceip")
        .is_some_and(|v| v.starts_with("%config") || v == "%modecfg")
        || get("modecfgpull") == Some("yes")
        || get("leftmodecfgclient") == Some("yes");
    if let Some(dns) = get("modecfgdns").or_else(|| get("rightdns")) {
        profile.network.dns.servers = dns
            .split([',', ' '])
            .filter_map(|s| s.trim().parse().ok())
            .collect();
    }
    let local_id = get("leftid").and_then(id);
    if profile.ike.aggressive && xauth && local_id.as_deref().is_some_and(|i| !i.contains(':')) {
        warnings.push(crate::tr!(
            "group id imported as written (leftid): Cisco/FortiGate gateways usually expect keyid:<group>",
            "id do grupo importado como está (leftid): gateways Cisco/FortiGate costumam esperar keyid:<grupo>"
        ).to_owned());
    }
    profile.auth = AuthSettings::psk(local_id, xauth && version == 1, get("rightid").and_then(id));
    if xauth && version == 2 {
        warnings.push(
            crate::tr!(
                "XAuth only exists in IKEv1: it was left out (the connection is IKEv2)",
                "XAuth só existe no IKEv1: ficou de fora (a conexão é IKEv2)"
            )
            .to_owned(),
        );
    }
    warnings.push(
        crate::tr!(
            "the PSK lives in ipsec.secrets and was not read: you type it when connecting",
            "a PSK fica no ipsec.secrets e não foi lida: você a digita ao conectar"
        )
        .to_owned(),
    );
    Ok(profile)
}

#[cfg(test)]
mod tests {
    use crate::profiles::SwanctlImportOptions;
    use crate::profiles::legacy::{LegacyFormat, import_legacy};

    /// A strongSwan 5 ipsec.conf for a legacy VPN, FAKE values.
    const IPSEC_CONF: &str = "\
config setup
    charondebug=\"ike 1\"

conn %default
    keyexchange=ikev1
    ikelifetime=24h

conn work
    also=gateway
    aggressive=yes
    authby=xauthpsk
    xauth=client
    xauth_identity=usuario.exemplo
    left=%defaultroute
    leftid=keyid:grupo-exemplo
    leftsourceip=%config
    rightsubnet=192.0.2.0/24,198.51.100.0/24
    ike=aes256-sha1-modp1024!
    esp=aes256-sha1!
    dpddelay=30s
    dpdaction=restart
    auto=add

conn gateway
    right=vpn.example.com
    rightid=%any
";

    /// A libreswan connection (as NetworkManager's plugin writes it).
    const LIBRESWAN: &str = "\
conn office
    ikev2=never
    aggrmode=yes
    right=vpn.example.com
    leftid=@grupo
    leftxauthusername=usuario.exemplo
    leftxauthclient=yes
    leftmodecfgclient=yes
    ike=aes256-sha2_256;modp2048
    phase2alg=aes256-sha2_256
";

    #[test]
    fn a_strongswan_conn_with_default_and_also() {
        let imported = import_legacy(
            IPSEC_CONF,
            LegacyFormat::IpsecConf,
            &SwanctlImportOptions::default(),
        )
        .expect("ipsec.conf");
        let p = &imported.profile;
        assert_eq!(p.name, "work");
        assert_eq!(p.gateway, "vpn.example.com");
        assert_eq!((p.ike.version, p.ike.aggressive), (1, true));
        assert!(p.auth.uses_xauth());
        assert_eq!(p.auth.psk_id(), Some("keyid:grupo-exemplo"));
        assert_eq!(p.auth.remote_id(), None);
        assert_eq!(p.ike.proposals, ["aes256-sha1-modp1024"]);
        assert_eq!(p.child.remote_ts, ["192.0.2.0/24", "198.51.100.0/24"]);
        assert_eq!(p.ike.rekey_time_secs, Some(86_400));
        assert!(p.network.request_vip);
        assert!(
            !crate::profiles::to_json(p)
                .expect("json")
                .contains("usuario.exemplo")
        );
    }

    #[test]
    fn a_libreswan_conn_with_its_spellings() {
        let imported = import_legacy(
            LIBRESWAN,
            LegacyFormat::IpsecConf,
            &SwanctlImportOptions::default(),
        )
        .expect("libreswan");
        let p = &imported.profile;
        assert_eq!((p.ike.version, p.ike.aggressive), (1, true));
        assert_eq!(p.ike.proposals, ["aes256-sha256-modp2048"]);
        assert_eq!(p.child.esp_proposals, ["aes256-sha256"]);
        assert_eq!(p.auth.psk_id(), Some("grupo"));
        assert!(imported.warnings.iter().any(|w| w.contains("keyid:")));
    }

    #[test]
    fn certificates_are_refused() {
        let cert = IPSEC_CONF.replace("authby=xauthpsk", "authby=xauthrsasig");
        assert!(
            import_legacy(
                &cert,
                LegacyFormat::IpsecConf,
                &SwanctlImportOptions::default()
            )
            .is_err()
        );
    }
}
