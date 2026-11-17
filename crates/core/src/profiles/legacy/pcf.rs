//! Cisco VPN Client profiles (`.pcf`): an INI file with a `[main]` section.
//!
//! What is used: `Description`, `Host`, `GroupName`, `DHGroup`,
//! `TunnelingMode`, `AuthType`. `GroupPwd`, `enc_GroupPwd`, `Username`,
//! `UserPassword` and `enc_UserPassword` are skipped without being read (the
//! INI parser drops secret keys), so the group password is typed when
//! connecting.

use super::{CiscoStyle, LegacyFormat, cisco_profile, dh_group, err, ini_get, parse_ini};
use crate::profiles::ProfileError;
use crate::profiles::model::Profile;

const F: LegacyFormat = LegacyFormat::CiscoPcf;

pub(super) fn import(text: &str, warnings: &mut Vec<String>) -> Result<Profile, ProfileError> {
    let entries = parse_ini(text, &[]);
    let get = |key: &str| ini_get(&entries, "main", key);
    let gateway = get("Host").ok_or_else(|| {
        err(
            F,
            crate::tr!("there is no Host (the gateway)", "não há Host (o gateway)"),
        )
    })?;
    match get("AuthType").unwrap_or("1") {
        "1" => {}
        "3" | "5" => {
            return Err(err(
                F,
                crate::tr!(
                    "certificate authentication (AuthType 3 or 5) cannot be imported from a .pcf: \
                     create the profile in the app with your certificate (Profiles → New)",
                    "a autenticação por certificado (AuthType 3 ou 5) não pode ser importada de \
                     um .pcf: crie o perfil no app com o seu certificado (Perfis → Novo)"
                ),
            ));
        }
        other => warnings.push(crate::trf!(
            "AuthType {other} not recognized; importing as group authentication",
            "AuthType {other} não reconhecido; importando como autenticação de grupo"
        )),
    }
    if get("TunnelingMode") == Some("1") {
        return Err(err(
            F,
            crate::tr!(
                "IPsec over TCP (TunnelingMode=1) is a Cisco extension the app does not support",
                "IPsec sobre TCP (TunnelingMode=1) é uma extensão da Cisco que o app não suporta"
            ),
        ));
    }
    if get("BackupServer").is_some() {
        warnings.push(
            crate::tr!(
                "the backup servers (BackupServer) are not imported: only Host",
                "os servidores reserva (BackupServer) não são importados: só o Host"
            )
            .to_owned(),
        );
    }
    let dh = match get("DHGroup") {
        Some(v) => dh_group(v).ok_or_else(|| {
            err(
                F,
                crate::trf!("DHGroup {v} not recognized", "DHGroup {v} não reconhecido"),
            )
        })?,
        None => 2,
    };
    let group = get("GroupName").map(str::to_owned);
    if group.is_none() {
        warnings.push(
            crate::tr!(
                "there is no GroupName: the PSK will apply to any identity",
                "não há GroupName: a PSK valerá para qualquer identidade"
            )
            .to_owned(),
        );
    }
    warnings.push(
        crate::tr!(
            "the group password and the username were not read: you type them when connecting",
            "a senha do grupo e o usuário não foram lidos: você os digita ao conectar"
        )
        .to_owned(),
    );
    Ok(cisco_profile(
        CiscoStyle {
            name: get("Description").unwrap_or("Cisco VPN").to_owned(),
            gateway: gateway.to_owned(),
            group,
            dh,
            pfs: None,
            force_natt: false,
            domain: None,
        },
        warnings,
    ))
}

#[cfg(test)]
mod tests {
    use crate::profiles::legacy::{LegacyFormat, import_legacy};
    use crate::profiles::{ProfileError, SwanctlImportOptions};

    /// The shape of a real Cisco VPN Client 5 export, with FAKE values (the
    /// encrypted group password is a made-up hex string).
    pub(crate) const PCF: &str = "[main]\r\n\
Description=Example Corp VPN\r\n\
Host=vpn.example.com\r\n\
AuthType=1\r\n\
GroupName=grupo-exemplo\r\n\
GroupPwd=\r\n\
enc_GroupPwd=2C6B8A0F11223344556677889900AABBCCDDEEFF\r\n\
EnableISPConnect=0\r\n\
Username=usuario.exemplo\r\n\
SaveUserPassword=0\r\n\
UserPassword=\r\n\
enc_UserPassword=\r\n\
EnableNat=1\r\n\
TunnelingMode=0\r\n\
EnableLocalLAN=0\r\n\
DHGroup=2\r\n";

    #[test]
    fn a_pcf_becomes_an_aggressive_group_xauth_profile_without_secrets() {
        let imported = import_legacy(
            PCF,
            LegacyFormat::CiscoPcf,
            &SwanctlImportOptions::default(),
        )
        .expect("pcf");
        let p = &imported.profile;
        assert_eq!(p.name, "Example Corp VPN");
        assert_eq!(p.id, "example-corp-vpn");
        assert_eq!(p.gateway, "vpn.example.com");
        assert_eq!((p.ike.version, p.ike.aggressive), (1, true));
        assert_eq!(p.auth.psk_id(), Some("keyid:grupo-exemplo"));
        assert!(p.auth.uses_xauth());
        assert_eq!(p.ike.proposals[0], "aes256-sha1-modp1024");
        let json = crate::profiles::to_json(p).expect("json");
        for secret in ["2C6B8A0F", "usuario.exemplo"] {
            assert!(!json.contains(secret), "{secret} in {json}");
        }
        assert!(imported.warnings.iter().any(|w| w.contains("not read")));
    }

    #[test]
    fn what_cannot_be_imported_says_why() {
        let cert = PCF.replace("AuthType=1", "AuthType=3");
        let e = import_legacy(
            &cert,
            LegacyFormat::CiscoPcf,
            &SwanctlImportOptions::default(),
        )
        .expect_err("cert");
        assert!(matches!(e, ProfileError::Legacy("Cisco .pcf", _)), "{e}");
        assert!(e.to_string().contains("certificate"), "{e}");
        let tcp = PCF.replace("TunnelingMode=0", "TunnelingMode=1");
        assert!(
            import_legacy(
                &tcp,
                LegacyFormat::CiscoPcf,
                &SwanctlImportOptions::default()
            )
            .is_err()
        );
        assert!(
            import_legacy(
                "[main]\nDescription=x",
                LegacyFormat::CiscoPcf,
                &SwanctlImportOptions::default()
            )
            .is_err()
        );
    }
}
