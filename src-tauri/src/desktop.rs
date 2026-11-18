//! Integração com o desktop: bandeja, notificações e instância única.
//!
//! Tudo sem zbus (barrado pelo cargo-deny, ver passo 11 do diário):
//! - notificações: `org.freedesktop.Notifications` via libdbus (crate `dbus`);
//! - instância única: socket Unix em `$XDG_RUNTIME_DIR/ipsec-client/app.sock`;
//! - bandeja: `tray-icon` do Tauri (libappindicator). Num GNOME sem a extensão
//!   AppIndicator, ela não aparece, e nada quebra.

use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ipsec_client_core::session::{self, ConnectionInfo};
use ipsec_client_core::{tr, trf};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};
use tauri_specta::Event as _;

use crate::dto::{Connection, StatusUpdate};
use crate::settings::SettingsState;

/// The one status monitor of the app (phase F): it updates the tray, sends
/// notifications and feeds the window, which no longer polls on its own.
const MONITOR_INTERVAL: Duration = Duration::from_secs(2);

/// Was the tray created (an AppIndicator host may be missing, on GNOME)?
static TRAY_AVAILABLE: AtomicBool = AtomicBool::new(false);

/// Closing the main window: with "close to the tray" on, the window is only
/// hidden and the app keeps running (the VPN, the tray, a token's key agent);
/// it comes back from the tray or by opening the app again. The first time,
/// a notification says so.
pub fn handle_close(window: &tauri::Window, event: &tauri::WindowEvent) {
    let tauri::WindowEvent::CloseRequested { api, .. } = event else {
        return;
    };
    let state = window.app_handle().state::<SettingsState>();
    if !state.get().close_to_tray {
        return;
    }
    api.prevent_close();
    let _ = window.hide();
    if !state.get().close_notice_shown {
        state.update(|s| s.close_notice_shown = true);
        let body = if TRAY_AVAILABLE.load(Ordering::Relaxed) {
            tr!(
                "It keeps running in the tray. To quit, use \"Quit\" in the tray menu; you can change this in System.",
                "Ele continua na bandeja. Para sair, use \"Sair do app\" no menu da bandeja; dá para mudar isso em Sistema."
            )
        } else {
            tr!(
                "It keeps running in the background: open it again from the applications menu. You can change this in System.",
                "Ele continua rodando em segundo plano: abra de novo pelo menu de aplicativos. Dá para mudar isso em Sistema."
            )
        };
        notify(
            tr!(
                "IPsec Client is still open",
                "O IPsec Client continua aberto"
            ),
            body,
        );
    }
}

/// Estado da VPN visto pela bandeja.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayState {
    /// Ainda não consultado, ou Docker/engine inacessível.
    Unknown,
    /// Sem conexão.
    Disconnected,
    /// Túnel ativo.
    Connected {
        /// Perfil.
        profile: String,
        /// IP virtual.
        vip: Option<String>,
    },
    /// Engine de pé, sem túnel (reconectando).
    Reconnecting {
        /// Perfil.
        profile: String,
    },
}

impl TrayState {
    /// Converte o resultado de `session::status`.
    #[must_use]
    pub fn from_status(status: Option<&ConnectionInfo>) -> Self {
        match status {
            None => Self::Disconnected,
            Some(info) => {
                let profile = info.profile_id.clone().unwrap_or_else(|| "?".to_owned());
                if info.is_established() {
                    Self::Connected {
                        profile,
                        vip: info.vip.clone(),
                    }
                } else {
                    Self::Reconnecting { profile }
                }
            }
        }
    }

    /// Texto curto (dica da bandeja e item de status do menu).
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Unknown => "IPsec Client".to_owned(),
            Self::Disconnected => tr!("Disconnected", "Desconectado").to_owned(),
            Self::Connected { profile, vip } => match vip {
                Some(vip) => trf!(
                    "Connected: {profile} ({vip})",
                    "Conectado: {profile} ({vip})"
                ),
                None => trf!("Connected: {profile}", "Conectado: {profile}"),
            },
            Self::Reconnecting { profile } => {
                trf!("Reconnecting: {profile}…", "Reconectando: {profile}…")
            }
        }
    }

    fn is_active(&self) -> bool {
        matches!(self, Self::Connected { .. } | Self::Reconnecting { .. })
    }
}

/// Notificação a mostrar numa transição de estado (`None` = nada a avisar).
#[must_use]
pub fn transition_notice(previous: &TrayState, next: &TrayState) -> Option<(String, String)> {
    use TrayState as S;
    match (previous, next) {
        // Primeira leitura: só sincroniza, sem avisar.
        (S::Unknown, _) | (_, S::Unknown) => None,
        (a, b) if a == b => None,
        (S::Disconnected, S::Connected { profile, vip }) => Some((
            tr!("VPN connected", "VPN conectada").to_owned(),
            match vip {
                Some(vip) => format!("{profile} · IP {vip}"),
                None => profile.clone(),
            },
        )),
        (S::Connected { .. }, S::Reconnecting { profile }) => Some((
            tr!("Connection dropped", "Conexão caiu").to_owned(),
            trf!(
                "{profile}: trying to reconnect…",
                "{profile}: tentando reconectar…"
            ),
        )),
        (S::Reconnecting { .. }, S::Connected { profile, .. }) => Some((
            tr!("VPN reconnected", "VPN reconectada").to_owned(),
            profile.clone(),
        )),
        (S::Connected { .. } | S::Reconnecting { .. }, S::Disconnected) => Some((
            tr!("VPN disconnected", "VPN desconectada").to_owned(),
            tr!("The connection was closed.", "A conexão foi encerrada.").to_owned(),
        )),
        _ => None,
    }
}

/// Mostra uma notificação do sistema (best effort: sem servidor de
/// notificações, não faz nada).
pub fn notify(summary: &str, body: &str) {
    let (summary, body) = (summary.to_owned(), body.to_owned());
    tauri::async_runtime::spawn_blocking(move || {
        let result = (|| -> Result<(), dbus::Error> {
            let conn = dbus::blocking::Connection::new_session()?;
            let proxy = conn.with_proxy(
                "org.freedesktop.Notifications",
                "/org/freedesktop/Notifications",
                Duration::from_secs(2),
            );
            let hints = dbus::arg::PropMap::new();
            let actions: Vec<&str> = Vec::new();
            let _: (u32,) = proxy.method_call(
                "org.freedesktop.Notifications",
                "Notify",
                (
                    "IPsec Client",
                    0u32,
                    "network-vpn",
                    summary.as_str(),
                    body.as_str(),
                    actions,
                    hints,
                    5000i32,
                ),
            )?;
            Ok(())
        })();
        if let Err(e) = result {
            tracing::debug!("notification not sent: {e}");
        }
    });
}

fn instance_socket() -> Option<PathBuf> {
    let dir = ipsec_client_core::engine::default_runtime_dir().ok()?;
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("app.sock"))
}

/// Garante uma única janela do app. Devolve `false` se já havia outra
/// instância (que foi trazida para a frente); aí esta deve encerrar.
pub fn acquire_single_instance(app: &AppHandle) -> bool {
    let Some(path) = instance_socket() else {
        return true;
    };
    if let Ok(mut stream) = UnixStream::connect(&path) {
        let _ = stream.write_all(b"show\n");
        return false;
    }
    let _ = std::fs::remove_file(&path);
    let Ok(listener) = UnixListener::bind(&path) else {
        return true;
    };
    let app = app.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut line = String::new();
            if BufReader::new(stream).read_line(&mut line).is_ok() && line.trim() == "show" {
                show_main_window(&app);
            }
        }
    });
    true
}

/// Mostra e foca a janela principal.
pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

struct TrayHandles {
    tray: tauri::tray::TrayIcon,
    status: MenuItem<tauri::Wry>,
    show: MenuItem<tauri::Wry>,
    disconnect: MenuItem<tauri::Wry>,
    quit: MenuItem<tauri::Wry>,
}

impl TrayHandles {
    /// Fixed menu texts in the current language (they change when the user
    /// picks another language in the app).
    fn set_menu_texts(&self) {
        let _ = self
            .show
            .set_text(tr!("Open IPsec Client", "Abrir o IPsec Client"));
        let _ = self.disconnect.set_text(tr!("Disconnect", "Desconectar"));
        let _ = self.quit.set_text(tr!("Quit", "Sair do app"));
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<TrayHandles> {
    let status = MenuItem::with_id(app, "status", "IPsec Client", false, None::<&str>)?;
    let show = MenuItem::with_id(app, "show", "", true, None::<&str>)?;
    let disconnect = MenuItem::with_id(app, "disconnect", "", false, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &status,
            &PredefinedMenuItem::separator(app)?,
            &show,
            &disconnect,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;
    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("IPsec Client")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
            "disconnect" => {
                tauri::async_runtime::spawn(async {
                    let _ = session::disconnect().await;
                });
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    let tray = builder.build(app)?;
    let handles = TrayHandles {
        tray,
        status,
        show,
        disconnect,
        quit,
    };
    handles.set_menu_texts();
    Ok(handles)
}

/// Cria a bandeja (se o ambiente suportar) e inicia o monitor de estado, que
/// atualiza a bandeja e notifica as transições (inclusive de conexões abertas
/// pelo CLI).
pub fn start(app: &AppHandle) {
    // A libappindicator é carregada em tempo de execução; se faltar, segue sem
    // bandeja (as notificações continuam).
    let tray = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| build_tray(app)))
        .ok()
        .and_then(Result::ok);
    if tray.is_none() {
        tracing::info!("tray not available in this environment");
    }
    TRAY_AVAILABLE.store(tray.is_some(), Ordering::Relaxed);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut previous = TrayState::Unknown;
        let mut lang = ipsec_client_core::i18n::lang();
        loop {
            let current_lang = ipsec_client_core::i18n::lang();
            let lang_changed = current_lang != lang;
            lang = current_lang;
            let status = session::status().await;
            let next = match &status {
                Ok(status) => TrayState::from_status(status.as_ref()),
                Err(_) => TrayState::Unknown,
            };
            // The window gets the same reading, only while it is visible.
            if let Ok(status) = &status
                && app
                    .get_webview_window("main")
                    .is_some_and(|w| w.is_visible().unwrap_or(false))
            {
                let _ = StatusUpdate {
                    connection: status.as_ref().map(Connection::from),
                }
                .emit(&app);
            }
            crate::settings_cmd::check_if_due(&app);
            if let Some((summary, body)) = transition_notice(&previous, &next) {
                notify(&summary, &body);
            }
            // The VPN went down without this app (another process, or the
            // gateway): a token's key agent has nothing left to sign for.
            if previous.is_active() && next == TrayState::Disconnected {
                session::stop_token_agent();
            }
            if let Some(handles) = &tray
                && lang_changed
            {
                handles.set_menu_texts();
            }
            if (next != previous || lang_changed)
                && let Some(handles) = &tray
            {
                let label = next.label();
                let _ = handles.tray.set_tooltip(Some(&label));
                let _ = handles.status.set_text(&label);
                let _ = handles.disconnect.set_enabled(next.is_active());
            }
            if next != TrayState::Unknown {
                previous = next;
            }
            tokio::time::sleep(MONITOR_INTERVAL).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connected() -> TrayState {
        TrayState::Connected {
            profile: "trabalho".into(),
            vip: Some("198.51.100.10".into()),
        }
    }

    #[test]
    fn notices_follow_transitions() {
        let down = TrayState::Disconnected;
        let re = TrayState::Reconnecting {
            profile: "trabalho".into(),
        };
        assert_eq!(
            transition_notice(&down, &connected()),
            Some(("VPN connected".into(), "trabalho · IP 198.51.100.10".into()))
        );
        assert_eq!(
            transition_notice(&connected(), &re).map(|n| n.0),
            Some("Connection dropped".into())
        );
        assert_eq!(
            transition_notice(&re, &connected()).map(|n| n.0),
            Some("VPN reconnected".into())
        );
        assert_eq!(
            transition_notice(&connected(), &down).map(|n| n.0),
            Some("VPN disconnected".into())
        );
    }

    #[test]
    fn no_notice_on_first_sync_or_without_change() {
        assert_eq!(transition_notice(&TrayState::Unknown, &connected()), None);
        assert_eq!(transition_notice(&connected(), &TrayState::Unknown), None);
        assert_eq!(transition_notice(&connected(), &connected()), None);
        assert_eq!(
            transition_notice(&TrayState::Disconnected, &TrayState::Disconnected),
            None
        );
    }

    #[test]
    fn labels_and_conversion() {
        assert_eq!(TrayState::from_status(None), TrayState::Disconnected);
        let info = ConnectionInfo {
            profile_id: Some("vpn".into()),
            state: "CONNECTING".into(),
            ..Default::default()
        };
        assert_eq!(
            TrayState::from_status(Some(&info)),
            TrayState::Reconnecting {
                profile: "vpn".into()
            }
        );
        assert_eq!(connected().label(), "Connected: trabalho (198.51.100.10)");
        assert!(connected().is_active());
        assert!(!TrayState::Disconnected.is_active());
    }
}
