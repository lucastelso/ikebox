//! Perfis de conexão genéricos (ADR-0008).
//!
//! - [`Profile`]: modelo em JSON com `schema_version`, sem credenciais;
//! - [`validate`]: regras (o Rust é a fonte da verdade);
//! - [`Profile::to_load_conn`] e [`Profile::updown_command`]: geração da
//!   configuração do charon;
//! - [`ProfileStore`]: disco, importação e exportação;
//! - [`Profile::security`]: nível de segurança (só avisa, nunca bloqueia).

mod model;
mod render;
mod schema;
mod security;
mod store;
mod swanctl;
mod templates;
mod validate;

pub use model::{
    AuthSettings, ChildAction, ChildSettings, CredentialKind, DnsSettings, IkeSettings,
    LocalMethod, LocalRound, NetworkSettings, Profile, RemoteAuth, RemoteMethod, SCHEMA_VERSION,
};
pub use render::{ConnParams, UPDOWN_PATH};
pub use schema::{OLDEST_SCHEMA_VERSION, schema_of, to_json};
pub use security::{FindingKind, SecurityFinding, SecurityLevel, SecurityReport, proposals_level};
pub use store::{MAX_PROFILE_BYTES, ProfileList, ProfileStore, parse_profile, read_profile_file};
pub use swanctl::{SwanctlImport, SwanctlImportOptions, import_swanctl};
pub use templates::{ProposalPreset, Template};
pub use validate::{ValidationIssue, validate};

/// O id é válido (`[a-z0-9][a-z0-9-]{0,62}`)? Usado também nas chaves do
/// chaveiro.
#[must_use]
pub fn is_valid_id(id: &str) -> bool {
    validate::is_slug(id)
}

/// Profile errors. Messages are for the end user (see [`crate::i18n`]).
#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    /// Malformed JSON or unknown fields.
    #[error("{}", crate::trf!("profile with invalid JSON: {}", "perfil com JSON inválido: {}", .0))]
    Json(#[from] serde_json::Error),
    /// No `schema_version`.
    #[error("{}", crate::tr!(
        "the file does not look like an IPsec Client profile (schema_version is missing)",
        "o arquivo não parece um perfil do ipsec-client (falta schema_version)"
    ))]
    MissingSchemaVersion,
    /// Unknown format version.
    #[error("{}", crate::trf!(
        "unsupported profile version ({}); update IPsec Client",
        "versão de perfil não suportada ({}); atualize o ipsec-client",
        .0
    ))]
    UnsupportedVersion(u64),
    /// Invalid fields.
    #[error("{}", crate::trf!(
        "invalid profile: {}",
        "perfil inválido: {}",
        .0.iter().map(ToString::to_string).collect::<Vec<_>>().join("; ")
    ))]
    Invalid(Vec<ValidationIssue>),
    /// A required credential is missing.
    #[error("{}", crate::trf!("missing or invalid credential: {}", "credencial ausente ou inválida: {}", .0))]
    MissingCredential(&'static str),
    /// A value that cannot go into the updown command.
    #[error("{0}")]
    Unsafe(String),
    /// No such profile.
    #[error("{}", crate::trf!("profile '{}' not found", "perfil '{}' não encontrado", .0))]
    NotFound(String),
    /// Id already in use.
    #[error("{}", crate::trf!("a profile with id '{}' already exists", "já existe um perfil com id '{}'", .0))]
    AlreadyExists(String),
    /// File above the size limit.
    #[error("{}", crate::tr!("profile file too large", "arquivo de perfil grande demais"))]
    TooLarge,
    /// swanctl.conf that could not be imported.
    #[error("swanctl.conf: {0}")]
    Swanctl(String),
    /// No configuration directory.
    #[error("{}", crate::tr!(
        "could not find the configuration directory (XDG_CONFIG_HOME/HOME)",
        "não foi possível achar o diretório de configuração (XDG_CONFIG_HOME/HOME)"
    ))]
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

    #[test]
    fn profile_files_are_read_with_a_real_limit() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ok = dir.path().join("ok.json");
        std::fs::write(&ok, LEGACY).expect("write");
        assert_eq!(read_profile_file(&ok).expect("read"), LEGACY);

        let exact = dir.path().join("exact");
        std::fs::write(&exact, vec![b' '; MAX_PROFILE_BYTES as usize]).expect("write");
        assert!(read_profile_file(&exact).is_ok());
        let big = dir.path().join("big");
        std::fs::write(&big, vec![b' '; MAX_PROFILE_BYTES as usize + 1]).expect("write");
        assert!(matches!(
            read_profile_file(&big),
            Err(ProfileError::TooLarge)
        ));

        // Endless sources that report size zero: refused at once, not read
        // until the memory runs out.
        for special in ["/dev/zero", "/dev/urandom"] {
            let err = read_profile_file(Path::new(special)).expect_err(special);
            assert!(err.to_string().contains("regular file"), "{err}");
        }
        assert!(read_profile_file(dir.path()).is_err(), "a directory");
        assert!(read_profile_file(&dir.path().join("missing")).is_err());
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
            updown: Some(UPDOWN_PATH),
            if_id: profile.if_id(),
            xauth_user: Some("usuario.exemplo"),
        };
        let msg = profile.to_load_conn(&params).expect("load-conn");
        let mut text = String::new();
        render(&msg, 0, &mut text);
        insta::assert_snapshot!("legacy_load_conn", text);
    }

    #[test]
    fn native_engine_load_conn_has_no_updown() {
        // No engine embutido, o comando do updown é do root (ADR-0019).
        let profile = legacy();
        let params = ConnParams {
            iface: "ipsc0",
            updown: None,
            if_id: profile.if_id(),
            xauth_user: Some("usuario.exemplo"),
        };
        let msg = profile.to_load_conn(&params).expect("load-conn");
        let mut text = String::new();
        render(&msg, 0, &mut text);
        assert!(!text.contains("updown"), "{text}");
        assert!(text.contains(&format!("if_id_in = {}", profile.if_id())));
    }

    #[test]
    fn ikev2_preset_renders_expected_load_conn() {
        let profile = parse_profile(include_str!("../../../../presets/exemplo-ikev2-psk.json"))
            .expect("preset IKEv2 válido");
        let params = ConnParams {
            iface: "ipsc0",
            updown: Some(UPDOWN_PATH),
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
            updown: Some(UPDOWN_PATH),
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
        p.auth.local.truncate(1);
        assert_eq!(p.required_credentials(), [CredentialKind::Psk]);
        assert_eq!(p.storable_credentials(), [CredentialKind::Psk]);
        // A one-time XAuth password is required but never stored.
        let mut otp = legacy();
        otp.auth.local[1].otp = true;
        assert_eq!(otp.required_credentials().len(), 3);
        assert_eq!(
            otp.storable_credentials(),
            [CredentialKind::Psk, CredentialKind::XauthUser]
        );
    }

    #[test]
    fn schema_1_reads_as_rounds_and_is_written_back_unchanged() {
        let p = legacy();
        assert_eq!(p.auth.local.len(), 2);
        assert_eq!(p.auth.local[0].method, LocalMethod::Psk);
        assert_eq!(p.auth.local[1].method, LocalMethod::Xauth);
        assert_eq!(schema_of(&p), 1);
        // Same file as 0.5 wrote: colleagues on older versions can import it.
        let written = to_json(&p).expect("json");
        assert!(
            written.starts_with("{\n  \"schema_version\": 1,"),
            "{written}"
        );
        let original: serde_json::Value = serde_json::from_str(LEGACY).expect("json");
        let again: serde_json::Value = serde_json::from_str(&written).expect("json");
        assert_eq!(original, again);
        assert_eq!(parse_profile(&written).expect("parse"), p);
    }

    #[test]
    fn what_schema_1_cannot_say_is_written_as_schema_2() {
        let mut p = legacy();
        p.auth.local[1].otp = true;
        assert_eq!(schema_of(&p), 2);
        let written = to_json(&p).expect("json");
        assert!(
            written.starts_with("{\n  \"schema_version\": 2,"),
            "{written}"
        );
        assert!(written.contains("\"otp\": true"), "{written}");
        assert!(!written.contains("local_id"), "{written}");
        assert_eq!(parse_profile(&written).expect("parse"), p);
        // A schema 2 file of a profile schema 1 can say also reads back.
        let v2 = written.replace(",\n        \"otp\": true", "");
        let plain = parse_profile(&v2).expect("parse");
        assert_eq!(schema_of(&plain), 1);
        assert_eq!(plain, legacy());
    }

    #[test]
    fn auth_rounds_are_validated() {
        let mut p = legacy();
        p.auth.local.swap(0, 1);
        let fields: Vec<_> = validate(&p).into_iter().map(|i| i.field).collect();
        assert!(fields.contains(&"auth.local[0]".to_owned()), "{fields:?}");
        let mut p = legacy();
        p.auth.local[0].otp = true;
        p.auth.local[1].id = Some("usuario".to_owned());
        let fields: Vec<_> = validate(&p).into_iter().map(|i| i.field).collect();
        assert!(
            fields.contains(&"auth.local[0].otp".to_owned()),
            "{fields:?}"
        );
        assert!(
            fields.contains(&"auth.local[1].id".to_owned()),
            "{fields:?}"
        );
        let mut p = legacy();
        p.ike.version = 2;
        p.ike.aggressive = false;
        let fields: Vec<_> = validate(&p).into_iter().map(|i| i.field).collect();
        assert_eq!(fields, ["auth.local[1]"]);
        // In a schema 1 file the problem names the field of that file.
        let bad = LEGACY.replacen("\"keyid:", "\" keyid:", 1);
        let Err(ProfileError::Invalid(issues)) = parse_profile(&bad) else {
            panic!("should be invalid");
        };
        assert_eq!(issues[0].field, "auth.local_id");
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
            "auth.local[1]",
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
        let v3 = LEGACY.replacen("\"schema_version\": 1", "\"schema_version\": 3", 1);
        assert!(matches!(
            parse_profile(&v3),
            Err(ProfileError::UnsupportedVersion(3))
        ));
        // Schema 1 fields in a schema 2 file are refused, and the other way.
        let v2_with_v1_auth = LEGACY.replacen("\"schema_version\": 1", "\"schema_version\": 2", 1);
        assert!(matches!(
            parse_profile(&v2_with_v1_auth),
            Err(ProfileError::Json(_))
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
