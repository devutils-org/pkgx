//! Declared user configuration files (ADR 013): opaque owned content, never
//! configuration semantics. This module only observes; it never writes targets.
use crate::Error;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
};

pub const SOURCE_LIMIT: u64 = 1024 * 1024;
pub const TOTAL_LIMIT: u64 = 16 * 1024 * 1024;
const ALLOWED_MODES: [&str; 4] = ["0600", "0644", "0700", "0755"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileDeclaration {
    /// Relative to the manifest directory.
    pub source: String,
    #[serde(default = "default_mode")]
    pub mode: String,
}
fn default_mode() -> String {
    "0600".into()
}
fn mode_bits(mode: &str) -> Result<u32, Error> {
    if !ALLOWED_MODES.contains(&mode) {
        return Err(Error::InvalidInput(
            "file mode must be one of 0600, 0644, 0700 or 0755".into(),
        ));
    }
    Ok(u32::from_str_radix(mode, 8).expect("allowed octal mode"))
}
fn normal_components(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 4096
        && !path.chars().any(char::is_control)
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}
/// Targets are `~/`-relative, lexically normal paths below HOME.
pub fn validate_target(target: &str) -> Result<(), Error> {
    if !target.strip_prefix("~/").is_some_and(normal_components) {
        return Err(Error::InvalidInput(
            "file targets must be normal ~/ paths without ., .., empty components or a trailing slash; system paths are out of scope".into(),
        ));
    }
    Ok(())
}
pub fn validate_declarations(files: &BTreeMap<String, FileDeclaration>) -> Result<(), Error> {
    for (target, declaration) in files {
        validate_target(target)?;
        if !normal_components(&declaration.source) {
            return Err(Error::InvalidInput(
                "file sources must be relative paths inside the manifest directory".into(),
            ));
        }
        mode_bits(&declaration.mode)?;
    }
    Ok(())
}
/// Layers never override: an identical declaration collapses, a different one fails.
pub fn merge(
    target: &mut BTreeMap<String, FileDeclaration>,
    layer: &BTreeMap<String, FileDeclaration>,
) -> Result<(), Error> {
    for (path, declaration) in layer {
        match target.get(path) {
            Some(existing) if existing != declaration => {
                return Err(Error::InvalidInput(format!(
                    "conflicting declarations for {path} across manifest layers"
                )))
            }
            Some(_) => {}
            None => {
                target.insert(path.clone(), declaration.clone());
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileReceipt {
    pub target: String,
    pub sha256: String,
    pub bytes: u64,
    pub mode: String,
    pub source_sha256: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileRegistryState {
    pub schema_version: u32,
    pub files: Vec<FileReceipt>,
}
impl Default for FileRegistryState {
    fn default() -> Self {
        Self {
            schema_version: 1,
            files: vec![],
        }
    }
}
fn hex_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}
impl FileRegistryState {
    pub fn validate(&self) -> Result<(), Error> {
        if self.schema_version != 1 {
            return Err(Error::Unsupported("file registry schema version".into()));
        }
        let mut targets = BTreeSet::new();
        for receipt in &self.files {
            validate_target(&receipt.target)?;
            mode_bits(&receipt.mode)?;
            if !hex_digest(&receipt.sha256)
                || !hex_digest(&receipt.source_sha256)
                || !targets.insert(&receipt.target)
            {
                return Err(Error::Safety(
                    "file receipts require unique targets and lowercase SHA-256 digests".into(),
                ));
            }
        }
        Ok(())
    }
}

fn io(source: std::io::Error) -> Error {
    Error::Io {
        program: "managed files".into(),
        source,
    }
}
fn private(metadata: &fs::Metadata, directory: bool) -> Result<(), Error> {
    if metadata.file_type().is_symlink()
        || if directory {
            !metadata.is_dir()
        } else {
            !metadata.is_file()
        }
    {
        return Err(Error::Safety(
            "file registry requires regular files and real directories".into(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // SAFETY: geteuid has no pointer arguments.
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
            return Err(Error::Safety("file registry must belong to the current user and be private (0700 directory, 0600 files)".into()));
        }
    }
    Ok(())
}

/// Private receipts of files written or adopted by pkgx, usually
/// <transaction-directory>/files.
pub struct FileRegistry {
    directory: PathBuf,
}
impl FileRegistry {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }
    /// Missing state is empty; inspection never creates directories.
    pub fn read(&self) -> Result<FileRegistryState, Error> {
        match fs::symlink_metadata(&self.directory) {
            Ok(metadata) => private(&metadata, true)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(FileRegistryState::default())
            }
            Err(e) => return Err(io(e)),
        }
        let path = self.directory.join("registry.json");
        match fs::symlink_metadata(&path) {
            Ok(metadata) => private(&metadata, false)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(FileRegistryState::default())
            }
            Err(e) => return Err(io(e)),
        }
        let mut bytes = vec![];
        fs::File::open(&path)
            .map_err(io)?
            .take(SOURCE_LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(io)?;
        if bytes.len() as u64 > SOURCE_LIMIT {
            return Err(Error::Safety("file registry exceeds 1 MiB".into()));
        }
        let state: FileRegistryState = serde_json::from_slice(&bytes)
            .map_err(|_| Error::Parse("malformed file registry".into()))?;
        state.validate()?;
        Ok(state)
    }
}

/// A declared source read once from the manifest directory; its digest binds review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedSource {
    pub target: String,
    pub mode: u32,
    pub sha256: String,
    pub bytes: Vec<u8>,
}
fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
/// Read every source without following symlinks or leaving `manifest_dir`.
pub fn load_sources(
    manifest_dir: &Path,
    files: &BTreeMap<String, FileDeclaration>,
) -> Result<Vec<LoadedSource>, Error> {
    validate_declarations(files)?;
    let base = fs::canonicalize(manifest_dir).map_err(io)?;
    let mut total = 0;
    let mut loaded = vec![];
    for (target, declaration) in files {
        let refused = |what: &str| {
            Error::Safety(format!("source {} for {target} {what}", declaration.source))
        };
        let mut path = base.clone();
        let parts: Vec<_> = declaration.source.split('/').collect();
        for (index, part) in parts.iter().enumerate() {
            path.push(part);
            let metadata = match fs::symlink_metadata(&path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Err(refused("does not exist"))
                }
                other => other.map_err(io)?,
            };
            let last = index + 1 == parts.len();
            if metadata.file_type().is_symlink()
                || (last && !metadata.is_file())
                || (!last && !metadata.is_dir())
            {
                return Err(refused(
                    "must be a regular file below the manifest directory without symlinks",
                ));
            }
        }
        let mut options = fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        let file = options.open(&path).map_err(io)?;
        if !file.metadata().map_err(io)?.is_file() {
            return Err(refused("must be a regular file"));
        }
        let mut bytes = vec![];
        file.take(SOURCE_LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(io)?;
        if bytes.len() as u64 > SOURCE_LIMIT {
            return Err(refused("exceeds 1 MiB"));
        }
        total += bytes.len() as u64;
        if total > TOTAL_LIMIT {
            return Err(Error::Safety(
                "declared file sources exceed 16 MiB in total".into(),
            ));
        }
        loaded.push(LoadedSource {
            target: target.clone(),
            mode: mode_bits(&declaration.mode)?,
            sha256: sha256_hex(&bytes),
            bytes,
        });
    }
    Ok(loaded)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileState {
    Missing,
    InSync,
    Outdated,
    Drifted,
    UnmanagedConflict,
    Unsupported,
    /// A receipt whose target is no longer declared; it is retained, not removed.
    Undeclared,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileStatus {
    pub target: String,
    pub state: FileState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Classify declared targets and undeclared receipts against HOME without mutation.
pub fn observe(
    home: &Path,
    sources: &[LoadedSource],
    registry: &FileRegistryState,
) -> Result<Vec<FileStatus>, Error> {
    registry.validate()?;
    if cfg!(not(unix)) {
        return Err(Error::Unsupported("managed files require Unix".into()));
    }
    if !home.is_absolute() || home.parent().is_none() {
        return Err(Error::InvalidInput(
            "home must be an absolute non-root path".into(),
        ));
    }
    let home_metadata = fs::symlink_metadata(home).map_err(io)?;
    if !home_metadata.is_dir() || home_metadata.file_type().is_symlink() {
        return Err(Error::Safety("home must be a real directory".into()));
    }
    let mut statuses = vec![];
    for source in sources {
        let receipt = registry.files.iter().find(|r| r.target == source.target);
        let (state, reason) = classify(home, source, receipt)?;
        statuses.push(FileStatus {
            target: source.target.clone(),
            state,
            reason: reason.map(Into::into),
        });
    }
    for receipt in &registry.files {
        if !sources.iter().any(|s| s.target == receipt.target) {
            statuses.push(FileStatus {
                target: receipt.target.clone(),
                state: FileState::Undeclared,
                reason: Some("receipt retained; nothing is removed".into()),
            });
        }
    }
    statuses.sort_by(|a, b| a.target.cmp(&b.target));
    Ok(statuses)
}
fn classify(
    home: &Path,
    source: &LoadedSource,
    receipt: Option<&FileReceipt>,
) -> Result<(FileState, Option<&'static str>), Error> {
    validate_target(&source.target)?;
    let relative = source.target.strip_prefix("~/").expect("validated target");
    let path = home.join(relative);
    if path.strip_prefix(home).map_or(true, |rest| {
        rest.components()
            .any(|c| !matches!(c, Component::Normal(_)))
    }) {
        return Err(Error::Safety("file target escapes home".into()));
    }
    let mut ancestor = home.to_path_buf();
    let parts: Vec<_> = relative.split('/').collect();
    for part in &parts[..parts.len() - 1] {
        ancestor.push(part);
        match fs::symlink_metadata(&ancestor) {
            Ok(m) if m.file_type().is_symlink() || !m.is_dir() => {
                return Ok((
                    FileState::Unsupported,
                    Some("symlinked or non-directory ancestor"),
                ))
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok((FileState::Missing, None))
            }
            Err(e) => return Err(io(e)),
        }
    }
    let metadata = match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok((FileState::Missing, None))
        }
        other => other.map_err(io)?,
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Ok((FileState::Unsupported, Some("target is not a regular file")));
    }
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::MetadataExt;
        // SAFETY: geteuid has no pointer arguments.
        if metadata.uid() != unsafe { libc::geteuid() } {
            return Ok((
                FileState::Unsupported,
                Some("target belongs to another user"),
            ));
        }
        metadata.mode() & 0o7777
    };
    #[cfg(not(unix))]
    let mode = 0;
    let mut hasher = Sha256::new();
    let mut file = fs::File::open(&path).map_err(io)?;
    let mut buffer = [0u8; 65536];
    loop {
        let read = file.read(&mut buffer).map_err(io)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let current: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let Some(receipt) = receipt else {
        // Equal bytes do not prove the user delegated this file to pkgx.
        return Ok((
            FileState::UnmanagedConflict,
            Some("existing file has no receipt; adopt it explicitly"),
        ));
    };
    if current == source.sha256 && mode == source.mode {
        return Ok((FileState::InSync, None));
    }
    if current == receipt.sha256 && mode == mode_bits(&receipt.mode)? {
        return Ok((FileState::Outdated, None));
    }
    Ok((
        FileState::Drifted,
        Some("changed since pkgx last wrote it; re-adopt to discard local edits"),
    ))
}
