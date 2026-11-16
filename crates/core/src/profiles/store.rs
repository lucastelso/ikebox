//! Leitura, gravação, importação e exportação de perfis.
//!
//! Perfis ficam em `$XDG_CONFIG_HOME/ipsec-client/profiles/<id>.json`
//! (diretório 0700, arquivos 0600: contêm gateway e id do grupo da
//! organização). A gravação é atômica (arquivo temporário + rename).

use std::fs;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use super::ProfileError;
use super::model::{Profile, SCHEMA_VERSION};
use super::validate::{is_slug, validate};

/// Tamanho máximo de um arquivo de perfil importado.
pub const MAX_PROFILE_BYTES: u64 = 1024 * 1024;

/// Lê um perfil de JSON: confere a versão do formato antes do resto e valida.
///
/// # Errors
/// JSON inválido, versão não suportada ou perfil inválido.
pub fn parse_profile(json: &str) -> Result<Profile, ProfileError> {
    let value: serde_json::Value = serde_json::from_str(json)?;
    let version = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or(ProfileError::MissingSchemaVersion)?;
    if version != u64::from(SCHEMA_VERSION) {
        return Err(ProfileError::UnsupportedVersion(version));
    }
    // Migrações de versões antigas entram aqui quando houver schema 2.
    let profile: Profile = serde_json::from_value(value)?;
    let issues = validate(&profile);
    if issues.is_empty() {
        Ok(profile)
    } else {
        Err(ProfileError::Invalid(issues))
    }
}

/// Serializa o perfil em JSON legível.
///
/// # Errors
/// Só falha em casos patológicos do serde.
pub fn to_json(profile: &Profile) -> Result<String, ProfileError> {
    let mut json = serde_json::to_string_pretty(profile)?;
    json.push('\n');
    Ok(json)
}

/// Diretório de perfis no disco.
#[derive(Debug, Clone)]
pub struct ProfileStore {
    dir: PathBuf,
}

/// Resultado da listagem: perfis válidos e arquivos com problema.
#[derive(Debug, Default)]
pub struct ProfileList {
    /// Perfis válidos, ordenados por nome.
    pub profiles: Vec<Profile>,
    /// Arquivos ignorados e o motivo.
    pub broken: Vec<(PathBuf, String)>,
}

impl ProfileStore {
    /// Diretório padrão: `$XDG_CONFIG_HOME/ipsec-client/profiles` ou
    /// `~/.config/ipsec-client/profiles`.
    ///
    /// # Errors
    /// [`ProfileError::NoConfigDir`] sem `XDG_CONFIG_HOME` nem `HOME`.
    pub fn default_dir() -> Result<PathBuf, ProfileError> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .filter(|p| p.is_absolute())
                    .map(|home| home.join(".config"))
            })
            .ok_or(ProfileError::NoConfigDir)?;
        Ok(base.join("ipsec-client").join("profiles"))
    }

    /// Store num diretório específico.
    #[must_use]
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// Diretório do store.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn path_for(&self, id: &str) -> Result<PathBuf, ProfileError> {
        if !is_slug(id) {
            return Err(ProfileError::NotFound(id.to_owned()));
        }
        Ok(self.dir.join(format!("{id}.json")))
    }

    /// Lista os perfis (arquivos com problema vão para `broken`).
    ///
    /// # Errors
    /// Erro de E/S ao ler o diretório (diretório inexistente = lista vazia).
    pub fn list(&self) -> Result<ProfileList, ProfileError> {
        let mut list = ProfileList::default();
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(list),
            Err(e) => return Err(e.into()),
        };
        for entry in entries {
            let path = entry?.path();
            if path.extension().is_none_or(|ext| ext != "json") {
                continue;
            }
            match read_limited(&path).and_then(|json| parse_profile(&json)) {
                Ok(profile) => list.profiles.push(profile),
                Err(e) => list.broken.push((path, e.to_string())),
            }
        }
        list.profiles.sort_by_key(|p| p.name.to_lowercase());
        Ok(list)
    }

    /// Carrega um perfil pelo id.
    ///
    /// # Errors
    /// [`ProfileError::NotFound`] ou erro de leitura/validação.
    pub fn load(&self, id: &str) -> Result<Profile, ProfileError> {
        let path = self.path_for(id)?;
        match read_limited(&path) {
            Ok(json) => parse_profile(&json),
            Err(ProfileError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(ProfileError::NotFound(id.to_owned()))
            }
            Err(e) => Err(e),
        }
    }

    /// Grava (cria ou substitui) um perfil, de forma atômica.
    ///
    /// # Errors
    /// Perfil inválido ou erro de E/S.
    pub fn save(&self, profile: &Profile) -> Result<(), ProfileError> {
        let issues = validate(profile);
        if !issues.is_empty() {
            return Err(ProfileError::Invalid(issues));
        }
        self.ensure_dir()?;
        let path = self.path_for(&profile.id)?;
        let tmp = self.dir.join(format!(".{}.json.tmp", profile.id));
        {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&tmp)?;
            file.write_all(to_json(profile)?.as_bytes())?;
            file.sync_all()?;
        }
        fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// Remove um perfil.
    ///
    /// # Errors
    /// [`ProfileError::NotFound`] ou erro de E/S.
    pub fn delete(&self, id: &str) -> Result<(), ProfileError> {
        match fs::remove_file(self.path_for(id)?) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(ProfileError::NotFound(id.to_owned()))
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Importa um arquivo de perfil. Recusa substituir um perfil existente,
    /// salvo com `overwrite`.
    ///
    /// # Errors
    /// Arquivo grande demais, inválido, ou id já existente.
    pub fn import(&self, file: &Path, overwrite: bool) -> Result<Profile, ProfileError> {
        let profile = parse_profile(&read_limited(file)?)?;
        if !overwrite && self.path_for(&profile.id)?.exists() {
            return Err(ProfileError::AlreadyExists(profile.id));
        }
        self.save(&profile)?;
        Ok(profile)
    }

    /// Exporta um perfil para um arquivo (sem credenciais: o perfil não as tem).
    ///
    /// # Errors
    /// Perfil inexistente ou erro de E/S.
    pub fn export(&self, id: &str, file: &Path) -> Result<(), ProfileError> {
        let profile = self.load(id)?;
        fs::write(file, to_json(&profile)?)?;
        Ok(())
    }

    fn ensure_dir(&self) -> Result<(), ProfileError> {
        match fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.dir)
        {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.into()),
        }
        let mode = fs::metadata(&self.dir)?.permissions().mode() & 0o777;
        if mode != 0o700 {
            fs::set_permissions(&self.dir, fs::Permissions::from_mode(0o700))?;
        }
        Ok(())
    }
}

fn read_limited(path: &Path) -> Result<String, ProfileError> {
    let meta = fs::metadata(path)?;
    if meta.len() > MAX_PROFILE_BYTES {
        return Err(ProfileError::TooLarge);
    }
    Ok(fs::read_to_string(path)?)
}
