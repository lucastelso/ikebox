//! FortiClient configuration backups (XML): the IPsec connections under
//! `forticlient_configuration/vpn/ipsecvpn/connections/connection`, with
//! `ike_settings` and `ipsec_settings`.
//!
//! The element names follow FortiClient's documented XML (FortiClient EMS
//! "IPsec VPN" settings); the tests use files written from that
//! documentation, not real exports. Elements that hold secrets or the
//! username (`preshared_key`, `auth_key`, `xauth/username`, `password`) are
//! never looked at. The XML parser refuses DTDs (no entity expansion).

use roxmltree::{Document, Node, ParsingOptions};

use super::{LegacyFormat, dh_group, dh_keyword, err};
use crate::profiles::ProfileError;
use crate::profiles::model::{AuthSettings, ChildAction, Profile};

const F: LegacyFormat = LegacyFormat::FortiClient;

fn child<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Option<Node<'a, 'input>> {
    node.children().find(|c| c.has_tag_name(name))
}

fn text<'a>(node: Node<'a, '_>, path: &[&str]) -> Option<&'a str> {
    let mut current = node;
    for name in path {
        current = child(current, name)?;
    }
    current.text().map(str::trim).filter(|t| !t.is_empty())
}

/// `AES256|SHA256` with a DH group: a strongSwan proposal. IKE proposals
/// need a PRF when the cipher is AEAD.
fn proposal(value: &str, dh: Option<&str>, ike: bool) -> Option<String> {
    let (cipher, hash) = value.split_once('|').unwrap_or((value, ""));
    let cipher = match cipher.trim().to_ascii_uppercase().as_str() {
        "DES" => "des",
        "3DES" => "3des",
        "AES128" => "aes128",
        "AES192" => "aes192",
        "AES256" => "aes256",
        "AES128GCM" => "aes128gcm16",
        "AES256GCM" => "aes256gcm16",
        "CHACHA20POLY1305" => "chacha20poly1305",
        _ => return None,
    };
    let hash = match hash.trim().to_ascii_uppercase().as_str() {
        "" => None,
        "MD5" => Some("md5"),
        "SHA1" => Some("sha1"),
        "SHA256" => Some("sha256"),
        "SHA384" => Some("sha384"),
        "SHA512" => Some("sha512"),
        _ => return None,
    };
    let aead = cipher.contains("gcm") || cipher.starts_with("chacha");
    let mut parts = vec![cipher.to_owned()];
    match (aead, hash, ike) {
        (true, h, true) => parts.push(format!("prf{}", h.unwrap_or("sha256"))),
        (false, Some(h), _) => parts.push(h.to_owned()),
        _ => {}
    }
    if let Some(dh) = dh {
        parts.push(dh.to_owned());
    }
    Some(parts.join("-"))
}

/// `192.168.1.0` + `255.255.255.0` → `192.168.1.0/24`.
fn cidr(addr: &str, mask: &str) -> Option<String> {
    let addr: std::net::Ipv4Addr = addr.trim().parse().ok()?;
    let mask: std::net::Ipv4Addr = mask.trim().parse().ok()?;
    let bits = u32::from(mask);
    let prefix = bits.leading_ones();
    (bits.count_ones() == prefix).then(|| format!("{addr}/{prefix}"))
}

pub(super) fn import(text_in: &str, warnings: &mut Vec<String>) -> Result<Profile, ProfileError> {
    let options = ParsingOptions {
        allow_dtd: false,
        ..ParsingOptions::default()
    };
    let doc = Document::parse_with_options(text_in, options).map_err(|e| {
        err(
            F,
            crate::trf!("not a valid XML file: {e}", "não é um XML válido: {e}"),
        )
    })?;
    let connections: Vec<Node> = doc
        .descendants()
        .filter(|n| {
            n.has_tag_name("connection") && n.ancestors().any(|a| a.has_tag_name("ipsecvpn"))
        })
        .collect();
    let conn = *connections.first().ok_or_else(|| {
        err(
            F,
            crate::tr!(
                "there is no IPsec VPN connection",
                "não há conexão VPN IPsec"
            ),
        )
    })?;
    if connections.len() > 1 {
        warnings.push(crate::trf!(
            "the file has {} IPsec connections; importing the first",
            "o arquivo tem {} conexões IPsec; importando a primeira",
            connections.len()
        ));
    }
    let ike = child(conn, "ike_settings").ok_or_else(|| {
        err(
            F,
            crate::tr!(
                "the connection has no ike_settings",
                "a conexão não tem ike_settings"
            ),
        )
    })?;
    let gateway = text(ike, &["server"]).ok_or_else(|| {
        err(
            F,
            crate::tr!(
                "there is no server (the gateway)",
                "não há server (o gateway)"
            ),
        )
    })?;
    let gateway = gateway.rsplit_once(':').map_or(gateway, |(host, port)| {
        if port.chars().all(|c| c.is_ascii_digit()) {
            host
        } else {
            gateway
        }
    });
    if let Some(method) = text(ike, &["authentication_method"])
        && !method.to_ascii_lowercase().contains("preshared")
    {
        return Err(err(
            F,
            crate::trf!(
                "the {method} authentication cannot be imported: create the profile in the app \
                 with your certificate (Profiles → New)",
                "a autenticação {method} não pode ser importada: crie o perfil no app com o seu \
                 certificado (Perfis → Novo)"
            ),
        ));
    }
    let groups = |node: Node| -> Vec<&'static str> {
        text(node, &["dhgroup"])
            .unwrap_or("")
            .split([';', ',', ' '])
            .filter_map(dh_group)
            .filter_map(dh_keyword)
            .collect()
    };
    let proposals = |node: Node, dh: &[&'static str], is_ike: bool| -> Vec<String> {
        let list: Vec<&str> = child(node, "proposals")
            .map(|p| {
                p.children()
                    .filter(|c| c.has_tag_name("proposal"))
                    .filter_map(|c| c.text().map(str::trim))
                    .collect()
            })
            .unwrap_or_default();
        let mut out = Vec::new();
        for value in list {
            if dh.is_empty() {
                out.extend(proposal(value, None, is_ike));
            } else {
                for group in dh {
                    out.extend(proposal(value, Some(group), is_ike));
                }
            }
        }
        out
    };
    let ike_dh = groups(ike);
    let mut profile =
        super::ipsecconf::base_profile(text(conn, &["name"]).unwrap_or("FortiClient VPN"), gateway);
    profile.ike.version = 1;
    profile.ike.aggressive =
        text(ike, &["mode"]).is_some_and(|m| m.eq_ignore_ascii_case("aggressive"));
    let ike_proposals = proposals(ike, &ike_dh, true);
    if !ike_proposals.is_empty() {
        profile.ike.proposals = ike_proposals;
    }
    profile.ike.rekey_time_secs = text(ike, &["key_life"]).and_then(|v| v.parse().ok());
    if text(ike, &["dpd"]) == Some("1") {
        profile.ike.dpd_delay_secs = Some(30);
        profile.child.dpd_action = ChildAction::Restart;
        profile.child.close_action = ChildAction::Restart;
    }
    profile.network.request_vip = text(ike, &["mode_config"]) != Some("0");
    let xauth = text(ike, &["xauth", "enabled"]) == Some("1");
    profile.auth = AuthSettings::psk(text(ike, &["localid"]).map(str::to_owned), xauth, None);

    if let Some(ipsec) = child(conn, "ipsec_settings") {
        let pfs = groups(ipsec);
        let esp = proposals(ipsec, &pfs[..pfs.len().min(1)], false);
        if !esp.is_empty() {
            profile.child.esp_proposals = esp;
        }
        profile.child.rekey_time_secs =
            text(ipsec, &["key_life_seconds"]).and_then(|v| v.parse().ok());
        let networks: Vec<String> = child(ipsec, "remote_networks")
            .map(|n| {
                n.children()
                    .filter(|c| c.has_tag_name("network"))
                    .filter_map(|c| cidr(text(c, &["addr"])?, text(c, &["mask"])?))
                    .collect()
            })
            .unwrap_or_default();
        if !networks.is_empty() {
            profile.child.remote_ts = networks;
        }
    }
    warnings.push(
        crate::tr!(
            "the preshared key and the XAuth username and password were not read: you type them \
             when connecting",
            "a chave pré-compartilhada e o usuário e senha XAuth não foram lidos: você os digita \
             ao conectar"
        )
        .to_owned(),
    );
    Ok(profile)
}

#[cfg(test)]
mod tests {
    use crate::profiles::SwanctlImportOptions;
    use crate::profiles::legacy::{LegacyFormat, import_legacy};

    /// Written from FortiClient's documented XML, with FAKE values.
    const FORTICLIENT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<forticlient_configuration>
  <vpn>
    <ipsecvpn>
      <connections>
        <connection>
          <name>Example Forti</name>
          <type>manual</type>
          <ike_settings>
            <server>vpn.example.com</server>
            <authentication_method>Preshared Key</authentication_method>
            <preshared_key>Enc 0011223344556677</preshared_key>
            <mode>aggressive</mode>
            <dhgroup>5;14</dhgroup>
            <key_life>28800</key_life>
            <localid>grupo-exemplo</localid>
            <nat_traversal>1</nat_traversal>
            <mode_config>1</mode_config>
            <dpd>1</dpd>
            <xauth>
              <enabled>1</enabled>
              <username>Enc usuario-cifrado</username>
              <password></password>
            </xauth>
            <proposals>
              <proposal>AES256|SHA256</proposal>
              <proposal>AES256GCM|SHA256</proposal>
            </proposals>
          </ike_settings>
          <ipsec_settings>
            <remote_networks>
              <network><addr>192.0.2.0</addr><mask>255.255.255.0</mask></network>
            </remote_networks>
            <dhgroup>14</dhgroup>
            <key_life_seconds>3600</key_life_seconds>
            <proposals>
              <proposal>AES256|SHA256</proposal>
            </proposals>
          </ipsec_settings>
        </connection>
      </connections>
    </ipsecvpn>
  </vpn>
</forticlient_configuration>
"#;

    #[test]
    fn a_forticlient_connection_becomes_a_profile_without_secrets() {
        let imported = import_legacy(
            FORTICLIENT,
            LegacyFormat::FortiClient,
            &SwanctlImportOptions::default(),
        )
        .expect("forticlient");
        let p = &imported.profile;
        assert_eq!(p.name, "Example Forti");
        assert_eq!((p.ike.version, p.ike.aggressive), (1, true));
        assert_eq!(
            p.ike.proposals,
            [
                "aes256-sha256-modp1536",
                "aes256-sha256-modp2048",
                "aes256gcm16-prfsha256-modp1536",
                "aes256gcm16-prfsha256-modp2048"
            ]
        );
        assert_eq!(p.child.esp_proposals, ["aes256-sha256-modp2048"]);
        assert_eq!(p.child.remote_ts, ["192.0.2.0/24"]);
        assert_eq!(p.auth.psk_id(), Some("grupo-exemplo"));
        assert!(p.auth.uses_xauth());
        assert_eq!(p.child.rekey_time_secs, Some(3600));
        let json = crate::profiles::to_json(p).expect("json");
        for secret in ["0011223344556677", "usuario-cifrado"] {
            assert!(!json.contains(secret), "{secret}");
        }
    }

    #[test]
    fn certificates_and_dtds_are_refused() {
        let cert = FORTICLIENT.replace("Preshared Key", "X509 Certificate");
        assert!(
            import_legacy(
                &cert,
                LegacyFormat::FortiClient,
                &SwanctlImportOptions::default()
            )
            .is_err()
        );
        let dtd = format!(
            "<!DOCTYPE x [<!ENTITY a \"aaaa\">]>\n{}",
            FORTICLIENT.trim_start_matches("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n")
        );
        assert!(
            import_legacy(
                &dtd,
                LegacyFormat::FortiClient,
                &SwanctlImportOptions::default()
            )
            .is_err()
        );
    }
}
