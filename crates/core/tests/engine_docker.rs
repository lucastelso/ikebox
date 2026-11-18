//! Integration of `core::engine` with the real Docker (the `docker-tests`
//! feature).
//!
//! Starts the engine the way the app does (host network, NET_ADMIN only) using
//! an interface and an if_id of the test's own (`ipsct1`), without connecting
//! to any VPN, and checks the contract: XFRM interface with MTU 1400, VICI
//! socket 0660 for the user's gid, graceful re-creation and full cleanup on
//! stop.
#![cfg(feature = "docker-tests")]
#![allow(clippy::expect_used, clippy::unwrap_used)] // test code

use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::process::Command;

use ipsec_client_core::engine::{DockerEngine, EngineSpec, current_gid, default_runtime_dir};

const IFACE: &str = "ipsct1";
const IF_ID: u32 = 7342;
const CONTAINER: &str = "ipsec-client-rs-test-engine";

fn ike_ports_busy() -> bool {
    Command::new("ss")
        .args(["-Hlun", "( sport = :500 or sport = :4500 )"])
        .output()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false)
}

fn link_details(iface: &str) -> Option<String> {
    let out = Command::new("ip")
        .args(["-d", "link", "show", iface])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

#[tokio::test]
async fn engine_lifecycle_on_host_network() {
    if ike_ports_busy() {
        eprintln!("pulando: UDP 500/4500 em uso (outra VPN IPsec conectada?)");
        return;
    }
    assert!(
        link_details(IFACE).is_none(),
        "a interface {IFACE} já existe"
    );

    let engine = DockerEngine::connect().expect("cliente Docker");
    engine.ping().await.expect("Docker acessível");
    let image = engine
        .ensure_image(|p| eprintln!("build: {p:?}"))
        .await
        .expect("imagem do engine");

    let base = default_runtime_dir().expect("XDG_RUNTIME_DIR");
    let parent = base.parent().expect("pai do runtime dir").to_path_buf();
    let tmp = tempfile::Builder::new()
        .prefix("ipsec-client-rs-test.")
        .tempdir_in(parent)
        .expect("tempdir");
    let mut spec = EngineSpec::new(tmp.path().join("run"), current_gid(), IF_ID);
    spec.container_name = CONTAINER.to_owned();
    spec.iface = IFACE.to_owned();
    assert_eq!(spec.image, image);

    engine.start(&spec).await.expect("engine pronto");
    assert!(engine.is_running(CONTAINER).await.expect("inspect"));

    let link = link_details(IFACE).expect("interface criada");
    assert!(link.contains("mtu 1400"), "{link}");
    assert!(link.contains(&format!("if_id {IF_ID:#x}")), "{link}");

    let meta = std::fs::metadata(spec.socket_path()).expect("socket VICI");
    assert_eq!(meta.permissions().mode() & 0o777, 0o660);
    assert_eq!(meta.gid(), current_gid());

    // ADR-0002: starting again stops the previous engine (gracefully) and
    // re-creates it.
    engine.start(&spec).await.expect("recriação");
    assert!(link_details(IFACE).is_some());

    engine.stop(CONTAINER).await.expect("stop");
    assert!(!engine.is_running(CONTAINER).await.expect("inspect"));
    assert!(link_details(IFACE).is_none(), "a interface sobrou");
    assert!(!spec.socket_path().exists(), "o socket sobrou");
    // Idempotent.
    engine.stop(CONTAINER).await.expect("stop idempotente");
}
