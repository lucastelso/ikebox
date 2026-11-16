//! Perfis de conexão genéricos (ADR-0008).
//!
//! - [`Profile`]: modelo em JSON com `schema_version`, sem credenciais;
//! - [`validate`]: regras (o Rust é a fonte da verdade);
//! - [`Profile::to_load_conn`] e [`Profile::updown_command`]: geração da
//!   configuração do charon;
//! - [`ProfileStore`]: disco, importação e exportação.

mod model;
mod render;
mod store;
mod validate;

pub use model::{
    AuthMethod, AuthSettings, ChildAction, ChildSettings, CredentialKind, DnsSettings, IkeSettings,
    NetworkSettings, Profile, SCHEMA_VERSION,
};
pub use render::{ConnParams, UPDOWN_PATH};
pub use store::{MAX_PROFILE_BYTES, ProfileList, ProfileStore, parse_profile, to_json};
pub use validate::{ValidationIssue, validate};

/// O id é válido (`[a-z0-9][a-z0-9-]{0,62}`)? Usado também nas chaves do
/// chaveiro.
#[must_use]
pub fn is_valid_id(id: &str) -> bool {
    validate::is_slug(id)
}

/// Erros de perfil. As mensagens são para o usuário final (pt-BR).
#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    /// JSON malformado ou com campos desconhecidos.
    #[error("perfil com JSON inválido: {0}")]
    Json(#[from] serde_json::Error),
    /// Sem `schema_version`.
    #[error("o arquivo não parece um perfil do ipsec-client (falta schema_version)")]
    MissingSchemaVersion,
    /// Versão de formato desconhecida.
    #[error("versão de perfil não suportada ({0}); atualize o ipsec-client")]
    UnsupportedVersion(u64),
    /// Campos inválidos.
    #[error("perfil inválido: {}", .0.iter().map(ToString::to_string).collect::<Vec<_>>().join("; "))]
    Invalid(Vec<ValidationIssue>),
    /// Credencial necessária ausente.
    #[error("credencial ausente ou inválida: {0}")]
    MissingCredential(&'static str),
    /// Valor que não pode ir para o comando do updown.
    #[error("{0}")]
    Unsafe(String),
    /// Perfil inexistente.
    #[error("perfil '{0}' não encontrado")]
    NotFound(String),
    /// Id já usado.
    #[error("já existe um perfil com id '{0}'")]
    AlreadyExists(String),
    /// Arquivo acima do limite.
    #[error("arquivo de perfil grande demais")]
    TooLarge,
    /// Sem diretório de configuração.
    #[error("não foi possível achar o diretório de configuração (XDG_CONFIG_HOME/HOME)")]
    NoConfigDir,
    /// Erro de E/S.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    use super::*;
    use crate::vici::{Message, Value};

    const LEGACY: &str = include_str!("../../../../presets/exemplo-ikev1-agressivo-xauth.json");

    fn legacy() -> Profile {
        parse_profile(LEGACY).expect("preset legado válido")
    }

    /// Renderiza uma mensagem VICI como texto (só em testes: aqui não há
    /// segredos, e o snapshot precisa dos valores).
    fn render(msg: &Message, indent: usize, out: &mut String) {
        let pad = "    ".repeat(indent);
        for (key, value) in msg.iter() {
            match value {
                Value::Str(v) => {
                    let _ = writeln!(out, "{pad}{key} = {}", String::from_utf8_lossy(v));
                }
                Value::List(items) => {
                    let items: Vec<_> = items.iter().map(|i| String::from_utf8_lossy(i)).collect();
                    let _ = writeln!(out, "{pad}{key} = [{}]", items.join(", "));
                }
                Value::Section(section) => {
                    let _ = writeln!(out, "{pad}{key} {{");
                    render(section, indent + 1, out);
                    let _ = writeln!(out, "{pad}}}");
                }
            }
        }
    }

    #[test]
    fn legacy_preset_renders_expected_load_conn() {
        let profile = legacy();
        let params = ConnParams {
            iface: "ipsc0",
            if_id: profile.if_id(),
            xauth_user: Some("usuario.exemplo"),
        };
        let msg = profile.to_load_conn(&params).expect("load-conn");
        let mut text = String::new();
        render(&msg, 0, &mut text);
        insta::assert_snapshot!("legacy_load_conn", text);
    }

    #[test]
    fn ikev2_preset_renders_expected_load_conn() {
        let profile = parse_profile(include_str!("../../../../presets/exemplo-ikev2-psk.json"))
            .expect("preset IKEv2 válido");
        let params = ConnParams {
            iface: "ipsc0",
            if_id: 7,
            xauth_user: None,
        };
        let msg = profile.to_load_conn(&params).expect("load-conn");
        let mut text = String::new();
        render(&msg, 0, &mut text);
        insta::assert_snapshot!("ikev2_load_conn", text);
    }

    #[test]
    fn all_presets_are_valid_and_canonical() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../presets");
        let mut count = 0;
        for entry in std::fs::read_dir(dir).expect("presets/") {
            let path = entry.expect("entrada").path();
            if path.extension().is_some_and(|e| e == "json") {
                let json = std::fs::read_to_string(&path).expect("ler preset");
                let profile =
                    parse_profile(&json).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                let back = parse_profile(&to_json(&profile).expect("json")).expect("reler");
                assert_eq!(profile, back, "{}", path.display());
                count += 1;
            }
        }
        assert!(count >= 2);
    }

    #[test]
    fn xauth_requires_user() {
        let profile = legacy();
        let params = ConnParams {
            iface: "ipsc0",
            if_id: 1,
            xauth_user: None,
        };
        assert!(matches!(
            profile.to_load_conn(&params),
            Err(ProfileError::MissingCredential(_))
        ));
        let params = ConnParams {
            xauth_user: Some("com'aspas"),
            ..params
        };
        assert!(profile.to_load_conn(&params).is_err());
    }

    #[test]
    fn required_credentials_follow_method() {
        assert_eq!(
            legacy().required_credentials(),
            [
                CredentialKind::Psk,
                CredentialKind::XauthUser,
                CredentialKind::XauthPassword
            ]
        );
        let mut p = legacy();
        p.auth.method = AuthMethod::Psk;
        assert_eq!(p.required_credentials(), [CredentialKind::Psk]);
    }

    #[test]
    fn if_id_is_stable_and_nonzero() {
        let p = legacy();
        assert_eq!(p.if_id(), legacy().if_id());
        assert_ne!(p.if_id(), 0);
        assert!(p.if_id() <= 0x7fff_ffff);
    }

    #[test]
    fn invalid_profiles_report_every_field() {
        let mut p = legacy();
        p.id = "Com Espaço".into();
        p.gateway = "vpn..example".into();
        p.ike.version = 2; // agressivo + XAuth ficam inválidos
        p.child.remote_ts = vec!["192.0.2.0/99".into()];
        p.network.dns.domains = vec!["~corp'; reboot".into()];
        p.network.mtu = 10;
        let fields: Vec<String> = validate(&p).into_iter().map(|i| i.field).collect();
        for expected in [
            "id",
            "gateway",
            "ike.aggressive",
            "auth.method",
            "child.remote_ts[0]",
            "network.dns.domains[0]",
            "network.mtu",
        ] {
            assert!(
                fields.iter().any(|f| f == expected),
                "faltou {expected}: {fields:?}"
            );
        }
    }

    #[test]
    fn parse_rejects_unknown_fields_versions_and_secrets() {
        // Um campo "psk" (ou qualquer desconhecido) é recusado: perfis não
        // carregam credenciais.
        let with_psk = LEGACY.replacen(
            "\"schema_version\": 1,",
            "\"schema_version\": 1, \"psk\": \"x\",",
            1,
        );
        assert!(matches!(
            parse_profile(&with_psk),
            Err(ProfileError::Json(_))
        ));
        let v2 = LEGACY.replacen("\"schema_version\": 1", "\"schema_version\": 2", 1);
        assert!(matches!(
            parse_profile(&v2),
            Err(ProfileError::UnsupportedVersion(2))
        ));
        assert!(matches!(
            parse_profile("{}"),
            Err(ProfileError::MissingSchemaVersion)
        ));
        assert!(parse_profile("não é json").is_err());
    }

    #[test]
    fn store_roundtrip_with_private_permissions() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = ProfileStore::new(tmp.path().join("profiles"));
        assert!(store.list().expect("vazio").profiles.is_empty());

        let profile = legacy();
        store.save(&profile).expect("save");
        let dir_mode = std::fs::metadata(store.dir())
            .expect("dir")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dir_mode, 0o700);
        let file = store.dir().join("exemplo-legado.json");
        let file_mode = std::fs::metadata(&file)
            .expect("arquivo")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(file_mode, 0o600);

        assert_eq!(store.load("exemplo-legado").expect("load"), profile);
        std::fs::write(store.dir().join("quebrado.json"), "{").expect("arquivo quebrado");
        let list = store.list().expect("list");
        assert_eq!(list.profiles.len(), 1);
        assert_eq!(list.broken.len(), 1);

        let exported = tmp.path().join("export.json");
        store.export("exemplo-legado", &exported).expect("export");
        assert!(matches!(
            store.import(&exported, false),
            Err(ProfileError::AlreadyExists(_))
        ));
        store.import(&exported, true).expect("import com overwrite");

        store.delete("exemplo-legado").expect("delete");
        assert!(matches!(
            store.load("exemplo-legado"),
            Err(ProfileError::NotFound(_))
        ));
        // Ids com caminho não escapam do diretório.
        assert!(store.load("../etc/passwd").is_err());
    }
}
