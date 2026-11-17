//! Verificações do ambiente: preflight do `connect` e o comando `doctor`.
//!
//! Nada aqui exige root: as portas vêm de `/proc/net/udp*`, o módulo do kernel
//! de `/sys` e `/lib/modules`, e o resolved do seu socket em `/run`.

use std::fs;
use std::path::Path;

use crate::engine::{DockerEngine, NativeEngine, default_runtime_dir};

/// Resultado de uma verificação.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    /// Tudo certo.
    Ok,
    /// Funciona, mas com limitação.
    Warn,
    /// Impede a conexão.
    Fail,
}

/// One check, with a hint on how to fix it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    /// Stable identifier (does not change with the language).
    pub id: &'static str,
    /// What was checked (in the current language).
    pub name: &'static str,
    /// Result.
    pub status: CheckStatus,
    /// Result detail.
    pub detail: String,
    /// How to fix it (when not Ok).
    pub hint: Option<String>,
}

impl Check {
    fn ok(id: &'static str, name: &'static str, detail: impl Into<String>) -> Self {
        Self {
            id,
            name,
            status: CheckStatus::Ok,
            detail: detail.into(),
            hint: None,
        }
    }

    fn bad(
        id: &'static str,
        name: &'static str,
        status: CheckStatus,
        detail: impl Into<String>,
        hint: impl Into<String>,
    ) -> Self {
        Self {
            id,
            name,
            status,
            detail: detail.into(),
            hint: Some(hint.into()),
        }
    }
}

/// Portas UDP do IKE.
pub const IKE_PORTS: [u16; 2] = [500, 4500];

/// Portas locais UDP em uso, lidas de um conteúdo no formato de
/// `/proc/net/udp` (coluna `local_address` = `IP:PORTA` em hexadecimal).
#[must_use]
pub fn udp_ports_in_use(proc_net_udp: &str) -> Vec<u16> {
    proc_net_udp
        .lines()
        .skip(1)
        .filter_map(|line| line.split_whitespace().nth(1))
        .filter_map(|local| local.rsplit_once(':'))
        .filter_map(|(_, port)| u16::from_str_radix(port, 16).ok())
        .collect()
}

/// Portas do IKE ocupadas no host.
#[must_use]
pub fn busy_ike_ports() -> Vec<u16> {
    let mut used = Vec::new();
    for file in ["/proc/net/udp", "/proc/net/udp6"] {
        if let Ok(content) = fs::read_to_string(file) {
            used.extend(udp_ports_in_use(&content));
        }
    }
    let mut busy: Vec<u16> = IKE_PORTS.into_iter().filter(|p| used.contains(p)).collect();
    busy.dedup();
    busy
}

fn check_ike_ports() -> Check {
    const ID: &str = "ike-ports";
    let name = crate::tr!("UDP ports 500/4500", "Portas UDP 500/4500");
    let busy = busy_ike_ports();
    if busy.is_empty() {
        Check::ok(ID, name, crate::tr!("free", "livres"))
    } else {
        Check::bad(
            ID,
            name,
            CheckStatus::Fail,
            crate::trf!("in use: {busy:?}", "em uso: {busy:?}"),
            crate::tr!(
                "another IPsec VPN (strongSwan, libreswan, another container) is using \
                 the ports; disconnect it first",
                "outra VPN IPsec (strongSwan, libreswan, outro container) está usando as \
                 portas; desconecte-a antes"
            ),
        )
    }
}

fn check_xfrm_interface() -> Check {
    const ID: &str = "xfrm-interface";
    let name = crate::tr!("xfrm_interface module", "Módulo xfrm_interface");
    if Path::new("/sys/module/xfrm_interface").exists() {
        return Check::ok(ID, name, crate::tr!("loaded", "carregado"));
    }
    let release = fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default();
    let modules = Path::new("/lib/modules").join(release.trim());
    let builtin = fs::read_to_string(modules.join("modules.builtin")).unwrap_or_default();
    let listed = fs::read_to_string(modules.join("modules.dep")).unwrap_or_default();
    if builtin.contains("xfrm_interface") || listed.contains("xfrm_interface.ko") {
        Check::ok(
            ID,
            name,
            crate::tr!(
                "available (loaded automatically when connecting)",
                "disponível (carregado automaticamente ao conectar)"
            ),
        )
    } else {
        Check::bad(
            ID,
            name,
            CheckStatus::Fail,
            crate::tr!("not found in the kernel", "não encontrado no kernel"),
            crate::tr!(
                "a kernel ≥ 4.19 with CONFIG_XFRM_INTERFACE is required (the default in current distributions)",
                "é preciso um kernel ≥ 4.19 com CONFIG_XFRM_INTERFACE (padrão nas distribuições atuais)"
            ),
        )
    }
}

/// Directories where the engine looks for its tools (the `PATH` the helper
/// gives it).
const TOOL_DIRS: [&str; 4] = ["/usr/sbin", "/usr/bin", "/sbin", "/bin"];

fn tool_exists(dirs: &[&Path], name: &str) -> bool {
    dirs.iter().any(|dir| dir.join(name).is_file())
}

/// The engine creates the interface, the routes and the XFRM state with `ip`
/// (iproute2). Desktop distributions have it; a minimal installation may not,
/// and the AppImage cannot declare the dependency.
fn check_ip_command(dirs: &[&Path]) -> Check {
    const ID: &str = "iproute2";
    let name = crate::tr!("ip command (iproute2)", "Comando ip (iproute2)");
    if tool_exists(dirs, "ip") {
        Check::ok(ID, name, crate::tr!("available", "disponível"))
    } else {
        Check::bad(
            ID,
            name,
            CheckStatus::Fail,
            crate::tr!("not found", "não encontrado"),
            crate::tr!(
                "install the iproute2 package (iproute on Fedora) of your distribution",
                "instale o pacote iproute2 (iproute no Fedora) da sua distribuição"
            ),
        )
    }
}

/// Varlink socket of systemd-resolved (systemd 247 and newer). Any process can
/// connect to it: nss-resolve does.
const RESOLVED_SOCKET: &str = "/run/systemd/resolve/io.systemd.Resolve";

fn check_resolved() -> Check {
    check_resolved_at(Path::new(RESOLVED_SOCKET))
}

/// systemd-resolved is running if it **answers** on its socket. Neither the
/// files in `/run/systemd/resolve` nor a successful connection are proof:
/// the files stay after the service stops, and on recent systemd versions the
/// socket belongs to systemd itself (socket activation), which accepts the
/// connection and then drops it when the service cannot start. In both cases
/// the VPN DNS is silently not applied.
fn resolved_answers(socket: &Path) -> bool {
    use std::io::{Read as _, Write as _};
    let Ok(mut stream) = std::os::unix::net::UnixStream::connect(socket) else {
        return false;
    };
    let timeout = Some(std::time::Duration::from_secs(1));
    if stream.set_read_timeout(timeout).is_err() || stream.set_write_timeout(timeout).is_err() {
        return false;
    }
    // Any Varlink reply will do: systemd 249 answers this one with
    // "MethodNotImplemented", newer versions with their description.
    if stream
        .write_all(b"{\"method\":\"org.varlink.service.GetInfo\"}\0")
        .is_err()
    {
        return false;
    }
    let mut reply = Vec::new();
    let mut chunk = [0_u8; 1024];
    while reply.len() < 64 * 1024 {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                reply.extend_from_slice(&chunk[..n]);
                if reply.contains(&0) {
                    break;
                }
            }
        }
    }
    reply.first() == Some(&b'{') && reply.contains(&0)
}

fn check_resolved_at(socket: &Path) -> Check {
    const ID: &str = "resolved";
    if resolved_answers(socket) {
        Check::ok(ID, "systemd-resolved", crate::tr!("active", "ativo"))
    } else {
        Check::bad(
            ID,
            "systemd-resolved",
            CheckStatus::Warn,
            crate::tr!("not running", "não está em execução"),
            crate::tr!(
                "without systemd-resolved, the VPN DNS (internal domains) is not configured; \
                 the tunnel works by IP. Debian and openSUSE do not use it by default: install \
                 the systemd-resolved package and enable it",
                "sem o systemd-resolved, o DNS da VPN (domínios internos) não é configurado; \
                 o túnel funciona por IP. O Debian e o openSUSE não o usam por padrão: instale \
                 o pacote systemd-resolved e ative-o"
            ),
        )
    }
}

fn check_dbus_socket() -> Check {
    const ID: &str = "system-dbus";
    let name = crate::tr!("System D-Bus", "D-Bus do sistema");
    if Path::new(crate::engine::DBUS_SYSTEM_SOCKET).exists() {
        Check::ok(ID, name, crate::tr!("available", "disponível"))
    } else {
        Check::bad(
            ID,
            name,
            CheckStatus::Warn,
            crate::tr!("socket missing", "socket ausente"),
            crate::tr!(
                "without the system D-Bus, the VPN DNS is not configured",
                "sem o D-Bus do sistema, o DNS da VPN não é configurado"
            ),
        )
    }
}

fn check_runtime_dir() -> Check {
    const ID: &str = "runtime-dir";
    let name = crate::tr!("Runtime directory", "Diretório de runtime");
    match default_runtime_dir() {
        Ok(dir) => Check::ok(ID, name, dir.display().to_string()),
        Err(e) => Check::bad(
            ID,
            name,
            CheckStatus::Fail,
            e.to_string(),
            crate::tr!(
                "run it in a regular graphical or login session (with systemd-logind)",
                "rode numa sessão gráfica ou de login normal (com systemd-logind)"
            ),
        ),
    }
}

fn check_not_root() -> Check {
    const ID: &str = "user";
    let name = crate::tr!("User", "Usuário");
    if rustix::process::geteuid().is_root() {
        Check::bad(
            ID,
            name,
            CheckStatus::Fail,
            crate::tr!("running as root", "rodando como root"),
            crate::tr!(
                "do not use sudo: run it as your own user",
                "não use sudo: rode como o seu usuário"
            ),
        )
    } else {
        Check::ok(
            ID,
            name,
            crate::tr!("regular (not root)", "comum (não root)"),
        )
    }
}

async fn check_docker() -> Check {
    const ID: &str = "docker";
    let result = match DockerEngine::connect() {
        Ok(engine) => engine.ping().await,
        Err(e) => Err(e),
    };
    match result {
        Ok(()) => Check::ok(ID, "Docker", crate::tr!("reachable", "acessível")),
        Err(e) => Check::bad(
            ID,
            "Docker",
            CheckStatus::Fail,
            e.to_string(),
            crate::tr!(
                "sudo systemctl enable --now docker; sudo usermod -aG docker $USER (then log out and back in)",
                "sudo systemctl enable --now docker; sudo usermod -aG docker $USER (e saia/entre na sessão)"
            ),
        ),
    }
}

/// Id of the network component check (the UI offers to install it when it
/// fails).
pub const COMPONENT_CHECK: &str = "network-component";

/// Name of the network component check, in the current language.
#[must_use]
pub fn component_check_name() -> &'static str {
    crate::tr!("Network component", "Componente de rede")
}

/// How to install the network component.
#[must_use]
pub fn component_hint() -> &'static str {
    crate::tr!(
        "in the app: System → Network component → Install (asks for the administrator \
         password once); in a terminal: ipsec-client-cli component install --bundle <bundle>",
        "no app: Sistema → Componente de rede → Instalar (pede a senha de administrador \
         uma vez); no terminal: ipsec-client-cli component install --bundle <pacote>"
    )
}

/// Network component (embedded engine). Without it, the app uses Docker, if
/// available; without either, it cannot connect.
async fn check_engine() -> Vec<Check> {
    let name = component_check_name();
    let native = NativeEngine::default();
    if native.installed() {
        let ping = tokio::task::spawn_blocking(move || native.ping()).await;
        return vec![match ping {
            Ok(Ok(version)) => Check::ok(
                COMPONENT_CHECK,
                name,
                crate::trf!(
                    "installed (embedded engine, helper {version})",
                    "instalado (engine embutido, helper {version})"
                ),
            ),
            Ok(Err(e)) => Check::bad(
                COMPONENT_CHECK,
                name,
                CheckStatus::Fail,
                e.to_string(),
                component_hint(),
            ),
            Err(e) => Check::bad(
                COMPONENT_CHECK,
                name,
                CheckStatus::Fail,
                e.to_string(),
                component_hint(),
            ),
        }];
    }
    let docker = check_docker().await;
    if docker.status == CheckStatus::Ok {
        vec![
            Check::bad(
                COMPONENT_CHECK,
                name,
                CheckStatus::Warn,
                crate::tr!(
                    "not installed; using the Docker engine",
                    "não instalado; usando o engine em Docker"
                ),
                component_hint(),
            ),
            check_runtime_dir(),
            docker,
        ]
    } else {
        vec![Check::bad(
            COMPONENT_CHECK,
            name,
            CheckStatus::Fail,
            crate::tr!("not installed", "não instalado"),
            component_hint(),
        )]
    }
}

/// Checks that prevent or limit a connection (run before `connect`). The
/// ports are not checked when one of our engines is already running (it
/// would be the one using them).
pub async fn preflight(check_ports: bool) -> Vec<Check> {
    let mut checks = vec![check_not_root()];
    checks.extend(check_engine().await);
    checks.push(check_xfrm_interface());
    // Only the embedded engine runs `ip` on this computer (the Docker engine
    // has its own inside the container).
    if NativeEngine::default().installed() {
        checks.push(check_ip_command(&TOOL_DIRS.map(Path::new)));
    }
    checks.extend([check_resolved(), check_dbus_socket()]);
    if check_ports {
        checks.push(check_ike_ports());
    }
    checks
}

/// All checks (`doctor`): preflight + keyring + image.
pub async fn doctor() -> Vec<Check> {
    let mut checks = preflight(true).await;
    let keyring = crate::tr!("System keyring", "Chaveiro do sistema");
    checks.push(match crate::secrets::KeyringStore::new() {
        Ok(_) => Check::ok(
            "keyring",
            keyring,
            crate::tr!(
                "available (credentials can be remembered)",
                "disponível (credenciais podem ser lembradas)"
            ),
        ),
        Err(e) => Check::bad(
            "keyring",
            keyring,
            CheckStatus::Warn,
            e.to_string(),
            crate::tr!(
                "install/enable GNOME Keyring or KWallet; without it, credentials are asked on every connection",
                "instale/ative o GNOME Keyring ou o KWallet; sem ele, as credenciais são pedidas a cada conexão"
            ),
        ),
    });
    if !NativeEngine::default().installed() {
        let image = crate::engine::context::image_tag();
        let name = crate::tr!("Engine image", "Imagem do engine");
        let present = match DockerEngine::connect() {
            Ok(engine) => engine.image_present(&image).await.unwrap_or(false),
            Err(_) => false,
        };
        checks.push(if present {
            Check::ok("engine-image", name, image)
        } else {
            Check::bad(
                "engine-image",
                name,
                CheckStatus::Warn,
                crate::trf!("{image} not built yet", "{image} ainda não construída"),
                crate::tr!(
                    "it will be built on the first connection (~1 min, needs internet)",
                    "será construída na primeira conexão (~1 min, precisa de internet)"
                ),
            )
        });
    }
    checks
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
   sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode ref pointer drops
  431: 00000000:01F4 00000000:0000 07 00000000:00000000 00:00000000 00000000     0        0 1 2 0000000000000000 0
  432: 00000000:1194 00000000:0000 07 00000000:00000000 00:00000000 00000000     0        0 1 2 0000000000000000 0
  433: 3500007F:0035 00000000:0000 07 00000000:00000000 00:00000000 00000000   991        0 1 2 0000000000000000 0
";

    /// Fake systemd-resolved: answers one request with `reply`, or closes the
    /// connection without answering, as systemd does when the service is
    /// masked.
    fn fake_resolved(socket: &Path, reply: Option<&'static [u8]>) -> std::thread::JoinHandle<()> {
        use std::io::{Read as _, Write as _};
        let listener = std::os::unix::net::UnixListener::bind(socket).expect("bind");
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut request = Vec::new();
                let mut byte = [0_u8; 1];
                while stream.read(&mut byte).is_ok_and(|n| n == 1) && byte[0] != 0 {
                    request.push(byte[0]);
                }
                assert!(request.starts_with(b"{\"method\":"), "a Varlink call");
                if let Some(reply) = reply {
                    let _ = stream.write_all(reply);
                }
            }
        })
    }

    #[test]
    fn resolved_is_active_only_if_it_answers() {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket = dir.path().join("io.systemd.Resolve");
        assert_eq!(
            check_resolved_at(&socket).status,
            CheckStatus::Warn,
            "no socket"
        );

        // systemd 262: its description; systemd 249: an error, still an answer.
        for reply in [
            &b"{\"parameters\":{\"vendor\":\"The systemd Project\"}}\0"[..],
            &b"{\"error\":\"org.varlink.service.MethodNotImplemented\"}\0"[..],
        ] {
            let _ = std::fs::remove_file(&socket);
            let server = fake_resolved(&socket, Some(reply));
            assert_eq!(check_resolved_at(&socket).status, CheckStatus::Ok);
            server.join().expect("server");
        }

        // Masked service behind systemd's socket: accepted, then dropped.
        let _ = std::fs::remove_file(&socket);
        let server = fake_resolved(&socket, None);
        let masked = check_resolved_at(&socket);
        assert_eq!(masked.status, CheckStatus::Warn);
        assert!(masked.hint.is_some());
        server.join().expect("server");

        // Stopped service: the socket file stays, nobody listens.
        assert!(socket.exists());
        assert_eq!(check_resolved_at(&socket).status, CheckStatus::Warn);
    }

    #[test]
    fn ip_command_is_looked_up_in_the_engine_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let other = tempfile::tempdir().expect("tempdir");
        let dirs = [other.path(), dir.path()];
        let missing = check_ip_command(&dirs);
        assert_eq!(missing.status, CheckStatus::Fail);
        assert!(missing.hint.is_some());
        std::fs::write(dir.path().join("ip"), "").expect("write");
        assert_eq!(check_ip_command(&dirs).status, CheckStatus::Ok);
        // The real one, on any computer that runs these tests with iproute2.
        if TOOL_DIRS.iter().any(|d| Path::new(d).join("ip").is_file()) {
            assert_eq!(
                check_ip_command(&TOOL_DIRS.map(Path::new)).status,
                CheckStatus::Ok
            );
        }
    }

    #[test]
    fn parses_proc_net_udp() {
        assert_eq!(udp_ports_in_use(SAMPLE), [500, 4500, 53]);
        assert!(udp_ports_in_use("cabeçalho só\n").is_empty());
        let v6 = "  sl local_address\n   1: 00000000000000000000000000000000:01F4 0000:0000 07\n";
        assert_eq!(udp_ports_in_use(v6), [500]);
    }
}
