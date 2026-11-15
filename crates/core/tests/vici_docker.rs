//! Conexão IPsec completa feita só pelo cliente VICI em Rust (sem swanctl),
//! contra o gateway de teste local (feature `docker-tests`).
//!
//! Cobre: `version`, `load-shared` (PSK + XAuth), `load-conn`, `list-conns`,
//! `initiate`, `list-sas`, eventos (`child-updown`, `log`), ping pelo túnel,
//! `terminate` e `unload-conn`.
#![cfg(feature = "docker-tests")]
#![allow(clippy::expect_used, clippy::unwrap_used)] // código de teste

mod support;

use std::time::Duration;

use ipsec_client_core::engine::{DockerEngine, EngineSpec, current_gid, default_runtime_dir};
use ipsec_client_core::vici::{
    Charon, CharonEvent, Message, SecretKind, SharedSecret, ViciClient, ViciEvents,
};
use support::{
    TEST_GROUP_ID, TEST_PSK, TEST_XAUTH_PASSWORD, TEST_XAUTH_USER, TestGateway, host_cmd,
    ike_ports_busy, link_details,
};
use tokio::sync::mpsc;

const PREFIX: &str = "ipsec-client-rs-vici";
const IFACE: &str = "ipsct2";
const IF_ID: u32 = 7343;
const CONN: &str = "teste";
const CHILD: &str = "rede-teste";

/// Mesma forma de uma VPN legada (equivalente ao tests/engine/initiator.conf.in).
fn test_conn(gateway: &str) -> Message {
    let child = Message::new()
        .list("remote_ts", ["192.0.2.0/24"])
        .list(
            "esp_proposals",
            [
                "aes256-sha1-modp1024",
                "aes256-sha1-modp768",
                "aes128-sha1-modp1024",
            ],
        )
        .kv("rekey_time", "43200s")
        .kv("start_action", "none")
        .kv("dpd_action", "restart")
        .kv("close_action", "start")
        .kv("if_id_in", IF_ID.to_string())
        .kv("if_id_out", IF_ID.to_string())
        .kv(
            "updown",
            format!(
                "/usr/local/libexec/ipsec-client/updown --iface '{IFACE}' \
                 --domain '~ipsec-client.test' --use-received-dns"
            ),
        );
    Message::new()
        .list("remote_addrs", [gateway])
        .kv("version", "1")
        .kv("aggressive", "yes")
        .kv("encap", "yes")
        .list(
            "proposals",
            ["aes256-sha1-modp1024", "aes128-sha1-modp1024"],
        )
        .kv("rekey_time", "86400s")
        .kv("dpd_delay", "30s")
        .list("vips", ["0.0.0.0"])
        .section(
            "local-1",
            Message::new().kv("auth", "psk").kv("id", TEST_GROUP_ID),
        )
        .section(
            "local-2",
            Message::new()
                .kv("auth", "xauth")
                .kv("xauth_id", TEST_XAUTH_USER),
        )
        .section("remote", Message::new().kv("auth", "psk"))
        .section("children", Message::new().section(CHILD, child))
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
    assert_eq!(version.version, "5.8.2");

    charon
        .load_shared(&SharedSecret::new(
            "ike-teste",
            SecretKind::Ike,
            vec![TEST_GROUP_ID.to_owned()],
            TEST_PSK.as_bytes(),
        ))
        .await
        .expect("load-shared IKE");
    charon
        .load_shared(&SharedSecret::new(
            "xauth-teste",
            SecretKind::Xauth,
            vec![TEST_XAUTH_USER.to_owned()],
            TEST_XAUTH_PASSWORD.as_bytes(),
        ))
        .await
        .expect("load-shared XAuth");
    charon
        .load_conn(CONN, &test_conn(&gateway_ip))
        .await
        .expect("load-conn");
    assert_eq!(charon.list_conns().await.expect("list-conns"), [CONN]);

    charon
        .initiate(CONN, CHILD, Duration::from_secs(30))
        .await
        .expect("initiate");
    assert!(
        wait_for(&mut rx, |e| matches!(
            e,
            CharonEvent::ChildUpDown { up: true, children, .. } if children.iter().any(|c| c == CHILD)
        ))
        .await,
        "evento child-updown (up) não chegou"
    );

    let sas = charon.list_sas().await.expect("list-sas");
    let sa = sas.iter().find(|s| s.name == CONN).expect("IKE_SA");
    assert_eq!(sa.state, "ESTABLISHED");
    assert_eq!(sa.remote_host, gateway_ip);
    let vip = sa.local_vips.first().expect("VIP").clone();
    assert!(vip.starts_with("198.51.100."), "{vip}");
    let child = sa
        .children
        .iter()
        .find(|c| c.name == CHILD)
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

    charon
        .terminate(CONN, Duration::from_secs(10))
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

    charon.unload_conn(CONN).await.expect("unload-conn");
    assert!(charon.list_conns().await.expect("list-conns").is_empty());
}
