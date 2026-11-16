//! Sessão de VPN: preflight, engine, negociação, estado e encerramento.
//!
//! Fluxo do [`connect`] (docs/arquitetura.md, "Fluxo de conexão"):
//! preflight → imagem → engine → eventos VICI → segredos + conexão →
//! `initiate` → estado *Conectado*. Em qualquer falha depois de subir o
//! engine, ele é derrubado (o host fica limpo).
//!
//! O engine é um container independente do processo do app: depois de
//! conectado, [`status`] e [`disconnect`] funcionam a partir de qualquer
//! processo (por exemplo, outro terminal ou a GUI).

mod establish;
mod failure;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, warn};

pub use establish::establish;
pub use failure::{FailureKind, diagnose};

use crate::diagnostics::{self, Check, CheckStatus};
use crate::engine::{Backend, BuildProgress, EngineError, RunningEngine};
use crate::profiles::{Profile, ProfileError};
use crate::secrets::{Anonymizer, Credentials, Redactor, SecretsError};
use crate::vici::{Charon, CharonEvent, IkeSa, ViciClient, ViciError, ViciEvents};

/// Linhas de log guardadas para diagnóstico de falhas.
const LOG_BUFFER: usize = 500;

/// Estado da sessão (para a UI/CLI).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionState {
    /// Preparando: preflight, imagem, engine.
    Preparing(String),
    /// Negociando com o gateway.
    Negotiating,
    /// Túnel ativo.
    Connected(ConnectionInfo),
}

/// Evento da sessão.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEvent {
    /// Mudança de estado.
    State(SessionState),
    /// Linha de log do charon, com identidades mascaradas.
    Log(String),
    /// Progresso do build da imagem.
    Build(String),
    /// O túnel caiu (o charon pode reconectar sozinho, conforme o perfil).
    TunnelDown,
    /// O túnel (re)subiu.
    TunnelUp,
    /// A conexão com o engine terminou (engine parado ou morto).
    EngineGone,
}

/// Canal de eventos da sessão.
pub type EventSink = UnboundedSender<SessionEvent>;

/// Resumo de uma conexão ativa (sem identidades).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConnectionInfo {
    /// Perfil conectado (label do engine).
    pub profile_id: Option<String>,
    /// Estado do IKE_SA (`ESTABLISHED`...), ou vazio se não houver.
    pub state: String,
    /// Gateway.
    pub gateway: String,
    /// IP virtual.
    pub vip: Option<String>,
    /// Redes remotas acessíveis.
    pub remote_ts: Vec<String>,
    /// Segundos desde o estabelecimento.
    pub established_secs: Option<u64>,
    /// Bytes recebidos (todos os CHILD_SAs).
    pub bytes_in: u64,
    /// Bytes enviados.
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

    /// Há um túnel estabelecido?
    #[must_use]
    pub fn is_established(&self) -> bool {
        self.state == "ESTABLISHED"
    }
}

/// Opções de conexão.
#[derive(Debug, Clone)]
pub struct ConnectOptions {
    /// Modo privilegiado de reserva (ADR-0005).
    pub privileged: bool,
    /// Tempo máximo da negociação.
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

/// Erros da sessão. As mensagens são para o usuário final (pt-BR).
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// Já existe um engine rodando.
    #[error("já existe uma conexão ativa (perfil: {0}); desconecte antes")]
    AlreadyConnected(String),
    /// O ambiente não permite conectar.
    #[error("o ambiente não permite conectar: {}", describe_failures(.0))]
    Preflight(Vec<Check>),
    /// A negociação falhou.
    #[error("{kind} (charon: {detail})")]
    Negotiation {
        /// Causa provável.
        kind: FailureKind,
        /// Mensagem do charon (identidades mascaradas).
        detail: String,
    },
    /// O túnel não apareceu depois de uma negociação bem-sucedida.
    #[error("a negociação terminou, mas o túnel não está ativo")]
    NotEstablished,
    /// Erro do engine.
    #[error(transparent)]
    Engine(#[from] EngineError),
    /// Erro do VICI.
    #[error(transparent)]
    Vici(#[from] ViciError),
    /// Erro do perfil.
    #[error(transparent)]
    Profile(#[from] ProfileError),
    /// Erro de credenciais.
    #[error(transparent)]
    Secrets(#[from] SecretsError),
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

/// Identidades a mascarar nos logs desta sessão.
#[must_use]
pub fn redactor_for(profile: &Profile, credentials: &Credentials) -> Redactor {
    let mut identities: Vec<&str> = Vec::new();
    identities.extend(credentials.xauth_user());
    identities.extend(profile.auth.local_id.as_deref());
    Redactor::new(identities)
}

/// Anonimizador para o log que sai do app (copiar, relatório de bug): além
/// das identidades, o gateway, os DNS e os domínios do perfil e todo IP.
#[must_use]
pub fn anonymizer_for(profile: &Profile, xauth_user: Option<&str>) -> Anonymizer {
    let mut identities: Vec<&str> = Vec::new();
    identities.extend(xauth_user);
    identities.extend(profile.auth.local_id.as_deref());
    let dns = &profile.network.dns;
    Anonymizer::new(identities, &profile.gateway, &dns.servers, &dns.domains)
}

/// Conecta o perfil. Devolve quando o túnel está ativo; os eventos seguintes
/// (quedas, reconexões, logs) continuam chegando em `events` enquanto o
/// runtime estiver vivo.
///
/// # Errors
/// Conexão já ativa, preflight, imagem, engine ou negociação (com a causa
/// provável em [`SessionError::Negotiation`]).
pub async fn connect(
    profile: &Profile,
    credentials: &Credentials,
    options: &ConnectOptions,
    events: Option<EventSink>,
) -> Result<ConnectionInfo, SessionError> {
    // Falhas de credencial/perfil antes de mexer em qualquer coisa.
    credentials.shared_secrets(profile)?;

    let backend = Backend::detect()?;
    if let Some(active) = backend.running_profile().await? {
        return Err(SessionError::AlreadyConnected(active));
    }

    send(
        events.as_ref(),
        SessionEvent::State(SessionState::Preparing("verificando o ambiente".into())),
    );
    let checks = diagnostics::preflight(true).await;
    if checks.iter().any(|c| c.status == CheckStatus::Fail) {
        return Err(SessionError::Preflight(checks));
    }

    if matches!(backend, Backend::Docker(_)) {
        send(
            events.as_ref(),
            SessionEvent::State(SessionState::Preparing("imagem do engine".into())),
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
        SessionEvent::State(SessionState::Preparing("subindo o engine".into())),
    );
    let engine = backend.start(profile, options.privileged).await?;

    let result = negotiate(&engine, profile, credentials, options, events).await;
    if result.is_err()
        && let Err(e) = backend.stop().await
    {
        warn!("falha ao derrubar o engine após erro: {e}");
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
        // Os eventos de log chegam por outra conexão: dá um instante para os
        // últimos chegarem antes do diagnóstico.
        tokio::time::sleep(Duration::from_millis(300)).await;
        let lines: Vec<String> = logs
            .lock()
            .map(|l| l.iter().cloned().collect())
            .unwrap_or_default();
        return Err(SessionError::Negotiation {
            kind: diagnose(&lines),
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
                let line = redactor.redact(&message);
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

/// Estado da conexão ativa, de qualquer processo (`None` = desconectado).
///
/// # Errors
/// Engine (helper/Docker) ou VICI inacessíveis.
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
        // Engine de pé, mas sem túnel (negociando ou reconectando).
        None => ConnectionInfo {
            profile_id,
            ..Default::default()
        },
    }))
}

/// Encerra a conexão ativa: `terminate` (avisa o gateway) e derruba o engine.
/// Devolve `false` se não havia conexão.
///
/// # Errors
/// Engine (helper/Docker) inacessível.
pub async fn disconnect() -> Result<bool, SessionError> {
    let backend = Backend::detect()?;
    if backend.running_profile().await?.is_some()
        && let Ok(socket) = backend.socket_path()
        && let Ok(mut charon) = ViciClient::connect(socket).await
        && let Ok(sas) = charon.list_sas().await
    {
        for sa in sas {
            if let Err(e) = charon.terminate(&sa.name, Duration::from_secs(5)).await {
                debug!("terminate de {}: {e}", sa.name);
            }
        }
    }
    // Também remove um container parado que tenha sobrado (Docker).
    Ok(backend.stop().await?)
}

/// Últimas linhas do log do engine em uso (identidades ainda não mascaradas).
///
/// # Errors
/// Engine inacessível.
pub async fn engine_logs(tail: usize) -> Result<Vec<String>, SessionError> {
    Ok(Backend::detect()?.logs(tail).await?)
}

/// Perfil do engine em execução, se houver.
///
/// # Errors
/// Engine inacessível.
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
        let line = r.redact("XAuth authentication of 'usuario.exemplo' ok, [nome-do-grupo]");
        assert!(
            !line.contains("usuario.exemplo") && !line.contains("nome-do-grupo"),
            "{line}"
        );
    }

    #[test]
    fn preflight_error_lists_only_failures() {
        let checks = vec![
            Check {
                name: "Docker",
                status: CheckStatus::Fail,
                detail: "inacessível".into(),
                hint: Some("usermod".into()),
            },
            Check {
                name: "resolved",
                status: CheckStatus::Warn,
                detail: "ausente".into(),
                hint: None,
            },
        ];
        let msg = SessionError::Preflight(checks).to_string();
        assert!(msg.contains("Docker: inacessível (usermod)"));
        assert!(!msg.contains("resolved"));
    }
}
