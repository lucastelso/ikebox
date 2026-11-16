//! App desktop do IPsec Client (Tauri v2).
//!
//! Esta camada é fina (ADR-0009): os comandos convertem entre os DTOs da UI
//! ([`dto`]) e o `ipsec-client-core`, onde fica toda a lógica. O webview só
//! enxerga os comandos registrados aqui; não há plugins de shell/fs.

pub mod commands;
pub mod dto;

use tauri_specta::{Builder, collect_commands};

/// Registro dos comandos expostos à UI (fonte dos bindings TypeScript).
#[must_use]
pub fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new().commands(collect_commands![commands::app_info])
}

/// Sobe o app.
///
/// # Panics
/// Se o Tauri não conseguir iniciar (ex.: sem display gráfico).
pub fn run() {
    let builder = specta_builder();
    #[allow(clippy::expect_used)] // falha na inicialização: não há como seguir
    tauri::Builder::default()
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("falha ao iniciar o IPsec Client");
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
