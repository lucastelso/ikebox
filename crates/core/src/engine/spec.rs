//! Configuração de uma instância do engine e sua tradução para a Docker API.
//!
//! Tudo aqui é puro (sem I/O de rede), para ser testado sem Docker. As regras
//! de validação espelham as do `engine/entrypoint.sh`: o Rust valida antes, o
//! script valida de novo (defesa em profundidade).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bollard::models::{ContainerCreateBody, HostConfig};

use super::context;
use super::{DBUS_SYSTEM_SOCKET, DEFAULT_IFACE, DEFAULT_MTU, EngineError};

/// Nome padrão do container do engine.
pub const CONTAINER_NAME: &str = "ipsec-client-engine";
/// Onde o diretório do socket VICI é montado dentro do container.
pub const CONTAINER_SOCKET_DIR: &str = "/run/ipsec-client";
/// Nome do arquivo do socket VICI.
pub const SOCKET_FILE: &str = "charon.vici";
/// Label que marca containers gerenciados pelo app.
pub const MANAGED_LABEL: &str = "io.github.ipsec-client.managed";
/// Label com o id do perfil conectado (para status/disconnect/logs).
pub const PROFILE_LABEL: &str = "io.github.ipsec-client.profile";

/// Opções de segurança do container que dependem do host (ADR-0005).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HostSecurity {
    /// Host com SELinux: `--security-opt label=disable`.
    pub selinux: bool,
    /// Host com AppArmor: `--security-opt apparmor=unconfined`.
    pub apparmor: bool,
}

impl HostSecurity {
    /// Detecta SELinux/AppArmor pelo sysfs.
    #[must_use]
    pub fn detect() -> Self {
        Self {
            selinux: Path::new("/sys/fs/selinux/enforce").exists(),
            apparmor: Path::new("/sys/kernel/security/apparmor").exists(),
        }
    }
}

/// Tudo o que define uma instância do engine. Nada aqui é sensível: os
/// segredos chegam ao charon só via VICI (ADR-0007).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineSpec {
    /// Nome do container.
    pub container_name: String,
    /// Imagem (normalmente [`context::image_tag`]).
    pub image: String,
    /// Interface XFRM dedicada.
    pub iface: String,
    /// `if_id` das SAs/políticas (≠ 0).
    pub if_id: u32,
    /// MTU da interface.
    pub mtu: u16,
    /// gid que pode usar o socket VICI.
    pub socket_gid: u32,
    /// Nível de log do charon (0..=2).
    pub log_level: u8,
    /// Diretório do host onde fica o socket VICI (0700, do usuário).
    pub runtime_dir: PathBuf,
    /// Socket D-Bus do sistema (para o resolvectl), se existir.
    pub dbus_socket: Option<PathBuf>,
    /// Ajustes para SELinux/AppArmor.
    pub security: HostSecurity,
    /// Modo privilegiado de reserva (desligado por padrão).
    pub privileged: bool,
    /// Id do perfil desta sessão (vira label do container).
    pub profile_id: Option<String>,
}

impl EngineSpec {
    /// Spec padrão para uma sessão: imagem embutida, `ipsc0`, MTU 1400, log 1.
    #[must_use]
    pub fn new(runtime_dir: PathBuf, socket_gid: u32, if_id: u32) -> Self {
        let dbus = PathBuf::from(DBUS_SYSTEM_SOCKET);
        Self {
            container_name: CONTAINER_NAME.to_owned(),
            image: context::image_tag(),
            iface: DEFAULT_IFACE.to_owned(),
            if_id,
            mtu: DEFAULT_MTU,
            socket_gid,
            log_level: 1,
            runtime_dir,
            dbus_socket: dbus.exists().then_some(dbus),
            security: HostSecurity::detect(),
            privileged: false,
            profile_id: None,
        }
    }

    /// Caminho do socket VICI no host.
    #[must_use]
    pub fn socket_path(&self) -> PathBuf {
        self.runtime_dir.join(SOCKET_FILE)
    }

    /// Valida a spec com as mesmas regras do entrypoint.
    ///
    /// # Errors
    /// [`EngineError::InvalidSpec`] com a descrição do campo inválido.
    pub fn validate(&self) -> Result<(), EngineError> {
        let invalid = |msg: &str| Err(EngineError::InvalidSpec(msg.to_owned()));
        if !is_valid_iface(&self.iface) {
            return invalid(crate::tr!(
                "invalid interface name (use ^[a-z][a-z0-9_-]{0,14}$)",
                "nome de interface inválido (use ^[a-z][a-z0-9_-]{0,14}$)"
            ));
        }
        if self.if_id == 0 {
            return invalid(crate::tr!("if_id cannot be 0", "if_id não pode ser 0"));
        }
        if !(576..=9000).contains(&self.mtu) {
            return invalid(crate::tr!(
                "MTU outside 576..=9000",
                "MTU fora de 576..=9000"
            ));
        }
        if self.socket_gid > 2_147_483_647 {
            return invalid(crate::tr!("invalid socket gid", "gid do socket inválido"));
        }
        if self.log_level > 2 {
            return invalid(crate::tr!(
                "log level above 2 may expose sensitive material",
                "nível de log acima de 2 pode expor material sensível"
            ));
        }
        if !is_valid_container_name(&self.container_name) {
            return invalid(crate::tr!(
                "invalid container name",
                "nome de container inválido"
            ));
        }
        if !self.runtime_dir.is_absolute() {
            return invalid(crate::tr!(
                "the runtime directory must be absolute",
                "o diretório de runtime precisa ser absoluto"
            ));
        }
        if let Some(id) = &self.profile_id
            && !crate::profiles::is_valid_id(id)
        {
            return invalid(crate::tr!("invalid profile id", "id de perfil inválido"));
        }
        Ok(())
    }

    /// Variáveis de ambiente do container (só dados não sensíveis).
    #[must_use]
    pub fn env(&self) -> Vec<String> {
        vec![
            format!("IPSEC_CLIENT_IFACE={}", self.iface),
            format!("IPSEC_CLIENT_IF_ID={}", self.if_id),
            format!("IPSEC_CLIENT_MTU={}", self.mtu),
            format!("IPSEC_CLIENT_SOCKET_GID={}", self.socket_gid),
            format!("IPSEC_CLIENT_LOG_LEVEL={}", self.log_level),
        ]
    }

    /// `HostConfig`: rede do host, só `NET_ADMIN`, binds do socket e do D-Bus.
    #[must_use]
    pub fn host_config(&self) -> HostConfig {
        let mut binds = vec![format!(
            "{}:{CONTAINER_SOCKET_DIR}",
            self.runtime_dir.display()
        )];
        if let Some(dbus) = &self.dbus_socket {
            binds.push(format!("{}:{}", dbus.display(), DBUS_SYSTEM_SOCKET));
        }
        let mut security_opt = Vec::new();
        if self.security.selinux {
            security_opt.push("label=disable".to_owned());
        }
        if self.security.apparmor {
            security_opt.push("apparmor=unconfined".to_owned());
        }
        HostConfig {
            network_mode: Some("host".to_owned()),
            cap_add: Some(vec!["NET_ADMIN".to_owned()]),
            binds: Some(binds),
            security_opt: (!security_opt.is_empty()).then_some(security_opt),
            privileged: Some(self.privileged),
            auto_remove: Some(false),
            init: Some(false),
            ..Default::default()
        }
    }

    /// Corpo do `POST /containers/create`.
    #[must_use]
    pub fn create_body(&self) -> ContainerCreateBody {
        let mut labels = HashMap::from([
            (MANAGED_LABEL.to_owned(), "true".to_owned()),
            (context::HASH_LABEL.to_owned(), context::content_hash()),
        ]);
        if let Some(id) = &self.profile_id {
            labels.insert(PROFILE_LABEL.to_owned(), id.clone());
        }
        ContainerCreateBody {
            image: Some(self.image.clone()),
            env: Some(self.env()),
            labels: Some(labels),
            stop_timeout: Some(10),
            host_config: Some(self.host_config()),
            ..Default::default()
        }
    }
}

fn is_valid_iface(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some('a'..='z'))
        && name.len() <= 15
        && chars.all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_' | '-'))
}

fn is_valid_container_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> EngineSpec {
        let mut spec = EngineSpec::new(PathBuf::from("/run/user/1000/ipsec-client"), 1000, 4096);
        spec.dbus_socket = Some(PathBuf::from(DBUS_SYSTEM_SOCKET));
        spec.security = HostSecurity::default();
        spec
    }

    #[test]
    fn default_spec_is_valid() {
        spec().validate().expect("spec padrão válida");
        assert_eq!(spec().image, context::image_tag());
        assert_eq!(
            spec().socket_path(),
            PathBuf::from("/run/user/1000/ipsec-client/charon.vici")
        );
    }

    #[test]
    fn invalid_values_are_rejected() {
        let cases: &[fn(&mut EngineSpec)] = &[
            |s| s.iface = "Ipsc0".into(),
            |s| s.iface = "0ipsc".into(),
            |s| s.iface = "a-very-long-iface-name".into(),
            |s| s.iface = "ipsc0;rm".into(),
            |s| s.if_id = 0,
            |s| s.mtu = 100,
            |s| s.mtu = 9001,
            |s| s.log_level = 3,
            |s| s.socket_gid = u32::MAX,
            |s| s.container_name = "bad name".into(),
            |s| s.runtime_dir = PathBuf::from("relativo"),
        ];
        for (i, mutate) in cases.iter().enumerate() {
            let mut s = spec();
            mutate(&mut s);
            assert!(s.validate().is_err(), "caso {i} deveria ser inválido");
        }
    }

    #[test]
    fn env_has_only_non_sensitive_settings() {
        assert_eq!(
            spec().env(),
            [
                "IPSEC_CLIENT_IFACE=ipsc0",
                "IPSEC_CLIENT_IF_ID=4096",
                "IPSEC_CLIENT_MTU=1400",
                "IPSEC_CLIENT_SOCKET_GID=1000",
                "IPSEC_CLIENT_LOG_LEVEL=1",
            ]
        );
    }

    #[test]
    fn host_config_is_least_privilege() {
        let hc = spec().host_config();
        assert_eq!(hc.network_mode.as_deref(), Some("host"));
        assert_eq!(hc.cap_add, Some(vec!["NET_ADMIN".to_owned()]));
        assert_eq!(hc.privileged, Some(false));
        assert_eq!(hc.security_opt, None);
        assert_eq!(
            hc.binds,
            Some(vec![
                "/run/user/1000/ipsec-client:/run/ipsec-client".to_owned(),
                "/run/dbus/system_bus_socket:/run/dbus/system_bus_socket".to_owned(),
            ])
        );
    }

    #[test]
    fn host_config_adapts_to_selinux_apparmor_and_fallback() {
        let mut s = spec();
        s.security = HostSecurity {
            selinux: true,
            apparmor: true,
        };
        s.privileged = true;
        s.dbus_socket = None;
        let hc = s.host_config();
        assert_eq!(
            hc.security_opt,
            Some(vec![
                "label=disable".to_owned(),
                "apparmor=unconfined".to_owned()
            ])
        );
        assert_eq!(hc.privileged, Some(true));
        assert_eq!(hc.binds.map(|b| b.len()), Some(1));
    }

    #[test]
    fn create_body_carries_labels() {
        let body = spec().create_body();
        let labels = body.labels.expect("labels");
        assert_eq!(labels.get(MANAGED_LABEL).map(String::as_str), Some("true"));
        assert_eq!(
            labels.get(context::HASH_LABEL),
            Some(&context::content_hash())
        );
        assert_eq!(body.stop_timeout, Some(10));
        assert!(!labels.contains_key(PROFILE_LABEL));

        let mut s = spec();
        s.profile_id = Some("minha-vpn".into());
        let labels = s.create_body().labels.expect("labels");
        assert_eq!(
            labels.get(PROFILE_LABEL).map(String::as_str),
            Some("minha-vpn")
        );
        s.profile_id = Some("../x".into());
        assert!(s.validate().is_err());
    }
}
