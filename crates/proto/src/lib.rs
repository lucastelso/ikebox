//! Protocolo entre o app (usuário) e o `ipsec-client-helper` (root), ADR-0014.
//!
//! Uma requisição e uma resposta por conexão, cada uma numa linha JSON.
//!
//! **Princípio de segurança:** o cliente nunca manda caminhos, comandos nem
//! segredos. Ele só manda números validados e o id do perfil (um rótulo). O
//! helper decide sozinho o que executar, onde e com qual sandbox. As mesmas
//! regras de validação valem dos dois lados (este crate).

use std::net::IpAddr;

use serde::{Deserialize, Serialize};

/// Socket do helper (criado pelo systemd, ativação por socket).
pub const HELPER_SOCKET: &str = "/run/ipsec-client-helper.sock";
/// Ação do polkit autorizada para conectar/desconectar.
pub const POLKIT_ACTION: &str = "io.github.lucastelso.ipsecclient.connect";
/// Unidade transitória do systemd que roda o engine.
pub const ENGINE_UNIT: &str = "ipsec-client-engine";
/// Raiz dos diretórios de socket VICI por usuário (`<raiz>/<uid>/charon.vici`).
pub const SOCKET_ROOT: &str = "/run/ipsec-client";
/// Nome do arquivo do socket VICI.
pub const VICI_SOCKET_FILE: &str = "charon.vici";
/// Tamanho máximo de uma linha do protocolo.
pub const MAX_LINE: usize = 64 * 1024;
/// Máximo de linhas de log devolvidas.
pub const MAX_LOG_LINES: u32 = 2000;
/// Máximo de servidores DNS fixos (o mesmo limite do perfil).
pub const MAX_DNS_SERVERS: usize = 8;
/// Máximo de domínios DNS (o mesmo limite do perfil).
pub const MAX_DNS_DOMAINS: usize = 16;

/// Diretório do socket VICI de um usuário.
#[must_use]
pub fn socket_dir(uid: u32) -> String {
    format!("{SOCKET_ROOT}/{uid}")
}

/// Caminho do socket VICI de um usuário.
#[must_use]
pub fn vici_socket(uid: u32) -> String {
    format!("{}/{VICI_SOCKET_FILE}", socket_dir(uid))
}

/// Requisição ao helper.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Request {
    /// O helper está vivo? Devolve a versão.
    Ping,
    /// Sobe o engine (para um anterior do mesmo usuário, se houver).
    Start(StartParams),
    /// Para o engine.
    Stop,
    /// Estado do engine.
    Status,
    /// Últimas linhas do log do engine.
    Logs {
        /// Quantas linhas (limitado a [`MAX_LOG_LINES`]).
        tail: u32,
    },
}

/// Parâmetros não sensíveis do engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartParams {
    /// Interface XFRM.
    pub iface: String,
    /// `if_id` das SAs/políticas (≠ 0).
    pub if_id: u32,
    /// MTU da interface.
    pub mtu: u16,
    /// Nível de log do charon (0..=2).
    pub log_level: u8,
    /// Id do perfil (rótulo da sessão).
    pub profile_id: String,
    /// DNS da sessão, aplicado pelo `updown` na interface da VPN. Vai pelo
    /// helper, e não pelo VICI, porque o comando do `updown` é fixado pelo
    /// root (ADR-0019).
    #[serde(default)]
    pub dns: DnsParams,
}

/// DNS da sessão (dados validados; nunca viram texto de shell).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DnsParams {
    /// Servidores fixos.
    pub servers: Vec<IpAddr>,
    /// Domínios (`dominio`, `~dominio` ou `~.`).
    pub domains: Vec<String>,
    /// Usar também os DNS enviados pelo gateway.
    pub use_received: bool,
    /// Usar estes DNS também fora dos domínios.
    pub default_route: bool,
}

impl DnsParams {
    /// Valida com as mesmas regras do perfil e do `updown`.
    ///
    /// # Errors
    /// Descrição do primeiro problema.
    pub fn validate(&self) -> Result<(), String> {
        if self.servers.len() > MAX_DNS_SERVERS {
            return Err(format!("no máximo {MAX_DNS_SERVERS} servidores DNS"));
        }
        if self.domains.len() > MAX_DNS_DOMAINS {
            return Err(format!("no máximo {MAX_DNS_DOMAINS} domínios DNS"));
        }
        if !self.domains.iter().all(|d| valid_dns_domain(d)) {
            return Err("domínio DNS inválido".to_owned());
        }
        Ok(())
    }
}

impl StartParams {
    /// Valida com as mesmas regras do `entrypoint.sh`.
    ///
    /// # Errors
    /// Descrição do primeiro campo inválido.
    pub fn validate(&self) -> Result<(), String> {
        if !valid_iface(&self.iface) {
            return Err("nome de interface inválido".to_owned());
        }
        if self.if_id == 0 {
            return Err("if_id não pode ser 0".to_owned());
        }
        if !(576..=9000).contains(&self.mtu) {
            return Err("MTU fora de 576..=9000".to_owned());
        }
        if self.log_level > 2 {
            return Err("nível de log acima de 2".to_owned());
        }
        if !valid_profile_id(&self.profile_id) {
            return Err("id de perfil inválido".to_owned());
        }
        self.dns.validate()
    }
}

/// Estado do engine.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineStatus {
    /// A unidade está ativa?
    pub running: bool,
    /// Perfil da sessão.
    pub profile_id: Option<String>,
    /// uid de quem iniciou.
    pub owner_uid: Option<u32>,
}

/// Resposta do helper.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Response {
    /// Resposta ao `Ping`.
    Pong {
        /// Versão do helper.
        version: String,
    },
    /// Engine pronto; o socket VICI aceita conexão.
    Started {
        /// Caminho do socket VICI.
        socket: String,
    },
    /// Engine parado.
    Stopped {
        /// Havia um engine rodando?
        was_running: bool,
    },
    /// Estado.
    Status(EngineStatus),
    /// Log.
    Logs {
        /// Linhas.
        lines: Vec<String>,
    },
    /// Falha (mensagem para o usuário, pt-BR).
    Error {
        /// Mensagem.
        message: String,
    },
}

/// Erro de (de)codificação.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeError(pub String);

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DecodeError {}

/// Serializa uma mensagem numa linha (com `\n`).
///
/// # Errors
/// Só em casos patológicos do serde.
pub fn encode<T: Serialize>(message: &T) -> Result<String, DecodeError> {
    let mut line = serde_json::to_string(message).map_err(|e| DecodeError(e.to_string()))?;
    line.push('\n');
    Ok(line)
}

/// Lê uma mensagem de uma linha (limite [`MAX_LINE`]).
///
/// # Errors
/// Linha grande demais ou JSON inválido/desconhecido.
pub fn decode<T: for<'de> Deserialize<'de>>(line: &str) -> Result<T, DecodeError> {
    if line.len() > MAX_LINE {
        return Err(DecodeError("mensagem grande demais".to_owned()));
    }
    serde_json::from_str(line.trim_end())
        .map_err(|e| DecodeError(format!("mensagem inválida: {e}")))
}

/// `^[a-z][a-z0-9_-]{0,14}$` (IFNAMSIZ - 1).
#[must_use]
pub fn valid_iface(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some('a'..='z'))
        && name.len() <= 15
        && chars.all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_' | '-'))
}

/// `^[a-z0-9][a-z0-9-]{0,62}$`.
#[must_use]
pub fn valid_profile_id(id: &str) -> bool {
    let mut chars = id.chars();
    matches!(chars.next(), Some('a'..='z' | '0'..='9'))
        && id.len() <= 63
        && chars.all(|c| matches!(c, 'a'..='z' | '0'..='9' | '-'))
}

/// Domínio para o systemd-resolved: `~.`, `dominio` ou `~dominio` (rótulos
/// `[A-Za-z0-9-]`, sem hífen nas pontas, até 253 caracteres).
#[must_use]
pub fn valid_dns_domain(s: &str) -> bool {
    if s == "~." {
        return true;
    }
    let name = s.strip_prefix('~').unwrap_or(s);
    let name = name.strip_suffix('.').unwrap_or(name);
    let label = |l: &str| {
        !l.is_empty()
            && l.len() <= 63
            && !l.starts_with('-')
            && !l.ends_with('-')
            && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    };
    !name.is_empty() && name.len() <= 253 && name.split('.').all(label)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> StartParams {
        StartParams {
            iface: "ipsc0".into(),
            if_id: 42,
            mtu: 1400,
            log_level: 1,
            profile_id: "trabalho".into(),
            dns: DnsParams {
                servers: vec!["192.0.2.53".parse().expect("ip")],
                domains: vec!["~corp.example.com".into(), "~.".into()],
                use_received: true,
                default_route: false,
            },
        }
    }

    #[test]
    fn roundtrip_requests_and_responses() {
        for req in [
            Request::Ping,
            Request::Start(params()),
            Request::Stop,
            Request::Status,
            Request::Logs { tail: 10 },
        ] {
            let line = encode(&req).expect("encode");
            assert!(line.ends_with('\n') && !line[..line.len() - 1].contains('\n'));
            assert_eq!(decode::<Request>(&line).expect("decode"), req);
        }
        let resp = Response::Started {
            socket: vici_socket(1000),
        };
        assert_eq!(
            decode::<Response>(&encode(&resp).expect("enc")).expect("dec"),
            resp
        );
        assert_eq!(vici_socket(1000), "/run/ipsec-client/1000/charon.vici");
    }

    #[test]
    fn dns_is_optional_on_the_wire() {
        // Cliente sem o campo `dns` (anterior à 0.1.1): DNS vazio.
        let old =
            r#"{"op":"start","iface":"ipsc0","if_id":1,"mtu":1400,"log_level":1,"profile_id":"x"}"#;
        let Ok(Request::Start(p)) = decode::<Request>(old) else {
            panic!("start sem dns deveria decodificar");
        };
        assert_eq!(p.dns, DnsParams::default());
        // Servidor que não é IP é recusado já na decodificação.
        let bad = r#"{"op":"start","iface":"ipsc0","if_id":1,"mtu":1400,"log_level":1,"profile_id":"x","dns":{"servers":["a;b"],"domains":[],"use_received":false,"default_route":false}}"#;
        assert!(decode::<Request>(bad).is_err());
    }

    #[test]
    fn dns_domains() {
        for ok in [
            "~.",
            "corp.example.com",
            "~corp.example.com",
            "~corp",
            "corp.example.com.",
        ] {
            assert!(valid_dns_domain(ok), "{ok}");
        }
        for bad in [
            "",
            "~",
            "~~corp",
            "corp..x",
            "co rp",
            "corp'; rm -rf /",
            "-x",
            "a\nb",
        ] {
            assert!(!valid_dns_domain(bad), "{bad}");
        }
    }

    #[test]
    fn rejects_unknown_fields_and_ops() {
        // Um campo a mais (ex.: tentar passar um caminho ou comando) é recusado.
        let sneaky = r#"{"op":"start","iface":"ipsc0","if_id":1,"mtu":1400,"log_level":1,"profile_id":"x","charon":"/tmp/x"}"#;
        assert!(decode::<Request>(sneaky).is_err());
        assert!(decode::<Request>(r#"{"op":"exec","cmd":"id"}"#).is_err());
        assert!(decode::<Request>("não é json").is_err());
        assert!(decode::<Request>(&"x".repeat(MAX_LINE + 1)).is_err());
    }

    #[test]
    fn validation_mirrors_entrypoint_rules() {
        assert!(params().validate().is_ok());
        let cases: &[fn(&mut StartParams)] = &[
            |p| p.iface = "Ipsc0".into(),
            |p| p.iface = "ipsc0;id".into(),
            |p| p.iface = "a-very-long-iface-name".into(),
            |p| p.if_id = 0,
            |p| p.mtu = 100,
            |p| p.log_level = 3,
            |p| p.profile_id = "../etc".into(),
            |p| p.profile_id = "Perfil".into(),
            |p| p.dns.domains = vec!["x'; touch /tmp/p; '".into()],
            |p| p.dns.domains = vec!["--iface".into()],
            |p| p.dns.domains = vec!["corp..example".into()],
            |p| p.dns.domains = vec!["a.example".into(); MAX_DNS_DOMAINS + 1],
            |p| p.dns.servers = vec!["192.0.2.1".parse().expect("ip"); MAX_DNS_SERVERS + 1],
        ];
        for (i, mutate) in cases.iter().enumerate() {
            let mut p = params();
            mutate(&mut p);
            assert!(p.validate().is_err(), "caso {i}");
        }
    }
}
