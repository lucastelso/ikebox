//! Network component: the embedded engine and the privileged helper (ADRs
//! 0013, 0014 and 0018). This module:
//!
//! - reads the **bundle** that comes inside the app: a directory with
//!   `SHA256SUMS`, `VERSION` and the files, put together by
//!   `scripts/stage-network-component.sh`;
//! - tells the **state** of the installation: missing, ready, outdated or
//!   broken;
//! - **installs** and **removes** it with the administrator password, through
//!   `pkexec`.
//!
//! To install, the installer runs **in place** when the bundle is already in a
//! directory only root can change (the app installed by `.rpm`/`.deb`): nothing
//! root runs goes through a file of the user's. In the AppImage, root cannot
//! read the FUSE mount, so the bundle is copied to a private directory and
//! checked. Either way the installer, as root, checks it again in a copy of its
//! own before installing. Removing uses the installer copy left in
//! `/opt/ipsec-client` (root's), never a file of the user's.

use std::ffi::OsStr;
use std::fs;
use std::io::{self, Read};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

use sha2::{Digest, Sha256};

use crate::engine::NativeEngine;

/// Where the component is installed.
pub const PREFIX: &str = "/opt/ipsec-client";
/// Name of the bundle directory among the app's resources.
pub const BUNDLE_RESOURCE: &str = "network";

const SUMS: &str = "SHA256SUMS";
const VERSION_FILE: &str = "VERSION";
const ORIGIN_FILE: &str = "ORIGIN";
const INSTALLER: &str = "install-native.sh";
const HELPER: &str = "ipsec-client-helper";
/// The helper goes compressed in the bundle: linuxdeploy (AppImage) rewrites the
/// RPATH of every loose ELF inside the app, and the hash would no longer match.
const HELPER_GZ: &str = "ipsec-client-helper.gz";

/// Files the bundle must have (the same list as `install-native.sh`).
pub const REQUIRED_FILES: [&str; 10] = [
    VERSION_FILE,
    INSTALLER,
    "engine.tar.gz",
    HELPER_GZ,
    "engine/entrypoint.sh",
    "engine/updown.sh",
    "engine/strongswan.d/zz-ipsec-client.conf",
    "packaging/systemd/ipsec-client-helper.socket",
    "packaging/systemd/ipsec-client-helper.service",
    "packaging/polkit/io.github.lucastelso.ipsecclient.policy",
];

/// Files that are made executable in the copy.
const EXECUTABLES: [&str; 3] = [INSTALLER, "engine/entrypoint.sh", "engine/updown.sh"];

const PKEXEC_PATHS: [&str; 2] = ["/usr/bin/pkexec", "/bin/pkexec"];

/// Prefix of the installer's lines: tells its failures from `pkexec`'s.
const INSTALLER_TAG: &str = "[network component]";

/// Log lines kept from an installation that failed.
const MAX_LOG_LINES: usize = 40;

/// Network component errors (messages for the end user, see [`crate::i18n`]).
#[derive(Debug, thiserror::Error)]
pub enum ComponentError {
    /// Malformed bundle.
    #[error("{}", crate::trf!("invalid network component bundle: {}", "pacote do componente de rede inválido: {}", .0))]
    InvalidBundle(String),

    /// A bundle file does not match `SHA256SUMS`.
    #[error("{}", crate::trf!(
        "corrupted network component file: {} (download the app again)",
        "arquivo do componente de rede corrompido: {} (baixe o app de novo)",
        .0
    ))]
    Corrupted(String),

    /// No `pkexec`.
    #[error("{}", crate::tr!(
        "pkexec was not found; install it (package pkexec, or polkit on Fedora) or use \
         the IPsec Client .rpm/.deb package",
        "o pkexec não foi encontrado; instale-o (pacote pkexec, ou polkit no Fedora) ou \
         use o pacote .rpm/.deb do IPsec Client"
    ))]
    NoPkexec,

    /// The user closed the password prompt.
    #[error("{}", crate::tr!("authorization cancelled", "autorização cancelada"))]
    Cancelled,

    /// polkit did not authorize (wrong password, no authentication agent...).
    #[error("{}", crate::trf!(
        "could not get administrator authorization: {}",
        "não foi possível obter a autorização de administrador: {}",
        .0
    ))]
    NotAuthorized(String),

    /// The installer ran and failed.
    #[error("{}", crate::tr!("the network component installer failed", "o instalador do componente de rede falhou"))]
    Failed {
        /// Last lines of the installer's output.
        log: Vec<String>,
    },

    /// Installed by a system package: the app does not touch it.
    #[error("{}", crate::tr!(
        "the network component came from a system package (.rpm/.deb); update or \
         remove it with the package manager",
        "o componente de rede veio de um pacote do sistema (.rpm/.deb); atualize ou \
         remova pelo gerenciador de pacotes"
    ))]
    ManagedByPackage,

    /// Nothing installed.
    #[error("{}", crate::tr!("the network component is not installed", "o componente de rede não está instalado"))]
    NotInstalled,

    /// A file that should belong to root does not.
    #[error("{}", crate::trf!(
        "untrusted network component installation: {}",
        "instalação do componente de rede não confiável: {}",
        .0
    ))]
    Untrusted(String),

    /// Local I/O error.
    #[error(transparent)]
    Io(#[from] io::Error),
}

fn invalid(msg: impl Into<String>) -> ComponentError {
    ComponentError::InvalidBundle(msg.into())
}

/// Versions recorded in `VERSION` (`version=` and `strongswan=`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Version {
    /// Version of IPsec Client that put the bundle together.
    pub app: Option<String>,
    /// Version of the embedded strongSwan.
    pub strongswan: Option<String>,
}

impl Version {
    /// Reads the content of a `VERSION` (unknown keys are ignored).
    #[must_use]
    pub fn parse(content: &str) -> Self {
        let mut version = Self::default();
        for line in content.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = Some(value.trim().to_owned()).filter(|v| !v.is_empty());
            match key.trim() {
                "version" => version.app = value,
                "strongswan" => version.strongswan = value,
                _ => {}
            }
        }
        version
    }
}

/// Who installed the component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// The app itself (AppImage), with the administrator password.
    App,
    /// A system package (.rpm/.deb).
    Package,
    /// By hand (`scripts/test-native.sh`) or by a version that kept no record.
    Manual,
}

impl Origin {
    /// Value in the `ORIGIN` file and in the installer's argument.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::App => "app",
            Self::Package => "package",
            Self::Manual => "manual",
        }
    }

    fn parse(value: &str) -> Self {
        match value.trim() {
            "app" => Self::App,
            "package" => Self::Package,
            _ => Self::Manual,
        }
    }
}

/// Component bundle: the directory that comes inside the app.
#[derive(Debug, Clone)]
pub struct Bundle {
    dir: PathBuf,
    sums: String,
    /// (SHA-256 hash in hexadecimal, relative path)
    entries: Vec<(String, String)>,
    digest: String,
    version: Version,
}

impl Bundle {
    /// Reads the bundle: `SHA256SUMS` (`sha256sum` format) and `VERSION`. Does not
    /// check the hashes (see [`Bundle::verify`]).
    ///
    /// # Errors
    /// Bundle missing or malformed.
    pub fn open(dir: &Path) -> Result<Self, ComponentError> {
        let sums = fs::read_to_string(dir.join(SUMS)).map_err(|e| {
            let dir = dir.display();
            invalid(crate::trf!(
                "{SUMS} unreadable in {dir}: {e}",
                "{SUMS} ilegível em {dir}: {e}"
            ))
        })?;
        let entries = parse_sums(&sums)?;
        let version = fs::read_to_string(dir.join(VERSION_FILE))
            .map(|c| Version::parse(&c))
            .map_err(|e| {
                invalid(crate::trf!(
                    "{VERSION_FILE} unreadable: {e}",
                    "{VERSION_FILE} ilegível: {e}"
                ))
            })?;
        Ok(Self {
            dir: dir.to_owned(),
            digest: sha256_hex(sums.as_bytes()),
            sums,
            entries,
            version,
        })
    }

    /// Bundle directory.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Bundle versions.
    #[must_use]
    pub fn version(&self) -> &Version {
        &self.version
    }

    /// Bundle identity: SHA-256 of `SHA256SUMS`. Equal to the installed one =
    /// same component version.
    #[must_use]
    pub fn digest(&self) -> &str {
        &self.digest
    }

    /// Checks every listed file: a regular file (no links) with a matching hash.
    ///
    /// # Errors
    /// [`ComponentError::Corrupted`] or a missing file.
    pub fn verify(&self) -> Result<(), ComponentError> {
        for (hash, rel) in &self.entries {
            let path = self.dir.join(rel);
            let meta = fs::symlink_metadata(&path).map_err(|e| invalid(format!("{rel}: {e}")))?;
            if !meta.file_type().is_file() {
                return Err(invalid(crate::trf!(
                    "{rel} is not a regular file",
                    "{rel} não é um arquivo comum"
                )));
            }
            if sha256_file(&path)? != *hash {
                return Err(ComponentError::Corrupted(rel.clone()));
            }
        }
        Ok(())
    }

    /// Copies the bundle to a new private directory (0700) inside `parent`, with
    /// the right permissions, and checks the copy. The directory is deleted when
    /// the [`StagedBundle`] goes out of scope.
    ///
    /// # Errors
    /// I/O, or a copy that does not match.
    pub fn stage_in(&self, parent: &Path) -> Result<StagedBundle, ComponentError> {
        let dir = tempfile::Builder::new()
            .prefix("ipsec-client-setup-")
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in(parent)?;
        for (_, rel) in &self.entries {
            let dest = dir.path().join(rel);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(self.dir.join(rel), &dest)?;
            let mode = if EXECUTABLES.contains(&rel.as_str()) {
                0o755
            } else {
                0o644
            };
            fs::set_permissions(&dest, fs::Permissions::from_mode(mode))?;
        }
        fs::write(dir.path().join(SUMS), &self.sums)?;
        let bundle = Self::open(dir.path())?;
        bundle.verify()?;
        Ok(StagedBundle { dir, bundle })
    }
}

/// Private copy of the bundle (deleted when it goes out of scope).
#[derive(Debug)]
pub struct StagedBundle {
    dir: tempfile::TempDir,
    bundle: Bundle,
}

impl StagedBundle {
    /// Directory of the copy.
    #[must_use]
    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Copied bundle (already checked).
    #[must_use]
    pub fn bundle(&self) -> &Bundle {
        &self.bundle
    }
}

/// Reads a `SHA256SUMS`: simple relative paths, no repeats, with every one of
/// the [`REQUIRED_FILES`].
fn parse_sums(content: &str) -> Result<Vec<(String, String)>, ComponentError> {
    let mut entries: Vec<(String, String)> = Vec::new();
    for line in content.lines().filter(|l| !l.trim().is_empty()) {
        let bad = || {
            invalid(crate::trf!(
                "invalid line in {SUMS}: {line}",
                "linha inválida no {SUMS}: {line}"
            ))
        };
        let (hash, rest) = line.split_once(' ').ok_or_else(bad)?;
        // `sha256sum`: "<hash>  <path>" (text) or "<hash> *<path>" (binary).
        let path = rest
            .strip_prefix(' ')
            .or_else(|| rest.strip_prefix('*'))
            .ok_or_else(bad)?;
        if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(bad());
        }
        if !is_plain_relative(path) || path == SUMS {
            return Err(invalid(crate::trf!(
                "path not allowed in {SUMS}: {path}",
                "caminho não permitido no {SUMS}: {path}"
            )));
        }
        if entries.iter().any(|(_, p)| p == path) {
            return Err(invalid(crate::trf!(
                "repeated path in {SUMS}: {path}",
                "caminho repetido no {SUMS}: {path}"
            )));
        }
        entries.push((hash.to_ascii_lowercase(), path.to_owned()));
    }
    if let Some(missing) = REQUIRED_FILES
        .iter()
        .find(|f| !entries.iter().any(|(_, p)| p == *f))
    {
        return Err(invalid(crate::trf!(
            "incomplete bundle: missing {missing}",
            "pacote incompleto: falta {missing}"
        )));
    }
    Ok(entries)
}

/// Normal components only: nothing absolute, `.`, `..` or empty.
fn is_plain_relative(path: &str) -> bool {
    !path.is_empty()
        && !path.ends_with('/')
        && Path::new(path)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0_u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// What is installed in [`PREFIX`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    /// Recorded versions (empty in old installations, which kept no record).
    pub version: Version,
    /// Identity of the installed bundle (see [`Bundle::digest`]).
    pub digest: Option<String>,
    /// Who installed it.
    pub origin: Origin,
}

/// Installation in `prefix`, if there is one (the helper is what defines
/// "installed").
#[must_use]
pub fn installed_at(prefix: &Path) -> Option<Installed> {
    if !prefix.join("bin").join(HELPER).exists() {
        return None;
    }
    Some(Installed {
        version: fs::read_to_string(prefix.join(VERSION_FILE))
            .map(|c| Version::parse(&c))
            .unwrap_or_default(),
        digest: fs::read(prefix.join(SUMS)).ok().map(|c| sha256_hex(&c)),
        origin: fs::read_to_string(prefix.join(ORIGIN_FILE))
            .map(|c| Origin::parse(&c))
            .unwrap_or(Origin::Manual),
    })
}

/// State of the component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Nothing installed.
    NotInstalled,
    /// Installed, answering and equal to the app's bundle (or no bundle to compare).
    Ready,
    /// Installed and answering, but different from the app's bundle.
    Outdated,
    /// Incomplete installation, or a helper that does not answer.
    Broken,
}

/// State of the component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// State.
    pub state: State,
    /// What is installed.
    pub installed: Option<Installed>,
    /// Version reported by the helper (when it answers).
    pub helper_version: Option<String>,
    /// Reason, when [`State::Broken`].
    pub problem: Option<String>,
}

/// State of the component installed on this computer, compared with the app's
/// bundle (if any). Blocking: it asks the helper.
#[must_use]
pub fn status(bundle: Option<&Bundle>) -> Status {
    status_with(Path::new(PREFIX), &NativeEngine::default(), bundle)
}

/// [`status`] with the installation and helper given (tests).
#[must_use]
pub fn status_with(prefix: &Path, helper: &NativeEngine, bundle: Option<&Bundle>) -> Status {
    let installed = installed_at(prefix);
    if installed.is_none() && !helper.installed() {
        return Status {
            state: State::NotInstalled,
            installed,
            helper_version: None,
            problem: None,
        };
    }
    let (state, helper_version, problem) = match (helper.ping(), &installed) {
        (Err(e), _) => (State::Broken, None, Some(e.to_string())),
        (Ok(version), None) => (
            State::Broken,
            Some(version),
            Some(crate::trf!(
                "component files missing in {}",
                "arquivos do componente ausentes em {}",
                prefix.display()
            )),
        ),
        (Ok(version), Some(inst)) => {
            let outdated = bundle.is_some_and(|b| inst.digest.as_deref() != Some(b.digest()));
            let state = if outdated {
                State::Outdated
            } else {
                State::Ready
            };
            (state, Some(version), None)
        }
    };
    Status {
        state,
        installed,
        helper_version,
        problem,
    }
}

fn pkexec() -> Result<&'static Path, ComponentError> {
    PKEXEC_PATHS
        .iter()
        .map(Path::new)
        .find(|p| p.is_file())
        .ok_or(ComponentError::NoPkexec)
}

/// Can the administrator password be asked from here (`pkexec` present)?
#[must_use]
pub fn pkexec_available() -> bool {
    pkexec().is_ok()
}

/// `pkexec <program> <args>` with a clean environment (the AppImage sets
/// variables such as `LD_LIBRARY_PATH` that must not reach root).
fn privileged(pkexec: &Path, program: &Path, args: &[&OsStr]) -> Command {
    let mut cmd = Command::new(pkexec);
    cmd.arg(program)
        .args(args)
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .stdin(Stdio::null());
    for key in ["LANG", "LC_ALL", "LC_MESSAGES", "LANGUAGE"] {
        if let Some(value) = std::env::var_os(key) {
            cmd.env(key, value);
        }
    }
    cmd
}

fn run_privileged(mut cmd: Command) -> Result<Vec<String>, ComponentError> {
    let out = cmd.output()?;
    classify(
        out.status.code(),
        &String::from_utf8_lossy(&out.stdout),
        &String::from_utf8_lossy(&out.stderr),
    )
}

/// Interprets how `pkexec` + installer ended. `pkexec` exits with 126 when the
/// prompt is closed and with 127 when it does not authorize; the installer tags
/// its own lines with [`INSTALLER_TAG`].
fn classify(code: Option<i32>, stdout: &str, stderr: &str) -> Result<Vec<String>, ComponentError> {
    let mut log: Vec<String> = stdout
        .lines()
        .chain(stderr.lines())
        .map(str::trim_end)
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect();
    if code == Some(0) {
        return Ok(log);
    }
    let from_installer = log.iter().any(|l| l.starts_with(INSTALLER_TAG));
    match code {
        Some(126) if !from_installer => Err(ComponentError::Cancelled),
        Some(127) if !from_installer => Err(ComponentError::NotAuthorized(
            stderr
                .lines()
                .rev()
                .find(|l| !l.trim().is_empty())
                .unwrap_or(crate::tr!("no details", "sem detalhes"))
                .trim()
                .to_owned(),
        )),
        _ => {
            let skip = log.len().saturating_sub(MAX_LOG_LINES);
            log.drain(..skip);
            Err(ComponentError::Failed { log })
        }
    }
}

/// Where the private copy of the bundle is made: `$XDG_RUNTIME_DIR` (tmpfs, the
/// user's only) or the temporary directory.
fn staging_parent() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute() && p.is_dir())
        .unwrap_or_else(std::env::temp_dir)
}

/// Does `path` belong to root, with nobody else able to change it?
fn root_only(meta: &fs::Metadata) -> bool {
    meta.uid() == 0 && meta.mode() & 0o022 == 0
}

/// Is every directory from `/` down to `dir` (given in canonical form) a real
/// directory that only root can change? If one of them is writable by
/// someone else, that someone can swap what is below it.
fn protected_dir(dir: &Path) -> bool {
    dir.is_absolute()
        && dir.ancestors().all(|ancestor| {
            fs::symlink_metadata(ancestor).is_ok_and(|m| m.file_type().is_dir() && root_only(&m))
        })
}

/// The bundle directory, if the installer can run from it **in place**: the
/// whole path and every bundle file belong to root and nobody else can write
/// to them (the app installed by a `.rpm`/`.deb`).
///
/// Then the copy to the user's runtime directory is skipped. That copy is a
/// window: between the password prompt and the moment root runs the
/// installer, a program running as the user could swap the copied installer
/// and run as root.
fn in_place_dir(bundle: &Bundle) -> Option<PathBuf> {
    let dir = fs::canonicalize(bundle.dir()).ok()?;
    if !protected_dir(&dir) {
        return None;
    }
    let files = bundle
        .entries
        .iter()
        .map(|(_, rel)| rel.as_str())
        .chain([SUMS]);
    for rel in files {
        let path = dir.join(rel);
        let parent = fs::canonicalize(path.parent()?).ok()?;
        let meta = fs::symlink_metadata(&path).ok()?;
        if !parent.starts_with(&dir)
            || !protected_dir(&parent)
            || !meta.file_type().is_file()
            || !root_only(&meta)
        {
            return None;
        }
    }
    let installer = fs::symlink_metadata(dir.join(INSTALLER)).ok()?;
    (installer.mode() & 0o100 != 0).then_some(dir)
}

/// Installs or updates the component from the bundle. Asks for the
/// administrator password (`pkexec`). Blocking. Returns the installer's output.
///
/// # Errors
/// Invalid bundle, no `pkexec`, authorization denied or installer failure.
pub fn install(bundle: &Bundle, origin: Origin) -> Result<Vec<String>, ComponentError> {
    let pkexec = pkexec()?;
    let in_place = in_place_dir(bundle);
    let installed = installed_at(Path::new(PREFIX)).map(|i| i.origin);
    let origin = effective_origin(in_place.is_some(), installed, origin)?;
    // Root-owned bundle: run it where it is. Otherwise (AppImage), a private
    // copy, deleted when `staged` goes out of scope.
    let staged;
    let dir = if let Some(dir) = in_place {
        bundle.verify()?;
        dir
    } else {
        staged = bundle.stage_in(&staging_parent())?;
        staged.path().to_owned()
    };
    let cmd = privileged(
        pkexec,
        &dir.join(INSTALLER),
        &[
            OsStr::new("install"),
            OsStr::new("--bundle"),
            dir.as_os_str(),
            OsStr::new("--origin"),
            OsStr::new(origin.as_str()),
        ],
    );
    run_privileged(cmd)
}

/// Origin recorded by an installation through the app. A root-owned bundle
/// is the one an `.rpm`/`.deb` ships: what it installs belongs to the
/// package (removing the package removes it), and it may repair or update a
/// component the package installed. Any other bundle (the AppImage) must
/// not replace a package's component.
fn effective_origin(
    package_bundle: bool,
    installed: Option<Origin>,
    requested: Origin,
) -> Result<Origin, ComponentError> {
    if package_bundle {
        return Ok(Origin::Package);
    }
    if installed == Some(Origin::Package) {
        return Err(ComponentError::ManagedByPackage);
    }
    Ok(requested)
}

/// Removes the component (asks for the administrator password). Blocking.
///
/// # Errors
/// Nothing installed, installed by a package, installer missing or untrusted,
/// authorization denied, or failure.
pub fn uninstall() -> Result<Vec<String>, ComponentError> {
    let pkexec = pkexec()?;
    let installer = uninstaller(Path::new(PREFIX))?;
    run_privileged(privileged(pkexec, &installer, &[OsStr::new("uninstall")]))
}

/// Installer left in the installation, provided it belongs to root and nobody
/// else can change it (nor the directories above it).
fn uninstaller(prefix: &Path) -> Result<PathBuf, ComponentError> {
    let installed = installed_at(prefix).ok_or(ComponentError::NotInstalled)?;
    if installed.origin == Origin::Package {
        return Err(ComponentError::ManagedByPackage);
    }
    let libexec = prefix.join("libexec");
    let installer = libexec.join(INSTALLER);
    if fs::symlink_metadata(&installer).is_err() {
        return Err(ComponentError::Untrusted(crate::trf!(
            "{} is missing; update the component before removing it",
            "{} ausente; atualize o componente antes de removê-lo",
            installer.display()
        )));
    }
    for (path, want_file) in [
        (prefix, false),
        (libexec.as_path(), false),
        (&installer, true),
    ] {
        let meta = fs::symlink_metadata(path)?;
        let kind_ok = if want_file {
            meta.file_type().is_file()
        } else {
            meta.file_type().is_dir()
        };
        if !kind_ok || meta.uid() != 0 || meta.mode() & 0o022 != 0 {
            return Err(ComponentError::Untrusted(crate::trf!(
                "{} is not owned by root or is writable by others",
                "{} não pertence ao root ou pode ser alterado por outros",
                path.display()
            )));
        }
    }
    Ok(installer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;

    /// Builds a complete bundle in `dir` (fake content).
    fn make_bundle(dir: &Path) {
        for rel in REQUIRED_FILES {
            let path = dir.join(rel);
            fs::create_dir_all(path.parent().expect("pai")).expect("mkdir");
            let content = if rel == VERSION_FILE {
                "version=1.2.3\nstrongswan=6.1.0\n".to_owned()
            } else {
                format!("conteúdo de {rel}\n")
            };
            fs::write(&path, content).expect("write");
        }
        resum(dir);
    }

    fn resum(dir: &Path) {
        let mut sums = String::new();
        let mut files: Vec<&str> = REQUIRED_FILES.to_vec();
        files.sort_unstable();
        for rel in files {
            if let Ok(hash) = sha256_file(&dir.join(rel)) {
                sums.push_str(&format!("{hash}  {rel}\n"));
            }
        }
        fs::write(dir.join(SUMS), sums).expect("sums");
    }

    #[test]
    fn installer_requires_the_same_files() {
        let script = include_str!("../../../packaging/install-native.sh");
        for rel in REQUIRED_FILES {
            let name = rel.rsplit('/').next().expect("nome");
            assert!(script.contains(name), "install-native.sh não exige {rel}");
        }
    }

    #[test]
    fn version_and_origin_parse() {
        let v = Version::parse("version=0.1.0\nstrongswan= 6.1.0 \nlixo\nx=y\n");
        assert_eq!(v.app.as_deref(), Some("0.1.0"));
        assert_eq!(v.strongswan.as_deref(), Some("6.1.0"));
        assert_eq!(Version::parse("version=\n"), Version::default());
        assert_eq!(Origin::parse("package\n"), Origin::Package);
        assert_eq!(Origin::parse("app"), Origin::App);
        assert_eq!(Origin::parse("??"), Origin::Manual);
    }

    #[test]
    fn sums_reject_unsafe_or_incomplete_lists() {
        let hash = "a".repeat(64);
        let full: String = REQUIRED_FILES
            .iter()
            .map(|f| format!("{hash}  {f}\n"))
            .collect();
        assert_eq!(
            parse_sums(&full).expect("válido").len(),
            REQUIRED_FILES.len()
        );
        // sha256sum's binary mode is valid too.
        assert!(parse_sums(&full.replace("  ", " *")).is_ok());

        for bad in [
            format!("{hash}  ../etc/passwd\n"),
            format!("{hash}  /etc/passwd\n"),
            format!("{hash}  ./VERSION\n"),
            format!("{hash}  {SUMS}\n"),
            format!("{hash}  engine/\n"),
            format!("{hash}  VERSION\n"),
            "xyz  VERSION\n".to_owned(),
            format!("{hash}VERSION\n"),
        ] {
            assert!(parse_sums(&format!("{full}{bad}")).is_err(), "{bad}");
        }
        let incomplete: String = full.lines().skip(1).map(|l| format!("{l}\n")).collect();
        let err = parse_sums(&incomplete).expect_err("incompleto");
        assert!(err.to_string().contains("missing VERSION"), "{err}");
    }

    #[test]
    fn bundle_opens_verifies_and_detects_tampering() {
        let dir = tempfile::tempdir().expect("tempdir");
        make_bundle(dir.path());
        let bundle = Bundle::open(dir.path()).expect("abre");
        assert_eq!(bundle.version().app.as_deref(), Some("1.2.3"));
        assert_eq!(bundle.digest().len(), 64);
        bundle.verify().expect("íntegro");

        fs::write(dir.path().join("engine/updown.sh"), "adulterado").expect("write");
        assert!(matches!(
            bundle.verify(),
            Err(ComponentError::Corrupted(f)) if f == "engine/updown.sh"
        ));

        make_bundle(dir.path());
        let target = dir.path().join("engine.tar.gz");
        fs::remove_file(&target).expect("rm");
        std::os::unix::fs::symlink("/etc/hostname", &target).expect("symlink");
        let err = Bundle::open(dir.path())
            .expect("abre")
            .verify()
            .expect_err("link");
        assert!(err.to_string().contains("regular file"), "{err}");
    }

    #[test]
    fn staging_copies_privately_with_modes() {
        let src = tempfile::tempdir().expect("tempdir");
        make_bundle(src.path());
        let bundle = Bundle::open(src.path()).expect("abre");
        let parent = tempfile::tempdir().expect("tempdir");
        let staged = bundle.stage_in(parent.path()).expect("copia");
        let path = staged.path().to_owned();
        let mode = |rel: &str| {
            fs::metadata(path.join(rel))
                .expect("meta")
                .permissions()
                .mode()
                & 0o777
        };
        assert_eq!(mode(""), 0o700, "diretório privado");
        assert_eq!(mode(INSTALLER), 0o755);
        assert_eq!(mode(HELPER_GZ), 0o644);
        assert_eq!(mode("engine.tar.gz"), 0o644);
        assert_eq!(staged.bundle().digest(), bundle.digest());
        drop(staged);
        assert!(!path.exists(), "a cópia é apagada");
    }

    #[test]
    fn only_root_owned_paths_are_protected() {
        // System directories: root's, and nobody else writes to them.
        assert!(protected_dir(Path::new("/")));
        let usr = fs::canonicalize("/usr").expect("/usr");
        assert!(protected_dir(&usr));
        // Anything the user owns, or under a world-writable directory.
        let dir = tempfile::tempdir().expect("tempdir");
        let canonical = fs::canonicalize(dir.path()).expect("canonical");
        assert!(!protected_dir(&canonical));
        assert!(!protected_dir(&std::env::temp_dir()));
        assert!(!protected_dir(Path::new("relative/path")));
        assert!(!protected_dir(Path::new("/does/not/exist")));
    }

    #[test]
    fn a_user_owned_bundle_never_runs_in_place() {
        // The tests do not run as root: the bundle belongs to the user, as in
        // the AppImage. It must go through the private copy.
        let src = tempfile::tempdir().expect("tempdir");
        make_bundle(src.path());
        let bundle = Bundle::open(src.path()).expect("abre");
        assert_eq!(in_place_dir(&bundle), None);
    }

    #[test]
    fn installed_reads_registry() {
        let prefix = tempfile::tempdir().expect("tempdir");
        assert_eq!(installed_at(prefix.path()), None);
        fs::create_dir_all(prefix.path().join("bin")).expect("mkdir");
        fs::write(prefix.path().join("bin").join(HELPER), "").expect("helper");
        let legacy = installed_at(prefix.path()).expect("instalado");
        assert_eq!(legacy.origin, Origin::Manual);
        assert_eq!(legacy.digest, None);
        fs::write(prefix.path().join(ORIGIN_FILE), "package\n").expect("origin");
        fs::write(prefix.path().join(SUMS), "x").expect("sums");
        let inst = installed_at(prefix.path()).expect("instalado");
        assert_eq!(inst.origin, Origin::Package);
        assert_eq!(inst.digest.as_deref(), Some(sha256_hex(b"x").as_str()));
        assert!(matches!(
            uninstaller(prefix.path()),
            Err(ComponentError::ManagedByPackage)
        ));
    }

    #[test]
    fn uninstaller_must_belong_to_root() {
        let prefix = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(prefix.path().join("bin")).expect("mkdir");
        fs::write(prefix.path().join("bin").join(HELPER), "").expect("helper");
        let err = uninstaller(prefix.path()).expect_err("ausente");
        assert!(err.to_string().contains("is missing"), "{err}");
        fs::create_dir_all(prefix.path().join("libexec")).expect("mkdir");
        fs::write(prefix.path().join("libexec").join(INSTALLER), "").expect("inst");
        // The tests do not run as root: the files belong to the user.
        let err = uninstaller(prefix.path()).expect_err("não é do root");
        assert!(err.to_string().contains("root"), "{err}");
    }

    /// Fake helper that answers `Pong`.
    fn fake_helper(dir: &Path) -> NativeEngine {
        let path = dir.join("helper.sock");
        let listener = UnixListener::bind(&path).expect("bind");
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut line = String::new();
                let _ = BufReader::new(&stream).read_line(&mut line);
                let reply = ipsec_client_proto::Response::Pong {
                    version: "9.9.9".into(),
                };
                let _ = (&stream).write_all(
                    ipsec_client_proto::encode(&reply)
                        .expect("encode")
                        .as_bytes(),
                );
            }
        });
        NativeEngine::new(path)
    }

    #[test]
    fn status_compares_with_bundle() {
        let base = tempfile::tempdir().expect("tempdir");
        let prefix = base.path().join("opt");
        let nobody = NativeEngine::new(base.path().join("nao-existe.sock"));
        assert_eq!(
            status_with(&prefix, &nobody, None).state,
            State::NotInstalled
        );

        // Files installed, but the helper is down.
        fs::create_dir_all(prefix.join("bin")).expect("mkdir");
        fs::write(prefix.join("bin").join(HELPER), "").expect("helper");
        let broken = status_with(&prefix, &nobody, None);
        assert_eq!(broken.state, State::Broken);
        assert!(broken.problem.is_some());

        let helper = fake_helper(base.path());
        let src = base.path().join("bundle");
        make_bundle(&src);
        let bundle = Bundle::open(&src).expect("abre");
        // Old installation, with no record: outdated if the app carries a bundle.
        let s = status_with(&prefix, &helper, Some(&bundle));
        assert_eq!(s.state, State::Outdated);
        assert_eq!(s.helper_version.as_deref(), Some("9.9.9"));
        assert_eq!(status_with(&prefix, &helper, None).state, State::Ready);

        fs::copy(src.join(SUMS), prefix.join(SUMS)).expect("registro");
        assert_eq!(
            status_with(&prefix, &helper, Some(&bundle)).state,
            State::Ready
        );
    }

    #[test]
    fn classify_pkexec_outcomes() {
        assert_eq!(
            classify(Some(0), "[network component] installed\n", "").expect("ok"),
            ["[network component] installed"]
        );
        assert!(matches!(
            classify(
                Some(126),
                "",
                "Error executing command as another user: Request dismissed\n"
            ),
            Err(ComponentError::Cancelled)
        ));
        match classify(
            Some(127),
            "",
            "Error executing command as another user: Not authorized\n",
        ) {
            Err(ComponentError::NotAuthorized(why)) => assert!(why.contains("Not authorized")),
            other => panic!("{other:?}"),
        }
        // 127 coming from the installer (missing command) is its failure, not pkexec's.
        match classify(
            Some(127),
            "[network component] installing\nfoo: not found\n",
            "",
        ) {
            Err(ComponentError::Failed { log }) => assert_eq!(log.len(), 2),
            other => panic!("{other:?}"),
        }
        let long: String = (0..100).map(|i| format!("linha {i}\n")).collect();
        match classify(Some(1), &long, "") {
            Err(ComponentError::Failed { log }) => {
                assert_eq!(log.len(), MAX_LOG_LINES);
                assert_eq!(log.last().map(String::as_str), Some("linha 99"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn privileged_command_has_clean_environment() {
        let cmd = privileged(
            Path::new("/usr/bin/pkexec"),
            Path::new("/run/x/install-native.sh"),
            &[OsStr::new("uninstall")],
        );
        let args: Vec<_> = cmd.get_args().collect();
        assert_eq!(args, ["/run/x/install-native.sh", "uninstall"]);
        let envs: Vec<_> = cmd.get_envs().collect();
        assert!(
            envs.iter()
                .all(|(k, _)| *k != "LD_LIBRARY_PATH" && *k != "LD_PRELOAD"),
            "{envs:?}"
        );
        assert!(envs.iter().any(|(k, v)| *k == "PATH" && v.is_some()));
    }
    #[test]
    fn package_bundles_install_as_package_and_others_keep_off() {
        // The .rpm/.deb bundle installs as the package, and may repair the
        // package's own component.
        for installed in [None, Some(Origin::App), Some(Origin::Package)] {
            assert_eq!(
                effective_origin(true, installed, Origin::App).ok(),
                Some(Origin::Package)
            );
        }
        // The AppImage keeps its origin, but never replaces the package's.
        assert_eq!(
            effective_origin(false, None, Origin::App).ok(),
            Some(Origin::App)
        );
        assert_eq!(
            effective_origin(false, Some(Origin::App), Origin::App).ok(),
            Some(Origin::App)
        );
        assert!(matches!(
            effective_origin(false, Some(Origin::Package), Origin::App),
            Err(ComponentError::ManagedByPackage)
        ));
    }
}
