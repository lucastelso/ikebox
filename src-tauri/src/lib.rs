//! App desktop do IPsec Client (Tauri v2).
//!
//! Esta camada é fina (ADR-0009): os comandos convertem entre os DTOs da UI
//! ([`dto`]) e o `ipsec-client-core`, onde fica toda a lógica. O webview só
//! enxerga os comandos registrados aqui; não há plugins de shell/fs.

pub mod commands;
pub mod component_cmd;
pub mod desktop;
pub mod dto;
pub mod profiles_cmd;

use tauri_specta::{Builder, ErrorHandlingMode, collect_commands, collect_events};

/// Registro dos comandos e eventos expostos à UI (fonte dos bindings
/// TypeScript). Erros chegam tipados (`{ status: "error", error: UiError }`).
#[must_use]
pub fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .error_handling(ErrorHandlingMode::Result)
        .disable_serde_phases()
        .commands(collect_commands![
            commands::app_info,
            commands::run_diagnostics,
            commands::list_profiles,
            commands::credentials_status,
            commands::connect,
            commands::disconnect,
            commands::status,
            commands::forget_credentials,
            profiles_cmd::choose_import_file,
            profiles_cmd::confirm_import,
            profiles_cmd::profile_details,
            profiles_cmd::export_profile,
            profiles_cmd::delete_profile,
            component_cmd::component_status,
            component_cmd::install_component,
            component_cmd::uninstall_component,
        ])
        .events(collect_events![dto::SessionUpdate, dto::ImportPreview])
}

/// Sobe o app.
///
/// # Panics
/// Se o Tauri não conseguir iniciar (ex.: sem display gráfico).
pub fn run() {
    let builder = specta_builder();
    #[allow(clippy::expect_used)] // falha na inicialização: não há como seguir
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(commands::ConnectionLock::default())
        .manage(profiles_cmd::PendingImports::default())
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            if !desktop::acquire_single_instance(app.handle()) {
                // Já havia uma janela aberta: ela foi trazida para a frente.
                app.handle().exit(0);
                return Ok(());
            }
            builder.mount_events(app);
            desktop::start(app.handle());
            Ok(())
        })
        .on_window_event(handle_drop)
        .run(tauri::generate_context!())
        .expect("falha ao iniciar o IPsec Client");
}

/// Arquivo solto na janela: o Rust lê, monta a prévia e avisa a UI (o webview
/// não recebe o caminho).
fn handle_drop(window: &tauri::Window, event: &tauri::WindowEvent) {
    use tauri::{DragDropEvent, Manager, WindowEvent};
    use tauri_specta::Event as _;
    let WindowEvent::DragDrop(DragDropEvent::Drop { paths, .. }) = event else {
        return;
    };
    let Some(path) = paths.first().cloned() else {
        return;
    };
    let app = window.app_handle().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let pending = app.state::<profiles_cmd::PendingImports>();
        if let Ok(preview) = profiles_cmd::preview_path(&pending, path) {
            let _ = preview.emit(&app);
        }
    });
}

#[cfg(test)]
mod tests {
    use specta_typescript::Typescript;

    /// `src/bindings.ts` precisa estar em dia com os comandos/tipos do Rust.
    /// Para regenerar: `UPDATE_BINDINGS=1 cargo test -p ipsec-client-app bindings`.
    #[test]
    fn bindings_are_up_to_date() {
        let committed = concat!(env!("CARGO_MANIFEST_DIR"), "/../src/bindings.ts");
        let tmp = tempfile::NamedTempFile::new().expect("tmp");
        super::specta_builder()
            .export(
                Typescript::default().header("// Gerado pelo tauri-specta; não editar.\n"),
                tmp.path(),
            )
            .expect("exportar bindings");
        let generated = std::fs::read_to_string(tmp.path()).expect("ler gerado");
        if std::env::var_os("UPDATE_BINDINGS").is_some() {
            std::fs::write(committed, &generated).expect("gravar bindings");
            return;
        }
        let current = std::fs::read_to_string(committed).unwrap_or_default();
        assert!(
            current == generated,
            "src/bindings.ts desatualizado: rode UPDATE_BINDINGS=1 cargo test -p ipsec-client-app bindings"
        );
    }
}
