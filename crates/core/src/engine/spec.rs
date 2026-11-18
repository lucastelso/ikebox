//! Configuration of an engine instance and its translation to the Docker API.
//!
//! Everything here is pure (no network I/O), so it can be tested without
//! Docker. The validation rules mirror those of `engine/entrypoint.sh`: Rust
//! validates first, the script validates again (defense in depth).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bollard::models::{ContainerCreateBody, HostConfig};

use super::context;
use super::{DBUS_SYSTEM_SOCKET, DEFAULT_IFACE, DEFAULT_MTU, EngineError};

/// Default name of the engine container.
pub const CONTAINER_NAME: &str = "ikebox-engine";
/// Where the VICI socket directory is mounted inside the container.
pub const CONTAINER_SOCKET_DIR: &str = "/run/ikebox";
/// File name of the VICI socket.
pub const SOCKET_FILE: &str = "charon.vici";
/// Label that marks the containers managed by the app.
pub const MANAGED_LABEL: &str = "io.github.ikebox.managed";
/// Label with the connected profile's id (for status/disconnect/logs).
pub const PROFILE_LABEL: &str = "io.github.ikebox.profile";

/// Container security options that depend on the host (ADR-0005).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HostSecurity {
    /// Host with SELinux: `--security-opt label=disable`.
    pub selinux: bool,
    /// Host with AppArmor: `--security-opt apparmor=unconfined`.
    pub apparmor: bool,
}

impl HostSecurity {
    /// Detects SELinux/AppArmor through sysfs.
    #[must_use]
    pub fn detect() -> Self {
        Self {
            selinux: Path::new("/sys/fs/selinux/enforce").exists(),
            apparmor: Path::new("/sys/kernel/security/apparmor").exists(),
        }
    }
}

/// Everything that defines an engine instance. Nothing here is sensitive: the
/// secrets reach charon only over VICI (ADR-0007).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineSpec {
    /// Container name.
    pub container_name: String,
    /// Image (usually [`context::image_tag`]).
    pub image: String,
    /// Dedicated XFRM interface.
    pub iface: String,
    /// `if_id` of the SAs and policies (≠ 0).
    pub if_id: u32,
    /// Interface MTU.
    pub mtu: u16,
    /// gid that may use the VICI socket.
    pub socket_gid: u32,
    /// charon log level (0..=2).
    pub log_level: u8,
    /// Host directory holding the VICI socket (0700, the user's).
    pub runtime_dir: PathBuf,
    /// System D-Bus socket (for resolvectl), if it exists.
    pub dbus_socket: Option<PathBuf>,
    /// Adjustments for SELinux/AppArmor.
    pub security: HostSecurity,
    /// Fallback privileged mode (off by default).
    pub privileged: bool,
    /// Profile id of this session (it becomes the container's label).
    pub profile_id: Option<String>,
}

impl EngineSpec {
    /// Default spec for a session: embedded image, `ipsc0`, MTU 1400, log 1.
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

    /// Path of the VICI socket on the host.
    #[must_use]
    pub fn socket_path(&self) -> PathBuf {
        self.runtime_dir.join(SOCKET_FILE)
    }

    /// Validates the spec with the same rules as the entrypoint.
    ///
    /// # Errors
    /// [`EngineError::InvalidSpec`] with the description of the invalid field.
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

    /// Container environment variables (non-sensitive data only).
    #[must_use]
    pub fn env(&self) -> Vec<String> {
        vec![
            format!("IKEBOX_IFACE={}", self.iface),
            format!("IKEBOX_IF_ID={}", self.if_id),
            format!("IKEBOX_MTU={}", self.mtu),
            format!("IKEBOX_SOCKET_GID={}", self.socket_gid),
            format!("IKEBOX_LOG_LEVEL={}", self.log_level),
        ]
    }

    /// `HostConfig`: host network, `NET_ADMIN` only, binds of the socket and D-Bus.
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

    /// Body of `POST /containers/create`.
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
        let mut spec = EngineSpec::new(PathBuf::from("/run/user/1000/ikebox"), 1000, 4096);
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
            PathBuf::from("/run/user/1000/ikebox/charon.vici")
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
                "IKEBOX_IFACE=ipsc0",
                "IKEBOX_IF_ID=4096",
                "IKEBOX_MTU=1400",
                "IKEBOX_SOCKET_GID=1000",
                "IKEBOX_LOG_LEVEL=1",
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
                "/run/user/1000/ikebox:/run/ikebox".to_owned(),
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
