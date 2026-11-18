//! VPN session: preflight, engine, negotiation, state and teardown.
//!
//! [`connect`] flow (docs/architecture.md, "Connection flow"):
//! preflight → image → engine → VICI events → secrets + connection →
//! `initiate` → *Connected* state. On any failure after the engine is up, it
//! is torn down (the host is left clean).
//!
//! The engine runs apart from the app's process: once connected, [`status`]
//! and [`disconnect`] work from any process (for example, another terminal or
//! the GUI).

pub mod agent;
mod establish;
mod failure;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use secrecy::ExposeSecret;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, warn};

pub use establish::establish;
pub use failure::{FailureKind, ProfileAuth, diagnose};

use crate::diagnostics::{self, Check, CheckStatus};
use crate::engine::{Backend, BuildProgress, EngineError, RunningEngine};
use crate::profiles::{Profile, ProfileError};
use crate::secrets::{
    Anonymizer, Credentials, Redactor, SecretStore, SecretsError, TokenError, TokenRef,
    TokenSigner, printable,
};
use crate::vici::{Charon, CharonEvent, IkeSa, ViciClient, ViciError, ViciEvents};

/// Log lines kept to diagnose failures.
const LOG_BUFFER: usize = 500;

/// Session state (for the UI/CLI).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionState {
    /// Preparing: preflight, image, engine.
    Preparing(String),
    /// Negotiating with the gateway.
    Negotiating,
    /// Tunnel up.
    Connected(ConnectionInfo),
}

/// Session event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEvent {
    /// State change.
    State(SessionState),
    /// A charon log line, with identities masked.
    Log(String),
    /// Progress of the image build.
    Build(String),
    /// The tunnel went down (charon may reconnect by itself, depending on the
    /// profile).
    TunnelDown,
    /// The tunnel came (back) up.
    TunnelUp,
    /// The connection to the engine ended (engine stopped or dead).
    EngineGone,
}

/// Session event channel.
pub type EventSink = UnboundedSender<SessionEvent>;

/// Summary of an active connection (without identities).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConnectionInfo {
    /// Connected profile (the engine's label).
    pub profile_id: Option<String>,
    /// IKE_SA state (`ESTABLISHED`...), or empty if there is none.
    pub state: String,
    /// Gateway.
    pub gateway: String,
    /// IP virtual.
    pub vip: Option<String>,
    /// Reachable remote networks.
    pub remote_ts: Vec<String>,
    /// Seconds since it was established.
    pub established_secs: Option<u64>,
    /// Bytes received (all CHILD_SAs).
    pub bytes_in: u64,
    /// Bytes sent.
    pub bytes_out: u64,
}

impl ConnectionInfo {
    fn from_sa(profile_id: Option<String>, sa: &IkeSa) -> Self {
        Self {
            profile_id,
            state: sa.state.clone(),
            gateway: sa.remote_host.clone(),
            vip: sa.local_vips.first().cloned(),
            remote_ts: sa
                .children
                .iter()
                .flat_map(|c| c.remote_ts.iter().cloned())
                .collect(),
            established_secs: sa.established_secs,
            bytes_in: sa.children.iter().map(|c| c.bytes_in).sum(),
            bytes_out: sa.children.iter().map(|c| c.bytes_out).sum(),
        }
    }

    /// Is a tunnel established?
    #[must_use]
    pub fn is_established(&self) -> bool {
        self.state == "ESTABLISHED"
    }
}

/// Connection options.
#[derive(Debug, Clone)]
pub struct ConnectOptions {
    /// Fallback privileged mode (ADR-0005).
    pub privileged: bool,
    /// Maximum negotiation time.
    pub initiate_timeout: Duration,
}

impl Default for ConnectOptions {
    fn default() -> Self {
        Self {
            privileged: false,
            initiate_timeout: Duration::from_secs(30),
        }
    }
}

/// Session errors. Messages are for the end user (see [`crate::i18n`]).
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// An engine is already running.
    #[error("{}", crate::trf!(
        "a connection is already active (profile: {}); disconnect first",
        "já existe uma conexão ativa (perfil: {}); desconecte antes",
        .0
    ))]
    AlreadyConnected(String),
    /// The environment does not allow connecting.
    #[error("{}", crate::trf!(
        "this computer cannot connect yet: {}",
        "o ambiente não permite conectar: {}",
        describe_failures(.0)
    ))]
    Preflight(Vec<Check>),
    /// The negotiation failed.
    #[error("{kind} (charon: {detail})")]
    Negotiation {
        /// Likely cause.
        kind: FailureKind,
        /// charon's message (identities masked).
        detail: String,
    },
    /// The tunnel did not show up after a successful negotiation.
    #[error("{}", crate::tr!(
        "the negotiation finished, but the tunnel is not active",
        "a negociação terminou, mas o túnel não está ativo"
    ))]
    NotEstablished,
    /// Engine error.
    #[error(transparent)]
    Engine(#[from] EngineError),
    /// VICI error.
    #[error(transparent)]
    Vici(#[from] ViciError),
    /// Profile error.
    #[error(transparent)]
    Profile(#[from] ProfileError),
    /// Credentials error.
    #[error(transparent)]
    Secrets(#[from] SecretsError),
    /// Smartcard or token (ADR-0029).
    #[error(transparent)]
    Token(#[from] TokenError),
    /// The key agent of a token could not start.
    #[error("{}", crate::trf!(
        "the smartcard/token key agent could not start: {}",
        "o agente de chaves do smartcard/token não pôde iniciar: {}",
        .0
    ))]
    TokenAgent(String),
    /// Tokens need the embedded engine (the Docker engine is only for
    /// development).
    #[error("{}", crate::tr!(
        "a smartcard or token needs the network component (not the Docker engine)",
        "um smartcard ou token precisa do componente de rede (não do engine Docker)"
    ))]
    TokenNeedsNativeEngine,
}

fn describe_failures(checks: &[Check]) -> String {
    checks
        .iter()
        .filter(|c| c.status == CheckStatus::Fail)
        .map(|c| {
            format!(
                "{}: {}{}",
                c.name,
                c.detail,
                c.hint
                    .as_ref()
                    .map(|h| format!(" ({h})"))
                    .unwrap_or_default()
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn send(events: Option<&EventSink>, event: SessionEvent) {
    if let Some(tx) = events {
        let _ = tx.send(event);
    }
}

/// Identities to mask in this session's logs.
#[must_use]
pub fn redactor_for(profile: &Profile, credentials: &Credentials) -> Redactor {
    // The client certificate's subject values and alternative names (the
    // person's name, e-mail, organization) appear in charon's log too.
    let cert_names: Vec<String> = credentials
        .client_certificate()
        .and_then(Result::ok)
        .map(|cert| cert.info().names.clone())
        .unwrap_or_default();
    let mut identities: Vec<&str> = Vec::new();
    identities.extend(credentials.xauth_user());
    identities.extend(credentials.eap_identity());
    identities.extend(profile.auth.psk_id());
    identities.extend(profile.auth.remote_id());
    identities.extend(cert_names.iter().map(String::as_str));
    Redactor::new(identities)
}

/// Anonymizer for the log that leaves the app (copy, bug report): besides the
/// identities, the profile's gateway, DNS servers and domains, and every IP.
#[must_use]
pub fn anonymizer_for(profile: &Profile, user: Option<&str>) -> Anonymizer {
    let mut identities: Vec<&str> = Vec::new();
    identities.extend(user);
    identities.extend(profile.auth.psk_id());
    identities.extend(profile.auth.remote_id());
    let dns = &profile.network.dns;
    Anonymizer::new(identities, &profile.gateway, &dns.servers, &dns.domains)
}

/// Connects like [`connect`] and, only once the tunnel is up, stores the
/// credentials in `store` (audit P2: a mistyped password must not end up in
/// the keyring). Storing happens after the connection, so its failure does
/// not undo it: it comes back as the second value (`None` without a store).
///
/// # Errors
/// The same as [`connect`]; nothing is stored then.
pub async fn connect_then_remember(
    profile: &Profile,
    credentials: &Credentials,
    options: &ConnectOptions,
    events: Option<EventSink>,
    store: Option<&dyn SecretStore>,
) -> Result<(ConnectionInfo, Option<Result<(), SecretsError>>), SessionError> {
    let info = connect(profile, credentials, options, events).await?;
    let stored = store.map(|store| credentials.save(store, profile));
    Ok((info, stored))
}

/// Connects the profile. Returns when the tunnel is up; the following events
/// (drops, reconnections, logs) keep arriving on `events` while the runtime is
/// alive.
///
/// # Errors
/// Connection already active, preflight, image, engine or negotiation (with the
/// likely cause in [`SessionError::Negotiation`]).
pub async fn connect(
    profile: &Profile,
    credentials: &Credentials,
    options: &ConnectOptions,
    events: Option<EventSink>,
) -> Result<ConnectionInfo, SessionError> {
    // Credential or profile failures before touching anything.
    credentials.shared_secrets(profile)?;

    let backend = Backend::detect()?;
    if profile.auth.uses_token() && !matches!(backend, Backend::Native(_)) {
        return Err(SessionError::TokenNeedsNativeEngine);
    }
    if let Some(active) = backend.running_profile().await? {
        return Err(SessionError::AlreadyConnected(active));
    }

    send(
        events.as_ref(),
        SessionEvent::State(SessionState::Preparing(
            crate::tr!("checking the environment", "verificando o ambiente").into(),
        )),
    );
    let checks = diagnostics::preflight(true).await;
    if checks.iter().any(|c| c.status == CheckStatus::Fail) {
        return Err(SessionError::Preflight(checks));
    }

    if backend.is_docker() {
        send(
            events.as_ref(),
            SessionEvent::State(SessionState::Preparing(
                crate::tr!("engine image", "imagem do engine").into(),
            )),
        );
    }
    backend
        .prepare(|p| {
            if let BuildProgress::Step(line) = p {
                send(events.as_ref(), SessionEvent::Build(line));
            }
        })
        .await?;

    send(
        events.as_ref(),
        SessionEvent::State(SessionState::Preparing(
            crate::tr!("starting the engine", "subindo o engine").into(),
        )),
    );
    let engine = backend.start(profile, options.privileged).await?;

    let result = negotiate(&engine, profile, credentials, options, events).await;
    if result.is_err() {
        set_token_agent(None);
        if let Err(e) = backend.stop().await {
            warn!("failed to stop the engine after an error: {e}");
        }
    }
    result
}

async fn negotiate(
    engine: &RunningEngine,
    profile: &Profile,
    credentials: &Credentials,
    options: &ConnectOptions,
    events: Option<EventSink>,
) -> Result<ConnectionInfo, SessionError> {
    let socket = engine.socket.clone();
    let redactor = redactor_for(profile, credentials);
    let logs = Arc::new(Mutex::new(VecDeque::with_capacity(LOG_BUFFER)));

    let subscription =
        ViciEvents::subscribe(&socket, &["ike-updown", "child-updown", "log"]).await?;
    tokio::spawn(forward_events(
        subscription,
        redactor.clone(),
        Arc::clone(&logs),
        events.clone(),
    ));

    if profile.auth.uses_token() {
        start_token_agent(std::path::Path::new(&socket), credentials).await?;
    }
    let mut charon = ViciClient::connect(&socket).await?;
    send(
        events.as_ref(),
        SessionEvent::State(SessionState::Negotiating),
    );
    let outcome = establish(
        &mut charon,
        profile,
        credentials,
        &engine.iface,
        engine.if_id,
        engine.updown,
        options.initiate_timeout,
    )
    .await;
    if let Err(SessionError::Negotiation { detail, .. }) = outcome {
        // Log events arrive over another connection: give the last ones a moment to
        // arrive before the diagnosis.
        tokio::time::sleep(Duration::from_millis(300)).await;
        let lines: Vec<String> = logs
            .lock()
            .map(|l| l.iter().cloned().collect())
            .unwrap_or_default();
        return Err(SessionError::Negotiation {
            kind: diagnose(&lines, failure::ProfileAuth::of(profile)),
            detail: redactor.redact(&detail),
        });
    }
    outcome?;

    let info = charon
        .list_sas()
        .await?
        .iter()
        .find(|sa| sa.name == profile.conn_name())
        .map(|sa| ConnectionInfo::from_sa(Some(profile.id.clone()), sa))
        .filter(ConnectionInfo::is_established)
        .ok_or(SessionError::NotEstablished)?;
    send(
        events.as_ref(),
        SessionEvent::State(SessionState::Connected(info.clone())),
    );
    Ok(info)
}

async fn forward_events(
    mut subscription: ViciEvents,
    redactor: Redactor,
    logs: Arc<Mutex<VecDeque<String>>>,
    events: Option<EventSink>,
) {
    loop {
        let (name, message) = match subscription.next().await {
            Ok(event) => event,
            Err(e) => {
                debug!("fim dos eventos do engine: {e}");
                send(events.as_ref(), SessionEvent::EngineGone);
                return;
            }
        };
        match CharonEvent::parse(&name, &message) {
            CharonEvent::Log { message, .. } => {
                let line = redactor.redact(&printable(&message));
                if let Ok(mut buffer) = logs.lock() {
                    if buffer.len() == LOG_BUFFER {
                        buffer.pop_front();
                    }
                    buffer.push_back(line.clone());
                }
                send(events.as_ref(), SessionEvent::Log(line));
            }
            CharonEvent::ChildUpDown { up: true, .. } => {
                send(events.as_ref(), SessionEvent::TunnelUp);
            }
            CharonEvent::ChildUpDown { up: false, .. } => {
                send(events.as_ref(), SessionEvent::TunnelDown);
            }
            CharonEvent::IkeUpDown { .. } | CharonEvent::Other(_) => {}
        }
    }
}

/// The key agent of the current session, when its key is on a token
/// (ADR-0029). It lives in this process as long as the VPN: dropping it stops
/// the agent and removes its socket.
static TOKEN_AGENT: Mutex<Option<agent::AgentHandle>> = Mutex::new(None);

fn set_token_agent(handle: Option<agent::AgentHandle>) {
    if let Ok(mut slot) = TOKEN_AGENT.lock() {
        *slot = handle;
    }
}

/// Stops the token's key agent of this process, if one runs (for a process
/// that ends while the VPN is going down: the CLI in the foreground, the app
/// on quit).
pub fn stop_token_agent() {
    set_token_agent(None);
}

/// Opens the token with the PIN and starts the key agent next to the VICI
/// socket, before charon is told about the key.
async fn start_token_agent(
    socket: &std::path::Path,
    credentials: &Credentials,
) -> Result<(), SessionError> {
    use crate::profiles::CredentialKind as K;
    let missing = |kind| SessionError::Secrets(SecretsError::Missing(kind));
    let reference = credentials
        .get(K::TokenKey)
        .ok_or_else(|| missing(K::TokenKey))?;
    let reference = TokenRef::from_json(reference.expose_secret())?;
    let pin = credentials
        .get(K::TokenPin)
        .ok_or_else(|| missing(K::TokenPin))?
        .clone();
    let cert = credentials
        .client_cert_pem()
        .ok_or_else(|| missing(K::ClientCert))?
        .to_owned();
    let signer = tokio::task::spawn_blocking(move || TokenSigner::open(&reference, &pin, &cert))
        .await
        .map_err(|e| SessionError::TokenAgent(e.to_string()))??;
    let dir = socket
        .parent()
        .ok_or_else(|| SessionError::TokenAgent("socket without a directory".to_owned()))?;
    let handle = agent::start(&dir.join("agent").join("agent.sock"), signer)
        .map_err(|e| SessionError::TokenAgent(e.to_string()))?;
    set_token_agent(Some(handle));
    Ok(())
}

/// State of the active connection, from any process (`None` = disconnected).
///
/// # Errors
/// Engine (helper/Docker) or VICI unreachable.
pub async fn status() -> Result<Option<ConnectionInfo>, SessionError> {
    let backend = Backend::detect()?;
    let Some(profile_id) = backend.running_profile().await? else {
        return Ok(None);
    };
    let profile_id = Some(profile_id);
    let mut charon = ViciClient::connect(backend.socket_path()?).await?;
    let sas = charon.list_sas().await?;
    Ok(Some(match sas.first() {
        Some(sa) => ConnectionInfo::from_sa(profile_id, sa),
        // Engine up, but no tunnel (negotiating or reconnecting).
        None => ConnectionInfo {
            profile_id,
            ..Default::default()
        },
    }))
}

/// Ends the active connection: `terminate` (tells the gateway) and tears the
/// engine down. Returns `false` if there was no connection.
///
/// # Errors
/// Engine (helper/Docker) unreachable.
pub async fn disconnect() -> Result<bool, SessionError> {
    let backend = Backend::detect()?;
    if backend.running_profile().await?.is_some()
        && let Ok(socket) = backend.socket_path()
        && let Ok(mut charon) = ViciClient::connect(socket).await
        && let Ok(sas) = charon.list_sas().await
    {
        for sa in sas {
            if let Err(e) = charon.terminate(&sa.name, Duration::from_secs(5)).await {
                debug!("terminate of {}: {e}", sa.name);
            }
        }
    }
    set_token_agent(None);
    // Also removes a stopped container left behind (Docker).
    Ok(backend.stop().await?)
}

/// Last lines of the running engine's log (identities not masked yet).
///
/// # Errors
/// Engine unreachable.
pub async fn engine_logs(tail: usize) -> Result<Vec<String>, SessionError> {
    let lines = Backend::detect()?.logs(tail).await?;
    // The log has text chosen by the gateway: nothing in it may reach a
    // terminal or the screen as a control character.
    Ok(lines.iter().map(|line| printable(line)).collect())
}

/// Profile of the running engine, if any.
///
/// # Errors
/// Engine unreachable.
pub async fn active_profile() -> Result<Option<String>, SessionError> {
    Ok(Backend::detect()?.running_profile().await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vici::ChildSa;

    #[test]
    fn connection_info_from_sa() {
        let sa = IkeSa {
            name: "vpn".into(),
            state: "ESTABLISHED".into(),
            remote_host: "203.0.113.1".into(),
            local_vips: vec!["198.51.100.10".into()],
            established_secs: Some(60),
            children: vec![
                ChildSa {
                    remote_ts: vec!["192.0.2.0/24".into()],
                    bytes_in: 10,
                    bytes_out: 5,
                    ..Default::default()
                },
                ChildSa {
                    remote_ts: vec!["198.51.100.0/24".into()],
                    bytes_in: 1,
                    bytes_out: 2,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let info = ConnectionInfo::from_sa(Some("vpn".into()), &sa);
        assert!(info.is_established());
        assert_eq!(info.vip.as_deref(), Some("198.51.100.10"));
        assert_eq!(info.remote_ts.len(), 2);
        assert_eq!((info.bytes_in, info.bytes_out), (11, 7));
    }

    #[test]
    fn redactor_masks_user_and_group() {
        let r = redactor_for(
            &establish::tests::profile(),
            &establish::tests::credentials(),
        );
        let line = r.redact("XAuth authentication of 'usuario.exemplo' ok, [group-name]");
        assert!(
            !line.contains("usuario.exemplo") && !line.contains("group-name"),
            "{line}"
        );
    }

    #[test]
    fn preflight_error_lists_only_failures() {
        let checks = vec![
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
        ];
        let msg = SessionError::Preflight(checks).to_string();
        assert!(msg.contains("Docker: unreachable (usermod)"));
        assert!(!msg.contains("resolved"));
    }
}
