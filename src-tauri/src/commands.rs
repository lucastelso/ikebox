//! Comandos expostos à UI. Finos: delegam ao `ipsec-client-core`.

use crate::dto::AppInfo;

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
