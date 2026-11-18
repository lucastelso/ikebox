//! Reading, saving, importing and exporting profiles.
//!
//! Profiles live in `$XDG_CONFIG_HOME/ikebox/profiles/<id>.json`
//! (directory 0700, files 0600: they contain the organization's gateway and
//! group id). Saving is atomic (temporary file + rename).

use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use super::ProfileError;
use super::model::Profile;
use super::schema::{from_json, to_json, v1_field};
use super::validate::{is_slug, validate};

/// Maximum size of an imported profile file.
pub const MAX_PROFILE_BYTES: u64 = 1024 * 1024;

/// Reads a profile from JSON (format 1 or 2): checks the format version before
/// the rest, converts and validates. In a format 1 file, the problems name its
/// fields (`auth.local_id`, not `auth.local[0].id`).
///
/// # Errors
/// Invalid JSON, unsupported version or invalid profile.
pub fn parse_profile(json: &str) -> Result<Profile, ProfileError> {
    let (profile, version) = from_json(json)?;
    let mut issues = validate(&profile);
    if issues.is_empty() {
        return Ok(profile);
    }
    if version == 1 {
        for issue in &mut issues {
            issue.field = v1_field(&issue.field);
        }
    }
    Err(ProfileError::Invalid(issues))
}

/// Profile directory on disk.
#[derive(Debug, Clone)]
pub struct ProfileStore {
    dir: PathBuf,
}

/// Listing result: valid profiles and broken files.
#[derive(Debug, Default)]
pub struct ProfileList {
    /// Valid profiles, sorted by name.
    pub profiles: Vec<Profile>,
    /// Ignored files and the reason.
    pub broken: Vec<(PathBuf, String)>,
}

impl ProfileStore {
    /// Default directory: `$XDG_CONFIG_HOME/ikebox/profiles` or
    /// `~/.config/ikebox/profiles`.
    ///
    /// # Errors
    /// [`ProfileError::NoConfigDir`] with neither `XDG_CONFIG_HOME` nor `HOME`.
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
        Ok(base.join("ikebox").join("profiles"))
    }

    /// Store in a specific directory.
    #[must_use]
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// Store directory.
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

    /// Lists the profiles (broken files go to `broken`).
    ///
    /// # Errors
    /// I/O error reading the directory (a missing directory = empty list).
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

    /// Loads a profile by id.
    ///
    /// # Errors
    /// [`ProfileError::NotFound`] or a read or validation error.
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

    /// Saves (creates or replaces) a profile, atomically.
    ///
    /// # Errors
    /// Invalid profile or I/O error.
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

    /// Removes a profile.
    ///
    /// # Errors
    /// [`ProfileError::NotFound`] or I/O error.
    pub fn delete(&self, id: &str) -> Result<(), ProfileError> {
        match fs::remove_file(self.path_for(id)?) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(ProfileError::NotFound(id.to_owned()))
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Imports a profile file. Refuses to replace an existing profile, unless
    /// `overwrite` is set.
    ///
    /// # Errors
    /// File too large, invalid, or an id that already exists.
    pub fn import(&self, file: &Path, overwrite: bool) -> Result<Profile, ProfileError> {
        let profile = parse_profile(&read_limited(file)?)?;
        if !overwrite && self.path_for(&profile.id)?.exists() {
            return Err(ProfileError::AlreadyExists(profile.id));
        }
        self.save(&profile)?;
        Ok(profile)
    }

    /// Exports a profile to a file (no credentials: the profile has none).
    ///
    /// # Errors
    /// Missing profile or I/O error.
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

/// Reads a file that should be a profile (or a `swanctl.conf` to import): a
/// regular file of at most [`MAX_PROFILE_BYTES`].
///
/// The size is enforced while reading, not taken from the file's metadata: a
/// device or a pipe reports size zero and never ends (`/dev/zero` picked in
/// the import dialog would fill the memory), and a file can grow after being
/// measured.
///
/// # Errors
/// Not a regular file, too large, unreadable or not UTF-8.
pub fn read_profile_file(path: &Path) -> Result<String, ProfileError> {
    // Checked before opening too: opening a pipe would block.
    let not_a_file =
        || std::io::Error::other(crate::tr!("not a regular file", "não é um arquivo comum"));
    if !fs::metadata(path)?.is_file() {
        return Err(not_a_file().into());
    }
    let file = fs::File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(not_a_file().into());
    }
    let mut text = String::new();
    file.take(MAX_PROFILE_BYTES + 1).read_to_string(&mut text)?;
    if text.len() as u64 > MAX_PROFILE_BYTES {
        return Err(ProfileError::TooLarge);
    }
    Ok(text)
}

fn read_limited(path: &Path) -> Result<String, ProfileError> {
    read_profile_file(path)
}
