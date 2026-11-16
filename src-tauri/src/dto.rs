//! Tipos trocados com a UI (o contrato do IPC).
//!
//! São separados dos tipos do `core` de propósito: só o que a UI precisa ver
//! atravessa para o webview. **Segredos só entram** (em [`CredentialsInput`]),
//! nunca saem: a UI vê apenas se cada credencial existe e o usuário mascarado.

use std::fmt;

use ipsec_client_core::component::{self, Bundle, ComponentError, Origin};
use ipsec_client_core::diagnostics::{COMPONENT_CHECK, CheckStatus};
use ipsec_client_core::engine::EngineError;
use ipsec_client_core::profiles::{AuthMethod, CredentialKind, Profile, ProfileError};
use ipsec_client_core::secrets::SecretsError;
use ipsec_client_core::session::{ConnectionInfo, SessionError, SessionEvent, SessionState};
use ipsec_client_core::{tr, trf};
use serde::{Deserialize, Serialize};
use specta::Type;

/// Informações do app (tela "Sobre", diagnóstico).
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    /// Versão do app.
    pub version: String,
    /// Imagem do engine usada nesta versão.
    pub engine_image: String,
}

/// Resumo de um perfil para listas.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProfileSummary {
    /// Id do perfil.
    pub id: String,
    /// Nome exibido.
    pub name: String,
    /// Gateway.
    pub gateway: String,
    /// Usa XAuth (pede usuário e senha)?
    pub uses_xauth: bool,
    /// Redes acessadas pelo túnel.
    pub remote_ts: Vec<String>,
}

impl From<&Profile> for ProfileSummary {
    fn from(p: &Profile) -> Self {
        Self {
            id: p.id.clone(),
            name: p.name.clone(),
            gateway: p.gateway.clone(),
            uses_xauth: p.auth.method == AuthMethod::PskXauth,
            remote_ts: p.child.remote_ts.clone(),
        }
    }
}

/// Arquivo de perfil que não pôde ser lido.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BrokenProfile {
    /// Nome do arquivo.
    pub file: String,
    /// Motivo.
    pub error: String,
}

/// Lista de perfis.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProfileList {
    /// Perfis válidos, por nome.
    pub profiles: Vec<ProfileSummary>,
    /// Arquivos com problema.
    pub broken: Vec<BrokenProfile>,
}

/// Campo de credencial.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum CredentialField {
    /// PSK do grupo.
    Psk,
    /// Usuário XAuth.
    XauthUser,
    /// Senha XAuth.
    XauthPassword,
}

impl From<CredentialKind> for CredentialField {
    fn from(k: CredentialKind) -> Self {
        match k {
            CredentialKind::Psk => Self::Psk,
            CredentialKind::XauthUser => Self::XauthUser,
            CredentialKind::XauthPassword => Self::XauthPassword,
        }
    }
}

/// O que está guardado para um perfil (sem revelar valores).
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CredentialsStatus {
    /// Campos que o perfil exige.
    pub required: Vec<CredentialField>,
    /// Campos já guardados no chaveiro.
    pub stored: Vec<CredentialField>,
    /// Usuário XAuth guardado, mascarado (ex.: `us***lo`).
    pub masked_user: Option<String>,
    /// O chaveiro do sistema está disponível ("lembrar" possível)?
    pub keyring_available: bool,
}

/// Credenciais digitadas na UI (vazias = usar as guardadas). O `Debug` não
/// mostra os valores.
#[derive(Clone, Default, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CredentialsInput {
    /// PSK do grupo.
    #[serde(default)]
    #[specta(optional)]
    pub psk: Option<String>,
    /// Usuário XAuth.
    #[serde(default)]
    #[specta(optional)]
    pub xauth_user: Option<String>,
    /// Senha XAuth.
    #[serde(default)]
    #[specta(optional)]
    pub xauth_password: Option<String>,
}

impl fmt::Debug for CredentialsInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let given = |v: &Option<String>| v.as_ref().is_some_and(|s| !s.is_empty());
        f.debug_struct("CredentialsInput")
            .field("psk", &given(&self.psk))
            .field("xauth_user", &given(&self.xauth_user))
            .field("xauth_password", &given(&self.xauth_password))
            .finish()
    }
}

/// Conexão ativa (sem identidades).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    /// Perfil conectado.
    pub profile_id: Option<String>,
    /// Túnel estabelecido (senão: engine de pé negociando/reconectando).
    pub established: bool,
    /// Gateway.
    pub gateway: String,
    /// IP virtual.
    pub vip: Option<String>,
    /// Redes acessíveis.
    pub remote_ts: Vec<String>,
    /// Segundos desde o estabelecimento.
    pub established_secs: Option<u32>,
    /// Bytes recebidos.
    pub bytes_in: f64,
    /// Bytes enviados.
    pub bytes_out: f64,
}

impl From<&ConnectionInfo> for Connection {
    #[allow(clippy::cast_precision_loss)] // contadores de bytes: f64 é exato até 2^53
    fn from(info: &ConnectionInfo) -> Self {
        Self {
            profile_id: info.profile_id.clone(),
            established: info.is_established(),
            gateway: info.gateway.clone(),
            vip: info.vip.clone(),
            remote_ts: info.remote_ts.clone(),
            established_secs: info
                .established_secs
                .map(|s| u32::try_from(s).unwrap_or(u32::MAX)),
            bytes_in: info.bytes_in as f64,
            bytes_out: info.bytes_out as f64,
        }
    }
}

/// Atualização da sessão, enviada como evento para a UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type, tauri_specta::Event)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SessionUpdate {
    /// Preparando (etapa em texto).
    Preparing {
        /// Etapa.
        step: String,
    },
    /// Negociando com o gateway.
    Negotiating,
    /// Conectado.
    Connected {
        /// Conexão.
        connection: Connection,
    },
    /// Linha de log do charon (identidades mascaradas).
    Log {
        /// Linha.
        line: String,
    },
    /// Linha do build da imagem do engine (primeiro uso).
    Build {
        /// Linha.
        line: String,
    },
    /// O túnel caiu (pode reconectar sozinho).
    TunnelDown,
    /// O túnel (re)subiu.
    TunnelUp,
    /// O engine parou.
    EngineGone,
}

impl From<SessionEvent> for SessionUpdate {
    fn from(event: SessionEvent) -> Self {
        match event {
            SessionEvent::State(SessionState::Preparing(step)) => Self::Preparing { step },
            SessionEvent::State(SessionState::Negotiating) => Self::Negotiating,
            SessionEvent::State(SessionState::Connected(info)) => Self::Connected {
                connection: Connection::from(&info),
            },
            SessionEvent::Log(line) => Self::Log { line },
            SessionEvent::Build(line) => Self::Build { line },
            SessionEvent::TunnelDown => Self::TunnelDown,
            SessionEvent::TunnelUp => Self::TunnelUp,
            SessionEvent::EngineGone => Self::EngineGone,
        }
    }
}

/// Tipo de erro, para a UI escolher a apresentação.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum UiErrorKind {
    /// Já há uma conexão ativa.
    AlreadyConnected,
    /// O ambiente não permite conectar (detalhes com dicas).
    Preflight,
    /// A negociação falhou (mensagem = causa provável).
    Negotiation,
    /// Falta alguma credencial.
    MissingCredentials,
    /// Perfil inexistente ou inválido.
    Profile,
    /// Chaveiro do sistema.
    Keyring,
    /// Engine/Docker.
    Engine,
    /// Falta instalar o componente de rede (a UI oferece instalar).
    ComponentMissing,
    /// O componente de rede instalado é antigo (a UI oferece atualizar).
    ComponentOutdated,
    /// Instalação/remoção do componente de rede.
    Component,
    /// O usuário cancelou (ex.: fechou o pedido de senha).
    Cancelled,
    /// Outra operação em andamento.
    Busy,
    /// Outro erro.
    Other,
}

/// Erro para a UI: mensagem em pt-BR e detalhes opcionais (sem segredos).
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UiError {
    /// Tipo.
    pub kind: UiErrorKind,
    /// Mensagem principal.
    pub message: String,
    /// Detalhes (ex.: verificações que falharam, mensagem do charon).
    pub details: Vec<String>,
}

impl UiError {
    /// Erro sem detalhes.
    #[must_use]
    pub fn new(kind: UiErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            details: Vec::new(),
        }
    }
}

impl From<SessionError> for UiError {
    fn from(e: SessionError) -> Self {
        match e {
            SessionError::AlreadyConnected(profile) => Self::new(
                UiErrorKind::AlreadyConnected,
                trf!(
                    "A connection is already active (profile {profile}). Disconnect first.",
                    "Já existe uma conexão ativa (perfil {profile}). Desconecte antes."
                ),
            ),
            SessionError::Preflight(checks) => Self {
                kind: if checks
                    .iter()
                    .any(|c| c.status == CheckStatus::Fail && c.id == COMPONENT_CHECK)
                {
                    UiErrorKind::ComponentMissing
                } else {
                    UiErrorKind::Preflight
                },
                message: tr!(
                    "This computer is not ready to connect yet.",
                    "O computador ainda não está pronto para conectar."
                )
                .to_owned(),
                details: checks
                    .iter()
                    .filter(|c| c.status == CheckStatus::Fail)
                    .map(|c| match &c.hint {
                        Some(hint) => format!("{}: {} → {hint}", c.name, c.detail),
                        None => format!("{}: {}", c.name, c.detail),
                    })
                    .collect(),
            },
            SessionError::Negotiation { kind, detail } => Self {
                kind: UiErrorKind::Negotiation,
                message: capitalize(&kind.to_string()),
                details: vec![format!("charon: {detail}")],
            },
            SessionError::Secrets(e) => Self::from(e),
            SessionError::Profile(e) => Self::from(e),
            SessionError::Engine(e) => Self::from(e),
            other => Self::new(UiErrorKind::Other, capitalize(&other.to_string())),
        }
    }
}

impl From<SecretsError> for UiError {
    fn from(e: SecretsError) -> Self {
        let kind = match e {
            SecretsError::Missing(_) => UiErrorKind::MissingCredentials,
            _ => UiErrorKind::Keyring,
        };
        Self::new(kind, capitalize(&e.to_string()))
    }
}

impl From<ProfileError> for UiError {
    fn from(e: ProfileError) -> Self {
        Self::new(UiErrorKind::Profile, capitalize(&e.to_string()))
    }
}

impl From<EngineError> for UiError {
    fn from(e: EngineError) -> Self {
        match e {
            EngineError::NotReady {
                reason,
                recent_logs,
            } => Self {
                kind: UiErrorKind::Engine,
                message: capitalize(&trf!(
                    "the engine did not become ready: {reason}",
                    "o engine não ficou pronto: {reason}"
                )),
                details: recent_logs,
            },
            EngineError::HelperOutdated => Self::new(
                UiErrorKind::ComponentOutdated,
                capitalize(&EngineError::HelperOutdated.to_string()),
            ),
            other => Self::new(UiErrorKind::Engine, capitalize(&other.to_string())),
        }
    }
}

impl From<ComponentError> for UiError {
    fn from(e: ComponentError) -> Self {
        match e {
            ComponentError::Cancelled => Self::new(
                UiErrorKind::Cancelled,
                tr!("Authorization cancelled.", "Autorização cancelada."),
            ),
            ComponentError::Failed { log } => Self {
                kind: UiErrorKind::Component,
                message: tr!(
                    "The network component installer failed.",
                    "O instalador do componente de rede falhou."
                )
                .to_owned(),
                details: log,
            },
            other => Self::new(UiErrorKind::Component, capitalize(&other.to_string())),
        }
    }
}

/// Situação do componente de rede.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ComponentState {
    /// Nada instalado.
    NotInstalled,
    /// Instalado e funcionando.
    Ready,
    /// Instalado, mas o app traz uma versão diferente.
    Outdated,
    /// Instalação com problema (helper não responde).
    Broken,
}

/// Componente de rede: o que está instalado e o que dá para fazer.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ComponentInfo {
    /// Situação.
    pub state: ComponentState,
    /// Versão do IPsec Client que instalou o componente.
    pub installed_version: Option<String>,
    /// strongSwan instalado.
    pub installed_strongswan: Option<String>,
    /// Versão do componente que vem com este app.
    pub bundled_version: Option<String>,
    /// strongSwan que vem com este app.
    pub bundled_strongswan: Option<String>,
    /// Instalado por pacote do sistema (.rpm/.deb): o app não mexe.
    pub managed_by_package: bool,
    /// O app pode instalar/atualizar daqui.
    pub can_install: bool,
    /// O app pode remover daqui.
    pub can_uninstall: bool,
    /// Motivo, quando há problema.
    pub problem: Option<String>,
}

impl ComponentInfo {
    /// Junta o estado, o pacote do app e a disponibilidade do `pkexec`.
    #[must_use]
    pub fn new(status: &component::Status, bundle: Option<&Bundle>, pkexec: bool) -> Self {
        let installed = status.installed.as_ref();
        let managed_by_package = installed.is_some_and(|i| i.origin == Origin::Package);
        Self {
            state: match status.state {
                component::State::NotInstalled => ComponentState::NotInstalled,
                component::State::Ready => ComponentState::Ready,
                component::State::Outdated => ComponentState::Outdated,
                component::State::Broken => ComponentState::Broken,
            },
            installed_version: installed.and_then(|i| i.version.app.clone()),
            installed_strongswan: installed.and_then(|i| i.version.strongswan.clone()),
            bundled_version: bundle.and_then(|b| b.version().app.clone()),
            bundled_strongswan: bundle.and_then(|b| b.version().strongswan.clone()),
            managed_by_package,
            can_install: pkexec && bundle.is_some() && !managed_by_package,
            can_uninstall: pkexec && installed.is_some() && !managed_by_package,
            problem: status.problem.clone(),
        }
    }
}

/// Primeira letra maiúscula (as mensagens do core começam em minúscula).
fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ipsec_client_core::diagnostics::Check;
    use ipsec_client_core::session::FailureKind;

    #[test]
    fn credentials_input_debug_hides_values() {
        let input = CredentialsInput {
            psk: Some("psk-secreta".into()),
            xauth_user: Some("usuario".into()),
            xauth_password: Some(String::new()),
        };
        let debug = format!("{input:?}");
        assert!(
            !debug.contains("psk-secreta") && !debug.contains("usuario"),
            "{debug}"
        );
        assert!(debug.contains("psk: true") && debug.contains("xauth_password: false"));
    }

    #[test]
    fn negotiation_error_carries_probable_cause() {
        let ui = UiError::from(SessionError::Negotiation {
            kind: FailureKind::XauthCredentials,
            detail: "establishing CHILD_SA failed".into(),
        });
        assert_eq!(ui.kind, UiErrorKind::Negotiation);
        assert_eq!(ui.message, "Wrong XAuth username or password");
        assert_eq!(ui.details, ["charon: establishing CHILD_SA failed"]);
    }

    #[test]
    fn preflight_error_lists_failures_with_hints() {
        let ui = UiError::from(SessionError::Preflight(vec![
            Check {
                id: "docker",
                name: "Docker",
                status: CheckStatus::Fail,
                detail: "unreachable".into(),
                hint: Some("usermod".into()),
            },
            Check {
                id: "resolved",
                name: "resolved",
                status: CheckStatus::Warn,
                detail: "missing".into(),
                hint: None,
            },
        ]));
        assert_eq!(ui.kind, UiErrorKind::Preflight);
        assert_eq!(ui.details, ["Docker: unreachable → usermod"]);
    }

    #[test]
    fn missing_component_is_its_own_kind() {
        let ui = UiError::from(SessionError::Preflight(vec![Check {
            id: COMPONENT_CHECK,
            name: "Network component",
            status: CheckStatus::Fail,
            detail: "not installed".into(),
            hint: Some("install it".into()),
        }]));
        assert_eq!(ui.kind, UiErrorKind::ComponentMissing);
        let cancelled = UiError::from(ComponentError::Cancelled);
        assert_eq!(cancelled.kind, UiErrorKind::Cancelled);
        let failed = UiError::from(ComponentError::Failed {
            log: vec!["[network component] error: x".into()],
        });
        assert_eq!(failed.kind, UiErrorKind::Component);
        assert_eq!(failed.details.len(), 1);
    }

    #[test]
    fn component_info_respects_package_origin() {
        use ipsec_client_core::component::{Installed, State, Status, Version};
        let status = |origin| Status {
            state: State::Ready,
            installed: Some(Installed {
                version: Version {
                    app: Some("0.1.0".into()),
                    strongswan: Some("6.1.0".into()),
                },
                digest: None,
                origin,
            }),
            helper_version: Some("0.1.0".into()),
            problem: None,
        };
        let by_app = ComponentInfo::new(&status(Origin::App), None, true);
        assert!(
            by_app.can_uninstall && !by_app.can_install,
            "sem pacote no app"
        );
        assert_eq!(by_app.installed_strongswan.as_deref(), Some("6.1.0"));
        let by_pkg = ComponentInfo::new(&status(Origin::Package), None, true);
        assert!(by_pkg.managed_by_package && !by_pkg.can_uninstall);
        let no_pkexec = ComponentInfo::new(&status(Origin::App), None, false);
        assert!(!no_pkexec.can_uninstall);
    }

    #[test]
    fn connection_from_info() {
        let info = ConnectionInfo {
            profile_id: Some("vpn".into()),
            state: "ESTABLISHED".into(),
            gateway: "203.0.113.1".into(),
            vip: Some("198.51.100.10".into()),
            remote_ts: vec!["192.0.2.0/24".into()],
            established_secs: Some(65),
            bytes_in: 2048,
            bytes_out: 1024,
        };
        let c = Connection::from(&info);
        assert!(c.established);
        assert_eq!(c.established_secs, Some(65));
        assert!((c.bytes_in - 2048.0).abs() < f64::EPSILON);
    }
}

/// Tipo do arquivo importado.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ImportKind {
    /// Perfil do IPsec Client (.json).
    Json,
    /// `swanctl.conf` de uma VPN que já funciona.
    Swanctl,
}

/// Prévia de um arquivo a importar. O arquivo fica guardado no Rust, e a UI
/// só recebe o `token` (o webview nunca manipula caminhos).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    /// Identifica o arquivo na confirmação.
    pub token: String,
    /// Nome do arquivo (sem diretório).
    pub file_name: String,
    /// Tipo detectado.
    pub kind: ImportKind,
    /// Id sugerido.
    pub suggested_id: String,
    /// Nome sugerido.
    pub suggested_name: String,
    /// Gateway (se o arquivo pôde ser lido).
    pub gateway: Option<String>,
    /// Redes remotas.
    pub remote_ts: Vec<String>,
    /// Avisos (ex.: chaves ignoradas do swanctl.conf).
    pub warnings: Vec<String>,
    /// Problema que impede importar (arquivo inválido).
    pub error: Option<String>,
    /// Já existe perfil com o id sugerido?
    pub exists: bool,
}

/// Opções da confirmação de importação.
#[derive(Debug, Clone, Default, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportOptions {
    /// Id do perfil (vazio = o sugerido).
    #[serde(default)]
    #[specta(optional)]
    pub id: Option<String>,
    /// Nome exibido (vazio = o sugerido).
    #[serde(default)]
    #[specta(optional)]
    pub name: Option<String>,
    /// Servidores DNS (só swanctl.conf; o JSON já tem os seus).
    #[serde(default)]
    pub dns_servers: Vec<String>,
    /// Domínios DNS (só swanctl.conf).
    #[serde(default)]
    pub dns_domains: Vec<String>,
    /// Usar também os DNS enviados pelo gateway (só swanctl.conf).
    #[serde(default)]
    pub use_received_dns: bool,
    /// Substituir um perfil existente com o mesmo id.
    #[serde(default)]
    pub overwrite: bool,
}

/// Detalhes de um perfil (somente leitura; não há credenciais no perfil).
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProfileDetails {
    /// Resumo.
    pub summary: ProfileSummary,
    /// "IKEv1 agressivo", "IKEv2"...
    pub ike: String,
    /// Propostas da fase 1.
    pub ike_proposals: Vec<String>,
    /// Propostas da fase 2.
    pub esp_proposals: Vec<String>,
    /// Autenticação, em texto.
    pub auth: String,
    /// Identidade local (ex.: id do grupo).
    pub local_id: Option<String>,
    /// Servidores DNS fixos.
    pub dns_servers: Vec<String>,
    /// Domínios DNS.
    pub dns_domains: Vec<String>,
    /// Usa os DNS enviados pelo gateway.
    pub use_received_dns: bool,
    /// MTU.
    pub mtu: u16,
}

impl From<&Profile> for ProfileDetails {
    fn from(p: &Profile) -> Self {
        let ike = match (p.ike.version, p.ike.aggressive) {
            (1, true) => tr!("IKEv1 (aggressive mode)", "IKEv1 (modo agressivo)").to_owned(),
            (1, false) => tr!("IKEv1 (main mode)", "IKEv1 (modo principal)").to_owned(),
            (v, _) => format!("IKEv{v}"),
        };
        let auth = match p.auth.method {
            AuthMethod::Psk => "PSK".to_owned(),
            AuthMethod::PskXauth => tr!(
                "group PSK + username and password (XAuth)",
                "PSK do grupo + usuário e senha (XAuth)"
            )
            .to_owned(),
        };
        Self {
            summary: ProfileSummary::from(p),
            ike,
            ike_proposals: p.ike.proposals.clone(),
            esp_proposals: p.child.esp_proposals.clone(),
            auth,
            local_id: p.auth.local_id.clone(),
            dns_servers: p
                .network
                .dns
                .servers
                .iter()
                .map(ToString::to_string)
                .collect(),
            dns_domains: p.network.dns.domains.clone(),
            use_received_dns: p.network.dns.use_received,
            mtu: p.network.mtu,
        }
    }
}

/// Resultado de uma verificação do ambiente.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum CheckLevel {
    /// Tudo certo.
    Ok,
    /// Funciona com limitação.
    Warn,
    /// Impede a conexão.
    Fail,
}

/// Uma verificação do diagnóstico (o `doctor` do CLI).
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticCheck {
    /// O que foi verificado.
    pub name: String,
    /// Resultado.
    pub level: CheckLevel,
    /// Detalhe.
    pub detail: String,
    /// Como corrigir.
    pub hint: Option<String>,
}

impl From<&ipsec_client_core::diagnostics::Check> for DiagnosticCheck {
    fn from(c: &ipsec_client_core::diagnostics::Check) -> Self {
        Self {
            name: c.name.to_owned(),
            level: match c.status {
                CheckStatus::Ok => CheckLevel::Ok,
                CheckStatus::Warn => CheckLevel::Warn,
                CheckStatus::Fail => CheckLevel::Fail,
            },
            detail: c.detail.clone(),
            hint: c.hint.clone(),
        }
    }
}
