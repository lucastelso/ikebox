//! Certificates on smartcards and tokens (ADR-0029): list what the connected
//! tokens offer and remember the person's choice for a profile. The module
//! path, when the person picks one, stays in Rust behind a token, as the
//! `.p12` files do (the webview never sees paths). The PIN is not asked
//! here: it goes with the connection and is never stored.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use ipsec_client_core::profiles::{CredentialKind, ProfileStore};
use ipsec_client_core::secrets::{
    Credentials, KeyringStore, TokenCertificate, TokenError, default_module, list_certificates,
};
use ipsec_client_core::tr;
use secrecy::SecretString;
use serde::Serialize;
use specta::Type;
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

use crate::commands::blocking;
use crate::dto::{ClientCertSummary, UiError, UiErrorKind};

/// PKCS#11 module files picked in the dialog, by token.
#[derive(Debug, Default)]
pub struct PendingModules {
    next: AtomicU64,
    files: Mutex<HashMap<String, PathBuf>>,
}

impl PendingModules {
    fn insert(&self, path: PathBuf) -> String {
        let token = format!("module-{}", self.next.fetch_add(1, Ordering::Relaxed));
        if let Ok(mut files) = self.files.lock() {
            files.insert(token.clone(), path);
        }
        token
    }

    fn get(&self, token: &str) -> Option<PathBuf> {
        self.files.lock().ok().and_then(|f| f.get(token).cloned())
    }
}

/// A module picked in the dialog.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ModuleChoice {
    /// Opaque token for the other commands.
    pub token: String,
    /// File name, without the directory.
    pub file_name: String,
}

/// A certificate a token offers.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TokenCertOption {
    /// Key id (`CKA_ID`, hexadecimal): what the person chooses.
    pub key_id: String,
    /// Label of the token it is on.
    pub token_label: String,
    /// The certificate, when it can be used.
    pub certificate: Option<ClientCertSummary>,
    /// Why it cannot be used.
    pub problem: Option<String>,
}

/// What the tokens of one module offer.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TokenCertList {
    /// File name of the module used.
    pub module_name: String,
    /// Certificates found (empty: no token, or none with a certificate).
    pub certificates: Vec<TokenCertOption>,
}

fn token_error(e: &TokenError) -> UiError {
    UiError::new(UiErrorKind::Profile, e.to_string())
}

fn module_path(pending: &PendingModules, token: Option<&str>) -> Result<PathBuf, UiError> {
    match token {
        Some(token) => pending.get(token).ok_or_else(|| {
            UiError::new(
                UiErrorKind::Profile,
                tr!(
                    "Choose the module file again.",
                    "Escolha o arquivo do módulo de novo."
                ),
            )
        }),
        None => default_module().ok_or_else(|| token_error(&TokenError::NoModule)),
    }
}

fn list(module: &std::path::Path) -> Result<Vec<TokenCertificate>, UiError> {
    list_certificates(module).map_err(|e| token_error(&e))
}

/// Opens the native dialog to pick a PKCS#11 module (`.so`). `None` when
/// cancelled.
///
/// # Errors
/// The chosen file is not local.
#[tauri::command]
#[specta::specta]
pub async fn choose_token_module(
    app: AppHandle,
    pending: tauri::State<'_, PendingModules>,
) -> Result<Option<ModuleChoice>, UiError> {
    let picked = blocking(move || {
        Ok(app
            .dialog()
            .file()
            .set_title(tr!(
                "Smartcard or token module",
                "Módulo do smartcard ou token"
            ))
            .add_filter(tr!("PKCS#11 module (.so)", "Módulo PKCS#11 (.so)"), &["so"])
            .add_filter(tr!("All files", "Todos os arquivos"), &["*"])
            .blocking_pick_file())
    })
    .await?;
    let Some(file) = picked else { return Ok(None) };
    let path = file.into_path().map_err(|_| {
        UiError::new(
            UiErrorKind::Other,
            tr!(
                "The chosen file is not local.",
                "O arquivo escolhido não é local."
            ),
        )
    })?;
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(Some(ModuleChoice {
        token: pending.insert(path),
        file_name,
    }))
}

/// The certificates on the connected tokens, through the chosen module or
/// the system's (p11-kit's proxy, or OpenSC). No PIN is needed.
///
/// # Errors
/// No module, or the module or the token fails.
#[tauri::command]
#[specta::specta]
pub async fn list_token_certificates(
    pending: tauri::State<'_, PendingModules>,
    module_token: Option<String>,
) -> Result<TokenCertList, UiError> {
    let module = module_path(&pending, module_token.as_deref())?;
    blocking(move || {
        let module_name = module
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let certificates = list(&module)?
            .into_iter()
            .map(|c| TokenCertOption {
                key_id: c.reference.id,
                token_label: c.token_label,
                certificate: c.info.as_ref().ok().map(ClientCertSummary::from),
                problem: c.info.err(),
            })
            .collect();
        Ok(TokenCertList {
            module_name,
            certificates,
        })
    })
    .await
}

/// Remembers, for the profile, the certificate with `key_id` on the tokens of
/// the chosen (or the system's) module: the certificate and where its key
/// is, in the keyring. Never the PIN.
///
/// # Errors
/// No keyring, unknown profile or one without a token certificate, or the
/// certificate is gone or unusable.
#[tauri::command]
#[specta::specta]
pub async fn remember_token_certificate(
    pending: tauri::State<'_, PendingModules>,
    profile_id: String,
    module_token: Option<String>,
    key_id: String,
) -> Result<ClientCertSummary, UiError> {
    let module = module_path(&pending, module_token.as_deref())?;
    blocking(move || {
        let profile = ProfileStore::new(ProfileStore::default_dir()?).load(&profile_id)?;
        if !profile.auth.uses_token() {
            return Err(UiError::new(
                UiErrorKind::Profile,
                tr!(
                    "This profile does not use a smartcard or token.",
                    "Este perfil não usa smartcard ou token."
                ),
            ));
        }
        let store = KeyringStore::new().map_err(|_| {
            UiError::new(
                UiErrorKind::Keyring,
                tr!(
                    "The system keyring is not available: it keeps your choice of certificate.",
                    "O chaveiro do sistema não está disponível: ele guarda a sua escolha de certificado."
                ),
            )
        })?;
        let chosen = list(&module)?
            .into_iter()
            .find(|c| c.reference.id.eq_ignore_ascii_case(&key_id))
            .ok_or_else(|| token_error(&TokenError::NotPresent))?;
        let info = chosen
            .info
            .as_ref()
            .map_err(|why| UiError::new(UiErrorKind::Profile, why.clone()))?;
        let summary = ClientCertSummary::from(info);
        let reference = chosen.reference.to_json().map_err(|e| token_error(&e))?;
        let mut creds = Credentials::load(&store, &profile)?;
        creds.set(CredentialKind::ClientCert, SecretString::from(chosen.cert_pem));
        creds.set(CredentialKind::TokenKey, SecretString::from(reference));
        creds.save(&store, &profile)?;
        Ok(summary)
    })
    .await
}
