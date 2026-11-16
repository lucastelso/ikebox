//! Comandos do systemd usados pelo helper. A montagem do `systemd-run` é pura
//! (testada): é aqui que fica o sandbox do engine (ADR-0014).

use std::path::Path;
use std::process::Command;

use ipsec_client_proto::{ENGINE_UNIT, EngineStatus, StartParams, socket_dir};

/// Diretório de runtime do engine (gravável; `RuntimeDirectory` do systemd).
pub const ENGINE_RUNTIME_DIR: &str = "/run/ipsec-client-engine";

/// Capabilities que o engine pode ter:
/// - `NET_ADMIN`: XFRM, rotas, endereços e interface;
/// - `NET_BIND_SERVICE`: portas UDP 500/4500;
/// - `NET_RAW`: sockets do charon;
/// - `CHOWN`: entregar o socket VICI ao usuário.
const CAPABILITIES: &str = "CAP_NET_ADMIN CAP_NET_BIND_SERVICE CAP_NET_RAW CAP_CHOWN";

/// Quem pede e onde estão os arquivos do engine.
#[derive(Debug, Clone)]
pub struct Launch<'a> {
    /// Parâmetros validados.
    pub params: &'a StartParams,
    /// uid do usuário (dono da sessão e do socket VICI).
    pub uid: u32,
    /// Diretório do strongSwan embutido (`.../engine`).
    pub engine_dir: &'a Path,
    /// Diretório com `entrypoint.sh` e `updown`.
    pub libexec_dir: &'a Path,
}

/// Junta valores já validados com vírgula (o entrypoint separa de volta).
fn join<T: ToString>(items: &[T]) -> String {
    items
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

/// Argumentos do `systemd-run` que sobe o engine numa unidade transitória com
/// sandbox. Nada aqui vem do cliente além dos números/rótulo já validados.
#[must_use]
pub fn engine_run_args(launch: &Launch<'_>) -> Vec<String> {
    let p = launch.params;
    let sock_dir = socket_dir(launch.uid);
    let engine = launch.engine_dir.display();
    let mut args: Vec<String> = vec![
        format!("--unit={ENGINE_UNIT}"),
        format!(
            "--description=IPsec Client engine (profile {}, uid {})",
            p.profile_id, launch.uid
        ),
        "--collect".into(),
        "--quiet".into(),
    ];
    let properties = [
        "Type=simple".to_owned(),
        "KillMode=mixed".to_owned(),
        "TimeoutStopSec=15".to_owned(),
        format!("CapabilityBoundingSet={CAPABILITIES}"),
        "NoNewPrivileges=yes".to_owned(),
        "ProtectSystem=strict".to_owned(),
        "ProtectHome=yes".to_owned(),
        "PrivateTmp=yes".to_owned(),
        "ProtectKernelModules=yes".to_owned(),
        "ProtectControlGroups=yes".to_owned(),
        "ProtectClock=yes".to_owned(),
        "ProtectHostname=yes".to_owned(),
        "RestrictNamespaces=yes".to_owned(),
        "RestrictRealtime=yes".to_owned(),
        "RestrictSUIDSGID=yes".to_owned(),
        "LockPersonality=yes".to_owned(),
        "MemoryDenyWriteExecute=yes".to_owned(),
        "SystemCallArchitectures=native".to_owned(),
        "RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6 AF_NETLINK".to_owned(),
        // O engine roda como root: sem /dev de verdade (discos), sem gravar
        // em /proc/sys e /sys (core_pattern, modprobe), sem ver outros
        // processos nem o log do kernel (ADR-0019).
        "PrivateDevices=yes".to_owned(),
        "ProtectKernelTunables=yes".to_owned(),
        "ProtectKernelLogs=yes".to_owned(),
        "ProtectProc=invisible".to_owned(),
        "PrivateIPC=yes".to_owned(),
        "UMask=0077".to_owned(),
        "SystemCallFilter=@system-service".to_owned(),
        "SystemCallErrorNumber=EPERM".to_owned(),
        "RuntimeDirectory=ipsec-client-engine".to_owned(),
        "RuntimeDirectoryMode=0700".to_owned(),
        format!("ReadWritePaths={sock_dir}"),
    ];
    for prop in properties {
        args.push(format!("--property={prop}"));
    }
    let env = [
        ("IPSEC_CLIENT_IFACE", p.iface.clone()),
        ("IPSEC_CLIENT_IF_ID", p.if_id.to_string()),
        ("IPSEC_CLIENT_MTU", p.mtu.to_string()),
        ("IPSEC_CLIENT_LOG_LEVEL", p.log_level.to_string()),
        ("IPSEC_CLIENT_SOCKET_UID", launch.uid.to_string()),
        ("IPSEC_CLIENT_SOCKET_DIR", sock_dir.clone()),
        ("IPSEC_CLIENT_STATE_DIR", ENGINE_RUNTIME_DIR.to_owned()),
        (
            "IPSEC_CLIENT_CHARON",
            format!("{engine}/libexec/ipsec/charon"),
        ),
        (
            "IPSEC_CLIENT_STRONGSWAN_CONF",
            format!("{engine}/etc/strongswan.conf"),
        ),
        // updown forçado: o charon ignora o que o cliente VICI mandar.
        (
            "IPSEC_CLIENT_UPDOWN",
            launch.libexec_dir.join("updown").display().to_string(),
        ),
        ("IPSEC_CLIENT_DNS_SERVERS", join(&p.dns.servers)),
        ("IPSEC_CLIENT_DNS_DOMAINS", join(&p.dns.domains)),
        (
            "IPSEC_CLIENT_DNS_FLAGS",
            join(
                &[
                    (p.dns.use_received, "use-received"),
                    (p.dns.default_route, "default-route"),
                ]
                .iter()
                .filter(|(on, _)| *on)
                .map(|(_, flag)| *flag)
                .collect::<Vec<_>>(),
            ),
        ),
        ("IPSEC_CLIENT_PROFILE", p.profile_id.clone()),
        ("IPSEC_CLIENT_OWNER_UID", launch.uid.to_string()),
        ("PATH", "/usr/sbin:/usr/bin:/sbin:/bin".to_owned()),
    ];
    for (key, value) in env {
        args.push(format!("--setenv={key}={value}"));
    }
    args.push("/bin/bash".into());
    args.push(
        launch
            .libexec_dir
            .join("entrypoint.sh")
            .display()
            .to_string(),
    );
    args
}

/// Interpreta `systemctl show <unit> --property=ActiveState --property=Environment`.
#[must_use]
pub fn parse_status(show_output: &str) -> EngineStatus {
    let mut status = EngineStatus::default();
    for line in show_output.lines() {
        if let Some(state) = line.strip_prefix("ActiveState=") {
            status.running = matches!(state.trim(), "active" | "activating" | "reloading");
        } else if let Some(env) = line.strip_prefix("Environment=") {
            for pair in env.split_whitespace() {
                if let Some(id) = pair.strip_prefix("IPSEC_CLIENT_PROFILE=") {
                    status.profile_id = Some(id.to_owned());
                } else if let Some(uid) = pair.strip_prefix("IPSEC_CLIENT_OWNER_UID=") {
                    status.owner_uid = uid.parse().ok();
                }
            }
        }
    }
    if !status.running {
        status.profile_id = None;
        status.owner_uid = None;
    }
    status
}

/// Roda um comando e devolve (sucesso, stdout, stderr).
pub fn run(program: &str, args: &[String]) -> (bool, String, String) {
    match Command::new(program).args(args).output() {
        Ok(out) => (
            out.status.success(),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        ),
        Err(e) => (false, String::new(), e.to_string()),
    }
}

/// Estado atual da unidade do engine.
pub fn engine_status() -> EngineStatus {
    let (_, out, _) = run(
        "systemctl",
        &[
            "show".into(),
            ENGINE_UNIT.into(),
            "--property=ActiveState".into(),
            "--property=Environment".into(),
        ],
    );
    parse_status(&out)
}

/// Para a unidade do engine (espera o fim: o entrypoint limpa o host).
pub fn stop_engine() -> Result<(), String> {
    let (ok, _, err) = run("systemctl", &["stop".into(), ENGINE_UNIT.into()]);
    if ok || err.contains("not loaded") {
        Ok(())
    } else {
        Err(format!("failed to stop the engine: {}", err.trim()))
    }
}

/// Últimas linhas do log da unidade.
pub fn engine_logs(tail: u32) -> Vec<String> {
    let (_, out, _) = run(
        "journalctl",
        &[
            "--unit".into(),
            ENGINE_UNIT.into(),
            "--lines".into(),
            tail.to_string(),
            "--output=cat".into(),
            "--no-pager".into(),
            "--quiet".into(),
        ],
    );
    out.lines().map(str::to_owned).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> StartParams {
        StartParams {
            iface: "ipsc0".into(),
            if_id: 4096,
            mtu: 1400,
            log_level: 1,
            profile_id: "trabalho".into(),
            dns: ipsec_client_proto::DnsParams {
                servers: vec![
                    "192.0.2.53".parse().expect("ip"),
                    "192.0.2.54".parse().expect("ip"),
                ],
                domains: vec!["~corp.example.com".into()],
                use_received: true,
                default_route: false,
            },
        }
    }

    #[test]
    fn run_args_have_sandbox_and_fixed_paths() {
        let p = params();
        let args = engine_run_args(&Launch {
            params: &p,
            uid: 1000,
            engine_dir: Path::new("/opt/ipsec-client/engine"),
            libexec_dir: Path::new("/opt/ipsec-client/libexec"),
        });
        let has = |s: &str| args.iter().any(|a| a == s);
        assert!(has("--unit=ipsec-client-engine"));
        assert!(has("--property=ProtectSystem=strict"));
        assert!(has("--property=NoNewPrivileges=yes"));
        assert!(has(
            "--property=CapabilityBoundingSet=CAP_NET_ADMIN CAP_NET_BIND_SERVICE CAP_NET_RAW CAP_CHOWN"
        ));
        assert!(has("--property=ReadWritePaths=/run/ipsec-client/1000"));
        assert!(has(
            "--setenv=IPSEC_CLIENT_SOCKET_DIR=/run/ipsec-client/1000"
        ));
        assert!(has(
            "--setenv=IPSEC_CLIENT_CHARON=/opt/ipsec-client/engine/libexec/ipsec/charon"
        ));
        assert!(has("--setenv=IPSEC_CLIENT_IF_ID=4096"));
        assert!(has("--setenv=IPSEC_CLIENT_SOCKET_UID=1000"));
        assert!(has(
            "--setenv=IPSEC_CLIENT_UPDOWN=/opt/ipsec-client/libexec/updown"
        ));
        assert!(has(
            "--setenv=IPSEC_CLIENT_DNS_SERVERS=192.0.2.53,192.0.2.54"
        ));
        assert!(has("--setenv=IPSEC_CLIENT_DNS_DOMAINS=~corp.example.com"));
        assert!(has("--setenv=IPSEC_CLIENT_DNS_FLAGS=use-received"));
        for prop in [
            "PrivateDevices=yes",
            "ProtectKernelTunables=yes",
            "ProtectKernelLogs=yes",
            "ProtectProc=invisible",
        ] {
            assert!(has(&format!("--property={prop}")), "{prop}");
        }
        assert!(!args.iter().any(|a| a.contains("SOCKET_GID")));
        assert_eq!(
            args[args.len() - 2..],
            ["/bin/bash", "/opt/ipsec-client/libexec/entrypoint.sh"]
        );
        // Nada de capabilities perigosas.
        assert!(
            !args
                .iter()
                .any(|a| a.contains("SYS_ADMIN") || a.contains("SYS_MODULE"))
        );
    }

    #[test]
    fn parses_systemctl_show() {
        let running = "ActiveState=active\nEnvironment=IPSEC_CLIENT_IFACE=ipsc0 IPSEC_CLIENT_PROFILE=trabalho IPSEC_CLIENT_OWNER_UID=1000 PATH=/usr/bin\n";
        let s = parse_status(running);
        assert!(s.running);
        assert_eq!(s.profile_id.as_deref(), Some("trabalho"));
        assert_eq!(s.owner_uid, Some(1000));
        let stopped = parse_status("ActiveState=inactive\nEnvironment=\n");
        assert_eq!(stopped, EngineStatus::default());
        assert_eq!(parse_status(""), EngineStatus::default());
    }
}
