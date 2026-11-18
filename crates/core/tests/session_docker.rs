//! Sessão completa contra o gateway de teste (feature `docker-tests`):
//! `session::connect` (preflight, imagem, engine, eventos, negociação),
//! `status`, `disconnect` e o diagnóstico de uma senha XAuth errada.
//!
//! Usa os nomes de produção (container `ipsec-client-engine`, interface
//! `ipsc0`): o teste se recusa a rodar se já houver uma conexão ativa ou se as
//! portas do IKE estiverem ocupadas.
#![cfg(feature = "docker-tests")]
#![allow(clippy::expect_used, clippy::unwrap_used)] // código de teste

mod support;

use std::time::Duration;

use ipsec_client_core::engine::{CONTAINER_NAME, DEFAULT_IFACE, DockerEngine};
use ipsec_client_core::profiles::{CredentialKind, Profile, parse_profile};
use ipsec_client_core::secrets::{Credentials, MemoryStore, SecretStore};
use ipsec_client_core::session::{
    self, ConnectOptions, FailureKind, SessionError, SessionEvent, SessionState,
};
use secrecy::SecretString;
use support::{
    TEST_PSK, TEST_XAUTH_PASSWORD, TEST_XAUTH_USER, TestGateway, host_cmd, ike_ports_busy,
    link_details,
};
use tokio::sync::mpsc;

fn profile(gateway: &str) -> Profile {
    let mut p = parse_profile(include_str!("fixtures/perfil-gateway-teste.json")).expect("perfil");
    p.gateway = gateway.to_owned();
    p
}

fn credentials(password: &str) -> Credentials {
    let mut c = Credentials::new();
    c.set(CredentialKind::Psk, SecretString::from(TEST_PSK));
    c.set(
        CredentialKind::XauthUser,
        SecretString::from(TEST_XAUTH_USER),
    );
    c.set(CredentialKind::XauthPassword, SecretString::from(password));
    c
}

#[tokio::test]
async fn session_connect_status_disconnect_and_failure_diagnosis() {
    if ike_ports_busy() {
        eprintln!("pulando: UDP 500/4500 em uso (outra VPN IPsec conectada?)");
        return;
    }
    let engine = DockerEngine::connect().expect("Docker");
    if engine.is_running(CONTAINER_NAME).await.expect("inspect") {
        eprintln!("pulando: já existe um engine do ipsec-client rodando");
        return;
    }
    let image = engine.ensure_image(|_| {}).await.expect("imagem");
    let gateway = TestGateway::start(&image, "ipsec-client-rs-session").await;

    let result = tokio::spawn(scenario(gateway.ip.clone())).await;

    let _ = session::disconnect().await;
    gateway.stop().await;
    assert!(
        link_details(DEFAULT_IFACE).is_none(),
        "a {DEFAULT_IFACE} sobrou"
    );
    if let Err(e) = result {
        std::panic::resume_unwind(e.into_panic());
    }
}

async fn scenario(gateway_ip: String) {
    let profile = profile(&gateway_ip);

    // 1) Wrong XAuth password: diagnosed error, nothing left running, and
    //    with "remember" nothing is stored (audit P2).
    let store = MemoryStore::new();
    let err = session::connect_then_remember(
        &profile,
        &credentials("senha-errada"),
        &ConnectOptions::default(),
        None,
        Some(&store),
    )
    .await
    .expect_err("deveria falhar");
    for kind in [
        CredentialKind::Psk,
        CredentialKind::XauthUser,
        CredentialKind::XauthPassword,
    ] {
        assert!(
            store.get(&profile.id, kind).expect("store").is_none(),
            "{kind:?} stored after a failed connection"
        );
    }
    match &err {
        SessionError::Negotiation { kind, detail } => {
            assert_eq!(*kind, FailureKind::XauthCredentials, "{err}");
            assert!(
                !detail.contains(TEST_XAUTH_USER),
                "identidade sem máscara: {detail}"
            );
        }
        other => panic!("erro inesperado: {other}"),
    }
    assert!(session::status().await.expect("status").is_none());
    assert!(link_details(DEFAULT_IFACE).is_none());

    // 2) Conexão real, com eventos.
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (info, stored) = session::connect_then_remember(
        &profile,
        &credentials(TEST_XAUTH_PASSWORD),
        &ConnectOptions::default(),
        Some(tx),
        Some(&store),
    )
    .await
    .expect("conexão");
    assert!(matches!(stored, Some(Ok(()))), "credentials not stored");
    assert!(
        store
            .get(&profile.id, CredentialKind::XauthPassword)
            .expect("store")
            .is_some(),
        "the password of the established connection was not stored"
    );
    assert!(info.is_established());
    assert_eq!(info.profile_id.as_deref(), Some("gateway-teste"));
    assert!(info.vip.as_deref().unwrap_or("").starts_with("198.51.100."));
    assert_eq!(info.remote_ts, ["192.0.2.0/24"]);

    let mut states = Vec::new();
    let mut logs = Vec::new();
    while let Ok(event) = rx.try_recv() {
        match event {
            SessionEvent::State(s) => states.push(s),
            SessionEvent::Log(l) => logs.push(l),
            _ => {}
        }
    }
    assert!(
        matches!(states.last(), Some(SessionState::Connected(_))),
        "{states:?}"
    );
    assert!(states.contains(&SessionState::Negotiating));
    assert!(
        logs.iter()
            .all(|l| !l.contains(TEST_XAUTH_USER) && !l.contains("grupo-teste")),
        "identidade sem máscara nos eventos"
    );

    let (pinged, _) = host_cmd("ping", &["-c", "2", "-W", "2", "-q", "192.0.2.1"]);
    assert!(pinged, "ping pelo túnel falhou");

    // 3) Uma segunda conexão é recusada enquanto há uma ativa.
    let again = session::connect(
        &profile,
        &credentials(TEST_XAUTH_PASSWORD),
        &ConnectOptions::default(),
        None,
    )
    .await;
    assert!(matches!(again, Err(SessionError::AlreadyConnected(ref p)) if p == "gateway-teste"));

    // 4) status() de "outro processo" (sem estado em memória).
    let status = session::status().await.expect("status").expect("conectado");
    assert!(status.is_established());
    assert!(status.bytes_in > 0, "{status:?}");

    // 5) disconnect() avisa o gateway e derruba tudo.
    assert!(session::disconnect().await.expect("disconnect"));
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(session::status().await.expect("status").is_none());
    assert!(link_details(DEFAULT_IFACE).is_none(), "a interface sobrou");
    assert!(!session::disconnect().await.expect("disconnect idempotente"));
}
