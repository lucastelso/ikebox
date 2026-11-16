//! Comandos expostos à UI. Finos: delegam ao `ipsec-client-core`.
//!
//! Todos são `async` (comandos síncronos do Tauri rodam na thread da UI), e as
//! operações bloqueantes (disco, chaveiro via D-Bus) vão para
//! `spawn_blocking`.

use ipsec_client_core::profiles::{Profile, ProfileStore};
use ipsec_client_core::secrets::{Credentials, KeyringStore, SecretStore, mask};
use ipsec_client_core::session::{self, ConnectOptions};
use ipsec_client_core::{tr, trf};
use secrecy::{ExposeSecret, SecretString};
use tauri::AppHandle;
use tauri_specta::Event as _;
use tokio::sync::Mutex;

use crate::dto::{
    AppInfo, BrokenProfile, Connection, CredentialField, CredentialsInput, CredentialsStatus,
    DiagnosticCheck, ProfileList, ProfileSummary, SessionUpdate, UiError, UiErrorKind,
};

/// Impede duas operações de conexão ao mesmo tempo (ex.: clique duplo).
#[derive(Debug, Default)]
pub struct ConnectionLock(pub(crate) Mutex<()>);

/// Roda trabalho bloqueante fora das threads do runtime.
pub(crate) async fn blocking<T, F>(f: F) -> Result<T, UiError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, UiError> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| {
        UiError::new(
            UiErrorKind::Other,
            trf!("Internal failure: {e}", "Falha interna: {e}"),
        )
    })?
}

fn store() -> Result<ProfileStore, UiError> {
    Ok(ProfileStore::new(ProfileStore::default_dir()?))
}

fn load_profile(id: &str) -> Result<Profile, UiError> {
    Ok(store()?.load(id)?)
}

/// Versão do app e do engine.
#[tauri::command]
#[specta::specta]
#[must_use]
pub fn app_info() -> AppInfo {
    AppInfo {
        version: ipsec_client_core::VERSION.to_owned(),
        engine_image: ipsec_client_core::engine::context::image_tag(),
    }
}

/// Diagnóstico do ambiente (o mesmo do `ipsec-client-cli doctor`).
#[tauri::command]
#[specta::specta]
pub async fn run_diagnostics() -> Vec<DiagnosticCheck> {
    ipsec_client_core::diagnostics::doctor()
        .await
        .iter()
        .map(DiagnosticCheck::from)
        .collect()
}

/// Perfis disponíveis.
#[tauri::command]
#[specta::specta]
pub async fn list_profiles() -> Result<ProfileList, UiError> {
    blocking(|| {
        let list = store()?.list()?;
        Ok(ProfileList {
            profiles: list.profiles.iter().map(ProfileSummary::from).collect(),
            broken: list
                .broken
                .into_iter()
                .map(|(path, error)| BrokenProfile {
                    file: path
                        .file_name()
                        .map(|f| f.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    error,
                })
                .collect(),
        })
    })
    .await
}

/// O que o perfil exige e o que já está guardado (sem revelar valores).
#[tauri::command]
#[specta::specta]
pub async fn credentials_status(profile_id: String) -> Result<CredentialsStatus, UiError> {
    blocking(move || {
        let profile = load_profile(&profile_id)?;
        let required = profile.required_credentials();
        let keyring = KeyringStore::new().ok();
        let mut stored = Vec::new();
        let mut masked_user = None;
        if let Some(store) = &keyring {
            for kind in &required {
                if let Some(value) = store.get(&profile.id, *kind)? {
                    if !kind.is_secret() {
                        masked_user = Some(mask(value.expose_secret()));
                    }
                    stored.push(CredentialField::from(*kind));
                }
            }
        }
        Ok(CredentialsStatus {
            required: required.into_iter().map(CredentialField::from).collect(),
            stored,
            masked_user,
            keyring_available: keyring.is_some(),
        })
    })
    .await
}

/// Conecta o perfil. As credenciais digitadas completam (ou substituem) as
/// guardadas; com `remember`, ficam no chaveiro. O progresso chega pelo
/// evento [`SessionUpdate`].
#[tauri::command]
#[specta::specta]
pub async fn connect(
    app: AppHandle,
    lock: tauri::State<'_, ConnectionLock>,
    profile_id: String,
    credentials: CredentialsInput,
    remember: bool,
) -> Result<Connection, UiError> {
    let Ok(_guard) = lock.0.try_lock() else {
        return Err(UiError::new(
            UiErrorKind::Busy,
            tr!(
                "A connection is already in progress.",
                "Já há uma conexão em andamento."
            ),
        ));
    };

    let (profile, creds) = blocking(move || {
        let profile = load_profile(&profile_id)?;
        let keyring = KeyringStore::new().ok();
        let mut creds = match &keyring {
            Some(store) => Credentials::load(store, &profile)?,
            None => Credentials::new(),
        };
        apply_input(&mut creds, credentials);
        let missing = creds.missing(&profile);
        if !missing.is_empty() {
            return Err(UiError {
                kind: UiErrorKind::MissingCredentials,
                message: tr!(
                    "Enter the missing credentials.",
                    "Informe as credenciais que faltam."
                )
                .to_owned(),
                details: missing
                    .iter()
                    .map(|k| ipsec_client_core::secrets::describe(*k).to_owned())
                    .collect(),
            });
        }
        if remember {
            match &keyring {
                Some(store) => creds.save(store, &profile)?,
                None => {
                    return Err(UiError::new(
                        UiErrorKind::Keyring,
                        tr!(
                            "The system keyring is not available; uncheck \"remember\".",
                            "O chaveiro do sistema não está disponível; desmarque \"lembrar\"."
                        ),
                    ));
                }
            }
        }
        Ok((profile, creds))
    })
    .await?;

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let emitter = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.recv().await {
            let _ = SessionUpdate::from(event).emit(&emitter);
        }
    });

    let info = session::connect(&profile, &creds, &ConnectOptions::default(), Some(tx)).await?;
    Ok(Connection::from(&info))
}

/// Completa as credenciais com o que foi digitado (campos vazios são ignorados).
fn apply_input(creds: &mut Credentials, input: CredentialsInput) {
    use ipsec_client_core::profiles::CredentialKind as K;
    for (kind, value) in [
        (K::Psk, input.psk),
        (K::XauthUser, input.xauth_user),
        (K::XauthPassword, input.xauth_password),
    ] {
        if let Some(value) = value.filter(|v| !v.is_empty()) {
            creds.set(kind, SecretString::from(value));
        }
    }
}

/// Desconecta (avisa o gateway e derruba o engine).
#[tauri::command]
#[specta::specta]
pub async fn disconnect() -> Result<bool, UiError> {
    Ok(session::disconnect().await?)
}

/// Conexão ativa, se houver (funciona mesmo se ela foi aberta pelo CLI).
#[tauri::command]
#[specta::specta]
pub async fn status() -> Result<Option<Connection>, UiError> {
    Ok(session::status().await?.as_ref().map(Connection::from))
}

/// Apaga as credenciais guardadas do perfil.
#[tauri::command]
#[specta::specta]
pub async fn forget_credentials(profile_id: String) -> Result<(), UiError> {
    blocking(move || {
        let profile = load_profile(&profile_id)?;
        Credentials::forget(&KeyringStore::new()?, &profile)?;
        Ok(())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use ipsec_client_core::profiles::CredentialKind;

    #[test]
    fn input_fills_only_given_fields() {
        let mut creds = Credentials::new();
        creds.set(CredentialKind::Psk, SecretString::from("guardada"));
        apply_input(
            &mut creds,
            CredentialsInput {
                psk: Some(String::new()),
                xauth_user: Some("usuario".into()),
                xauth_password: None,
            },
        );
        let psk = creds.get(CredentialKind::Psk).expect("psk");
        assert_eq!(psk.expose_secret(), "guardada", "vazio não substitui");
        assert_eq!(creds.xauth_user(), Some("usuario"));
        assert!(creds.get(CredentialKind::XauthPassword).is_none());
    }
}

/// Log ready to copy or attach to a bug report: besides the identities, the
/// gateway, the profile's DNS servers and domains and every IP address become
/// placeholders (`<gateway>`, `<ip-1>`, `<domain-1>`). Text copied in the app
/// always goes through here.
#[tauri::command]
#[specta::specta]
pub async fn anonymize_log(profile_id: String, lines: Vec<String>) -> Result<String, UiError> {
    blocking(move || {
        let profile = load_profile(&profile_id)?;
        // As linhas da tela já vêm com o usuário XAuth mascarado: não precisa
        // abrir o chaveiro (o que poderia pedir senha só para copiar).
        let anonymizer = session::anonymizer_for(&profile, None);
        Ok(anonymizer.anonymize(&lines.join("\n")))
    })
    .await
}

/// Language of the messages built in Rust (errors, tray, notifications):
/// `"en"` or `"pt"`. The UI calls it at startup and when the user changes the
/// language.
#[tauri::command]
#[specta::specta]
pub fn set_language(lang: String) {
    use ipsec_client_core::i18n::{Lang, set_lang};
    set_lang(if lang == "pt" { Lang::Pt } else { Lang::En });
}
