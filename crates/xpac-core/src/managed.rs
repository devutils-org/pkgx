//! Explicit native-package management grants, never filesystem ownership claims.
use crate::{
    dependency::installed_match, Error, InstallReason, InstalledPackage, OperationKind, PackageId,
    Plan, ProviderId,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::PathBuf,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedPackage {
    pub id: PackageId,
    pub version: String,
    pub install_reason: InstallReason,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedState {
    pub schema_version: u32,
    pub packages: Vec<ManagedPackage>,
}
impl Default for ManagedState {
    fn default() -> Self {
        Self {
            schema_version: 1,
            packages: vec![],
        }
    }
}
impl ManagedState {
    pub fn validate(&self) -> Result<(), Error> {
        if self.schema_version != 1 {
            return Err(Error::Unsupported("managed registry schema version".into()));
        }
        let mut ids = std::collections::BTreeSet::new();
        for package in &self.packages {
            crate::identity::validate(&package.id)?;
            if package.id.provider != ProviderId::Apt
                || package.id.architecture_or_scope.is_none()
                || package.version.is_empty()
                || package.version.chars().any(char::is_control)
                || !ids.insert(package.id.to_string())
            {
                return Err(Error::Safety(
                    "management grants require unique, concrete APT identities and native versions"
                        .into(),
                ));
            }
        }
        Ok(())
    }
    pub fn authorize_removals(&self, plan: &Plan, requested: &[PackageId]) -> Result<(), Error> {
        self.validate()?;
        for op in &plan.operations {
            if op.kind != OperationKind::Remove {
                continue;
            }
            if !requested.contains(&op.package) {
                return Err(Error::Safety("strict native removal cascade exceeds the reviewed undeclared management scope".into()));
            }
            let granted = self
                .packages
                .iter()
                .find(|p| p.id == op.package)
                .ok_or_else(|| {
                    Error::Safety(
                        "strict plan would remove an unmanaged package; native transaction refused"
                            .into(),
                    )
                })?;
            let observed = installed_match(plan.observed_packages.clone(), &op.package)?;
            if granted.version != observed.version
                || granted.install_reason != observed.install_reason
            {
                return Err(Error::Safety("managed package version or installation reason changed; review a new management grant".into()));
            }
        }
        Ok(())
    }
}

pub struct ManagedRegistry {
    directory: PathBuf,
}
pub struct ManagedGuard<'a> {
    registry: &'a ManagedRegistry,
    pub state: ManagedState,
    lock: fs::File,
}
fn io(source: std::io::Error) -> Error {
    Error::Io {
        program: "managed registry".into(),
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
            "managed registry requires regular files and real directories".into(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // SAFETY: geteuid has no pointer arguments.
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
            return Err(Error::Safety("managed registry must belong to the current user and be private (0700 directory, 0600 files)".into()));
        }
    }
    Ok(())
}
impl ManagedRegistry {
    /// Usually <transaction-directory>/managed, separate from transaction entries.
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }
    /// Missing state is empty; inspection never creates directories or locks.
    pub fn read(&self) -> Result<ManagedState, Error> {
        let metadata = match fs::symlink_metadata(&self.directory) {
            Ok(metadata) => metadata,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ManagedState::default())
            }
            Err(e) => return Err(io(e)),
        };
        private(&metadata, true)?;
        let path = self.directory.join("registry.json");
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ManagedState::default())
            }
            Err(e) => return Err(io(e)),
        };
        private(&metadata, false)?;
        let mut bytes = vec![];
        fs::File::open(path)
            .map_err(io)?
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(io)?;
        if bytes.len() > 1024 * 1024 {
            return Err(Error::Safety("managed registry exceeds 1 MiB".into()));
        }
        let state: ManagedState = serde_json::from_slice(&bytes)
            .map_err(|_| Error::Parse("invalid managed registry".into()))?;
        state.validate()?;
        Ok(state)
    }
    /// Serialize xpac writers. A lock left by a crash is never auto-recovered.
    pub fn lock(&self, expected: &ManagedState) -> Result<ManagedGuard<'_>, Error> {
        expected.validate()?;
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&self.directory).map_err(io)?;
        private(&fs::symlink_metadata(&self.directory).map_err(io)?, true)?;
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock = options.open(self.directory.join("lock")).map_err(|e| if e.kind() == std::io::ErrorKind::AlreadyExists {
            Error::Safety("managed registry is locked; another operation is running or interrupted; inspect before manual recovery".into())
        } else { io(e) })?;
        let guard = ManagedGuard {
            registry: self,
            state: expected.clone(),
            lock,
        };
        guard.validate()?;
        Ok(guard)
    }
}
impl ManagedGuard<'_> {
    pub fn validate(&self) -> Result<(), Error> {
        private(&self.lock.metadata().map_err(io)?, false)?;
        if self.registry.read()? != self.state {
            return Err(Error::Safety(
                "managed registry changed since review; no native transaction authorized".into(),
            ));
        }
        Ok(())
    }
    pub fn grant(&mut self, package: &InstalledPackage) -> Result<(), Error> {
        self.validate()?;
        let mut next = self.state.clone();
        next.packages.retain(|p| p.id != package.id);
        next.packages.push(ManagedPackage {
            id: package.id.clone(),
            version: package.version.clone(),
            install_reason: package.install_reason,
        });
        next.packages.sort_by_key(|p| p.id.to_string());
        self.write(next)
    }
    pub fn forget(&mut self, id: &PackageId) -> Result<(), Error> {
        self.validate()?;
        let mut next = self.state.clone();
        if !next.packages.iter().any(|p| p.id == *id) {
            return Err(Error::InvalidInput(
                "no matching concrete management grant".into(),
            ));
        }
        next.packages.retain(|p| p.id != *id);
        self.write(next)
    }
    fn write(&mut self, next: ManagedState) -> Result<(), Error> {
        next.validate()?;
        let bytes = serde_json::to_vec_pretty(&next)
            .map_err(|_| Error::Parse("cannot serialize managed registry".into()))?;
        if bytes.len() > 1024 * 1024 {
            return Err(Error::Safety("managed registry exceeds 1 MiB".into()));
        }
        let mut temp = tempfile::NamedTempFile::new_in(&self.registry.directory).map_err(io)?;
        temp.write_all(&bytes).map_err(io)?;
        temp.as_file().sync_all().map_err(io)?;
        temp.persist(self.registry.directory.join("registry.json"))
            .map_err(|e| io(e.error))?;
        #[cfg(unix)]
        fs::File::open(&self.registry.directory)
            .and_then(|f| f.sync_all())
            .map_err(io)?;
        self.state = next;
        Ok(())
    }
}
impl Drop for ManagedGuard<'_> {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.registry.directory.join("lock"));
    }
}
