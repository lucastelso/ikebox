//! Componente de rede: estado, instalação e remoção pela UI (ADR-0018).
//!
//! O pacote do componente vem entre os recursos do app
//! (`<recursos>/network`, só nos builds de release:
//! `tauri.release.conf.json`). Em builds de desenvolvimento, dá para apontar
//! um pacote com `IPSEC_CLIENT_COMPONENT_BUNDLE`.

use std::path::PathBuf;

use ipsec_client_core::component::{self, BUNDLE_RESOURCE, Bundle, Origin};
use ipsec_client_core::session;
use ipsec_client_core::{tr, trf};
use tauri::{AppHandle, Manager};

use crate::commands::{ConnectionLock, blocking};
use crate::dto::{ComponentInfo, UiError, UiErrorKind};

/// Diretório do pacote que vem com o app, se houver.
fn bundle_dir(app: &AppHandle) -> Option<PathBuf> {
    #[cfg(debug_assertions)]
    if let Some(dir) = std::env::var_os("IPSEC_CLIENT_COMPONENT_BUNDLE") {
        return Some(PathBuf::from(dir));
    }
    app.path()
        .resource_dir()
        .ok()
        .map(|dir| dir.join(BUNDLE_RESOURCE))
        .filter(|dir| dir.is_dir())
}

fn bundle(app: &AppHandle) -> Option<Bundle> {
    let dir = bundle_dir(app)?;
    Bundle::open(&dir)
        .inspect_err(|e| tracing::warn!("component bundle ignored: {e}"))
        .ok()
}

fn current(bundle: Option<&Bundle>) -> ComponentInfo {
    ComponentInfo::new(
        &component::status(bundle),
        bundle,
        component::pkexec_available(),
    )
}

/// Instalar ou remover derruba o engine: só sem conexão ativa.
async fn ensure_disconnected() -> Result<(), UiError> {
    if let Ok(Some(profile)) = session::active_profile().await {
        return Err(UiError::new(
            UiErrorKind::Busy,
            trf!(
                "Disconnect the VPN (profile {profile}) before changing the network component.",
                "Desconecte a VPN (perfil {profile}) antes de mexer no componente de rede."
            ),
        ));
    }
    Ok(())
}

/// Estado do componente de rede.
#[tauri::command]
#[specta::specta]
pub async fn component_status(app: AppHandle) -> Result<ComponentInfo, UiError> {
    blocking(move || Ok(current(bundle(&app).as_ref()))).await
}

/// Instala ou atualiza o componente de rede (o sistema pede a senha de
/// administrador). Devolve o novo estado.
#[tauri::command]
#[specta::specta]
pub async fn install_component(
    app: AppHandle,
    lock: tauri::State<'_, ConnectionLock>,
) -> Result<ComponentInfo, UiError> {
    let Ok(_guard) = lock.0.try_lock() else {
        return Err(UiError::new(
            UiErrorKind::Busy,
            tr!(
                "Wait for the current operation to finish.",
                "Aguarde a operação em andamento terminar."
            ),
        ));
    };
    ensure_disconnected().await?;
    blocking(move || {
        let bundle = bundle(&app).ok_or_else(|| {
            UiError::new(
                UiErrorKind::Component,
                tr!(
                    "This build of the app does not include the network component (development build).",
                    "Esta versão do app não traz o componente de rede (build de desenvolvimento)."
                ),
            )
        })?;
        let log = component::install(&bundle, Origin::App)?;
        tracing::info!("network component installed: {}", log.join(" | "));
        Ok(current(Some(&bundle)))
    })
    .await
}

/// Remove o componente de rede (o sistema pede a senha de administrador).
#[tauri::command]
#[specta::specta]
pub async fn uninstall_component(
    app: AppHandle,
    lock: tauri::State<'_, ConnectionLock>,
) -> Result<ComponentInfo, UiError> {
    let Ok(_guard) = lock.0.try_lock() else {
        return Err(UiError::new(
            UiErrorKind::Busy,
            tr!(
                "Wait for the current operation to finish.",
                "Aguarde a operação em andamento terminar."
            ),
        ));
    };
    ensure_disconnected().await?;
    blocking(move || {
        let log = component::uninstall()?;
        tracing::info!("network component removed: {}", log.join(" | "));
        Ok(current(bundle(&app).as_ref()))
    })
    .await
}
