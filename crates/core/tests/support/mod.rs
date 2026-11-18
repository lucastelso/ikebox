//! Support for the Docker integration tests (the `docker-tests` feature).
//!
//! [`TestGateway`] starts the same IPsec test gateway used by
//! `tests/engine/e2e.sh` (IKEv1 aggressive, group PSK + XAuth, modp1024/768,
//! network 192.0.2.0/24, VIPs 198.51.100.10-20, DNS 192.0.2.53), on an isolated
//! Docker network. Only RFC 5737/2606 values and fake credentials.
#![allow(dead_code)] // each test file uses a part
#![allow(clippy::expect_used, clippy::unwrap_used)] // test code

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use bollard::Docker;
use bollard::models::{ContainerCreateBody, HostConfig, NetworkCreateRequest};
use bollard::query_parameters::{
    CreateContainerOptionsBuilder, InspectContainerOptions, LogsOptionsBuilder,
    RemoveContainerOptionsBuilder, StartContainerOptions,
};
use futures_util::StreamExt;
use ipsec_client_core::engine::HostSecurity;

/// Fake PSK of the test gateway.
pub const TEST_PSK: &str = "ipsec-client-test-psk";
/// Id (keyid) of the test group.
pub const TEST_GROUP_ID: &str = "keyid:grupo-teste";
/// Test XAuth username.
pub const TEST_XAUTH_USER: &str = "usuario.teste";
/// Fake XAuth password.
pub const TEST_XAUTH_PASSWORD: &str = "senha-de-teste";

/// UDP 500/4500 in use on the host (another IPsec VPN connected)?
pub fn ike_ports_busy() -> bool {
    Command::new("ss")
        .args(["-Hlun", "( sport = :500 or sport = :4500 )"])
        .output()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false)
}

/// Details of an interface (`ip -d link show`), if it exists.
pub fn link_details(iface: &str) -> Option<String> {
    let out = Command::new("ip")
        .args(["-d", "link", "show", iface])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Output of a host command.
pub fn host_cmd(program: &str, args: &[&str]) -> (bool, String) {
    match Command::new(program).args(args).output() {
        Ok(out) => (
            out.status.success(),
            String::from_utf8_lossy(&out.stdout).into_owned(),
        ),
        Err(e) => (false, e.to_string()),
    }
}

/// Test IPsec gateway on an isolated Docker network.
pub struct TestGateway {
    docker: Docker,
    name: String,
    network: String,
    /// Gateway IP on the test network.
    pub ip: String,
}

impl TestGateway {
    /// Starts the gateway (with the image `image`) and waits for it to be ready.
    pub async fn start(image: &str, prefix: &str) -> Self {
        let docker = Docker::connect_with_unix_defaults().expect("Docker");
        let name = format!("{prefix}-gw");
        let network = format!("{prefix}-net");
        let _ = remove_container(&docker, &name).await;
        let _ = docker.remove_network(&network).await;

        docker
            .create_network(NetworkCreateRequest {
                name: network.clone(),
                driver: Some("bridge".to_owned()),
                ..Default::default()
            })
            .await
            .expect("rede de teste");

        let gateway_dir: PathBuf = [
            env!("CARGO_MANIFEST_DIR"),
            "..",
            "..",
            "tests",
            "engine",
            "gateway",
        ]
        .iter()
        .collect();
        let gateway_dir = gateway_dir.canonicalize().expect("tests/engine/gateway");
        let security = HostSecurity::detect();
        let body = ContainerCreateBody {
            image: Some(image.to_owned()),
            entrypoint: Some(vec!["/test/gateway.sh".to_owned()]),
            host_config: Some(HostConfig {
                network_mode: Some(network.clone()),
                cap_add: Some(vec!["NET_ADMIN".to_owned()]),
                binds: Some(vec![format!("{}:/test:ro", gateway_dir.display())]),
                security_opt: security.selinux.then(|| vec!["label=disable".to_owned()]),
                ..Default::default()
            }),
            ..Default::default()
        };
        let options = CreateContainerOptionsBuilder::default().name(&name).build();
        docker
            .create_container(Some(options), body)
            .await
            .expect("container do gateway");
        docker
            .start_container(&name, None::<StartContainerOptions>)
            .await
            .expect("start do gateway");

        let gateway = Self {
            ip: String::new(),
            docker,
            name,
            network,
        };
        gateway.wait_ready().await;
        let ip = gateway.inspect_ip().await;
        Self { ip, ..gateway }
    }

    async fn wait_ready(&self) {
        let options = LogsOptionsBuilder::default()
            .follow(true)
            .stdout(true)
            .stderr(true)
            .build();
        let mut logs = self.docker.logs(&self.name, Some(options));
        let ready = tokio::time::timeout(Duration::from_secs(15), async {
            while let Some(Ok(chunk)) = logs.next().await {
                if String::from_utf8_lossy(&chunk.into_bytes()).contains("[test-gateway] ready") {
                    return true;
                }
            }
            false
        })
        .await;
        assert_eq!(ready, Ok(true), "o gateway de teste não ficou pronto");
    }

    async fn inspect_ip(&self) -> String {
        let info = self
            .docker
            .inspect_container(&self.name, None::<InspectContainerOptions>)
            .await
            .expect("inspect do gateway");
        info.network_settings
            .and_then(|n| n.networks)
            .and_then(|mut nets| nets.remove(&self.network))
            .and_then(|ep| ep.ip_address)
            .filter(|ip| !ip.is_empty())
            .expect("IP do gateway")
    }

    /// Forces the end of the SAs on the gateway's side (simulates a DPD timeout).
    pub async fn forget_sessions(&self) {
        let _ = Command::new("docker")
            .args([
                "exec",
                &self.name,
                "swanctl",
                "--terminate",
                "--ike",
                "gateway-teste",
                "--force",
                "--timeout",
                "2",
            ])
            .output();
    }

    /// Removes the gateway and the network.
    pub async fn stop(self) {
        let _ = remove_container(&self.docker, &self.name).await;
        let _ = self.docker.remove_network(&self.network).await;
    }
}

async fn remove_container(docker: &Docker, name: &str) -> Result<(), bollard::errors::Error> {
    let options = RemoveContainerOptionsBuilder::default().force(true).build();
    docker.remove_container(name, Some(options)).await
}
