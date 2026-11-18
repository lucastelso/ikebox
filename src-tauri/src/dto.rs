//! Types exchanged with the UI (the IPC contract).
//!
//! They are kept apart from the `core` types on purpose: only what the UI needs
//! to see crosses into the webview. **Secrets only go in** (in
//! [`CredentialsInput`]), never out: the UI only sees whether each credential
//! exists, and the masked username.

use std::fmt;

use ipsec_client_core::component::{self, Bundle, ComponentError, Origin};
use ipsec_client_core::diagnostics::{COMPONENT_CHECK, CheckStatus};
use ipsec_client_core::engine::EngineError;
use ipsec_client_core::profiles::{
    CredentialKind, Profile, ProfileError, SecurityFinding, SecurityLevel,
};
use ipsec_client_core::secrets::SecretsError;
use ipsec_client_core::session::{ConnectionInfo, SessionError, SessionEvent, SessionState};
use ipsec_client_core::{tr, trf};
use serde::{Deserialize, Serialize};
use specta::Type;

/// App information ("About" screen, diagnostics).
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    /// App version.
    pub version: String,
    /// Engine image used by this version.
    pub engine_image: String,
}

/// Profile summary for lists.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProfileSummary {
    /// Profile id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Gateway.
    pub gateway: String,
    /// Uses XAuth (asks for username and password)?
    pub uses_xauth: bool,
    /// The password is a one-time code (never stored).
    pub otp: bool,
    /// Networks reached through the tunnel.
    pub remote_ts: Vec<String>,
    /// Security level (badge); the reason comes in the details.
    pub security: Security,
}

impl From<&Profile> for ProfileSummary {
    fn from(p: &Profile) -> Self {
        Self {
            id: p.id.clone(),
            name: p.name.clone(),
            gateway: p.gateway.clone(),
            uses_xauth: p.auth.uses_xauth(),
            otp: p.auth.otp(),
            remote_ts: p.child.remote_ts.clone(),
            security: p.security().level.into(),
        }
    }
}

/// Security level of a profile: it only warns, never prevents connecting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Security {
    /// Nothing to point out.
    Good,
    /// Obsolete, with no known cheap attack.
    Legacy,
    /// There is a known practical attack.
    Weak,
}

impl From<SecurityLevel> for Security {
    fn from(level: SecurityLevel) -> Self {
        match level {
            SecurityLevel::Good => Self::Good,
            SecurityLevel::Legacy => Self::Legacy,
            SecurityLevel::Weak => Self::Weak,
        }
    }
}

/// A weakness of the profile, explained in the interface language.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SecurityNote {
    /// Severity.
    pub level: Security,
    /// Explanation.
    pub message: String,
}

impl From<&SecurityFinding> for SecurityNote {
    fn from(f: &SecurityFinding) -> Self {
        Self {
            level: f.level.into(),
            message: f.message.clone(),
        }
    }
}

/// Profile file that could not be read.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BrokenProfile {
    /// File name.
    pub file: String,
    /// Reason.
    pub error: String,
}

/// List of profiles.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProfileList {
    /// Valid profiles, by name.
    pub profiles: Vec<ProfileSummary>,
    /// Broken files.
    pub broken: Vec<BrokenProfile>,
}

/// Credential field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum CredentialField {
    /// Group PSK.
    Psk,
    /// XAuth username.
    XauthUser,
    /// XAuth password.
    XauthPassword,
    /// EAP username.
    EapIdentity,
    /// EAP password.
    EapPassword,
    /// Client certificate and its key: imported from a `.p12`, never typed.
    ClientCertificate,
    /// The certificate on a smartcard or token, chosen once (ADR-0029).
    TokenCertificate,
    /// The PIN of the smartcard or token: asked at every connection.
    TokenPin,
}

impl From<CredentialKind> for CredentialField {
    fn from(k: CredentialKind) -> Self {
        match k {
            CredentialKind::Psk => Self::Psk,
            CredentialKind::XauthUser => Self::XauthUser,
            CredentialKind::XauthPassword => Self::XauthPassword,
            CredentialKind::EapIdentity => Self::EapIdentity,
            CredentialKind::EapPassword => Self::EapPassword,
            CredentialKind::ClientCert | CredentialKind::ClientKey => Self::ClientCertificate,
            CredentialKind::TokenKey => Self::TokenCertificate,
            CredentialKind::TokenPin => Self::TokenPin,
        }
    }
}

/// What is shown of a client certificate (nothing secret).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ClientCertSummary {
    /// Subject.
    pub subject: String,
    /// Issuer.
    pub issuer: String,
    /// Last day of validity (`YYYY-MM-DD`).
    pub not_after: String,
    /// Days until it expires (negative: expired).
    pub days_left: i32,
    /// `RSA`, `ECDSA` or `Ed25519`.
    pub key: String,
}

impl From<&ipsec_client_core::secrets::ClientCertInfo> for ClientCertSummary {
    fn from(info: &ipsec_client_core::secrets::ClientCertInfo) -> Self {
        Self {
            subject: info.subject.clone(),
            issuer: info.issuer.clone(),
            not_after: info.not_after.clone(),
            days_left: i32::try_from(info.days_left()).unwrap_or(i32::MIN),
            key: info.key.to_owned(),
        }
    }
}

/// What is stored for a profile (without revealing values).
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CredentialsStatus {
    /// Fields the profile requires.
    pub required: Vec<CredentialField>,
    /// Fields already in the keyring.
    pub stored: Vec<CredentialField>,
    /// Stored username (XAuth or EAP), masked (e.g. `us***lo`).
    pub masked_user: Option<String>,
    /// Is the system keyring available ("remember" possible)?
    pub keyring_available: bool,
    /// Stored client certificate (profiles with a certificate).
    pub client_certificate: Option<ClientCertSummary>,
}

/// Credentials typed in the UI (empty = use the stored ones). `Debug` does not
/// show the values.
#[derive(Clone, Default, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CredentialsInput {
    /// Group PSK.
    #[serde(default)]
    #[specta(optional)]
    pub psk: Option<String>,
    /// XAuth username.
    #[serde(default)]
    #[specta(optional)]
    pub xauth_user: Option<String>,
    /// XAuth password.
    #[serde(default)]
    #[specta(optional)]
    pub xauth_password: Option<String>,
    /// EAP username.
    #[serde(default)]
    #[specta(optional)]
    pub eap_identity: Option<String>,
    /// EAP password.
    #[serde(default)]
    #[specta(optional)]
    pub eap_password: Option<String>,
    /// PIN of the smartcard or token (ADR-0029): used for the login on the
    /// token and dropped, never stored.
    #[serde(default)]
    #[specta(optional)]
    pub token_pin: Option<String>,
}

impl fmt::Debug for CredentialsInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let given = |v: &Option<String>| v.as_ref().is_some_and(|s| !s.is_empty());
        f.debug_struct("CredentialsInput")
            .field("psk", &given(&self.psk))
            .field("xauth_user", &given(&self.xauth_user))
            .field("xauth_password", &given(&self.xauth_password))
            .field("eap_identity", &given(&self.eap_identity))
            .field("eap_password", &given(&self.eap_password))
            .field("token_pin", &given(&self.token_pin))
            .finish()
    }
}

/// Active connection (without identities).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    /// Connected profile.
    pub profile_id: Option<String>,
    /// Tunnel established (otherwise: engine up, negotiating or reconnecting).
    pub established: bool,
    /// Gateway.
    pub gateway: String,
    /// IP virtual.
    pub vip: Option<String>,
    /// Reachable networks.
    pub remote_ts: Vec<String>,
    /// Seconds since it was established.
    pub established_secs: Option<u32>,
    /// Bytes received.
    pub bytes_in: f64,
    /// Bytes sent.
    pub bytes_out: f64,
}

impl From<&ConnectionInfo> for Connection {
    #[allow(clippy::cast_precision_loss)] // byte counters: f64 is exact up to 2^53
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

/// The connection as the status monitor sees it, sent to the UI on every
/// tick while the window is visible (one monitor for the tray and the UI).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct StatusUpdate {
    /// The active connection, or `None`.
    pub connection: Option<Connection>,
}

/// A newer version was found by the optional update check.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct UpdateFound {
    /// The new version (`X.Y.Z`).
    pub version: String,
}

/// The app settings the UI shows.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppSettingsView {
    /// Closing the window keeps the app in the tray.
    pub close_to_tray: bool,
    /// The optional update check is on.
    pub update_check: bool,
    /// The update check is built in (packagers may leave it out).
    pub update_check_available: bool,
    /// A newer version found by the check, if any.
    pub available_update: Option<String>,
}

/// Session update, sent to the UI as an event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type, tauri_specta::Event)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SessionUpdate {
    /// Preparing (the step, as text).
    Preparing {
        /// Step.
        step: String,
    },
    /// Negotiating with the gateway.
    Negotiating,
    /// Connected.
    Connected {
        /// Connection.
        connection: Connection,
    },
    /// A charon log line (identities masked).
    Log {
        /// Line.
        line: String,
    },
    /// A line of the engine image build (first use).
    Build {
        /// Line.
        line: String,
    },
    /// The tunnel went down (it may reconnect by itself).
    TunnelDown,
    /// The tunnel came (back) up.
    TunnelUp,
    /// The engine stopped.
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

/// Kind of error, so the UI can choose how to show it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum UiErrorKind {
    /// There is already an active connection.
    AlreadyConnected,
    /// The environment does not allow connecting (details with hints).
    Preflight,
    /// The negotiation failed (message = likely cause).
    Negotiation,
    /// A credential is missing.
    MissingCredentials,
    /// Profile missing or invalid.
    Profile,
    /// System keyring.
    Keyring,
    /// Engine/Docker.
    Engine,
    /// The network component is not installed (the UI offers to install it).
    ComponentMissing,
    /// The installed network component is old (the UI offers to update it).
    ComponentOutdated,
    /// Installing or removing the network component.
    Component,
    /// The user cancelled (e.g. closed the password prompt).
    Cancelled,
    /// Another operation in progress.
    Busy,
    /// Another error.
    Other,
}

/// Error for the UI: a message in the interface language and optional details
/// (no secrets).
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UiError {
    /// Kind.
    pub kind: UiErrorKind,
    /// Main message.
    pub message: String,
    /// Details (e.g. checks that failed, charon's message).
    pub details: Vec<String>,
}

impl UiError {
    /// Error without details.
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

/// State of the network component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ComponentState {
    /// Nothing installed.
    NotInstalled,
    /// Installed and working.
    Ready,
    /// Installed, but the app carries a different version.
    Outdated,
    /// Broken installation (the helper does not answer).
    Broken,
}

/// Network component: what is installed and what can be done.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ComponentInfo {
    /// State.
    pub state: ComponentState,
    /// Version of IPsec Client that installed the component.
    pub installed_version: Option<String>,
    /// Installed strongSwan.
    pub installed_strongswan: Option<String>,
    /// Version of the component that comes with this app.
    pub bundled_version: Option<String>,
    /// strongSwan that comes with this app.
    pub bundled_strongswan: Option<String>,
    /// Installed by a system package (.rpm/.deb): the app does not touch it.
    pub managed_by_package: bool,
    /// The app can install or update it from here.
    pub can_install: bool,
    /// The app can remove it from here.
    pub can_uninstall: bool,
    /// Reason, when there is a problem.
    pub problem: Option<String>,
}

impl ComponentInfo {
    /// Combines the state, the app's package and whether `pkexec` is available.
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

/// First letter in upper case (the core's messages start in lower case).
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
            eap_identity: Some("usuario.eap".into()),
            eap_password: Some("senha-eap".into()),
            token_pin: Some("pin-token-9".into()),
        };
        let debug = format!("{input:?}");
        assert!(!debug.contains("pin-token-9"), "{debug}");
        assert!(
            !debug.contains("psk-secreta")
                && !debug.contains("usuario")
                && !debug.contains("senha-eap"),
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

/// Kind of the imported file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ImportKind {
    /// IPsec Client profile (.json).
    Json,
    /// `swanctl.conf` of a VPN that already works.
    Swanctl,
    /// Cisco VPN Client profile (`.pcf`).
    CiscoPcf,
    /// vpnc configuration.
    Vpnc,
    /// NetworkManager connection (`.nmconnection`).
    NetworkManager,
    /// strongSwan/libreswan `ipsec.conf`.
    IpsecConf,
    /// FortiClient configuration backup (XML).
    FortiClient,
}

impl ImportKind {
    /// The format of another VPN client, if it is one of them.
    #[must_use]
    pub fn legacy(self) -> Option<ipsec_client_core::profiles::LegacyFormat> {
        use ipsec_client_core::profiles::LegacyFormat as L;
        Some(match self {
            Self::Json | Self::Swanctl => return None,
            Self::CiscoPcf => L::CiscoPcf,
            Self::Vpnc => L::Vpnc,
            Self::NetworkManager => L::NetworkManager,
            Self::IpsecConf => L::IpsecConf,
            Self::FortiClient => L::FortiClient,
        })
    }

    /// The kind of a format from another VPN client.
    #[must_use]
    pub fn from_legacy(format: ipsec_client_core::profiles::LegacyFormat) -> Self {
        use ipsec_client_core::profiles::LegacyFormat as L;
        match format {
            L::CiscoPcf => Self::CiscoPcf,
            L::Vpnc => Self::Vpnc,
            L::NetworkManager => Self::NetworkManager,
            L::IpsecConf => Self::IpsecConf,
            L::FortiClient => Self::FortiClient,
        }
    }
}

/// Preview of a file to import. The file stays in Rust, and the UI only gets
/// the `token` (the webview never handles paths).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    /// Identifies the file on confirmation.
    pub token: String,
    /// File name (without directory).
    pub file_name: String,
    /// Detected kind.
    pub kind: ImportKind,
    /// Suggested id.
    pub suggested_id: String,
    /// Suggested name.
    pub suggested_name: String,
    /// Gateway (if the file could be read).
    pub gateway: Option<String>,
    /// Remote networks.
    pub remote_ts: Vec<String>,
    /// Warnings (e.g. ignored swanctl.conf keys).
    pub warnings: Vec<String>,
    /// Problem that prevents importing (invalid file).
    pub error: Option<String>,
    /// Is there already a profile with the suggested id?
    pub exists: bool,
    /// Security level (if the file could be read).
    pub security: Option<Security>,
}

/// Import confirmation options.
#[derive(Debug, Clone, Default, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportOptions {
    /// Profile id (empty = the suggested one).
    #[serde(default)]
    #[specta(optional)]
    pub id: Option<String>,
    /// Display name (empty = the suggested one).
    #[serde(default)]
    #[specta(optional)]
    pub name: Option<String>,
    /// DNS servers (swanctl.conf only; the JSON has its own).
    #[serde(default)]
    pub dns_servers: Vec<String>,
    /// DNS domains (swanctl.conf only).
    #[serde(default)]
    pub dns_domains: Vec<String>,
    /// Also use the DNS servers sent by the gateway (swanctl.conf only).
    #[serde(default)]
    pub use_received_dns: bool,
    /// Replace an existing profile with the same id.
    #[serde(default)]
    pub overwrite: bool,
}

/// Profile details (read-only; there are no credentials in a profile).
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProfileDetails {
    /// Summary.
    pub summary: ProfileSummary,
    /// "IKEv1 aggressive", "IKEv2"...
    pub ike: String,
    /// Phase 1 proposals.
    pub ike_proposals: Vec<String>,
    /// Phase 2 proposals.
    pub esp_proposals: Vec<String>,
    /// Authentication, as text.
    pub auth: String,
    /// Local identity (e.g. the group id).
    pub local_id: Option<String>,
    /// Fixed DNS servers.
    pub dns_servers: Vec<String>,
    /// DNS domains.
    pub dns_domains: Vec<String>,
    /// Uses the DNS servers sent by the gateway.
    pub use_received_dns: bool,
    /// MTU.
    pub mtu: u16,
    /// Weaknesses, the worst first (empty = nothing to point out).
    pub security_notes: Vec<SecurityNote>,
}

impl From<&Profile> for ProfileDetails {
    fn from(p: &Profile) -> Self {
        let ike = match (p.ike.version, p.ike.aggressive) {
            (1, true) => tr!("IKEv1 (aggressive mode)", "IKEv1 (modo agressivo)").to_owned(),
            (1, false) => tr!("IKEv1 (main mode)", "IKEv1 (modo principal)").to_owned(),
            (v, _) => format!("IKEv{v}"),
        };
        let xauth = match (p.auth.uses_xauth(), p.auth.otp()) {
            (false, _) => None,
            (true, false) => Some(tr!(
                "username and password (XAuth)",
                "usuário e senha (XAuth)"
            )),
            (true, true) => Some(tr!(
                "username and one-time code (XAuth)",
                "usuário e código de uso único (XAuth)"
            )),
        };
        let auth = if p.auth.uses_eap() {
            tr!(
                "username and password (EAP), gateway certificate",
                "usuário e senha (EAP), certificado do gateway"
            )
            .to_owned()
        } else if p.auth.uses_client_certificate() {
            match xauth {
                Some(x) => trf!(
                    "your certificate + {x}, gateway certificate",
                    "o seu certificado + {x}, certificado do gateway"
                ),
                None => tr!(
                    "your certificate, gateway certificate",
                    "o seu certificado, certificado do gateway"
                )
                .to_owned(),
            }
        } else {
            match xauth {
                Some(x) => trf!("group PSK + {x}", "PSK do grupo + {x}"),
                None => "PSK".to_owned(),
            }
        };
        Self {
            summary: ProfileSummary::from(p),
            ike,
            ike_proposals: p.ike.proposals.clone(),
            esp_proposals: p.child.esp_proposals.clone(),
            auth,
            local_id: p.auth.psk_id().map(str::to_owned),
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
            security_notes: p
                .security()
                .findings
                .iter()
                .map(SecurityNote::from)
                .collect(),
        }
    }
}

/// Result of an environment check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum CheckLevel {
    /// All good.
    Ok,
    /// Works, with a limitation.
    Warn,
    /// Prevents connecting.
    Fail,
}

/// A diagnostics check (the CLI's `doctor`).
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticCheck {
    /// What was checked.
    pub name: String,
    /// Result.
    pub level: CheckLevel,
    /// Detail.
    pub detail: String,
    /// How to fix it.
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
