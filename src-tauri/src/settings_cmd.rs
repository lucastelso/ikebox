//! Commands of the app settings: closing to the tray and the optional
//! update notice (phase F of the program to 1.0.0).

use std::sync::atomic::{AtomicBool, Ordering};

use ipsec_client_core::tr;
use tauri::{AppHandle, Manager};
use tauri_specta::Event as _;

use crate::dto::{AppSettingsView, UiError, UiErrorKind, UpdateFound};
use crate::settings::{AppSettings, SettingsState};
use crate::update;

/// Only one update check at a time.
static CHECKING: AtomicBool = AtomicBool::new(false);

fn view(settings: &AppSettings) -> AppSettingsView {
    AppSettingsView {
        close_to_tray: settings.close_to_tray,
        update_check: settings.update_check && update::available(),
        update_check_available: update::available(),
        available_update: settings
            .latest_version
            .clone()
            .filter(|v| settings.update_check && update::is_newer(v)),
    }
}

/// The current settings.
#[tauri::command]
#[specta::specta]
pub fn app_settings(state: tauri::State<'_, SettingsState>) -> AppSettingsView {
    view(&state.get())
}

/// Closing the window keeps the app in the tray (`true`) or quits it.
#[tauri::command]
#[specta::specta]
pub fn set_close_to_tray(state: tauri::State<'_, SettingsState>, enabled: bool) -> AppSettingsView {
    view(&state.update(|s| s.close_to_tray = enabled))
}

/// Turns the update check on (checking right away) or off.
#[tauri::command]
#[specta::specta]
pub fn set_update_check(
    app: AppHandle,
    state: tauri::State<'_, SettingsState>,
    enabled: bool,
) -> AppSettingsView {
    let settings = state.update(|s| {
        s.update_check = enabled && update::available();
        if !s.update_check {
            s.latest_version = None;
            s.last_update_check = 0;
        }
    });
    if settings.update_check {
        check_now(&app);
    }
    view(&settings)
}

/// Opens the release page of the newer version found by the check, in the
/// browser. The address is built here from the validated version number.
///
/// # Errors
/// No newer version known, or no browser could be started.
#[tauri::command]
#[specta::specta]
pub fn open_release_page(state: tauri::State<'_, SettingsState>) -> Result<(), UiError> {
    let settings = state.get();
    let url = settings
        .latest_version
        .filter(|v| update::is_newer(v))
        .and_then(|v| update::release_page(&v))
        .ok_or_else(|| {
            UiError::new(
                UiErrorKind::Other,
                tr!(
                    "No newer version is known.",
                    "Nenhuma versão nova conhecida."
                ),
            )
        })?;
    open_in_browser(&url)
}

/// `xdg-open`, without the AppImage's library paths in its environment (the
/// browser it starts must use the system's libraries).
fn open_in_browser(url: &str) -> Result<(), UiError> {
    std::process::Command::new("xdg-open")
        .arg(url)
        .env_remove("LD_LIBRARY_PATH")
        .env_remove("APPDIR")
        .env_remove("APPIMAGE")
        .env_remove("GIO_MODULE_DIR")
        .env_remove("GDK_PIXBUF_MODULE_FILE")
        .spawn()
        .map(|_| ())
        .map_err(|e| {
            UiError::new(
                UiErrorKind::Other,
                format!(
                    "{} ({e}): {url}",
                    tr!(
                        "Could not open the browser",
                        "Não foi possível abrir o navegador"
                    )
                ),
            )
        })
}

/// Runs the check when it is on and due (once a day; an hour after a
/// failure). Called by the status monitor.
pub fn check_if_due(app: &AppHandle) {
    let settings = app.state::<SettingsState>().get();
    if settings.update_check
        && update::now().saturating_sub(settings.last_update_check) >= update::INTERVAL.as_secs()
    {
        check_now(app);
    }
}

fn check_now(app: &AppHandle) {
    if !update::available() || CHECKING.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let result = update::latest_version();
        let state = app.state::<SettingsState>();
        let settings = state.update(|s| match &result {
            Ok(version) => {
                s.latest_version = Some(version.clone());
                s.last_update_check = update::now();
            }
            // Try again in an hour.
            Err(_) => {
                s.last_update_check = update::now()
                    .saturating_sub(update::INTERVAL.as_secs())
                    .saturating_add(3600);
            }
        });
        match result {
            Ok(version) if settings.update_check && update::is_newer(&version) => {
                let _ = UpdateFound { version }.emit(&app);
            }
            Ok(_) => {}
            Err(e) => tracing::info!("update check failed: {e}"),
        }
        CHECKING.store(false, Ordering::SeqCst);
    });
}
