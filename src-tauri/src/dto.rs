//! Tipos trocados com a UI (o contrato do IPC).
//!
//! São separados dos tipos do `core` de propósito: só o que a UI precisa ver
//! atravessa para o webview, e nunca segredos.

use serde::Serialize;
use specta::Type;

/// Informações do app (tela "Sobre", diagnóstico).
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    /// Versão do app.
    pub version: String,
    /// Imagem do engine usada nesta versão.
    pub engine_image: String,
}
