//! Contexto de build da imagem do engine, embutido no binário (ADR-0006).
//!
//! Os arquivos de `engine/` entram no binário com `include_bytes!`. O hash do
//! conteúdo identifica a imagem (`ipsec-client-engine:6.0.1-<hash8>`), então
//! binário e engine sempre andam juntos.

use sha2::{Digest, Sha256};

/// Repositório (nome) da imagem do engine.
pub const IMAGE_REPO: &str = "ipsec-client-engine";
/// Versão do strongSwan fixada no Dockerfile.
pub const STRONGSWAN_VERSION: &str = "6.0.1";
/// Label com o hash completo do contexto, gravada na imagem.
pub const HASH_LABEL: &str = "io.github.ipsec-client.engine.hash";

/// Um arquivo do contexto de build.
#[derive(Debug, Clone, Copy)]
pub struct ContextFile {
    /// Caminho relativo dentro do contexto (e de `engine/`).
    pub path: &'static str,
    /// Permissão no tar.
    pub mode: u32,
    /// Conteúdo.
    pub contents: &'static [u8],
}

macro_rules! engine_file {
    ($path:literal, $mode:literal) => {
        ContextFile {
            path: $path,
            mode: $mode,
            contents: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../engine/", $path)),
        }
    };
}

/// Todos os arquivos do contexto, em ordem de caminho (o hash depende disso).
/// Um teste garante que esta lista bate com o conteúdo de `engine/`.
pub const FILES: &[ContextFile] = &[
    engine_file!("Dockerfile", 0o644),
    engine_file!("entrypoint.sh", 0o755),
    engine_file!("strongswan.d/zz-ipsec-client.conf", 0o644),
    engine_file!("updown.sh", 0o755),
];

/// SHA-256 (hex) do contexto: caminho, tamanho e conteúdo de cada arquivo.
#[must_use]
pub fn content_hash() -> String {
    let mut hasher = Sha256::new();
    for file in FILES {
        hasher.update(file.path.as_bytes());
        hasher.update([0u8]);
        hasher.update((file.contents.len() as u64).to_be_bytes());
        hasher.update(file.contents);
    }
    hex::encode(hasher.finalize())
}

/// Nome completo da imagem: `ipsec-client-engine:6.0.1-<8 primeiros do hash>`.
#[must_use]
pub fn image_tag() -> String {
    let hash = content_hash();
    format!("{IMAGE_REPO}:{STRONGSWAN_VERSION}-{}", &hash[..8])
}

/// Tar do contexto, determinístico (mtime 0, dono root), pronto para a API
/// `/build` do Docker.
///
/// # Errors
/// Só falha se a escrita em memória falhar (na prática, nunca).
pub fn tar_archive() -> std::io::Result<Vec<u8>> {
    let mut builder = tar::Builder::new(Vec::new());
    builder.mode(tar::HeaderMode::Deterministic);
    for file in FILES {
        let mut header = tar::Header::new_gnu();
        header.set_size(file.contents.len() as u64);
        header.set_mode(file.mode);
        header.set_mtime(0);
        header.set_uid(0);
        header.set_gid(0);
        builder.append_data(&mut header, file.path, file.contents)?;
    }
    builder.into_inner()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::path::Path;

    fn files_on_disk(dir: &Path, base: &Path, out: &mut BTreeSet<String>) {
        for entry in std::fs::read_dir(dir).expect("ler engine/") {
            let path = entry.expect("entrada de diretório").path();
            if path.is_dir() {
                files_on_disk(&path, base, out);
            } else if let Ok(rel) = path.strip_prefix(base) {
                out.insert(rel.to_string_lossy().into_owned());
            }
        }
    }

    #[test]
    fn embedded_files_match_engine_dir() {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../engine");
        let mut on_disk = BTreeSet::new();
        files_on_disk(&base, &base, &mut on_disk);
        let embedded: BTreeSet<String> = FILES.iter().map(|f| f.path.to_owned()).collect();
        assert_eq!(
            on_disk, embedded,
            "engine/ e context::FILES divergem: atualize a lista"
        );
    }

    #[test]
    fn files_are_sorted_by_path() {
        let paths: Vec<_> = FILES.iter().map(|f| f.path).collect();
        let mut sorted = paths.clone();
        sorted.sort_unstable();
        assert_eq!(paths, sorted);
    }

    #[test]
    fn hash_is_stable_and_tag_is_well_formed() {
        let hash = content_hash();
        assert_eq!(hash.len(), 64);
        assert_eq!(hash, content_hash());
        let tag = image_tag();
        assert_eq!(tag, format!("ipsec-client-engine:6.0.1-{}", &hash[..8]));
    }

    #[test]
    fn tar_is_deterministic_and_complete() {
        let first = tar_archive().expect("tar");
        assert_eq!(first, tar_archive().expect("tar"));

        let mut archive = tar::Archive::new(first.as_slice());
        let mut seen = Vec::new();
        for entry in archive.entries().expect("entradas") {
            let entry = entry.expect("entrada");
            let path = entry
                .path()
                .expect("caminho")
                .to_string_lossy()
                .into_owned();
            seen.push((path, entry.header().mode().expect("modo")));
        }
        let expected: Vec<_> = FILES.iter().map(|f| (f.path.to_owned(), f.mode)).collect();
        assert_eq!(seen, expected);
    }
}
