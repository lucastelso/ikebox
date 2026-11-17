//! Conexão IPsec completa feita só pelo Rust (sem swanctl), contra o gateway
//! de teste local (feature `docker-tests`).
//!
//! Cadeia testada: perfil JSON (`tests/fixtures`) → validação → `load-conn`
//! gerado pelo perfil → charon real → túnel.
//! Cobre: `version`, `load-shared` (PSK + XAuth), `load-conn`, `list-conns`,
//! `initiate`, `list-sas`, eventos (`child-updown`, `log`), ping pelo túnel,
//! `terminate` e `unload-conn`.
#![cfg(feature = "docker-tests")]
#![allow(clippy::expect_used, clippy::unwrap_used)] // código de teste

mod support;

use std::time::Duration;

use ipsec_client_core::engine::{DockerEngine, EngineSpec, current_gid, default_runtime_dir};
use ipsec_client_core::profiles::{ConnParams, CredentialKind, Profile, parse_profile};
use ipsec_client_core::secrets::{Credentials, Redactor};
use ipsec_client_core::vici::{Charon, CharonEvent, ViciClient, ViciEvents};
use secrecy::SecretString;
use support::{
    TEST_GROUP_ID, TEST_PSK, TEST_XAUTH_PASSWORD, TEST_XAUTH_USER, TestGateway, host_cmd,
    ike_ports_busy, link_details,
};
use tokio::sync::mpsc;

const PREFIX: &str = "ipsec-client-rs-vici";
const IFACE: &str = "ipsct2";
const IF_ID: u32 = 7343;

fn test_profile(gateway: &str) -> Profile {
    let mut profile = parse_profile(include_str!("fixtures/perfil-gateway-teste.json"))
        .expect("perfil de teste válido");
    profile.gateway = gateway.to_owned();
    profile
}

async fn wait_for(
    rx: &mut mpsc::UnboundedReceiver<CharonEvent>,
    pred: impl Fn(&CharonEvent) -> bool,
) -> bool {
    tokio::time::timeout(Duration::from_secs(15), async {
        while let Some(event) = rx.recv().await {
            if pred(&event) {
                return true;
            }
        }
        false
    })
    .await
    .unwrap_or(false)
}

#[tokio::test]
async fn full_tunnel_through_rust_vici_client() {
    if ike_ports_busy() {
        eprintln!("pulando: UDP 500/4500 em uso (outra VPN IPsec conectada?)");
        return;
    }
    assert!(
        link_details(IFACE).is_none(),
        "a interface {IFACE} já existe"
    );

    let engine = DockerEngine::connect().expect("Docker");
    let image = engine.ensure_image(|_| {}).await.expect("imagem");
    let gateway = TestGateway::start(&image, PREFIX).await;

    let parent = default_runtime_dir()
        .expect("XDG_RUNTIME_DIR")
        .parent()
        .expect("pai")
        .to_path_buf();
    let tmp = tempfile::Builder::new()
        .prefix("ipsec-client-rs-vici.")
        .tempdir_in(parent)
        .expect("tempdir");
    let mut spec = EngineSpec::new(tmp.path().join("run"), current_gid(), IF_ID);
    spec.container_name = format!("{PREFIX}-engine");
    spec.iface = IFACE.to_owned();

    // O corpo roda num bloco separado para a limpeza acontecer sempre.
    let result = tokio::spawn(scenario(spec.clone(), gateway.ip.clone())).await;

    engine.stop(&spec.container_name).await.expect("stop");
    gateway.stop().await;
    assert!(link_details(IFACE).is_none(), "a interface sobrou");

    if let Err(e) = result {
        std::panic::resume_unwind(e.into_panic());
    }
}

async fn scenario(spec: EngineSpec, gateway_ip: String) {
    let engine = DockerEngine::connect().expect("Docker");
    engine.start(&spec).await.expect("engine pronto");
    let socket = spec.socket_path();
    let profile = test_profile(&gateway_ip);
    let conn_name = profile.conn_name().to_owned();
    let child_name = profile.child.name.clone();
    let load_conn = profile
        .to_load_conn(&ConnParams {
            iface: IFACE,
            updown: Some(ipsec_client_core::profiles::UPDOWN_PATH),
            if_id: IF_ID,
            xauth_user: Some(TEST_XAUTH_USER),
            eap_identity: None,
            client_cert: None,
        })
        .expect("load-conn gerado pelo perfil");
    let redactor = Redactor::new([TEST_XAUTH_USER, TEST_GROUP_ID]);

    // Conexão de eventos (separada da de comandos).
    let mut events = ViciEvents::subscribe(&socket, &["ike-updown", "child-updown", "log"])
        .await
        .expect("assinatura de eventos");
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (log_tx, mut log_rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Ok((name, msg)) = events.next().await {
            let event = CharonEvent::parse(&name, &msg);
            if let CharonEvent::Log { message, .. } = &event {
                let _ = log_tx.send(message.clone());
            }
            let _ = tx.send(event);
        }
    });

    let mut charon = ViciClient::connect(&socket).await.expect("VICI");
    let version = charon.version().await.expect("version");
    assert_eq!(version.daemon, "charon");
    assert_eq!(
        version.version,
        ipsec_client_core::engine::context::STRONGSWAN_VERSION
    );

    let mut credentials = Credentials::new();
    credentials.set(CredentialKind::Psk, SecretString::from(TEST_PSK));
    credentials.set(
        CredentialKind::XauthUser,
        SecretString::from(TEST_XAUTH_USER),
    );
    credentials.set(
        CredentialKind::XauthPassword,
        SecretString::from(TEST_XAUTH_PASSWORD),
    );
    for secret in credentials.shared_secrets(&profile).expect("segredos") {
        charon.load_shared(&secret).await.expect("load-shared");
    }
    charon
        .load_conn(&conn_name, &load_conn)
        .await
        .expect("load-conn");
    assert_eq!(
        charon.list_conns().await.expect("list-conns"),
        [conn_name.as_str()]
    );

    charon
        .initiate(&conn_name, &child_name, Duration::from_secs(30))
        .await
        .expect("initiate");
    assert!(
        wait_for(&mut rx, |e| matches!(
            e,
            CharonEvent::ChildUpDown { up: true, children, .. } if children.contains(&child_name)
        ))
        .await,
        "evento child-updown (up) não chegou"
    );

    let sas = charon.list_sas().await.expect("list-sas");
    let sa = sas.iter().find(|s| s.name == conn_name).expect("IKE_SA");
    assert_eq!(sa.state, "ESTABLISHED");
    assert_eq!(sa.remote_host, gateway_ip);
    let vip = sa.local_vips.first().expect("VIP").clone();
    assert!(vip.starts_with("198.51.100."), "{vip}");
    let child = sa
        .children
        .iter()
        .find(|c| c.name == child_name)
        .expect("CHILD_SA");
    assert_eq!(child.state, "INSTALLED");
    assert_eq!(child.remote_ts, ["192.0.2.0/24"]);

    let (pinged, _) = host_cmd("ping", &["-c", "2", "-W", "2", "-q", "192.0.2.1"]);
    assert!(pinged, "ping pelo túnel falhou");
    let sas = charon.list_sas().await.expect("list-sas");
    let child = &sas[0].children[0];
    assert!(child.bytes_in > 0 && child.bytes_out > 0, "{child:?}");

    // Os logs chegam e não trazem PSK nem senha.
    let mut logs = Vec::new();
    while let Ok(line) = log_rx.try_recv() {
        logs.push(line);
    }
    assert!(logs.iter().any(|l| l.contains("established")), "{logs:?}");
    assert!(
        logs.iter()
            .all(|l| !l.contains(TEST_PSK) && !l.contains(TEST_XAUTH_PASSWORD)),
        "segredo apareceu no log"
    );
    // O charon registra as identidades; o Redactor as mascara para exibição.
    assert!(logs.iter().any(|l| l.contains(TEST_XAUTH_USER)));
    let shown: Vec<String> = logs.iter().map(|l| redactor.redact(l)).collect();
    assert!(
        shown
            .iter()
            .all(|l| !l.contains(TEST_XAUTH_USER) && !l.contains("grupo-teste")),
        "identidade sem máscara: {shown:?}"
    );

    charon
        .terminate(&conn_name, Duration::from_secs(10))
        .await
        .expect("terminate");
    assert!(
        wait_for(&mut rx, |e| matches!(
            e,
            CharonEvent::ChildUpDown { up: false, .. }
        ))
        .await,
        "evento child-updown (down) não chegou"
    );
    let (_, routes) = host_cmd("ip", &["-4", "route", "show", "dev", IFACE]);
    assert!(
        routes.trim().is_empty(),
        "rotas sobraram após o down: {routes}"
    );

    charon.unload_conn(&conn_name).await.expect("unload-conn");
    assert!(charon.list_conns().await.expect("list-conns").is_empty());
}
