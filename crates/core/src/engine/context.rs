//! Build context of the engine image, embedded in the binary (ADR-0006).
//!
//! The files of `engine/` go into the binary with `include_bytes!`. The hash
//! of the content identifies the image (`ikebox-engine:6.0.1-<hash8>`),
//! so the binary and the engine always go together.

use sha2::{Digest, Sha256};

/// Repository (name) of the engine image.
pub const IMAGE_REPO: &str = "ikebox-engine";
/// strongSwan version pinned in the Dockerfile.
pub const STRONGSWAN_VERSION: &str = "6.0.1";
/// Label with the full hash of the context, written into the image.
pub const HASH_LABEL: &str = "io.github.ikebox.engine.hash";

/// A file of the build context.
#[derive(Debug, Clone, Copy)]
pub struct ContextFile {
    /// Relative path inside the context (and inside `engine/`).
    pub path: &'static str,
    /// Mode in the tar.
    pub mode: u32,
    /// Content.
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

/// Every file of the context, in path order (the hash depends on it). A test
/// makes sure this list matches the content of `engine/`.
pub const FILES: &[ContextFile] = &[
    engine_file!("Dockerfile", 0o644),
    engine_file!("entrypoint.sh", 0o755),
    engine_file!("strongswan.d/zz-ikebox.conf", 0o644),
    engine_file!("updown.sh", 0o755),
];

/// SHA-256 (hex) of the context: path, size and content of each file.
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

/// Full image name: `ikebox-engine:6.0.1-<first 8 of the hash>`.
#[must_use]
pub fn image_tag() -> String {
    let hash = content_hash();
    format!("{IMAGE_REPO}:{STRONGSWAN_VERSION}-{}", &hash[..8])
}

/// Tar of the context, deterministic (mtime 0, owner root), ready for Docker's
/// `/build` API.
///
/// # Errors
/// Only fails if writing to memory fails (in practice, never).
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
        assert_eq!(tag, format!("ikebox-engine:6.0.1-{}", &hash[..8]));
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
