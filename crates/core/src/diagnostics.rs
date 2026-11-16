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

/// Uma verificação, com dica de correção.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    /// O que foi verificado.
    pub name: &'static str,
    /// Resultado.
    pub status: CheckStatus,
    /// Detalhe do resultado.
    pub detail: String,
    /// Como corrigir (quando não está Ok).
    pub hint: Option<String>,
}

impl Check {
    fn ok(name: &'static str, detail: impl Into<String>) -> Self {
        Self {
            name,
            status: CheckStatus::Ok,
            detail: detail.into(),
            hint: None,
        }
    }

    fn bad(
        name: &'static str,
        status: CheckStatus,
        detail: impl Into<String>,
        hint: impl Into<String>,
    ) -> Self {
        Self {
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
    let busy = busy_ike_ports();
    if busy.is_empty() {
        Check::ok("Portas UDP 500/4500", "livres")
    } else {
        Check::bad(
            "Portas UDP 500/4500",
            CheckStatus::Fail,
            format!("em uso: {busy:?}"),
            "outra VPN IPsec (strongSwan, libreswan, outro container) está usando as \
             portas; desconecte-a antes",
        )
    }
}

fn check_xfrm_interface() -> Check {
    const NAME: &str = "Módulo xfrm_interface";
    if Path::new("/sys/module/xfrm_interface").exists() {
        return Check::ok(NAME, "carregado");
    }
    let release = fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default();
    let modules = Path::new("/lib/modules").join(release.trim());
    let builtin = fs::read_to_string(modules.join("modules.builtin")).unwrap_or_default();
    let listed = fs::read_to_string(modules.join("modules.dep")).unwrap_or_default();
    if builtin.contains("xfrm_interface") || listed.contains("xfrm_interface.ko") {
        Check::ok(NAME, "disponível (carregado automaticamente ao conectar)")
    } else {
        Check::bad(
            NAME,
            CheckStatus::Fail,
            "não encontrado no kernel",
            "é preciso um kernel ≥ 4.19 com CONFIG_XFRM_INTERFACE (padrão nas distribuições atuais)",
        )
    }
}

fn check_resolved() -> Check {
    if Path::new("/run/systemd/resolve/io.systemd.Resolve").exists()
        || Path::new("/run/systemd/resolve/stub-resolv.conf").exists()
    {
        Check::ok("systemd-resolved", "ativo")
    } else {
        Check::bad(
            "systemd-resolved",
            CheckStatus::Warn,
            "não encontrado",
            "sem o systemd-resolved, o DNS da VPN (domínios internos) não é configurado; \
             o túnel funciona por IP",
        )
    }
}

fn check_dbus_socket() -> Check {
    if Path::new(crate::engine::DBUS_SYSTEM_SOCKET).exists() {
        Check::ok("D-Bus do sistema", "disponível")
    } else {
        Check::bad(
            "D-Bus do sistema",
            CheckStatus::Warn,
            "socket ausente",
            "sem o D-Bus do sistema, o DNS da VPN não é configurado",
        )
    }
}

fn check_runtime_dir() -> Check {
    match default_runtime_dir() {
        Ok(dir) => Check::ok("Diretório de runtime", dir.display().to_string()),
        Err(e) => Check::bad(
            "Diretório de runtime",
            CheckStatus::Fail,
            e.to_string(),
            "rode numa sessão gráfica ou de login normal (com systemd-logind)",
        ),
    }
}

fn check_not_root() -> Check {
    if rustix::process::geteuid().is_root() {
        Check::bad(
            "Usuário",
            CheckStatus::Fail,
            "rodando como root",
            "não use sudo: rode como o seu usuário (que precisa estar no grupo docker)",
        )
    } else {
        Check::ok("Usuário", "comum (não root)")
    }
}

async fn check_docker() -> Check {
    const NAME: &str = "Docker";
    let result = match DockerEngine::connect() {
        Ok(engine) => engine.ping().await,
        Err(e) => Err(e),
    };
    match result {
        Ok(()) => Check::ok(NAME, "acessível"),
        Err(e) => Check::bad(
            NAME,
            CheckStatus::Fail,
            e.to_string(),
            "sudo systemctl enable --now docker; sudo usermod -aG docker $USER (e saia/entre na sessão)",
        ),
    }
}

/// Componente de rede (engine embutido). Sem ele, o app usa o Docker.
async fn check_engine() -> Vec<Check> {
    const NAME: &str = "Componente de rede";
    let native = NativeEngine::default();
    if native.installed() {
        let ping = tokio::task::spawn_blocking(move || native.ping()).await;
        return vec![match ping {
            Ok(Ok(version)) => Check::ok(
                NAME,
                format!("instalado (engine embutido, helper {version})"),
            ),
            Ok(Err(e)) => Check::bad(
                NAME,
                CheckStatus::Fail,
                e.to_string(),
                "reinstale o IPsec Client",
            ),
            Err(e) => Check::bad(
                NAME,
                CheckStatus::Fail,
                e.to_string(),
                "reinstale o IPsec Client",
            ),
        }];
    }
    vec![
        Check::bad(
            NAME,
            CheckStatus::Warn,
            "não instalado; usando o engine em Docker",
            "instale o componente de rede para dispensar o Docker",
        ),
        check_runtime_dir(),
        check_docker().await,
    ]
}

/// Verificações que impedem ou limitam uma conexão (rodadas antes do
/// `connect`). As portas não são verificadas aqui quando já há um engine
/// nosso rodando (ele é que estaria usando as portas).
pub async fn preflight(check_ports: bool) -> Vec<Check> {
    let mut checks = vec![check_not_root()];
    checks.extend(check_engine().await);
    checks.extend([
        check_xfrm_interface(),
        check_resolved(),
        check_dbus_socket(),
    ]);
    if check_ports {
        checks.push(check_ike_ports());
    }
    checks
}

/// Todas as verificações (`doctor`): preflight + chaveiro + imagem.
pub async fn doctor() -> Vec<Check> {
    let mut checks = preflight(true).await;
    checks.push(match crate::secrets::KeyringStore::new() {
        Ok(_) => Check::ok("Chaveiro do sistema", "disponível (credenciais podem ser lembradas)"),
        Err(e) => Check::bad(
            "Chaveiro do sistema",
            CheckStatus::Warn,
            e.to_string(),
            "instale/ative o GNOME Keyring ou o KWallet; sem ele, as credenciais são pedidas a cada conexão",
        ),
    });
    if !NativeEngine::default().installed() {
        let image = crate::engine::context::image_tag();
        let present = match DockerEngine::connect() {
            Ok(engine) => engine.image_present(&image).await.unwrap_or(false),
            Err(_) => false,
        };
        checks.push(if present {
            Check::ok("Imagem do engine", image)
        } else {
            Check::bad(
                "Imagem do engine",
                CheckStatus::Warn,
                format!("{image} ainda não construída"),
                "será construída na primeira conexão (~1 min, precisa de internet)",
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

    #[test]
    fn parses_proc_net_udp() {
        assert_eq!(udp_ports_in_use(SAMPLE), [500, 4500, 53]);
        assert!(udp_ports_in_use("cabeçalho só\n").is_empty());
        let v6 = "  sl local_address\n   1: 00000000000000000000000000000000:01F4 0000:0000 07\n";
        assert_eq!(udp_ports_in_use(v6), [500]);
    }
}
