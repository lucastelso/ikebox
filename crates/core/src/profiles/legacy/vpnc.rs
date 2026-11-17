//! vpnc configuration (`/etc/vpnc/*.conf`): one `Key words value` per line.
//! NetworkManager's vpnc plugin stores the same keys as `key=value`, so the
//! mapping ([`from_pairs`]) is shared.
//!
//! What is used: `IPSec gateway`, `IPSec ID`, `IKE DH Group`, `Perfect
//! Forward Secrecy`, `NAT Traversal Mode`, `IKE Authmode`, `Domain`.
//! `IPSec secret`, `IPSec obfuscated secret`, `Xauth username` and `Xauth
//! password` are skipped without being read.

use super::{CiscoStyle, LegacyFormat, cisco_profile, dh_group, err, is_secret_key};
use crate::profiles::ProfileError;
use crate::profiles::model::Profile;

/// The keys vpnc knows that matter here, longest first (so that `IPSec
/// obfuscated secret` is not taken for `IPSec ID`).
const KEYS: &[&str] = &[
    "Cisco UDP Encapsulation Port",
    "Perfect Forward Secrecy",
    "IPSec obfuscated secret",
    "NAT Traversal Mode",
    "Enable no encryption",
    "Enable Single DES",
    "Xauth password",
    "Xauth username",
    "IPSec gateway",
    "IKE DH Group",
    "IKE Authmode",
    "IPSec secret",
    "IPSec ID",
    "Domain",
    "Vendor",
];

pub(super) fn import(text: &str, warnings: &mut Vec<String>) -> Result<Profile, ProfileError> {
    let mut pairs = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some(key) = KEYS.iter().find(|k| {
            line.len() > k.len()
                && line[..k.len()].eq_ignore_ascii_case(k)
                && line[k.len()..].starts_with(char::is_whitespace)
        }) else {
            continue;
        };
        // A secret's value is never copied out of the file text.
        if is_secret_key(key) {
            continue;
        }
        pairs.push(((*key).to_owned(), line[key.len()..].trim().to_owned()));
    }
    from_pairs(LegacyFormat::Vpnc, "vpnc", &pairs, warnings)
}

/// The profile from vpnc's keys (from a vpnc file or a NetworkManager vpnc
/// connection). Secret keys must already be gone.
pub(super) fn from_pairs(
    format: LegacyFormat,
    default_name: &str,
    pairs: &[(String, String)],
    warnings: &mut Vec<String>,
) -> Result<Profile, ProfileError> {
    let get = |key: &str| {
        pairs
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
            .filter(|v| !v.is_empty())
    };
    let gateway = get("IPSec gateway").ok_or_else(|| {
        err(
            format,
            crate::tr!(
                "there is no IPSec gateway (the gateway)",
                "não há IPSec gateway (o gateway)"
            ),
        )
    })?;
    match get("IKE Authmode").unwrap_or("psk") {
        "psk" => {}
        "cert" | "hybrid" => {
            return Err(err(
                format,
                crate::tr!(
                    "certificate authentication (IKE Authmode cert or hybrid) cannot be imported: \
                     create the profile in the app with your certificate (Profiles → New)",
                    "a autenticação por certificado (IKE Authmode cert ou hybrid) não pode ser \
                     importada: crie o perfil no app com o seu certificado (Perfis → Novo)"
                ),
            ));
        }
        other => warnings.push(crate::trf!(
            "IKE Authmode {other} not recognized; importing as psk",
            "IKE Authmode {other} não reconhecido; importando como psk"
        )),
    }
    let mut group = |key: &str, default: Option<u32>| -> Result<Option<u32>, ProfileError> {
        match get(key) {
            None => Ok(default),
            Some(v) if matches!(v, "nopfs" | "server") => {
                if v == "server" {
                    warnings_server(warnings);
                }
                Ok(None)
            }
            Some(v) => dh_group(v).map(Some).ok_or_else(|| {
                err(
                    format,
                    crate::trf!("{key} {v} not recognized", "{key} {v} não reconhecido"),
                )
            }),
        }
    };
    let dh = group("IKE DH Group", Some(2))?.unwrap_or(2);
    let pfs = group("Perfect Forward Secrecy", None)?;
    let force_natt = match get("NAT Traversal Mode") {
        Some("force-natt") => true,
        Some("cisco-udp") => {
            return Err(err(
                format,
                crate::tr!(
                    "Cisco's proprietary UDP encapsulation (cisco-udp) is not supported: try natt",
                    "o encapsulamento UDP proprietário da Cisco (cisco-udp) não é suportado: \
                     tente natt"
                ),
            ));
        }
        _ => false,
    };
    if get("Enable Single DES").is_some_and(|v| v != "0") || get("Enable no encryption").is_some() {
        warnings.push(
            crate::tr!(
                "single DES or no encryption was allowed in the file: the profile does not offer \
                 them",
                "o arquivo permitia DES simples ou nenhuma criptografia: o perfil não as oferece"
            )
            .to_owned(),
        );
    }
    let group_name = get("IPSec ID").map(str::to_owned);
    warnings.push(
        crate::tr!(
            "the group secret and the XAuth username and password were not read: you type them \
             when connecting",
            "o segredo do grupo e o usuário e senha XAuth não foram lidos: você os digita ao \
             conectar"
        )
        .to_owned(),
    );
    Ok(cisco_profile(
        CiscoStyle {
            name: group_name
                .clone()
                .unwrap_or_else(|| default_name.to_owned()),
            gateway: gateway.to_owned(),
            group: group_name,
            dh,
            pfs,
            force_natt,
            domain: get("Domain").map(str::to_owned),
        },
        warnings,
    ))
}

fn warnings_server(warnings: &mut Vec<String>) {
    warnings.push(
        crate::tr!(
            "PFS \"server\" (whatever the gateway says) cannot be expressed: phase 2 is offered \
             without PFS; add a DH group in the editor if the gateway requires one",
            "PFS \"server\" (o que o gateway disser) não tem equivalente: a fase 2 é oferecida \
             sem PFS; acrescente um grupo DH no editor se o gateway exigir"
        )
        .to_owned(),
    );
}

#[cfg(test)]
mod tests {
    use crate::profiles::SwanctlImportOptions;
    use crate::profiles::legacy::{LegacyFormat, import_legacy};

    /// A vpnc file as distributions document it, with FAKE values.
    const VPNC: &str = "\
# Work VPN
IPSec gateway vpn.example.com
IPSec ID grupo-exemplo
IPSec obfuscated secret 0123456789ABCDEF0123456789ABCDEF
IPSec secret segredo-falso
Xauth username usuario.exemplo
Xauth password senha-falsa
IKE DH Group dh5
Perfect Forward Secrecy dh5
NAT Traversal Mode force-natt
Domain corp.example.com
";

    #[test]
    fn a_vpnc_file_becomes_a_profile_without_secrets() {
        let imported = import_legacy(VPNC, LegacyFormat::Vpnc, &SwanctlImportOptions::default())
            .expect("vpnc");
        let p = &imported.profile;
        assert_eq!(p.gateway, "vpn.example.com");
        assert_eq!(p.auth.psk_id(), Some("keyid:grupo-exemplo"));
        assert!(p.ike.encap);
        assert_eq!(p.ike.proposals[0], "aes256-sha1-modp1536");
        assert_eq!(p.child.esp_proposals[0], "aes256-sha1-modp1536");
        assert_eq!(p.network.dns.domains, ["~corp.example.com"]);
        let json = crate::profiles::to_json(p).expect("json");
        for secret in [
            "0123456789ABCDEF",
            "segredo-falso",
            "usuario.exemplo",
            "senha-falsa",
        ] {
            assert!(!json.contains(secret), "{secret}");
        }
    }

    #[test]
    fn hybrid_and_cisco_udp_are_refused() {
        let hybrid = format!("{VPNC}IKE Authmode hybrid\n");
        assert!(
            import_legacy(
                &hybrid,
                LegacyFormat::Vpnc,
                &SwanctlImportOptions::default()
            )
            .is_err()
        );
        let udp = VPNC.replace("force-natt", "cisco-udp");
        assert!(import_legacy(&udp, LegacyFormat::Vpnc, &SwanctlImportOptions::default()).is_err());
    }
}
