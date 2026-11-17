//! NetworkManager connections (`.nmconnection`, the keyfile format of
//! `/etc/NetworkManager/system-connections`): `[connection]`, `[vpn]` with
//! the plugin's keys, `[ipv4]` with DNS and routes, and `[vpn-secrets]`,
//! which is skipped whole without being read.
//!
//! Plugins: vpnc (the same keys as a vpnc file), strongswan (IKEv2; PSK
//! only: its certificate and EAP methods point to files, which are never
//! opened) and libreswan (the same keys as `ipsec.conf`). OpenVPN,
//! OpenConnect, L2TP, PPTP and WireGuard are not IPsec VPNs this app runs.

use std::net::IpAddr;

use super::{LegacyFormat, err, ini_get, ipsecconf, parse_ini, vpnc};
use crate::profiles::ProfileError;
use crate::profiles::model::{AuthSettings, Profile};
use crate::profiles::swanctl::list;

const F: LegacyFormat = LegacyFormat::NetworkManager;

pub(super) fn import(text: &str, warnings: &mut Vec<String>) -> Result<Profile, ProfileError> {
    let entries = parse_ini(text, &["vpn-secrets"]);
    let service = ini_get(&entries, "vpn", "service-type").ok_or_else(|| {
        err(
            F,
            crate::tr!(
                "this is not a VPN connection (no service-type in [vpn])",
                "esta não é uma conexão VPN (não há service-type em [vpn])"
            ),
        )
    })?;
    let name = ini_get(&entries, "connection", "id")
        .unwrap_or("VPN")
        .to_owned();
    let vpn: Vec<(String, String)> = entries
        .iter()
        .filter(|(s, _, _)| s == "vpn")
        .map(|(_, k, v)| (k.clone(), v.clone()))
        .collect();
    let plugin = service.rsplit('.').next().unwrap_or(service);
    let mut profile = match plugin {
        "vpnc" => {
            let mut p = vpnc::from_pairs(F, &name, &vpn, warnings)?;
            p.name.clone_from(&name);
            p
        }
        "strongswan" => strongswan(&name, &vpn, warnings)?,
        "libreswan" | "openswan" => ipsecconf::from_conn(F, &name, &vpn, warnings)?,
        other => {
            return Err(err(
                F,
                crate::trf!(
                    "the {other} plugin is not an IPsec VPN this app runs",
                    "o plugin {other} não é uma VPN IPsec que este app executa"
                ),
            ));
        }
    };
    apply_ipv4(&entries, &mut profile, warnings);
    Ok(profile)
}

/// NetworkManager's strongswan plugin: always IKEv2.
fn strongswan(
    name: &str,
    vpn: &[(String, String)],
    warnings: &mut Vec<String>,
) -> Result<Profile, ProfileError> {
    let get = |key: &str| {
        vpn.iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
            .filter(|v| !v.is_empty())
    };
    let gateway = get("address").ok_or_else(|| {
        err(
            F,
            crate::tr!(
                "there is no address (the gateway)",
                "não há address (o gateway)"
            ),
        )
    })?;
    match get("method").unwrap_or("key") {
        "psk" => {}
        method => {
            return Err(err(
                F,
                crate::trf!(
                    "the {method} method uses certificates from files, which are not imported: \
                     create the profile in the app (Profiles → New: IKEv2 with username and \
                     password, or with your certificate)",
                    "o método {method} usa certificados de arquivos, que não são importados: \
                     crie o perfil no app (Perfis → Novo: IKEv2 com usuário e senha, ou com o \
                     seu certificado)"
                ),
            ));
        }
    }
    let strip = |v: &str| v.trim_end_matches('!').to_owned();
    let proposals = |key: &str| -> Vec<String> {
        if get("proposal") == Some("yes") {
            get(key)
                .map(list)
                .unwrap_or_default()
                .iter()
                .map(|p| strip(p))
                .collect()
        } else {
            Vec::new()
        }
    };
    let ike = proposals("ike");
    let esp = proposals("esp");
    let mut profile = ipsecconf::base_profile(name, gateway);
    profile.ike.version = 2;
    profile.ike.encap = get("encap") == Some("yes");
    if !ike.is_empty() {
        profile.ike.proposals = ike;
    }
    if !esp.is_empty() {
        profile.child.esp_proposals = esp;
    }
    profile.network.request_vip = get("virtual") != Some("no");
    profile.auth = AuthSettings::psk(
        get("local-identity").map(str::to_owned),
        false,
        get("remote-identity").map(str::to_owned),
    );
    warnings.push(
        crate::tr!(
            "the PSK was not read: you type it when connecting",
            "a PSK não foi lida: você a digita ao conectar"
        )
        .to_owned(),
    );
    Ok(profile)
}

/// DNS servers, search domains and routes from `[ipv4]`.
fn apply_ipv4(
    entries: &[(String, String, String)],
    profile: &mut Profile,
    warnings: &mut Vec<String>,
) {
    let get = |key: &str| ini_get(entries, "ipv4", key);
    let split = |v: &str| -> Vec<String> {
        v.split([';', ','])
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect()
    };
    if let Some(dns) = get("dns") {
        let servers: Vec<IpAddr> = split(dns).iter().filter_map(|s| s.parse().ok()).collect();
        if !servers.is_empty() {
            profile.network.dns.servers = servers;
        }
    }
    if let Some(search) = get("dns-search") {
        profile.network.dns.domains = split(search)
            .iter()
            .map(|d| format!("~{}", d.trim_start_matches('~')))
            .collect();
    }
    // Routes only matter when the VPN does not take the default route.
    if get("never-default") == Some("true") {
        let mut routes: Vec<String> = entries
            .iter()
            .filter(|(s, k, _)| s == "ipv4" && k.starts_with("route") && !k.contains("options"))
            .filter_map(|(_, _, v)| v.split(',').next().map(|r| r.trim().to_owned()))
            .filter(|r| crate::profiles::validate::is_subnet(r))
            .collect();
        if routes.is_empty()
            && let Some(legacy) = get("routes")
        {
            routes = split(legacy)
                .into_iter()
                .filter(|r| crate::profiles::validate::is_subnet(r))
                .collect();
        }
        if routes.is_empty() {
            warnings.push(
                crate::tr!(
                    "the connection does not take the default route, but lists no routes: the \
                     networks come from the gateway",
                    "a conexão não usa a rota padrão, mas não lista rotas: as redes vêm do gateway"
                )
                .to_owned(),
            );
        } else {
            profile.child.remote_ts = routes;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::profiles::SwanctlImportOptions;
    use crate::profiles::legacy::{LegacyFormat, import_legacy};

    /// A NetworkManager vpnc connection with FAKE values.
    const NM_VPNC: &str = "\
[connection]
id=Example VPN
uuid=00000000-0000-4000-8000-000000000000
type=vpn

[vpn]
IPSec gateway=vpn.example.com
IPSec ID=grupo-exemplo
IPSec secret-flags=1
Xauth username=usuario.exemplo
Xauth password-flags=2
NAT Traversal Mode=natt
IKE DH Group=dh2
service-type=org.freedesktop.NetworkManager.vpnc

[vpn-secrets]
IPSec secret=segredo-falso
Xauth password=senha-falsa

[ipv4]
dns=192.0.2.53;
dns-search=corp.example.com;
method=auto
never-default=true
route1=192.0.2.0/24
route2=198.51.100.0/24,0.0.0.0
";

    /// A NetworkManager strongswan connection with FAKE values.
    const NM_STRONGSWAN: &str = "\
[connection]
id=Office IKEv2
type=vpn

[vpn]
address=vpn.example.com
method=psk
user=usuario.exemplo
virtual=yes
encap=yes
proposal=yes
ike=aes256gcm16-prfsha256-curve25519!
esp=aes256gcm16!
remote-identity=vpn.example.com
service-type=org.freedesktop.NetworkManager.strongswan

[vpn-secrets]
password=psk-falsa
";

    #[test]
    fn a_vpnc_connection_with_dns_and_routes() {
        let imported = import_legacy(
            NM_VPNC,
            LegacyFormat::NetworkManager,
            &SwanctlImportOptions::default(),
        )
        .expect("nm vpnc");
        let p = &imported.profile;
        assert_eq!(p.name, "Example VPN");
        assert_eq!(p.auth.psk_id(), Some("keyid:grupo-exemplo"));
        assert_eq!(p.child.remote_ts, ["192.0.2.0/24", "198.51.100.0/24"]);
        assert_eq!(p.network.dns.servers.len(), 1);
        assert_eq!(p.network.dns.domains, ["~corp.example.com"]);
        let json = crate::profiles::to_json(p).expect("json");
        for secret in ["segredo-falso", "senha-falsa", "usuario.exemplo"] {
            assert!(!json.contains(secret), "{secret}");
        }
    }

    #[test]
    fn a_strongswan_connection_is_ikev2_with_its_proposals() {
        let imported = import_legacy(
            NM_STRONGSWAN,
            LegacyFormat::NetworkManager,
            &SwanctlImportOptions::default(),
        )
        .expect("nm strongswan");
        let p = &imported.profile;
        assert_eq!(p.ike.version, 2);
        assert!(p.ike.encap);
        assert_eq!(p.ike.proposals, ["aes256gcm16-prfsha256-curve25519"]);
        assert_eq!(p.child.esp_proposals, ["aes256gcm16"]);
        assert_eq!(p.auth.remote_id(), Some("vpn.example.com"));
        assert!(
            !crate::profiles::to_json(p)
                .expect("json")
                .contains("psk-falsa")
        );
    }

    #[test]
    fn certificates_and_other_vpns_are_refused() {
        let eap = NM_STRONGSWAN.replace("method=psk", "method=eap");
        assert!(
            import_legacy(
                &eap,
                LegacyFormat::NetworkManager,
                &SwanctlImportOptions::default()
            )
            .is_err()
        );
        let openvpn = NM_VPNC.replace(
            "service-type=org.freedesktop.NetworkManager.vpnc",
            "service-type=org.freedesktop.NetworkManager.openvpn",
        );
        let e = import_legacy(
            &openvpn,
            LegacyFormat::NetworkManager,
            &SwanctlImportOptions::default(),
        )
        .expect_err("openvpn");
        assert!(e.to_string().contains("openvpn"), "{e}");
    }
}
