//! Operações tipadas sobre o charon (ADR-0003): a trait [`Charon`], os tipos
//! das respostas e o parse dos eventos.
//!
//! A sessão (`core::session`) depende só da trait, então os testes dela podem
//! usar um charon de mentira.

use std::fmt;
use std::time::Duration;

use zeroize::Zeroizing;

use super::{Message, ViciClient, ViciError};

/// Tipo de segredo para `load-shared`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretKind {
    /// PSK do IKE (ex.: PSK do grupo).
    Ike,
    /// Senha XAuth (IKEv1).
    Xauth,
    /// Senha EAP (IKEv2).
    Eap,
}

impl SecretKind {
    fn as_vici(self) -> &'static str {
        match self {
            Self::Ike => "IKE",
            Self::Xauth => "XAUTH",
            Self::Eap => "EAP",
        }
    }
}

/// Um segredo a carregar no charon. O `Debug` não mostra os dados nem os
/// donos (identidades como id do grupo e usuário XAuth).
#[derive(Clone)]
pub struct SharedSecret {
    /// Identificador único do segredo no charon (para `unload-shared`).
    pub id: String,
    /// Tipo do segredo.
    pub kind: SecretKind,
    owners: Vec<String>,
    data: Zeroizing<Vec<u8>>,
}

impl SharedSecret {
    /// Cria um segredo. `data` é copiado para um buffer zerado no drop.
    #[must_use]
    pub fn new(id: &str, kind: SecretKind, owners: Vec<String>, data: &[u8]) -> Self {
        Self {
            id: id.to_owned(),
            kind,
            owners,
            data: Zeroizing::new(data.to_vec()),
        }
    }

    /// Mensagem `load-shared` correspondente.
    fn to_message(&self) -> Message {
        Message::new()
            .kv("id", &self.id)
            .kv("type", self.kind.as_vici())
            .kv("data", self.data.as_slice())
            .list("owners", &self.owners)
    }
}

impl fmt::Debug for SharedSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SharedSecret")
            .field("id", &self.id)
            .field("kind", &self.kind)
            .field(
                "owners",
                &format_args!("<{} identidade(s)>", self.owners.len()),
            )
            .field("data", &format_args!("<redigido>"))
            .finish()
    }
}

/// Resposta de `version`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VersionInfo {
    /// Nome do daemon (`charon`).
    pub daemon: String,
    /// Versão do strongSwan.
    pub version: String,
    /// Sistema (`Linux`).
    pub sysname: String,
    /// Versão do kernel.
    pub release: String,
    /// Arquitetura.
    pub machine: String,
}

/// Um CHILD_SA (fase 2) de `list-sas`. Sem identidades.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChildSa {
    /// Nome do child na configuração.
    pub name: String,
    /// Estado (`INSTALLED`, `REKEYING`...).
    pub state: String,
    /// Bytes recebidos.
    pub bytes_in: u64,
    /// Bytes enviados.
    pub bytes_out: u64,
    /// Segundos desde a instalação.
    pub install_time: Option<u64>,
    /// Traffic selectors locais.
    pub local_ts: Vec<String>,
    /// Traffic selectors remotos.
    pub remote_ts: Vec<String>,
}

/// Um IKE_SA (fase 1) de `list-sas`. Sem identidades (id do grupo, usuário
/// XAuth), para não vazarem pela UI ou por logs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IkeSa {
    /// Nome da conexão.
    pub name: String,
    /// Id único do IKE_SA no charon.
    pub uniqueid: String,
    /// Estado (`ESTABLISHED`, `CONNECTING`...).
    pub state: String,
    /// Endereço do gateway.
    pub remote_host: String,
    /// IPs virtuais recebidos.
    pub local_vips: Vec<String>,
    /// CHILD_SAs.
    pub children: Vec<ChildSa>,
}

impl IkeSa {
    fn from_message(name: &str, msg: &Message) -> Self {
        let text = |key: &str| msg.get_str(key).unwrap_or_default().to_owned();
        let children = msg
            .get_section("child-sas")
            .map(|c| {
                c.sections()
                    .map(|(_, m)| ChildSa::from_message(m))
                    .collect()
            })
            .unwrap_or_default();
        Self {
            name: name.to_owned(),
            uniqueid: text("uniqueid"),
            state: text("state"),
            remote_host: text("remote-host"),
            local_vips: msg
                .get_list("local-vips")
                .into_iter()
                .map(str::to_owned)
                .collect(),
            children,
        }
    }
}

impl ChildSa {
    fn from_message(msg: &Message) -> Self {
        let number = |key: &str| msg.get_str(key).and_then(|v| v.parse().ok());
        let list = |key: &str| msg.get_list(key).into_iter().map(str::to_owned).collect();
        Self {
            name: msg.get_str("name").unwrap_or_default().to_owned(),
            state: msg.get_str("state").unwrap_or_default().to_owned(),
            bytes_in: number("bytes-in").unwrap_or(0),
            bytes_out: number("bytes-out").unwrap_or(0),
            install_time: number("install-time"),
            local_ts: list("local-ts"),
            remote_ts: list("remote-ts"),
        }
    }
}

/// Evento do charon, já interpretado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CharonEvent {
    /// IKE_SA subiu (`up = true`) ou caiu.
    IkeUpDown {
        /// Subiu?
        up: bool,
        /// Nome da conexão.
        ike: String,
    },
    /// CHILD_SA subiu ou caiu.
    ChildUpDown {
        /// Subiu?
        up: bool,
        /// Nome da conexão.
        ike: String,
        /// Nomes dos children afetados.
        children: Vec<String>,
    },
    /// Linha de log do charon (nível ≤ 1). Pode conter identidades: quem
    /// exibe deve mascarar (ADR-0007).
    Log {
        /// Nível (0 = básico, 1 = controle).
        level: i32,
        /// Subsistema (`IKE`, `CFG`, `NET`...).
        group: String,
        /// Conexão relacionada, se houver.
        ike: Option<String>,
        /// Texto.
        message: String,
    },
    /// Outro evento.
    Other(String),
}

impl CharonEvent {
    /// Interpreta um evento VICI.
    #[must_use]
    pub fn parse(name: &str, msg: &Message) -> Self {
        let up = msg.get_str("up") == Some("yes");
        match name {
            "ike-updown" => Self::IkeUpDown {
                up,
                ike: first_section_name(msg),
            },
            "child-updown" => {
                let (ike, children) = msg
                    .sections()
                    .next()
                    .map(|(ike, sa)| {
                        let children = sa
                            .get_section("child-sas")
                            .map(|c| {
                                c.sections()
                                    .map(|(key, m)| m.get_str("name").unwrap_or(key).to_owned())
                                    .collect()
                            })
                            .unwrap_or_default();
                        (ike.to_owned(), children)
                    })
                    .unwrap_or_default();
                Self::ChildUpDown { up, ike, children }
            }
            "log" => Self::Log {
                level: msg
                    .get_str("level")
                    .and_then(|l| l.parse().ok())
                    .unwrap_or(0),
                group: msg.get_str("group").unwrap_or_default().to_owned(),
                ike: msg.get_str("ikesa-name").map(str::to_owned),
                message: msg.get_str("msg").unwrap_or_default().to_owned(),
            },
            other => Self::Other(other.to_owned()),
        }
    }
}

fn first_section_name(msg: &Message) -> String {
    msg.sections()
        .next()
        .map(|(name, _)| name.to_owned())
        .unwrap_or_default()
}

/// Operações usadas pelo app (ADR-0003).
#[allow(async_fn_in_trait)] // uso interno: sempre com tipos concretos/genéricos
pub trait Charon {
    /// `version`.
    async fn version(&mut self) -> Result<VersionInfo, ViciError>;
    /// `load-shared`: carrega um segredo na memória do charon.
    async fn load_shared(&mut self, secret: &SharedSecret) -> Result<(), ViciError>;
    /// `unload-shared`.
    async fn unload_shared(&mut self, id: &str) -> Result<(), ViciError>;
    /// `load-conn` com a configuração da conexão `name`.
    async fn load_conn(&mut self, name: &str, conn: &Message) -> Result<(), ViciError>;
    /// `unload-conn`.
    async fn unload_conn(&mut self, name: &str) -> Result<(), ViciError>;
    /// `list-conns`: nomes das conexões carregadas.
    async fn list_conns(&mut self) -> Result<Vec<String>, ViciError>;
    /// `initiate` do child `child` da conexão `ike`, esperando até `timeout`.
    async fn initiate(
        &mut self,
        ike: &str,
        child: &str,
        timeout: Duration,
    ) -> Result<(), ViciError>;
    /// `terminate` do IKE_SA `ike` (envia DELETE ao gateway).
    async fn terminate(&mut self, ike: &str, timeout: Duration) -> Result<(), ViciError>;
    /// `list-sas`.
    async fn list_sas(&mut self) -> Result<Vec<IkeSa>, ViciError>;
}

/// Confere `success = yes`; senão, devolve o `errmsg` do charon.
fn check(command: &str, response: &Message) -> Result<(), ViciError> {
    if response.get_str("success") == Some("yes") {
        return Ok(());
    }
    Err(ViciError::CommandFailed {
        command: command.to_owned(),
        message: response
            .get_str("errmsg")
            .unwrap_or("falha sem mensagem")
            .to_owned(),
    })
}

fn millis(timeout: Duration) -> String {
    timeout.as_millis().to_string()
}

impl Charon for ViciClient {
    async fn version(&mut self) -> Result<VersionInfo, ViciError> {
        let r = self.request("version", &Message::new()).await?;
        let text = |key: &str| r.get_str(key).unwrap_or_default().to_owned();
        Ok(VersionInfo {
            daemon: text("daemon"),
            version: text("version"),
            sysname: text("sysname"),
            release: text("release"),
            machine: text("machine"),
        })
    }

    async fn load_shared(&mut self, secret: &SharedSecret) -> Result<(), ViciError> {
        let response = self.request("load-shared", &secret.to_message()).await?;
        check("load-shared", &response)
    }

    async fn unload_shared(&mut self, id: &str) -> Result<(), ViciError> {
        let response = self
            .request("unload-shared", &Message::new().kv("id", id))
            .await?;
        check("unload-shared", &response)
    }

    async fn load_conn(&mut self, name: &str, conn: &Message) -> Result<(), ViciError> {
        let message = Message::new().section(name, conn.clone());
        let response = self.request("load-conn", &message).await?;
        check("load-conn", &response)
    }

    async fn unload_conn(&mut self, name: &str) -> Result<(), ViciError> {
        let response = self
            .request("unload-conn", &Message::new().kv("name", name))
            .await?;
        check("unload-conn", &response)
    }

    async fn list_conns(&mut self) -> Result<Vec<String>, ViciError> {
        let (events, _) = self
            .streamed_request("list-conns", "list-conn", &Message::new())
            .await?;
        Ok(events
            .iter()
            .flat_map(|e| e.sections().map(|(name, _)| name.to_owned()))
            .collect())
    }

    async fn initiate(
        &mut self,
        ike: &str,
        child: &str,
        timeout: Duration,
    ) -> Result<(), ViciError> {
        let message = Message::new()
            .kv("child", child)
            .kv("ike", ike)
            .kv("timeout", millis(timeout))
            .kv("init-limits", "no");
        let response = self.request("initiate", &message).await?;
        check("initiate", &response)
    }

    async fn terminate(&mut self, ike: &str, timeout: Duration) -> Result<(), ViciError> {
        let message = Message::new().kv("ike", ike).kv("timeout", millis(timeout));
        let response = self.request("terminate", &message).await?;
        check("terminate", &response)
    }

    async fn list_sas(&mut self) -> Result<Vec<IkeSa>, ViciError> {
        let (events, _) = self
            .streamed_request("list-sas", "list-sa", &Message::new())
            .await?;
        Ok(events
            .iter()
            .flat_map(|e| e.sections().map(|(name, m)| IkeSa::from_message(name, m)))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_secret_message_and_debug() {
        let secret = SharedSecret::new(
            "ike-perfil",
            SecretKind::Ike,
            vec!["keyid:grupo-teste".to_owned()],
            b"psk-falsa",
        );
        let msg = secret.to_message();
        assert_eq!(msg.get_str("type"), Some("IKE"));
        assert_eq!(msg.get_str("data"), Some("psk-falsa"));
        assert_eq!(msg.get_list("owners"), ["keyid:grupo-teste"]);
        let debug = format!("{secret:?}");
        assert!(!debug.contains("psk-falsa"), "{debug}");
        assert!(!debug.contains("grupo-teste"), "{debug}");
    }

    #[test]
    fn check_reports_errmsg() {
        assert!(check("x", &Message::new().kv("success", "yes")).is_ok());
        let err = check(
            "initiate",
            &Message::new().kv("success", "no").kv("errmsg", "timeout"),
        )
        .expect_err("falha");
        assert_eq!(err.to_string(), "o charon recusou 'initiate': timeout");
    }

    #[test]
    fn parses_list_sa() {
        let child = Message::new()
            .kv("name", "rede")
            .kv("state", "INSTALLED")
            .kv("bytes-in", "120")
            .kv("bytes-out", "80")
            .kv("install-time", "5")
            .list("local-ts", ["198.51.100.10/32"])
            .list("remote-ts", ["192.0.2.0/24"]);
        let sa = Message::new()
            .kv("uniqueid", "1")
            .kv("state", "ESTABLISHED")
            .kv("remote-host", "203.0.113.1")
            .kv("remote-xauth-id", "usuario.teste")
            .list("local-vips", ["198.51.100.10"])
            .section("child-sas", Message::new().section("rede-1", child));
        let ike = IkeSa::from_message("teste", &sa);
        assert_eq!(ike.state, "ESTABLISHED");
        assert_eq!(ike.local_vips, ["198.51.100.10"]);
        assert_eq!(ike.children.len(), 1);
        assert_eq!(ike.children[0].bytes_in, 120);
        assert_eq!(ike.children[0].install_time, Some(5));
        assert!(!format!("{ike:?}").contains("usuario"));
    }

    #[test]
    fn parses_events() {
        let child_up = Message::new().kv("up", "yes").section(
            "teste",
            Message::new().section(
                "child-sas",
                Message::new().section("rede-1", Message::new().kv("name", "rede")),
            ),
        );
        assert_eq!(
            CharonEvent::parse("child-updown", &child_up),
            CharonEvent::ChildUpDown {
                up: true,
                ike: "teste".into(),
                children: vec!["rede".into()]
            }
        );
        let ike_down = Message::new().section("teste", Message::new());
        assert_eq!(
            CharonEvent::parse("ike-updown", &ike_down),
            CharonEvent::IkeUpDown {
                up: false,
                ike: "teste".into()
            }
        );
        let log = Message::new()
            .kv("group", "IKE")
            .kv("level", "1")
            .kv("ikesa-name", "teste")
            .kv("msg", "IKE_SA established");
        assert!(matches!(
            CharonEvent::parse("log", &log),
            CharonEvent::Log { level: 1, ref group, .. } if group == "IKE"
        ));
        assert_eq!(
            CharonEvent::parse("x", &Message::new()),
            CharonEvent::Other("x".into())
        );
    }
}
