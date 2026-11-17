//! Importação de um `swanctl.conf` existente para um perfil.
//!
//! Segurança: a seção `secrets` é **pulada sem ser armazenada** (o parser só
//! conta chaves de abertura e fechamento), e qualquer chave `secret` fora
//! dela é recusada. O perfil gerado nunca contém credenciais (ADR-0007), e o
//! usuário XAuth (`xauth_id`) não é copiado.
//!
//! Formato aceito (o do strongswan.conf/swanctl.conf): `nome {` … `}`,
//! `chave = valor` e comentários com `#`. `include` não é seguido (gera
//! aviso). O DNS não existe no swanctl.conf e vem das opções de importação.

use std::net::IpAddr;

use super::ProfileError;
use super::model::{
    AuthSettings, ChildAction, ChildSettings, DnsSettings, IkeSettings, NetworkSettings, Profile,
};
use super::validate::{is_slug, validate};

/// Opções da importação.
#[derive(Debug, Clone, Default)]
pub struct SwanctlImportOptions {
    /// Conexão a importar (padrão: a única, ou a primeira).
    pub connection: Option<String>,
    /// Id do perfil (padrão: derivado do nome da conexão).
    pub id: Option<String>,
    /// Nome exibido (padrão: o nome da conexão).
    pub name: Option<String>,
    /// DNS da VPN (o swanctl.conf não tem essa informação).
    pub dns: DnsSettings,
}

/// Resultado: o perfil e o que foi ignorado ou ajustado.
#[derive(Debug, Clone)]
pub struct SwanctlImport {
    /// Perfil gerado (já validado).
    pub profile: Profile,
    /// Avisos para o usuário revisar.
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct Section {
    entries: Vec<(String, Node)>,
}

#[derive(Debug, Clone)]
enum Node {
    Value(String),
    Section(Section),
}

impl Section {
    fn value(&self, key: &str) -> Option<&str> {
        self.entries.iter().find_map(|(k, n)| match n {
            Node::Value(v) if k == key => Some(v.as_str()),
            _ => None,
        })
    }

    fn section(&self, key: &str) -> Option<&Section> {
        self.entries.iter().find_map(|(k, n)| match n {
            Node::Section(s) if k == key => Some(s),
            _ => None,
        })
    }

    fn sections(&self) -> impl Iterator<Item = (&str, &Section)> {
        self.entries.iter().filter_map(|(k, n)| match n {
            Node::Section(s) => Some((k.as_str(), s)),
            Node::Value(_) => None,
        })
    }

    fn keys(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|(k, _)| k.as_str())
    }
}

fn err(msg: impl Into<String>) -> ProfileError {
    ProfileError::Swanctl(msg.into())
}

/// Remove o comentário (`#` fora de aspas).
fn strip_comment(line: &str) -> &str {
    let mut quoted = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '#' if !quoted => return &line[..i],
            _ => {}
        }
    }
    line
}

fn is_name(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '%'))
}

/// Interpreta o arquivo numa árvore de seções, pulando `secrets`.
fn parse(text: &str, warnings: &mut Vec<String>) -> Result<Section, ProfileError> {
    let mut stack: Vec<(String, Section)> = vec![(String::new(), Section::default())];
    let mut skip_depth = 0usize; // > 0 enquanto estiver dentro de `secrets`
    for (n, raw) in text.lines().enumerate() {
        let lineno = n + 1;
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        if skip_depth > 0 {
            // Dentro de `secrets`: só acompanha as chaves, sem guardar nada.
            skip_depth += line.matches('{').count();
            skip_depth = skip_depth.saturating_sub(line.matches('}').count());
            continue;
        }
        if line == "}" {
            if stack.len() == 1 {
                return Err(err(crate::trf!(
                    "line {lineno}: '}}' without an open section",
                    "linha {lineno}: '}}' sem seção aberta"
                )));
            }
            let (name, section) = stack
                .pop()
                .ok_or_else(|| err(crate::tr!("empty stack", "pilha vazia")))?;
            if let Some((_, parent)) = stack.last_mut() {
                parent.entries.push((name, Node::Section(section)));
            }
            continue;
        }
        if let Some(name) = line.strip_suffix('{').map(str::trim) {
            if !is_name(name) {
                return Err(err(crate::trf!(
                    "line {lineno}: invalid section name",
                    "linha {lineno}: nome de seção inválido"
                )));
            }
            if name == "secrets" && stack.len() == 1 {
                skip_depth = 1;
                continue;
            }
            stack.push((name.to_owned(), Section::default()));
            continue;
        }
        if let Some(rest) = line.strip_prefix("include")
            && rest.starts_with(char::is_whitespace)
        {
            warnings.push(crate::trf!(
                "line {lineno}: 'include' is not followed; import the included file directly",
                "linha {lineno}: 'include' não é seguido; importe o arquivo incluído diretamente"
            ));
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(err(crate::trf!(
                "line {lineno}: expected 'key = value', 'name {{' or '}}'",
                "linha {lineno}: esperava 'chave = valor', 'nome {{' ou '}}'"
            )));
        };
        let key = key.trim();
        if !is_name(key) {
            return Err(err(crate::trf!(
                "line {lineno}: invalid key",
                "linha {lineno}: chave inválida"
            )));
        }
        if key == "secret" {
            return Err(err(crate::trf!(
                "line {lineno}: secret outside the 'secrets' section; the file was not imported",
                "linha {lineno}: segredo fora da seção 'secrets'; o arquivo não foi importado"
            )));
        }
        let value = value.trim().trim_matches('"').to_owned();
        if let Some((_, section)) = stack.last_mut() {
            section.entries.push((key.to_owned(), Node::Value(value)));
        }
    }
    if stack.len() != 1 || skip_depth > 0 {
        return Err(err(crate::tr!(
            "section without a closing '}'",
            "seção sem '}' de fechamento"
        )));
    }
    stack
        .pop()
        .map(|(_, root)| root)
        .ok_or_else(|| err(crate::tr!("empty file", "arquivo vazio")))
}

fn list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

fn yes(value: Option<&str>) -> bool {
    matches!(value, Some("yes" | "true" | "1"))
}

/// Duração do strongSwan: número (segundos) com sufixo opcional s/m/h/d.
fn duration(value: &str) -> Option<u32> {
    let value = value.trim();
    let (number, factor) = match value.chars().last()? {
        's' => (&value[..value.len() - 1], 1),
        'm' => (&value[..value.len() - 1], 60),
        'h' => (&value[..value.len() - 1], 3600),
        'd' => (&value[..value.len() - 1], 86_400),
        _ => (value, 1),
    };
    number.trim().parse::<u32>().ok()?.checked_mul(factor)
}

fn action(value: Option<&str>, field: &str, warnings: &mut Vec<String>) -> ChildAction {
    match value {
        None | Some("none" | "clear") => ChildAction::None,
        Some("trap" | "route" | "hold") => ChildAction::Trap,
        Some("start" | "restart") => ChildAction::Restart,
        Some(other) => {
            warnings.push(crate::trf!(
                "{field} = {other} not recognized; using 'none'",
                "{field} = {other} não reconhecido; usando 'none'"
            ));
            ChildAction::None
        }
    }
}

/// Converte um nome do swanctl num id de perfil (`[a-z0-9-]`).
fn slugify(name: &str) -> String {
    let mut slug: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    while slug.contains("--") {
        slug = slug.replace("--", "-");
    }
    let slug = slug.trim_matches('-');
    let slug: String = slug.chars().take(63).collect();
    if slug.is_empty() {
        "vpn".to_owned()
    } else {
        slug
    }
}

/// Converte o conteúdo de um `swanctl.conf` num perfil.
///
/// # Errors
/// [`ProfileError::Swanctl`] se o arquivo não puder ser interpretado ou usar
/// recursos não suportados (ex.: certificados); [`ProfileError::Invalid`] se
/// o perfil resultante for inválido.
pub fn import_swanctl(
    text: &str,
    options: &SwanctlImportOptions,
) -> Result<SwanctlImport, ProfileError> {
    let mut warnings = Vec::new();
    let root = parse(text, &mut warnings)?;
    let connections = root.section("connections").ok_or_else(|| {
        err(crate::tr!(
            "there is no 'connections' section",
            "não há seção 'connections'"
        ))
    })?;
    let all: Vec<(&str, &Section)> = connections.sections().collect();
    let (conn_name, conn) = match &options.connection {
        Some(wanted) => all
            .iter()
            .find(|(name, _)| name == wanted)
            .copied()
            .ok_or_else(|| {
                err(crate::trf!(
                    "connection '{wanted}' not found",
                    "conexão '{wanted}' não encontrada"
                ))
            })?,
        None => {
            let first = *all.first().ok_or_else(|| {
                err(crate::tr!(
                    "no connection in 'connections'",
                    "nenhuma conexão em 'connections'"
                ))
            })?;
            if all.len() > 1 {
                warnings.push(crate::trf!(
                    "the file has {} connections; importing '{}' (pick another one with the connection option)",
                    "o arquivo tem {} conexões; importando '{}' (escolha outra com a opção de conexão)",
                    all.len(),
                    first.0
                ));
            }
            first
        }
    };

    let gateway = conn
        .value("remote_addrs")
        .map(list)
        .and_then(|addrs| addrs.into_iter().next())
        .ok_or_else(|| {
            err(crate::tr!(
                "the connection has no remote_addrs (gateway)",
                "a conexão não tem remote_addrs (gateway)"
            ))
        })?;
    let version = match conn.value("version") {
        Some("1" | "ikev1") => 1,
        Some("2" | "ikev2") | None => 2,
        Some("0") => {
            warnings.push(
                crate::tr!(
                    "version = 0 (any): using IKEv2",
                    "version = 0 (qualquer): usando IKEv2"
                )
                .to_owned(),
            );
            2
        }
        Some(other) => {
            return Err(err(crate::trf!(
                "version = {other} is not supported",
                "version = {other} não suportada"
            )));
        }
    };

    // Rodadas de autenticação.
    let mut xauth = false;
    let mut local_id = None;
    let mut remote_id = None;
    for (name, round) in conn.sections() {
        let auth = round.value("auth").unwrap_or("pubkey");
        if name.starts_with("local") {
            match auth {
                "psk" => local_id = round.value("id").map(str::to_owned),
                a if a.starts_with("xauth") => xauth = true,
                other => {
                    return Err(err(crate::trf!(
                        "local authentication '{other}' cannot be imported (only psk and xauth); \
                         for EAP, create the profile in the app (Profiles → New)",
                        "autenticação local '{other}' não pode ser importada (só psk e xauth); \
                         para EAP, crie o perfil no app (Perfis → Novo)"
                    )));
                }
            }
        } else if name.starts_with("remote") {
            if auth != "psk" {
                return Err(err(crate::trf!(
                    "gateway authentication '{auth}' cannot be imported (only psk); for a \
                     gateway certificate, create the profile in the app (Profiles → New)",
                    "autenticação do gateway '{auth}' não pode ser importada (só psk); para \
                     certificado no gateway, crie o perfil no app (Perfis → Novo)"
                )));
            }
            remote_id = round.value("id").map(str::to_owned);
        }
    }
    if local_id.is_none() {
        warnings.push(
            crate::tr!(
                "the PSK round has no 'id'; the PSK will apply to any identity",
                "a rodada PSK não tem 'id'; a PSK valerá para qualquer identidade"
            )
            .to_owned(),
        );
    }

    // Fase 2.
    let children = conn.section("children").ok_or_else(|| {
        err(crate::tr!(
            "the connection has no 'children'",
            "a conexão não tem 'children'"
        ))
    })?;
    let child_list: Vec<_> = children.sections().collect();
    let (child_name, child) = *child_list.first().ok_or_else(|| {
        err(crate::tr!(
            "no child in 'children'",
            "nenhum child em 'children'"
        ))
    })?;
    if child_list.len() > 1 {
        warnings.push(crate::trf!(
            "there are {} children; importing only '{child_name}'",
            "há {} children; importando só '{child_name}'",
            child_list.len()
        ));
    }
    let local_ts = child
        .value("local_ts")
        .map(list)
        .unwrap_or_default()
        .into_iter()
        .filter(|ts| ts != "dynamic")
        .collect();

    let mut child_slug = slugify(child_name);
    if !is_slug(&child_slug) {
        child_slug = "rede".to_owned();
    }
    let id = match &options.id {
        Some(id) => id.clone(),
        None => slugify(conn_name),
    };

    let known_conn = [
        "remote_addrs",
        "local_addrs",
        "version",
        "aggressive",
        "encap",
        "proposals",
        "rekey_time",
        "reauth_time",
        "over_time",
        "dpd_delay",
        "dpd_timeout",
        "vips",
        "children",
        "mobike",
        "fragmentation",
        "unique",
        "keyingtries",
        "pull",
    ];
    for key in conn.keys() {
        if !known_conn.contains(&key) && !key.starts_with("local") && !key.starts_with("remote") {
            warnings.push(crate::trf!(
                "connection setting '{key}' was ignored",
                "'{key}' da conexão foi ignorado"
            ));
        }
    }
    let known_child = [
        "esp_proposals",
        "rekey_time",
        "life_time",
        "remote_ts",
        "local_ts",
        "start_action",
        "dpd_action",
        "close_action",
        "mode",
        "updown",
        "if_id_in",
        "if_id_out",
    ];
    for key in child.keys() {
        if !known_child.contains(&key) {
            warnings.push(crate::trf!(
                "child setting '{key}' was ignored",
                "'{key}' do child foi ignorado"
            ));
        }
    }
    if child.value("updown").is_some() {
        warnings.push(
            crate::tr!(
                "'updown' was ignored (the app uses its own)",
                "'updown' foi ignorado (o app usa o próprio)"
            )
            .to_owned(),
        );
    }

    let vips = conn.value("vips").map(list).unwrap_or_default();
    let profile = Profile {
        id,
        name: options.name.clone().unwrap_or_else(|| conn_name.to_owned()),
        gateway,
        ike: IkeSettings {
            version,
            aggressive: yes(conn.value("aggressive")),
            encap: yes(conn.value("encap")),
            proposals: conn
                .value("proposals")
                .map(list)
                .unwrap_or_else(|| vec!["default".to_owned()]),
            rekey_time_secs: conn.value("rekey_time").and_then(duration),
            dpd_delay_secs: conn.value("dpd_delay").and_then(duration),
        },
        auth: AuthSettings::psk(local_id, xauth, remote_id),
        child: ChildSettings {
            name: child_slug,
            esp_proposals: child
                .value("esp_proposals")
                .map(list)
                .unwrap_or_else(|| vec!["default".to_owned()]),
            rekey_time_secs: child.value("rekey_time").and_then(duration),
            remote_ts: child.value("remote_ts").map(list).unwrap_or_default(),
            local_ts,
            dpd_action: action(child.value("dpd_action"), "dpd_action", &mut warnings),
            close_action: action(child.value("close_action"), "close_action", &mut warnings),
        },
        network: NetworkSettings {
            request_vip: vips
                .iter()
                .any(|v| v.parse::<IpAddr>().is_ok_and(|ip| ip.is_unspecified())),
            mtu: 1400,
            dns: options.dns.clone(),
        },
    };

    let issues = validate(&profile);
    if !issues.is_empty() {
        return Err(ProfileError::Invalid(issues));
    }
    Ok(SwanctlImport { profile, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mesma estrutura de um swanctl.conf real de VPN legada (tabs,
    /// comentários, rodadas local-1/local-2, seção secrets), com valores
    /// FALSOS.
    const LEGACY: &str = "\
connections {
    vpn_trabalho {
        # O IP ou domínio do gateway
        remote_addrs = 203.0.113.10


        version = 1
\tencap = yes
        aggressive = yes

\t# FASE 1 (IKE)
\t#proposals = aes128-sha1-modp1024
\tproposals = aes256-sha1-modp1024,aes128-sha1-modp1024
        rekey_time = 86400s
        dpd_delay = 30s
        #local_id = algo
        vips = 0.0.0.0

\t# Autenticação
        local-1 {
            auth = psk
            id = keyid:grupo-exemplo
        }

\tlocal-2 {
\t    auth = xauth
\t    xauth_id = \tusuario.exemplo
\t}

        remote {
            auth = psk
        }

        children {
            rede_interna {
                esp_proposals = aes256-sha1-modp1024,aes256-sha1-modp768,aes128-sha1-modp1024
                rekey_time = 43200s
                remote_ts = 192.0.2.0/24
                start_action = none
\t\t# comentário
\t\tdpd_action = restart
\t\tclose_action = start
            }
        }
    }
}

secrets {
    ike-psk {
        id = keyid:grupo-exemplo
        secret = PSK-NAO-DEVE-SER-LIDA
    }
    xauth-pessoal {
\tid = usuario.exemplo
\tsecret = \"SENHA # NAO DEVE SER LIDA\"
    }
}
";

    fn options() -> SwanctlImportOptions {
        SwanctlImportOptions {
            id: Some("trabalho".into()),
            dns: DnsSettings {
                servers: vec!["192.0.2.53".parse().expect("ip")],
                domains: vec!["~corp.example.com".into()],
                use_received: false,
                default_route: false,
            },
            ..Default::default()
        }
    }

    #[test]
    fn imports_legacy_config_like_the_validated_flow() {
        let result = import_swanctl(LEGACY, &options()).expect("import");
        let p = result.profile;
        assert_eq!(p.id, "trabalho");
        assert_eq!(p.name, "vpn_trabalho");
        assert_eq!(p.gateway, "203.0.113.10");
        assert_eq!(p.ike.version, 1);
        assert!(p.ike.aggressive && p.ike.encap);
        assert_eq!(
            p.ike.proposals,
            ["aes256-sha1-modp1024", "aes128-sha1-modp1024"]
        );
        assert_eq!(p.ike.rekey_time_secs, Some(86_400));
        assert_eq!(p.ike.dpd_delay_secs, Some(30));
        assert!(p.auth.uses_xauth());
        assert_eq!(p.auth.psk_id(), Some("keyid:grupo-exemplo"));
        assert_eq!(p.child.name, "rede-interna");
        assert_eq!(
            p.child.esp_proposals,
            [
                "aes256-sha1-modp1024",
                "aes256-sha1-modp768",
                "aes128-sha1-modp1024"
            ]
        );
        assert_eq!(p.child.rekey_time_secs, Some(43_200));
        assert_eq!(p.child.remote_ts, ["192.0.2.0/24"]);
        assert_eq!(p.child.dpd_action, ChildAction::Restart);
        assert_eq!(p.child.close_action, ChildAction::Restart);
        assert!(p.network.request_vip);
        assert_eq!(p.network.dns.domains, ["~corp.example.com"]);
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
    }

    #[test]
    fn never_keeps_secrets_or_the_xauth_user() {
        let result = import_swanctl(LEGACY, &options()).expect("import");
        let json = super::super::to_json(&result.profile).expect("json");
        for leaked in ["PSK-NAO-DEVE", "SENHA", "usuario.exemplo", "secret"] {
            assert!(!json.contains(leaked), "{leaked} vazou: {json}");
        }
        let debug = format!("{result:?}");
        assert!(!debug.contains("NAO-DEVE") && !debug.contains("SENHA"));
    }

    #[test]
    fn rejects_secret_outside_secrets_section() {
        let text = "connections {\n vpn {\n  remote_addrs = 192.0.2.1\n  secret = x\n }\n}\n";
        assert!(matches!(
            import_swanctl(text, &SwanctlImportOptions::default()),
            Err(ProfileError::Swanctl(m)) if m.contains("secret outside")
        ));
    }

    #[test]
    fn reports_unsupported_and_malformed_inputs() {
        let cert = "connections {\n v {\n  remote_addrs = 192.0.2.1\n  local { auth = pubkey\n  }\n  children {\n   c {\n    remote_ts = 192.0.2.0/24\n   }\n  }\n }\n}\n";
        assert!(import_swanctl(cert, &SwanctlImportOptions::default()).is_err());
        for bad in [
            "connections {\n",
            "}\n",
            "connections {\n linha estranha\n}\n",
            "secrets {\n x {\n",
            "nada = aqui\n",
        ] {
            assert!(
                import_swanctl(bad, &SwanctlImportOptions::default()).is_err(),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn picks_named_connection_and_warns_about_extras() {
        let text = format!("{LEGACY}\nconnections {{\n}}\n");
        // Duas seções 'connections': usa a primeira (que tem a conexão).
        assert!(import_swanctl(&text, &options()).is_ok());
        let mut opts = options();
        opts.connection = Some("inexistente".into());
        assert!(import_swanctl(LEGACY, &opts).is_err());
    }

    #[test]
    fn helpers() {
        assert_eq!(duration("86400s"), Some(86_400));
        assert_eq!(duration("30m"), Some(1_800));
        assert_eq!(duration("1h"), Some(3_600));
        assert_eq!(duration("2d"), Some(172_800));
        assert_eq!(duration("45"), Some(45));
        assert_eq!(duration("xs"), None);
        assert_eq!(slugify("VPN_Trabalho--SP"), "vpn-trabalho-sp");
        assert_eq!(slugify("___"), "vpn");
        assert_eq!(
            strip_comment("a = \"x # y\" # comentário"),
            "a = \"x # y\" "
        );
    }
}
