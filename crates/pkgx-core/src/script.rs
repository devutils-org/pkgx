//! Tracked script installations. A verified artifact runs only after explicit
//! trust confirmation, as the invoking non-root user, through a fixed system
//! interpreter. Receipts record only resources that were absent before the
//! install and present afterwards; pkgx itself never deletes resource paths.
use crate::{
    command::{CommandRunner, CommandSpec},
    recipe::{Action, Interpreter, Recipe, ResourceKind},
    Error,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::atomic::AtomicBool,
    time::Duration,
};

const RECEIPT_LIMIT: u64 = 1024 * 1024;
const ARTIFACT_LIMIT: u64 = 8 * 1024 * 1024;
const SCRIPT_TIMEOUT: Duration = Duration::from_secs(1800);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptStatus {
    /// Intent recorded before execution; a leftover entry means interrupted/unknown.
    InstallStarted,
    Installed,
    InstallIncomplete,
    UninstallStarted,
    UninstallIncomplete,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum Evidence {
    /// Regular file content at install time.
    File {
        sha256: String,
        bytes: u64,
    },
    Link {
        target: String,
    },
    /// Declared but not present after the install; never claimed as owned.
    Missing,
    /// Present but not a regular file or symlink; never claimed as owned.
    Unsupported,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrackedResource {
    pub kind: ResourceKind,
    pub path: PathBuf,
    pub evidence: Evidence,
}
impl TrackedResource {
    /// Created by the recorded install: absent before, observed afterwards.
    pub fn owned(&self) -> bool {
        matches!(self.evidence, Evidence::File { .. } | Evidence::Link { .. })
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub schema_version: u32,
    pub name: String,
    pub pinned_version: String,
    pub source: String,
    pub sha256: String,
    pub interpreter: Interpreter,
    pub install: Action,
    pub uninstall: Action,
    pub status: ReceiptStatus,
    pub updated_unix_secs: u64,
    pub native_status: Option<i32>,
    pub resources: Vec<TrackedResource>,
}
impl Receipt {
    /// `tracked` only when every declared resource was created by the install.
    pub fn classification(&self) -> &'static str {
        if self.status == ReceiptStatus::Installed && self.resources.iter().all(|r| r.owned()) {
            "tracked"
        } else {
            "tracked_incomplete"
        }
    }
}
#[derive(Debug, Serialize)]
pub struct ReceiptSummary {
    pub classification: &'static str,
    #[serde(flatten)]
    pub receipt: Receipt,
}
#[derive(Debug, Serialize)]
pub struct ScriptRecord {
    pub name: String,
    pub success: bool,
    pub native_status: Option<i32>,
    pub status: Option<ReceiptStatus>,
    pub errors: Vec<String>,
    /// Paths deliberately left for manual review (configuration, residuals).
    pub retained: Vec<PathBuf>,
}
#[derive(Debug, Serialize)]
pub struct InstallPreview {
    pub recipe: crate::recipe::RecipeSummary,
    pub resolved_resources: Vec<PathBuf>,
    pub interpreter_path: &'static str,
    pub limitations: Vec<String>,
}

pub fn interpreter_path(interpreter: &Interpreter) -> &'static str {
    match interpreter {
        Interpreter::Sh => "/bin/sh",
        Interpreter::Bash => "/bin/bash",
        Interpreter::Python3 => "/usr/bin/python3",
    }
}
fn io(source: std::io::Error) -> Error {
    Error::Io {
        program: "script receipt store".into(),
        source,
    }
}
fn now() -> Result<u64, Error> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| Error::Safety("system clock predates Unix epoch".into()))?
        .as_secs())
}
fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn not_root() -> Result<(), Error> {
    #[cfg(unix)]
    // SAFETY: geteuid returns a scalar process identifier.
    if unsafe { libc::geteuid() } == 0 {
        return Err(Error::Safety(
            "script recipes never run as root; use a regular user account".into(),
        ));
    }
    Ok(())
}

/// Resolve declared paths inside `home`, refusing escapes and symlinked ancestors.
pub fn resolve_resources(recipe: &Recipe, home: &Path) -> Result<Vec<PathBuf>, Error> {
    recipe.validate()?;
    if !home.is_absolute() || home.parent().is_none() {
        return Err(Error::InvalidInput(
            "home must be an absolute non-root path".into(),
        ));
    }
    let home_metadata = fs::symlink_metadata(home).map_err(io)?;
    if !home_metadata.is_dir() {
        return Err(Error::Safety("home must be a real directory".into()));
    }
    recipe
        .resources
        .iter()
        .map(|resource| {
            let path = home.join(resource.path.strip_prefix("~/").ok_or_else(|| {
                Error::Safety(format!(
                    "{} is not home-relative; installable recipes declare ~/ resources only and system paths are never tracked",
                    resource.path
                ))
            })?);
            let relative = path.strip_prefix(home).map_err(|_| {
                Error::Safety(format!(
                    "{} is outside the user's home; system paths are never tracked",
                    path.display()
                ))
            })?;
            if relative.as_os_str().is_empty()
                || relative
                    .components()
                    .any(|c| !matches!(c, Component::Normal(_)))
            {
                return Err(Error::Safety("resource must be a path below home".into()));
            }
            let mut ancestor = home.to_path_buf();
            let parents: Vec<_> = relative.components().collect();
            for component in &parents[..parents.len() - 1] {
                ancestor.push(component);
                match fs::symlink_metadata(&ancestor) {
                    Ok(m) if m.file_type().is_symlink() || !m.is_dir() => {
                        return Err(Error::Safety(format!(
                            "{} has a symlinked or non-directory ancestor",
                            path.display()
                        )))
                    }
                    Ok(_) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
                    Err(e) => return Err(io(e)),
                }
            }
            Ok(path)
        })
        .collect()
}
fn require_absent(paths: &[PathBuf]) -> Result<(), Error> {
    for path in paths {
        match fs::symlink_metadata(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(io(e)),
            Ok(_) => {
                return Err(Error::Safety(format!(
                    "{} already exists; it cannot be attributed to this recipe",
                    path.display()
                )))
            }
        }
    }
    Ok(())
}
fn observe(path: &Path) -> Result<Option<Evidence>, Error> {
    let metadata = match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io(e)),
        Ok(m) => m,
    };
    Ok(Some(if metadata.file_type().is_symlink() {
        Evidence::Link {
            target: fs::read_link(path)
                .map_err(io)?
                .to_string_lossy()
                .into_owned(),
        }
    } else if metadata.is_file() {
        let mut hasher = Sha256::new();
        let mut file = fs::File::open(path).map_err(io)?;
        let mut buffer = [0u8; 65536];
        loop {
            let read = file.read(&mut buffer).map_err(io)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        Evidence::File {
            sha256: hasher
                .finalize()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
            bytes: metadata.len(),
        }
    } else {
        Evidence::Unsupported
    }))
}

pub struct ReceiptStore {
    directory: PathBuf,
}
impl ReceiptStore {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }
    fn private_dir(path: &Path) -> Result<(), Error> {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(path).map_err(io)?;
        let metadata = fs::symlink_metadata(path).map_err(io)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(Error::Safety(
                "receipt store must be a real directory".into(),
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            // SAFETY: geteuid returns a scalar process identifier.
            if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
                return Err(Error::Safety(
                    "receipt store must be private (mode 0700) and owned by the current user"
                        .into(),
                ));
            }
        }
        Ok(())
    }
    fn prepare(&self) -> Result<(), Error> {
        Self::private_dir(&self.directory)?;
        Self::private_dir(&self.directory.join("receipts"))?;
        Self::private_dir(&self.directory.join("artifacts"))
    }
    fn receipt_path(&self, name: &str) -> Result<PathBuf, Error> {
        if !crate::identity::token_like(name, b"._-") || name.len() < 2 {
            return Err(Error::InvalidInput("invalid recipe name".into()));
        }
        Ok(self.directory.join("receipts").join(format!("{name}.json")))
    }
    fn artifact_path(&self, sha256: &str) -> Result<PathBuf, Error> {
        if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error::InvalidInput("invalid artifact digest".into()));
        }
        Ok(self
            .directory
            .join("artifacts")
            .join(sha256.to_ascii_lowercase()))
    }
    fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, Error> {
        let metadata = fs::symlink_metadata(path).map_err(io)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(Error::Safety("store entry must be a regular file".into()));
        }
        let mut bytes = vec![];
        fs::File::open(path)
            .map_err(io)?
            .take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(io)?;
        if bytes.len() as u64 > limit {
            return Err(Error::Safety("store entry exceeds its size bound".into()));
        }
        Ok(bytes)
    }
    fn write_atomic(&self, target: &Path, bytes: &[u8], create: bool) -> Result<(), Error> {
        let parent = target.parent().expect("store paths have a parent");
        let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(io)?;
        temp.write_all(bytes).map_err(io)?;
        temp.as_file().sync_all().map_err(io)?;
        if create {
            temp.persist_noclobber(target).map_err(|e| io(e.error))?;
        } else {
            temp.persist(target).map_err(|e| io(e.error))?;
        }
        fs::File::open(parent)
            .and_then(|dir| dir.sync_all())
            .map_err(io)
    }
    fn save(&self, receipt: &Receipt, create: bool) -> Result<(), Error> {
        let bytes = serde_json::to_vec_pretty(receipt)
            .map_err(|_| Error::Parse("cannot serialize receipt".into()))?;
        if bytes.len() as u64 > RECEIPT_LIMIT {
            return Err(Error::Safety("receipt exceeds 1 MiB".into()));
        }
        self.write_atomic(&self.receipt_path(&receipt.name)?, &bytes, create)
    }
    pub fn read(&self, name: &str) -> Result<Receipt, Error> {
        let path = self.receipt_path(name)?;
        if !path.exists() {
            return Err(Error::Unavailable(format!(
                "no receipt for {name}; it is unmanaged by pkgx"
            )));
        }
        let receipt: Receipt = serde_json::from_slice(&Self::read_bounded(&path, RECEIPT_LIMIT)?)
            .map_err(|_| Error::Parse("invalid script receipt".into()))?;
        if receipt.schema_version != 1 {
            return Err(Error::Unsupported("script receipt schema version".into()));
        }
        if receipt.name != name {
            return Err(Error::Parse("receipt name does not match filename".into()));
        }
        Ok(receipt)
    }
    pub fn list(&self) -> Result<Vec<ReceiptSummary>, Error> {
        let directory = self.directory.join("receipts");
        let entries = match fs::read_dir(&directory) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(io(e)),
            Ok(entries) => entries,
        };
        let mut names = vec![];
        for entry in entries {
            let path = entry.map_err(io)?.path();
            if let Some(name) = path
                .extension()
                .filter(|e| *e == "json")
                .and_then(|_| path.file_stem())
                .and_then(|s| s.to_str())
            {
                names.push(name.to_owned());
            }
        }
        names.sort();
        names
            .iter()
            .map(|name| {
                let receipt = self.read(name)?;
                Ok(ReceiptSummary {
                    classification: receipt.classification(),
                    receipt,
                })
            })
            .collect()
    }
    /// Drop a receipt (and its unreferenced artifact) without touching any resource.
    pub fn forget(&self, name: &str) -> Result<(), Error> {
        let receipt = self.read(name)?;
        fs::remove_file(self.receipt_path(name)?).map_err(io)?;
        self.release_artifact(&receipt.sha256)
    }
    fn release_artifact(&self, sha256: &str) -> Result<(), Error> {
        if self
            .list()?
            .iter()
            .any(|r| r.receipt.sha256.eq_ignore_ascii_case(sha256))
        {
            return Ok(());
        }
        match fs::remove_file(self.artifact_path(sha256)?) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            other => other.map_err(io),
        }
    }
    /// Return the stored artifact path after re-verifying its digest.
    fn verified_artifact(&self, sha256: &str) -> Result<PathBuf, Error> {
        let path = self.artifact_path(sha256)?;
        let bytes = Self::read_bounded(&path, ARTIFACT_LIMIT)?;
        if !sha256_hex(&bytes).eq_ignore_ascii_case(sha256) {
            return Err(Error::Safety(
                "stored script artifact no longer matches its receipt digest; refusing to execute"
                    .into(),
            ));
        }
        Ok(path)
    }
    fn store_artifact(&self, sha256: &str, bytes: &[u8]) -> Result<PathBuf, Error> {
        if !sha256_hex(bytes).eq_ignore_ascii_case(sha256) {
            return Err(Error::Safety("artifact digest mismatch".into()));
        }
        let path = self.artifact_path(sha256)?;
        if !path.exists() {
            self.write_atomic(&path, bytes, true)?;
        }
        self.verified_artifact(sha256)
    }
}

fn run_action(
    runner: &dyn CommandRunner,
    receipt: &Receipt,
    artifact: &Path,
    action: &Action,
    canceled: &AtomicBool,
) -> (Option<i32>, Vec<String>) {
    let artifact = artifact.to_string_lossy().into_owned();
    let mut spec = CommandSpec::new(interpreter_path(&receipt.interpreter), &[&artifact]);
    spec.args.extend(action.args.iter().cloned());
    spec.env
        .insert("PKGX_RECIPE_NAME".into(), receipt.name.clone());
    spec.env
        .insert("PKGX_RECIPE_VERSION".into(), receipt.pinned_version.clone());
    match runner.run(&spec, SCRIPT_TIMEOUT, canceled) {
        Ok(output) if output.status == Some(0) => (output.status, vec![]),
        Ok(output) => (
            output.status,
            vec![format!(
                "script exited with status {:?}; partial changes may remain",
                output.status
            )],
        ),
        Err(error) => (None, vec![format!("{error}; partial changes may remain")]),
    }
}

/// Validation, scope and pre-existence checks without downloading or executing.
pub fn preview_install(
    recipe: &Recipe,
    store: &ReceiptStore,
    home: &Path,
) -> Result<InstallPreview, Error> {
    if recipe.uninstall.is_none() {
        return Err(Error::Safety(
            "recipe installation requires explicit uninstall instructions".into(),
        ));
    }
    if recipe.resources.is_empty() {
        return Err(Error::Safety(
            "recipe installation requires declared resources to track".into(),
        ));
    }
    let resolved = resolve_resources(recipe, home)?;
    require_absent(&resolved)?;
    if store.receipt_path(&recipe.name)?.exists() {
        return Err(Error::Safety(format!(
            "{} already has a receipt; uninstall or forget it first",
            recipe.name
        )));
    }
    Ok(InstallPreview {
        recipe: recipe.summary()?,
        resolved_resources: resolved,
        interpreter_path: interpreter_path(&recipe.interpreter),
        limitations: vec![
            "The script runs with your user's full permissions; only declared paths are tracked.".into(),
            "Undeclared side effects (other files, services, network changes) are not observed or reversed.".into(),
            "Uninstall runs the recipe's own uninstall action; pkgx never deletes resource paths itself.".into(),
        ],
    })
}

/// Download and verify, persist intent, run the trusted install action and
/// record which declared resources it created.
pub fn install(
    runner: &dyn CommandRunner,
    recipe: &Recipe,
    store: &ReceiptStore,
    home: &Path,
    os: &str,
    confirmed: bool,
    canceled: &AtomicBool,
) -> Result<ScriptRecord, Error> {
    not_root()?;
    let preview = preview_install(recipe, store, home)?;
    if !confirmed {
        return Err(Error::Safety(
            "script execution requires explicit trust confirmation".into(),
        ));
    }
    if canceled.load(std::sync::atomic::Ordering::Relaxed) {
        return Err(Error::Canceled("script installation".into()));
    }
    let bytes = recipe.download_verified(runner, os)?;
    store.prepare()?;
    let sha256 = recipe.sha256.to_ascii_lowercase();
    let artifact = store.store_artifact(&sha256, &bytes)?;
    let declared: Vec<_> = recipe
        .resources
        .iter()
        .zip(&preview.resolved_resources)
        .map(|(resource, path)| TrackedResource {
            kind: resource.kind.clone(),
            path: path.clone(),
            evidence: Evidence::Missing,
        })
        .collect();
    let mut receipt = Receipt {
        schema_version: 1,
        name: recipe.name.clone(),
        pinned_version: recipe.pinned_version.clone(),
        source: recipe.url.clone(),
        sha256,
        interpreter: recipe.interpreter.clone(),
        install: recipe.install.clone(),
        uninstall: recipe.uninstall.clone().expect("checked by preview"),
        status: ReceiptStatus::InstallStarted,
        updated_unix_secs: now()?,
        native_status: None,
        resources: declared,
    };
    store.save(&receipt, true)?;
    // Repeat the attribution precondition immediately before execution.
    if let Err(error) = require_absent(&preview.resolved_resources) {
        store.forget(&receipt.name)?;
        return Err(error);
    }
    let (native_status, mut errors) =
        run_action(runner, &receipt, &artifact, &recipe.install, canceled);
    for resource in &mut receipt.resources {
        resource.evidence = observe(&resource.path)?.unwrap_or(Evidence::Missing);
        if !resource.owned() {
            errors.push(format!(
                "{} was not created as a regular file or symlink; it is not tracked",
                resource.path.display()
            ));
        }
    }
    let success = native_status == Some(0) && errors.is_empty();
    receipt.status = if success {
        ReceiptStatus::Installed
    } else {
        ReceiptStatus::InstallIncomplete
    };
    receipt.native_status = native_status;
    receipt.updated_unix_secs = now()?;
    if let Err(error) = store.save(&receipt, false) {
        errors.push(format!(
            "receipt finalization failed: {error}; the receipt remains install_started"
        ));
    }
    Ok(ScriptRecord {
        name: receipt.name,
        success: success && errors.is_empty(),
        native_status,
        status: Some(receipt.status),
        errors,
        retained: vec![],
    })
}

/// Refuse when any owned resource changed since the install receipt.
pub fn preview_uninstall(store: &ReceiptStore, name: &str) -> Result<Receipt, Error> {
    let receipt = store.read(name)?;
    if matches!(
        receipt.status,
        ReceiptStatus::InstallStarted | ReceiptStatus::UninstallStarted
    ) {
        return Err(Error::Safety(format!(
            "{name} was interrupted ({:?}); ownership is unknown. Inspect manually, then `recipe forget`",
            receipt.status
        )));
    }
    for resource in receipt.resources.iter().filter(|r| r.owned()) {
        if resource.kind == ResourceKind::Configuration {
            continue;
        }
        match observe(&resource.path)? {
            None => {}
            Some(evidence) if evidence == resource.evidence => {}
            Some(_) => {
                return Err(Error::Safety(format!(
                    "{} changed since installation; ownership is uncertain and nothing was run",
                    resource.path.display()
                )))
            }
        }
    }
    store.verified_artifact(&receipt.sha256)?;
    Ok(receipt)
}

/// Run the recorded uninstall action and verify owned resources disappeared.
/// Configuration resources may be retained and are reported for manual review.
pub fn uninstall(
    runner: &dyn CommandRunner,
    store: &ReceiptStore,
    name: &str,
    confirmed: bool,
    canceled: &AtomicBool,
) -> Result<ScriptRecord, Error> {
    not_root()?;
    let mut receipt = preview_uninstall(store, name)?;
    if !confirmed {
        return Err(Error::Safety(
            "script execution requires explicit trust confirmation".into(),
        ));
    }
    if canceled.load(std::sync::atomic::Ordering::Relaxed) {
        return Err(Error::Canceled("script uninstallation".into()));
    }
    store.prepare()?;
    let artifact = store.verified_artifact(&receipt.sha256)?;
    receipt.status = ReceiptStatus::UninstallStarted;
    receipt.updated_unix_secs = now()?;
    store.save(&receipt, false)?;
    let action = receipt.uninstall.clone();
    let (native_status, mut errors) = run_action(runner, &receipt, &artifact, &action, canceled);
    let mut retained = vec![];
    // Only resources the install created are checked; others were never ours.
    for resource in receipt.resources.iter().filter(|r| r.owned()) {
        if observe(&resource.path)?.is_some() {
            if resource.kind != ResourceKind::Configuration {
                errors.push(format!(
                    "{} remains after uninstall",
                    resource.path.display()
                ));
            }
            retained.push(resource.path.clone());
        }
    }
    let success = native_status == Some(0) && errors.is_empty();
    receipt.native_status = native_status;
    receipt.updated_unix_secs = now()?;
    if success {
        store.forget(name)?;
    } else {
        receipt.status = ReceiptStatus::UninstallIncomplete;
        store.save(&receipt, false)?;
    }
    Ok(ScriptRecord {
        name: name.into(),
        success,
        native_status,
        status: (!success).then_some(ReceiptStatus::UninstallIncomplete),
        errors,
        retained,
    })
}
