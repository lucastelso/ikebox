//! Profile manager: import (JSON or swanctl.conf), export, view and delete.
//!
//! File paths stay **in Rust only**. The native dialog and drag and drop are
//! handled here, and the UI gets a preview with an opaque token
//! ([`ImportPreview`]).

use std::collections::HashMap;
use std::io::Write as _;
use std::net::IpAddr;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use ipsec_client_core::profiles::{
    DnsSettings, Profile, ProfileError, ProfileStore, SwanctlImport, SwanctlImportOptions,
    detect_legacy, import_legacy, import_swanctl, parse_profile, read_profile_file, to_json,
};
use ipsec_client_core::secrets::{Credentials, KeyringStore};
use ipsec_client_core::{tr, trf};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

use crate::dto::{
    ImportKind, ImportOptions, ImportPreview, ProfileDetails, ProfileSummary, UiError, UiErrorKind,
};

/// Files chosen for import, by token.
#[derive(Debug, Default)]
pub struct PendingImports {
    next: AtomicU64,
    files: Mutex<HashMap<String, PathBuf>>,
}

impl PendingImports {
    fn insert(&self, path: PathBuf) -> String {
        let token = format!("import-{}", self.next.fetch_add(1, Ordering::Relaxed));
        if let Ok(mut files) = self.files.lock() {
            files.insert(token.clone(), path);
        }
        token
    }

    fn take(&self, token: &str) -> Option<PathBuf> {
        self.files.lock().ok().and_then(|mut f| f.remove(token))
    }
}

fn other(msg: impl Into<String>) -> UiError {
    UiError::new(UiErrorKind::Other, msg)
}

async fn blocking<T, F>(f: F) -> Result<T, UiError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, UiError> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| other(trf!("Internal failure: {e}", "Falha interna: {e}")))?
}

fn store() -> Result<ProfileStore, UiError> {
    Ok(ProfileStore::new(ProfileStore::default_dir()?))
}

fn read_limited(path: &Path) -> Result<String, UiError> {
    read_profile_file(path).map_err(|e| match e {
        ProfileError::TooLarge => UiError::new(
            UiErrorKind::Profile,
            tr!(
                "File too large to be a profile.",
                "Arquivo grande demais para ser um perfil."
            ),
        ),
        ProfileError::Io(e) => other(trf!(
            "Could not read the file: {e}",
            "Não foi possível ler o arquivo: {e}"
        )),
        e => UiError::from(e),
    })
}

fn detect_kind(path: &Path, text: &str) -> ImportKind {
    let json_ext = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("json"));
    if json_ext || text.trim_start().starts_with('{') {
        return ImportKind::Json;
    }
    let name = path
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_default();
    detect_legacy(&name, text).map_or(ImportKind::Swanctl, ImportKind::from_legacy)
}

/// A swanctl.conf or another client's file, with the given options.
fn import_other(
    kind: ImportKind,
    text: &str,
    options: &SwanctlImportOptions,
) -> Result<SwanctlImport, ProfileError> {
    match kind.legacy() {
        Some(format) => import_legacy(text, format, options),
        None => import_swanctl(text, options),
    }
}

/// Builds the preview of a file (saving nothing).
pub fn preview_path(pending: &PendingImports, path: PathBuf) -> Result<ImportPreview, UiError> {
    let text = read_limited(&path)?;
    let kind = detect_kind(&path, &text);
    let file_name = path
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_default();
    let parsed: Result<(Profile, Vec<String>), ProfileError> = match kind {
        ImportKind::Json => parse_profile(&text).map(|p| (p, Vec::new())),
        other => import_other(other, &text, &SwanctlImportOptions::default())
            .map(|r| (r.profile, r.warnings)),
    };
    let token = pending.insert(path);
    Ok(match parsed {
        Ok((profile, warnings)) => ImportPreview {
            token,
            file_name,
            kind,
            exists: store()?.load(&profile.id).is_ok(),
            security: Some(profile.security().level.into()),
            suggested_id: profile.id,
            suggested_name: profile.name,
            gateway: Some(profile.gateway),
            remote_ts: profile.child.remote_ts,
            warnings,
            error: None,
        },
        Err(e) => ImportPreview {
            token,
            file_name,
            kind,
            suggested_id: String::new(),
            suggested_name: String::new(),
            gateway: None,
            remote_ts: Vec::new(),
            warnings: Vec::new(),
            error: Some(UiError::from(e).message),
            exists: false,
            security: None,
        },
    })
}

/// Opens the native dialog to choose a profile (.json) or a swanctl.conf.
#[tauri::command]
#[specta::specta]
pub async fn choose_import_file(app: AppHandle) -> Result<Option<ImportPreview>, UiError> {
    let dialog_app = app.clone();
    let picked = blocking(move || {
        Ok(dialog_app
            .dialog()
            .file()
            .set_title(tr!("Import profile", "Importar perfil"))
            .add_filter(
                tr!(
                    "IPsec Client profile (.json)",
                    "Perfil do IPsec Client (.json)"
                ),
                &["json"],
            )
            .add_filter("swanctl.conf", &["conf"])
            .add_filter(
                tr!(
                    "Other VPN clients (Cisco .pcf, vpnc, NetworkManager, ipsec.conf, FortiClient)",
                    "Outros clientes VPN (Cisco .pcf, vpnc, NetworkManager, ipsec.conf, FortiClient)"
                ),
                &["pcf", "conf", "nmconnection", "xml"],
            )
            .add_filter(tr!("All files", "Todos os arquivos"), &["*"])
            .blocking_pick_file())
    })
    .await?;
    let Some(file) = picked else { return Ok(None) };
    let path = file.into_path().map_err(|_| {
        other(tr!(
            "The chosen file is not local.",
            "O arquivo escolhido não é local."
        ))
    })?;
    blocking(move || {
        let pending = app.state::<PendingImports>();
        preview_path(&pending, path).map(Some)
    })
    .await
}

fn parse_dns(options: &ImportOptions) -> Result<DnsSettings, UiError> {
    let servers = options
        .dns_servers
        .iter()
        .filter(|s| !s.trim().is_empty())
        .map(|s| {
            s.trim().parse::<IpAddr>().map_err(|_| {
                UiError::new(
                    UiErrorKind::Profile,
                    trf!("Invalid DNS server: {s}", "Servidor DNS inválido: {s}"),
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(DnsSettings {
        servers,
        domains: options
            .dns_domains
            .iter()
            .map(|d| d.trim().to_owned())
            .filter(|d| !d.is_empty())
            .collect(),
        use_received: options.use_received_dns,
        default_route: false,
    })
}

fn non_empty(value: Option<&String>) -> Option<String> {
    value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())
}

/// Confirms the import of a preview.
#[tauri::command]
#[specta::specta]
pub async fn confirm_import(
    app: AppHandle,
    token: String,
    options: ImportOptions,
) -> Result<ProfileSummary, UiError> {
    blocking(move || {
        let pending = app.state::<PendingImports>();
        let path = pending.take(&token).ok_or_else(|| {
            UiError::new(
                UiErrorKind::Profile,
                tr!(
                    "The import expired; choose the file again.",
                    "A importação expirou; escolha o arquivo de novo."
                ),
            )
        })?;
        let text = read_limited(&path)?;
        let mut profile = match detect_kind(&path, &text) {
            ImportKind::Json => parse_profile(&text)?,
            other => {
                import_other(
                    other,
                    &text,
                    &SwanctlImportOptions {
                        dns: parse_dns(&options)?,
                        ..Default::default()
                    },
                )?
                .profile
            }
        };
        if let Some(id) = non_empty(options.id.as_ref()) {
            profile.id = id;
        }
        if let Some(name) = non_empty(options.name.as_ref()) {
            profile.name = name;
        }
        let store = store()?;
        if !options.overwrite && store.load(&profile.id).is_ok() {
            return Err(UiError::new(
                UiErrorKind::Profile,
                trf!(
                    "A profile with id \"{}\" already exists. Choose another id or replace it.",
                    "Já existe um perfil com o id \"{}\". Escolha outro id ou substitua.",
                    profile.id
                ),
            ));
        }
        store.save(&profile)?;
        Ok(ProfileSummary::from(&profile))
    })
    .await
}

/// Details of a profile.
#[tauri::command]
#[specta::specta]
pub async fn profile_details(profile_id: String) -> Result<ProfileDetails, UiError> {
    blocking(move || Ok(ProfileDetails::from(&store()?.load(&profile_id)?))).await
}

/// Exports the profile (without credentials) to a file chosen in the native
/// dialog. Returns the name of the saved file, or `None` if cancelled.
#[tauri::command]
#[specta::specta]
pub async fn export_profile(app: AppHandle, profile_id: String) -> Result<Option<String>, UiError> {
    let profile = blocking({
        let id = profile_id.clone();
        move || Ok(store()?.load(&id)?)
    })
    .await?;
    let json = to_json(&profile)?;
    let target = blocking(move || {
        Ok(app
            .dialog()
            .file()
            .set_title(tr!(
                "Export profile (without credentials)",
                "Exportar perfil (sem credenciais)"
            ))
            .set_file_name(format!("{profile_id}.json"))
            .add_filter("Perfil do IPsec Client (.json)", &["json"])
            .blocking_save_file())
    })
    .await?;
    let Some(file) = target else { return Ok(None) };
    let path = file.into_path().map_err(|_| {
        other(tr!(
            "The chosen destination is not local.",
            "O destino escolhido não é local."
        ))
    })?;
    blocking(move || {
        let mut out = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)
            .map_err(|e| {
                other(trf!(
                    "Could not write the file: {e}",
                    "Não foi possível gravar o arquivo: {e}"
                ))
            })?;
        out.write_all(json.as_bytes()).map_err(|e| {
            other(trf!(
                "Could not write the file: {e}",
                "Não foi possível gravar o arquivo: {e}"
            ))
        })?;
        Ok(path.file_name().map(|f| f.to_string_lossy().into_owned()))
    })
    .await
}

/// Deletes the profile and, if asked, its stored credentials.
#[tauri::command]
#[specta::specta]
pub async fn delete_profile(profile_id: String, forget_credentials: bool) -> Result<(), UiError> {
    blocking(move || {
        let store = store()?;
        let profile = store.load(&profile_id)?;
        if forget_credentials && let Ok(keyring) = KeyringStore::new() {
            Credentials::forget(&keyring, &profile)?;
        }
        store.delete(&profile_id)?;
        Ok(())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_kind_by_extension_and_content() {
        assert_eq!(detect_kind(Path::new("a.json"), ""), ImportKind::Json);
        assert_eq!(
            detect_kind(Path::new("perfil"), "  {\"x\":1}"),
            ImportKind::Json
        );
        assert_eq!(
            detect_kind(Path::new("vpn.conf"), "connections {"),
            ImportKind::Swanctl
        );
    }

    #[test]
    fn pending_tokens_are_single_use() {
        let pending = PendingImports::default();
        let token = pending.insert(PathBuf::from("/tmp/x.json"));
        assert!(pending.take(&token).is_some());
        assert!(pending.take(&token).is_none());
        assert!(pending.take("inexistente").is_none());
    }

    #[test]
    fn dns_options_are_validated() {
        let ok = parse_dns(&ImportOptions {
            dns_servers: vec![" 192.0.2.53 ".into(), String::new()],
            dns_domains: vec!["~corp.example.com".into(), " ".into()],
            use_received_dns: true,
            ..Default::default()
        })
        .expect("dns");
        assert_eq!(ok.servers.len(), 1);
        assert_eq!(ok.domains, ["~corp.example.com"]);
        assert!(ok.use_received);
        assert!(
            parse_dns(&ImportOptions {
                dns_servers: vec!["não-é-ip".into()],
                ..Default::default()
            })
            .is_err()
        );
    }
}
