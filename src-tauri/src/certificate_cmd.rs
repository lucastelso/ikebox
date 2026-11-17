//! Client certificate import (phase 9, ADR-0024): the person picks a `.p12`
//! in the native dialog (the path stays in Rust, behind a token) and types
//! its password; Rust opens it in memory, checks it and stores the private
//! key and the certificate in the keyring. The `.p12` password is not kept.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use ipsec_client_core::profiles::ProfileStore;
use ipsec_client_core::secrets::{ClientCertificate, Credentials, KeyringStore, MAX_PKCS12_BYTES};
use ipsec_client_core::{tr, trf};
use serde::Serialize;
use specta::Type;
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;
use zeroize::Zeroizing;

use crate::commands::blocking;
use crate::dto::{ClientCertSummary, UiError, UiErrorKind};

/// `.p12` files picked in the dialog, by token.
#[derive(Debug, Default)]
pub struct PendingP12 {
    next: AtomicU64,
    files: Mutex<HashMap<String, PathBuf>>,
}

impl PendingP12 {
    fn insert(&self, path: PathBuf) -> String {
        let token = format!("p12-{}", self.next.fetch_add(1, Ordering::Relaxed));
        if let Ok(mut files) = self.files.lock() {
            files.insert(token.clone(), path);
        }
        token
    }

    fn get(&self, token: &str) -> Option<PathBuf> {
        self.files.lock().ok().and_then(|f| f.get(token).cloned())
    }

    fn remove(&self, token: &str) {
        if let Ok(mut files) = self.files.lock() {
            files.remove(token);
        }
    }
}

/// A `.p12` picked in the dialog.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct P12Choice {
    /// Opaque token for [`import_client_certificate`].
    pub token: String,
    /// File name, without the directory.
    pub file_name: String,
}

fn profile_error(message: impl Into<String>) -> UiError {
    UiError::new(UiErrorKind::Profile, message)
}

/// Opens the native dialog to pick a `.p12`/`.pfx`. `None` when cancelled.
///
/// # Errors
/// The chosen file is not local.
#[tauri::command]
#[specta::specta]
pub async fn choose_p12_file(
    app: AppHandle,
    pending: tauri::State<'_, PendingP12>,
) -> Result<Option<P12Choice>, UiError> {
    let picked = blocking(move || {
        Ok(app
            .dialog()
            .file()
            .set_title(tr!("Your certificate", "O seu certificado"))
            .add_filter(
                tr!(
                    "Certificate with key (.p12, .pfx)",
                    "Certificado com chave (.p12, .pfx)"
                ),
                &["p12", "pfx"],
            )
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
    Ok(Some(P12Choice {
        token: pending.insert(path),
        file_name,
    }))
}

/// Opens the picked `.p12` with `password`, checks it and stores the key and
/// the certificate in the keyring for the profile. The token stays valid
/// after a wrong password, so the person can try again.
///
/// # Errors
/// Unknown token, no keyring, unreadable file, wrong password, or a
/// certificate the app cannot use (the message says why).
#[tauri::command]
#[specta::specta]
pub async fn import_client_certificate(
    pending: tauri::State<'_, PendingP12>,
    profile_id: String,
    token: String,
    password: String,
) -> Result<ClientCertSummary, UiError> {
    let password = Zeroizing::new(password);
    let path = pending.get(&token).ok_or_else(|| {
        profile_error(tr!(
            "Choose the certificate file again.",
            "Escolha o arquivo do certificado de novo."
        ))
    })?;
    let summary = blocking(move || {
        let profile = ProfileStore::new(ProfileStore::default_dir()?).load(&profile_id)?;
        if !profile.auth.uses_client_certificate() {
            return Err(profile_error(tr!(
                "This profile does not sign in with a certificate.",
                "Este perfil não entra com certificado."
            )));
        }
        // The key lives only in the keyring: without it there is nowhere safe
        // to keep it.
        let store = KeyringStore::new().map_err(|_| {
            profile_error(tr!(
                "The system keyring is not available: a client certificate needs it.",
                "O chaveiro do sistema não está disponível: o certificado do cliente precisa dele."
            ))
        })?;
        let size = std::fs::metadata(&path)
            .map(|m| m.len())
            .unwrap_or(u64::MAX);
        if size > MAX_PKCS12_BYTES as u64 {
            return Err(profile_error(tr!(
                "The file is too large for a certificate (.p12).",
                "O arquivo é grande demais para um certificado (.p12)."
            )));
        }
        let data = Zeroizing::new(std::fs::read(&path).map_err(|e| {
            UiError::new(
                UiErrorKind::Other,
                trf!(
                    "Could not read the file: {e}",
                    "Não foi possível ler o arquivo: {e}"
                ),
            )
        })?);
        let cert = ClientCertificate::from_pkcs12(&data, &password).map_err(profile_error)?;
        let mut creds = Credentials::load(&store, &profile)?;
        creds.set_client_certificate(&cert);
        creds.save(&store, &profile)?;
        Ok(ClientCertSummary::from(cert.info()))
    })
    .await?;
    pending.remove(&token);
    Ok(summary)
}
