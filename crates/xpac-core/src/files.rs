//! Declared user configuration files (ADR 013): opaque owned content, never
//! configuration semantics. Targets are written only through a locked registry
//! guard, after review, and only when they still match their planned state.
use crate::Error;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
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
/// Receipts may record any adopted permission mode, never special bits.
fn receipt_mode_bits(mode: &str) -> Result<u32, Error> {
    match u32::from_str_radix(mode, 8) {
        Ok(bits) if mode.len() == 4 && bits <= 0o777 => Ok(bits),
        _ => Err(Error::Safety("file receipt mode must be 0000-0777".into())),
    }
}
fn mode_text(bits: u32) -> String {
    format!("{bits:04o}")
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
            receipt_mode_bits(&receipt.mode)?;
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

/// Private receipts of files written or adopted by xpac, usually
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

pub struct FileGuard<'a> {
    registry: &'a FileRegistry,
    pub state: FileRegistryState,
    lock: fs::File,
}
impl FileRegistry {
    fn create_private_dir(path: &Path) -> Result<(), Error> {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(path).map_err(io)?;
        private(&fs::symlink_metadata(path).map_err(io)?, true)
    }
    /// Serialize xpac writers. A lock left by a crash is never auto-recovered.
    pub fn lock(&self, expected: &FileRegistryState) -> Result<FileGuard<'_>, Error> {
        expected.validate()?;
        Self::create_private_dir(&self.directory)?;
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock = options.open(self.directory.join("lock")).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                Error::Safety("file registry is locked; another operation is running or interrupted; inspect before manual recovery".into())
            } else {
                io(e)
            }
        })?;
        let guard = FileGuard {
            registry: self,
            state: expected.clone(),
            lock,
        };
        guard.validate()?;
        Ok(guard)
    }
}
impl FileGuard<'_> {
    pub fn validate(&self) -> Result<(), Error> {
        private(&self.lock.metadata().map_err(io)?, false)?;
        if self.registry.read()? != self.state {
            return Err(Error::Safety(
                "file registry changed since review; no file written".into(),
            ));
        }
        Ok(())
    }
    fn record(&mut self, receipt: FileReceipt) -> Result<(), Error> {
        let mut next = self.state.clone();
        next.files.retain(|r| r.target != receipt.target);
        next.files.push(receipt);
        next.files.sort_by(|a, b| a.target.cmp(&b.target));
        self.write(next)
    }
    /// Drop a receipt; the target file is never touched.
    pub fn forget(&mut self, target: &str) -> Result<(), Error> {
        self.validate()?;
        validate_target(target)?;
        let mut next = self.state.clone();
        if !next.files.iter().any(|r| r.target == target) {
            return Err(Error::InvalidInput(format!("no receipt for {target}")));
        }
        next.files.retain(|r| r.target != target);
        self.write(next)
    }
    /// Content-addressed private backups are never deleted automatically.
    fn backup(&self, sha256: &str, bytes: &[u8]) -> Result<(), Error> {
        let directory = self.registry.directory.join("backups");
        FileRegistry::create_private_dir(&directory)?;
        let path = directory.join(sha256);
        if let Ok(metadata) = fs::symlink_metadata(&path) {
            private(&metadata, false)?;
            if read_bounded(&path, SOURCE_LIMIT)?.is_some_and(|b| sha256_hex(&b) == sha256) {
                return Ok(());
            }
            return Err(Error::Safety(
                "existing backup does not match its digest".into(),
            ));
        }
        let mut temp = tempfile::NamedTempFile::new_in(&directory).map_err(io)?;
        temp.write_all(bytes).map_err(io)?;
        temp.as_file().sync_all().map_err(io)?;
        temp.persist_noclobber(&path).map_err(|e| io(e.error))?;
        fs::File::open(&directory)
            .and_then(|dir| dir.sync_all())
            .map_err(io)
    }
    fn write(&mut self, next: FileRegistryState) -> Result<(), Error> {
        next.validate()?;
        let bytes = serde_json::to_vec_pretty(&next)
            .map_err(|_| Error::Parse("cannot serialize file registry".into()))?;
        if bytes.len() as u64 > SOURCE_LIMIT {
            return Err(Error::Safety("file registry exceeds 1 MiB".into()));
        }
        let mut temp = tempfile::NamedTempFile::new_in(&self.registry.directory).map_err(io)?;
        temp.write_all(&bytes).map_err(io)?;
        temp.as_file().sync_all().map_err(io)?;
        temp.persist(self.registry.directory.join("registry.json"))
            .map_err(|e| io(e.error))?;
        fs::File::open(&self.registry.directory)
            .and_then(|dir| dir.sync_all())
            .map_err(io)?;
        self.state = next;
        Ok(())
    }
}
impl Drop for FileGuard<'_> {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.registry.directory.join("lock"));
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
impl FileState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::InSync => "in_sync",
            Self::Outdated => "outdated",
            Self::Drifted => "drifted",
            Self::UnmanagedConflict => "unmanaged_conflict",
            Self::Unsupported => "unsupported",
            Self::Undeclared => "undeclared",
        }
    }
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
/// What a target currently is, observed without following symlinks.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Inspection {
    Absent,
    Regular {
        sha256: String,
        mode: u32,
        bytes: u64,
    },
    Unsupported(&'static str),
}
fn target_path(home: &Path, target: &str) -> Result<PathBuf, Error> {
    validate_target(target)?;
    let path = home.join(target.strip_prefix("~/").expect("validated target"));
    if path.strip_prefix(home).map_or(true, |rest| {
        rest.components()
            .any(|c| !matches!(c, Component::Normal(_)))
    }) {
        return Err(Error::Safety("file target escapes home".into()));
    }
    Ok(path)
}
fn inspect(home: &Path, target: &str) -> Result<Inspection, Error> {
    let path = target_path(home, target)?;
    let relative = path.strip_prefix(home).expect("checked target").to_owned();
    let mut ancestor = home.to_path_buf();
    let parts: Vec<_> = relative.components().collect();
    for part in &parts[..parts.len() - 1] {
        ancestor.push(part);
        match fs::symlink_metadata(&ancestor) {
            Ok(m) if m.file_type().is_symlink() || !m.is_dir() => {
                return Ok(Inspection::Unsupported(
                    "symlinked or non-directory ancestor",
                ))
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Inspection::Absent),
            Err(e) => return Err(io(e)),
        }
    }
    let metadata = match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Inspection::Absent),
        other => other.map_err(io)?,
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Ok(Inspection::Unsupported("target is not a regular file"));
    }
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::MetadataExt;
        // SAFETY: geteuid has no pointer arguments.
        if metadata.uid() != unsafe { libc::geteuid() } {
            return Ok(Inspection::Unsupported("target belongs to another user"));
        }
        metadata.mode() & 0o7777
    };
    #[cfg(not(unix))]
    let mode = 0;
    if mode & !0o777 != 0 {
        return Ok(Inspection::Unsupported("target has special mode bits"));
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(&path).map_err(io)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 65536];
    let mut bytes = 0;
    loop {
        let read = file.read(&mut buffer).map_err(io)?;
        if read == 0 {
            break;
        }
        bytes += read as u64;
        hasher.update(&buffer[..read]);
    }
    Ok(Inspection::Regular {
        sha256: hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
        mode,
        bytes,
    })
}
fn classify(
    home: &Path,
    source: &LoadedSource,
    receipt: Option<&FileReceipt>,
) -> Result<(FileState, Option<&'static str>), Error> {
    let (current, mode) = match inspect(home, &source.target)? {
        Inspection::Absent => return Ok((FileState::Missing, None)),
        Inspection::Unsupported(reason) => return Ok((FileState::Unsupported, Some(reason))),
        Inspection::Regular { sha256, mode, .. } => (sha256, mode),
    };
    let Some(receipt) = receipt else {
        // Equal bytes do not prove the user delegated this file to xpac.
        return Ok((
            FileState::UnmanagedConflict,
            Some("existing file has no receipt; adopt it explicitly"),
        ));
    };
    if current == source.sha256 && mode == source.mode {
        return Ok((FileState::InSync, None));
    }
    if current == receipt.sha256 && mode == receipt_mode_bits(&receipt.mode)? {
        return Ok((FileState::Outdated, None));
    }
    Ok((
        FileState::Drifted,
        Some("changed since xpac last wrote it; copy the edits into the source or re-adopt to discard them"),
    ))
}

fn not_root() -> Result<(), Error> {
    #[cfg(unix)]
    // SAFETY: geteuid returns a scalar process identifier.
    if unsafe { libc::geteuid() } == 0 {
        return Err(Error::Safety(
            "managed files are written as a regular user, never as root".into(),
        ));
    }
    Ok(())
}
fn read_bounded(path: &Path, limit: u64) -> Result<Option<Vec<u8>>, Error> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut bytes = vec![];
    options
        .open(path)
        .map_err(io)?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    Ok((bytes.len() as u64 <= limit).then_some(bytes))
}
fn preview(previous: Option<&[u8]>, next: &[u8]) -> String {
    match (previous.map(std::str::from_utf8), std::str::from_utf8(next)) {
        (None, Ok(next)) => similar::TextDiff::from_lines("", next)
            .unified_diff()
            .header("/dev/null", "declared")
            .to_string(),
        (Some(Ok(previous)), Ok(next)) => similar::TextDiff::from_lines(previous, next)
            .unified_diff()
            .header("current", "declared")
            .to_string(),
        _ => format!(
            "binary or non-UTF-8 content: {} bytes, sha256 {}",
            next.len(),
            sha256_hex(next)
        ),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WriteAction {
    Create,
    Update,
}
/// One reviewed write, bound to the source digest and the target's planned state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlannedWrite {
    pub target: String,
    pub action: WriteAction,
    pub mode: String,
    pub source_sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_mode: Option<String>,
    pub diff: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FilePlan {
    pub files: Vec<FileStatus>,
    pub writes: Vec<PlannedWrite>,
    pub warnings: Vec<String>,
}
/// Propose writes for missing/outdated targets; any target needing a user decision
/// fails the whole plan with its name.
pub fn plan(
    home: &Path,
    sources: &[LoadedSource],
    registry: &FileRegistryState,
) -> Result<FilePlan, Error> {
    let files = observe(home, sources, registry)?;
    let blocked: Vec<_> = files
        .iter()
        .filter(|f| {
            matches!(
                f.state,
                FileState::Drifted | FileState::UnmanagedConflict | FileState::Unsupported
            )
        })
        .map(|f| format!("{} ({})", f.target, f.state.as_str()))
        .collect();
    if !blocked.is_empty() {
        return Err(Error::Safety(format!(
            "file targets need a decision before apply (see xpac diff; adopt or fix them): {}",
            blocked.join(", ")
        )));
    }
    let mut writes = vec![];
    for status in &files {
        if !matches!(status.state, FileState::Missing | FileState::Outdated) {
            continue;
        }
        let source = sources
            .iter()
            .find(|s| s.target == status.target)
            .expect("observed declared target");
        let (previous, previous_bytes) = match inspect(home, &source.target)? {
            Inspection::Regular { sha256, mode, .. } => {
                let path = target_path(home, &source.target)?;
                (Some((sha256, mode)), read_bounded(&path, SOURCE_LIMIT)?)
            }
            _ => (None, None),
        };
        let diff = match (&previous, &previous_bytes) {
            (Some(_), None) => format!(
                "current file exceeds 1 MiB; declared content: {} bytes, sha256 {}",
                source.bytes.len(),
                source.sha256
            ),
            _ => preview(previous_bytes.as_deref(), &source.bytes),
        };
        writes.push(PlannedWrite {
            target: source.target.clone(),
            action: if previous.is_some() {
                WriteAction::Update
            } else {
                WriteAction::Create
            },
            mode: mode_text(source.mode),
            source_sha256: source.sha256.clone(),
            previous_mode: previous.as_ref().map(|(_, mode)| mode_text(*mode)),
            previous_sha256: previous.map(|(sha, _)| sha),
            diff,
        });
    }
    let mut warnings = vec![
        "File previews display contents; do not store secrets in manifests.".into(),
        "Reload actions are never run (e.g. tmux source-file ~/.tmux.conf).".into(),
    ];
    if writes.is_empty() {
        warnings.push("All declared files are in sync.".into());
    }
    Ok(FilePlan {
        files,
        writes,
        warnings,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WriteResult {
    pub target: String,
    pub action: WriteAction,
    pub verified: bool,
}
/// Create missing ancestors below home as private real directories.
fn create_parents(home: &Path, path: &Path) -> Result<(), Error> {
    let relative = path.strip_prefix(home).expect("checked target");
    let parts: Vec<_> = relative.components().collect();
    let mut ancestor = home.to_path_buf();
    for part in &parts[..parts.len() - 1] {
        ancestor.push(part);
        if let Err(e) = fs::symlink_metadata(&ancestor) {
            if e.kind() != std::io::ErrorKind::NotFound {
                return Err(io(e));
            }
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder.create(&ancestor).map_err(io)?;
        }
        let metadata = fs::symlink_metadata(&ancestor).map_err(io)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(Error::Safety(format!(
                "{} has a symlinked or non-directory ancestor",
                path.display()
            )));
        }
    }
    Ok(())
}
fn write_one(home: &Path, source: &LoadedSource, write: &PlannedWrite) -> Result<(), Error> {
    let path = target_path(home, &write.target)?;
    let expected = match (&write.previous_sha256, &write.previous_mode) {
        (Some(sha256), Some(mode)) => Some((sha256.clone(), receipt_mode_bits(mode)?)),
        _ => None,
    };
    let current = match inspect(home, &write.target)? {
        Inspection::Absent => None,
        Inspection::Regular { sha256, mode, .. } => Some((sha256, mode)),
        Inspection::Unsupported(reason) => {
            return Err(Error::Safety(format!("{}: {reason}", write.target)))
        }
    };
    if current != expected {
        return Err(Error::Safety(format!(
            "{} changed since review; no file written",
            write.target
        )));
    }
    create_parents(home, &path)?;
    let parent = path.parent().expect("target below home");
    let mut temp = tempfile::Builder::new()
        .prefix(".xpac-")
        .tempfile_in(parent)
        .map_err(io)?;
    temp.write_all(&source.bytes).map_err(io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temp.as_file()
            .set_permissions(fs::Permissions::from_mode(source.mode))
            .map_err(io)?;
    }
    temp.as_file().sync_all().map_err(io)?;
    // Renaming replaces a racing symlink itself rather than following it.
    if expected.is_none() {
        temp.persist_noclobber(&path).map_err(|e| io(e.error))?;
    } else {
        temp.persist(&path).map_err(|e| io(e.error))?;
    }
    fs::File::open(parent)
        .and_then(|dir| dir.sync_all())
        .map_err(io)
}
/// Execute reviewed writes in order, stopping at the first failure. Each target and
/// source is checked again immediately before writing; receipts follow each write.
pub fn apply(
    home: &Path,
    sources: &[LoadedSource],
    plan: &FilePlan,
    guard: &mut FileGuard<'_>,
) -> Result<Vec<WriteResult>, Error> {
    not_root()?;
    let mut results = vec![];
    for write in &plan.writes {
        guard.validate()?;
        let source = sources
            .iter()
            .find(|s| s.target == write.target && s.sha256 == write.source_sha256)
            .ok_or_else(|| {
                Error::Safety(format!(
                    "source for {} changed since review; no file written",
                    write.target
                ))
            })?;
        write_one(home, source, write)?;
        let verified = inspect(home, &write.target)?
            == Inspection::Regular {
                sha256: source.sha256.clone(),
                mode: source.mode,
                bytes: source.bytes.len() as u64,
            };
        guard.record(FileReceipt {
            target: write.target.clone(),
            sha256: source.sha256.clone(),
            bytes: source.bytes.len() as u64,
            mode: mode_text(source.mode),
            source_sha256: source.sha256.clone(),
        })?;
        results.push(WriteResult {
            target: write.target.clone(),
            action: write.action,
            verified,
        });
        if !verified {
            return Err(Error::Safety(format!(
                "{} did not verify after writing; later files were not written",
                write.target
            )));
        }
    }
    Ok(results)
}

/// What adoption would record: the current content becomes the receipt baseline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AdoptionPreview {
    pub target: String,
    pub sha256: String,
    pub bytes: u64,
    pub mode: String,
    pub replaces_receipt: bool,
    pub warning: String,
}
pub fn preview_adoption(
    home: &Path,
    target: &str,
    registry: &FileRegistryState,
) -> Result<AdoptionPreview, Error> {
    registry.validate()?;
    match inspect(home, target)? {
        Inspection::Regular { sha256, mode, bytes } if bytes <= SOURCE_LIMIT => Ok(AdoptionPreview {
            target: target.into(),
            sha256,
            bytes,
            mode: mode_text(mode),
            replaces_receipt: registry.files.iter().any(|r| r.target == target),
            warning: "The current content is backed up privately and becomes xpac-managed: the next apply may replace it with the declared source.".into(),
        }),
        Inspection::Regular { .. } => Err(Error::Safety(format!("{target} exceeds 1 MiB"))),
        Inspection::Absent => Err(Error::InvalidInput(format!("{target} does not exist"))),
        Inspection::Unsupported(reason) => Err(Error::Safety(format!("{target}: {reason}"))),
    }
}
/// Back up the reviewed content, then record it as the receipt baseline.
pub fn adopt(
    home: &Path,
    preview: &AdoptionPreview,
    guard: &mut FileGuard<'_>,
) -> Result<FileReceipt, Error> {
    not_root()?;
    guard.validate()?;
    let path = target_path(home, &preview.target)?;
    let bytes = read_bounded(&path, SOURCE_LIMIT)?
        .ok_or_else(|| Error::Safety(format!("{} exceeds 1 MiB", preview.target)))?;
    let current = inspect(home, &preview.target)?;
    if sha256_hex(&bytes) != preview.sha256
        || current
            != (Inspection::Regular {
                sha256: preview.sha256.clone(),
                mode: receipt_mode_bits(&preview.mode)?,
                bytes: preview.bytes,
            })
    {
        return Err(Error::Safety(format!(
            "{} changed since review; nothing adopted",
            preview.target
        )));
    }
    guard.backup(&preview.sha256, &bytes)?;
    let receipt = FileReceipt {
        target: preview.target.clone(),
        sha256: preview.sha256.clone(),
        bytes: preview.bytes,
        mode: preview.mode.clone(),
        source_sha256: preview.sha256.clone(),
    };
    guard.record(receipt.clone())?;
    Ok(receipt)
}

/// What removal would delete: only a file still identical to its receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RemovalPreview {
    pub target: String,
    pub sha256: String,
    pub bytes: u64,
    pub mode: String,
    pub warning: String,
}
pub fn preview_removal(
    home: &Path,
    target: &str,
    registry: &FileRegistryState,
) -> Result<RemovalPreview, Error> {
    registry.validate()?;
    let receipt = registry
        .files
        .iter()
        .find(|r| r.target == target)
        .ok_or_else(|| {
            Error::Safety(format!(
                "{target} has no receipt; xpac never deletes unmanaged files"
            ))
        })?;
    match inspect(home, target)? {
        Inspection::Regular { sha256, mode, bytes }
            if sha256 == receipt.sha256 && mode == receipt_mode_bits(&receipt.mode)? =>
        {
            Ok(RemovalPreview {
                target: target.into(),
                sha256,
                bytes,
                mode: mode_text(mode),
                warning: "The file is backed up privately, deleted, and its receipt dropped. Keep or remove its manifest declaration, or the next apply recreates it.".into(),
            })
        }
        Inspection::Regular { .. } => Err(Error::Safety(format!(
            "{target} changed since xpac last wrote it; drifted files are never deleted"
        ))),
        Inspection::Absent => Err(Error::InvalidInput(format!(
            "{target} is already absent; use xpac file forget to drop its receipt"
        ))),
        Inspection::Unsupported(reason) => Err(Error::Safety(format!("{target}: {reason}"))),
    }
}
/// Back up and unlink a reviewed managed file, then drop its receipt.
pub fn remove(
    home: &Path,
    preview: &RemovalPreview,
    guard: &mut FileGuard<'_>,
) -> Result<(), Error> {
    not_root()?;
    guard.validate()?;
    let path = target_path(home, &preview.target)?;
    if inspect(home, &preview.target)?
        != (Inspection::Regular {
            sha256: preview.sha256.clone(),
            mode: receipt_mode_bits(&preview.mode)?,
            bytes: preview.bytes,
        })
    {
        return Err(Error::Safety(format!(
            "{} changed since review; nothing removed",
            preview.target
        )));
    }
    // Hash through one no-follow descriptor and require the path to still name it.
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(&path).map_err(io)?;
    let mut bytes = vec![];
    (&mut file)
        .take(SOURCE_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    if sha256_hex(&bytes) != preview.sha256 {
        return Err(Error::Safety(format!(
            "{} changed since review; nothing removed",
            preview.target
        )));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let opened = file.metadata().map_err(io)?;
        let current = fs::symlink_metadata(&path).map_err(io)?;
        if (opened.dev(), opened.ino()) != (current.dev(), current.ino()) {
            return Err(Error::Safety(format!(
                "{} was replaced since review; nothing removed",
                preview.target
            )));
        }
    }
    guard.backup(&preview.sha256, &bytes)?;
    fs::remove_file(&path).map_err(io)?;
    fs::File::open(path.parent().expect("target below home"))
        .and_then(|dir| dir.sync_all())
        .map_err(io)?;
    guard.forget(&preview.target)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptState {
    /// Content and mode still equal what xpac wrote or adopted.
    Intact,
    Drifted,
    Missing,
    Unsupported,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReceiptStatus {
    #[serde(flatten)]
    pub receipt: FileReceipt,
    pub state: ReceiptState,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReceiptAudit {
    pub schema_version: u32,
    pub files: Vec<ReceiptStatus>,
}
/// Compare every receipt with its target, without a manifest and without mutation.
pub fn audit_receipts(home: &Path, registry: &FileRegistryState) -> Result<ReceiptAudit, Error> {
    registry.validate()?;
    let files = registry
        .files
        .iter()
        .map(|receipt| {
            let state = match inspect(home, &receipt.target)? {
                Inspection::Regular { sha256, mode, .. }
                    if sha256 == receipt.sha256 && mode == receipt_mode_bits(&receipt.mode)? =>
                {
                    ReceiptState::Intact
                }
                Inspection::Regular { .. } => ReceiptState::Drifted,
                Inspection::Absent => ReceiptState::Missing,
                Inspection::Unsupported(_) => ReceiptState::Unsupported,
            };
            Ok(ReceiptStatus {
                receipt: receipt.clone(),
                state,
            })
        })
        .collect::<Result<_, Error>>()?;
    Ok(ReceiptAudit {
        schema_version: registry.schema_version,
        files,
    })
}
