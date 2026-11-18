//! Componente de rede: o engine embutido e o helper com privilégio (ADRs
//! 0013, 0014 e 0018). Este módulo:
//!
//! - lê o **pacote** que vem dentro do app: um diretório com `SHA256SUMS`,
//!   `VERSION` e os arquivos, montado por `scripts/stage-network-component.sh`;
//! - diz o **estado** da instalação: ausente, pronta, desatualizada ou com
//!   problema;
//! - **instala** e **remove** com a senha de administrador, via `pkexec`.
//!
//! Para instalar, o instalador roda **no lugar** quando o pacote já está num
//! diretório que só o root altera (o app instalado por `.rpm`/`.deb`): nada
//! do que o root executa passa por um arquivo do usuário. No AppImage, o
//! root não lê a montagem FUSE, então o pacote é copiado para um diretório
//! privado e conferido. Nos dois casos o instalador, como root, confere de
//! novo numa cópia só dele antes de instalar. A remoção usa a cópia do
//! instalador que ficou em `/opt/ipsec-client` (do root), nunca um arquivo do
//! usuário.

use std::ffi::OsStr;
use std::fs;
use std::io::{self, Read};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

use sha2::{Digest, Sha256};

use crate::engine::NativeEngine;

/// Onde o componente é instalado.
pub const PREFIX: &str = "/opt/ipsec-client";
/// Nome do diretório do pacote entre os recursos do app.
pub const BUNDLE_RESOURCE: &str = "network";

const SUMS: &str = "SHA256SUMS";
const VERSION_FILE: &str = "VERSION";
const ORIGIN_FILE: &str = "ORIGIN";
const INSTALLER: &str = "install-native.sh";
const HELPER: &str = "ipsec-client-helper";
/// O helper vai compactado no pacote: o linuxdeploy (AppImage) reescreve o
/// RPATH de todo ELF solto dentro do app, e o hash deixaria de bater.
const HELPER_GZ: &str = "ipsec-client-helper.gz";

/// Arquivos obrigatórios do pacote (a mesma lista do `install-native.sh`).
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

/// Arquivos que ficam executáveis na cópia.
const EXECUTABLES: [&str; 3] = [INSTALLER, "engine/entrypoint.sh", "engine/updown.sh"];

const PKEXEC_PATHS: [&str; 2] = ["/usr/bin/pkexec", "/bin/pkexec"];

/// Prefixo das linhas do instalador: distingue a falha dele da do `pkexec`.
const INSTALLER_TAG: &str = "[network component]";

/// Linhas de log guardadas de uma instalação que falhou.
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
        /// Últimas linhas da saída do instalador.
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

    /// Erro de E/S local.
    #[error(transparent)]
    Io(#[from] io::Error),
}

fn invalid(msg: impl Into<String>) -> ComponentError {
    ComponentError::InvalidBundle(msg.into())
}

/// Versões registradas em `VERSION` (`version=` e `strongswan=`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Version {
    /// Versão do IPsec Client que montou o pacote.
    pub app: Option<String>,
    /// Versão do strongSwan embutido.
    pub strongswan: Option<String>,
}

impl Version {
    /// Lê o conteúdo de um `VERSION` (chaves desconhecidas são ignoradas).
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

/// Quem instalou o componente.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// O próprio app (AppImage), com a senha de administrador.
    App,
    /// Um pacote do sistema (.rpm/.deb).
    Package,
    /// À mão (`scripts/test-native.sh`) ou por uma versão sem registro.
    Manual,
}

impl Origin {
    /// Valor no arquivo `ORIGIN` e no argumento do instalador.
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

/// Pacote do componente: o diretório que vem dentro do app.
#[derive(Debug, Clone)]
pub struct Bundle {
    dir: PathBuf,
    sums: String,
    /// (hash SHA-256 em hexadecimal, caminho relativo)
    entries: Vec<(String, String)>,
    digest: String,
    version: Version,
}

impl Bundle {
    /// Lê o pacote: `SHA256SUMS` (formato do `sha256sum`) e `VERSION`. Não
    /// confere os hashes (ver [`Bundle::verify`]).
    ///
    /// # Errors
    /// Pacote ausente ou malformado.
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

    /// Diretório do pacote.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Versões do pacote.
    #[must_use]
    pub fn version(&self) -> &Version {
        &self.version
    }

    /// Identidade do pacote: SHA-256 do `SHA256SUMS`. Igual ao instalado =
    /// mesma versão do componente.
    #[must_use]
    pub fn digest(&self) -> &str {
        &self.digest
    }

    /// Confere cada arquivo listado: arquivo comum (sem links) e hash igual.
    ///
    /// # Errors
    /// [`ComponentError::Corrupted`] ou arquivo ausente.
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

    /// Copia o pacote para um diretório novo e privado (0700) dentro de
    /// `parent`, com as permissões certas, e confere a cópia. O diretório é
    /// apagado quando o [`StagedBundle`] sai de escopo.
    ///
    /// # Errors
    /// E/S ou cópia que não confere.
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

/// Cópia privada do pacote (apagada ao sair de escopo).
#[derive(Debug)]
pub struct StagedBundle {
    dir: tempfile::TempDir,
    bundle: Bundle,
}

impl StagedBundle {
    /// Diretório da cópia.
    #[must_use]
    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Pacote copiado (já conferido).
    #[must_use]
    pub fn bundle(&self) -> &Bundle {
        &self.bundle
    }
}

/// Lê um `SHA256SUMS`: caminhos relativos simples, sem repetição, com todos
/// os [`REQUIRED_FILES`].
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
        // `sha256sum`: "<hash>  <caminho>" (texto) ou "<hash> *<caminho>" (binário).
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

/// Só componentes normais: nada absoluto, `.`, `..` ou vazio.
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

/// O que está instalado em [`PREFIX`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    /// Versões registradas (vazias em instalações antigas, sem registro).
    pub version: Version,
    /// Identidade do pacote instalado (ver [`Bundle::digest`]).
    pub digest: Option<String>,
    /// Quem instalou.
    pub origin: Origin,
}

/// Instalação em `prefix`, se houver (o helper é o que define "instalado").
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

/// Situação do componente.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Nada instalado.
    NotInstalled,
    /// Instalado, respondendo e igual ao pacote do app (ou sem pacote para comparar).
    Ready,
    /// Instalado e respondendo, mas diferente do pacote do app.
    Outdated,
    /// Instalação incompleta ou helper que não responde.
    Broken,
}

/// Estado do componente.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// Situação.
    pub state: State,
    /// O que está instalado.
    pub installed: Option<Installed>,
    /// Versão informada pelo helper (quando ele responde).
    pub helper_version: Option<String>,
    /// Motivo, quando [`State::Broken`].
    pub problem: Option<String>,
}

/// Estado do componente instalado neste computador, comparado com o pacote
/// do app (se houver). Bloqueante: pergunta ao helper.
#[must_use]
pub fn status(bundle: Option<&Bundle>) -> Status {
    status_with(Path::new(PREFIX), &NativeEngine::default(), bundle)
}

/// [`status`] com instalação e helper indicados (testes).
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

/// Dá para pedir a senha de administrador daqui (`pkexec` presente)?
#[must_use]
pub fn pkexec_available() -> bool {
    pkexec().is_ok()
}

/// `pkexec <programa> <args>` com o ambiente limpo (o AppImage define
/// variáveis como `LD_LIBRARY_PATH` que não devem chegar ao root).
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

/// Interpreta o fim do `pkexec` + instalador. O `pkexec` sai com 126 quando
/// o pedido é fechado e com 127 quando não autoriza; o instalador marca as
/// próprias linhas com [`INSTALLER_TAG`].
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

/// Onde a cópia privada do pacote é feita: `$XDG_RUNTIME_DIR` (tmpfs, só do
/// usuário) ou o diretório temporário.
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

/// Instala ou atualiza o componente a partir do pacote. Pede a senha de
/// administrador (`pkexec`). Bloqueante. Devolve a saída do instalador.
///
/// # Errors
/// Pacote inválido, sem `pkexec`, autorização negada ou falha do instalador.
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

/// Remove o componente (pede a senha de administrador). Bloqueante.
///
/// # Errors
/// Nada instalado, instalado por pacote, instalador ausente/não confiável,
/// autorização negada ou falha.
pub fn uninstall() -> Result<Vec<String>, ComponentError> {
    let pkexec = pkexec()?;
    let installer = uninstaller(Path::new(PREFIX))?;
    run_privileged(privileged(pkexec, &installer, &[OsStr::new("uninstall")]))
}

/// Instalador que ficou na instalação, desde que seja do root e ninguém mais
/// possa alterá-lo (nem os diretórios acima dele).
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

    /// Monta um pacote completo em `dir` (conteúdo falso).
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
        // Modo binário do sha256sum também vale.
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
        // Os testes não rodam como root: os arquivos são do usuário.
        let err = uninstaller(prefix.path()).expect_err("não é do root");
        assert!(err.to_string().contains("root"), "{err}");
    }

    /// Helper de mentira que responde `Pong`.
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

        // Arquivos instalados, mas helper fora do ar.
        fs::create_dir_all(prefix.join("bin")).expect("mkdir");
        fs::write(prefix.join("bin").join(HELPER), "").expect("helper");
        let broken = status_with(&prefix, &nobody, None);
        assert_eq!(broken.state, State::Broken);
        assert!(broken.problem.is_some());

        let helper = fake_helper(base.path());
        let src = base.path().join("bundle");
        make_bundle(&src);
        let bundle = Bundle::open(&src).expect("abre");
        // Instalação antiga, sem registro: desatualizada se o app traz pacote.
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
        // 127 vindo do instalador (comando ausente) é falha dele, não do pkexec.
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
