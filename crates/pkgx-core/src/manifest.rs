use crate::{
    files::{FileDeclaration, FileStatus},
    identity::parse_name,
    Error, InstalledPackage, PackageId, ProviderId,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesiredState {
    pub version: u32,
    #[serde(default)]
    pub system: System,
    #[serde(default)]
    pub packages: BTreeMap<String, PackageSelection>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub files: BTreeMap<String, FileDeclaration>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub profiles: BTreeMap<String, Profile>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub platforms: BTreeMap<String, Layer>,
    #[serde(skip)]
    pub resolution_notes: Vec<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct System {
    pub name: Option<String>,
    #[serde(default)]
    pub mode: SyncMode,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncMode {
    #[default]
    Additive,
    Strict,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layer {
    #[serde(default)]
    pub packages: BTreeMap<String, PackageSelection>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub files: BTreeMap<String, FileDeclaration>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    #[serde(default)]
    pub extends: Vec<String>,
    #[serde(default)]
    pub packages: BTreeMap<String, PackageSelection>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub files: BTreeMap<String, FileDeclaration>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageSelection {
    pub install: Vec<String>,
}
#[derive(Debug, Serialize)]
pub struct Diff {
    pub missing: Vec<PackageId>,
    pub present: Vec<PackageId>,
    pub undeclared: Vec<PackageId>,
    pub removals: Vec<PackageId>,
    /// Declared configuration files (ADR 013), filled by callers that observe HOME.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<FileStatus>,
    pub warnings: Vec<String>,
}
fn validate_packages(packages: &BTreeMap<String, PackageSelection>) -> Result<(), Error> {
    for (provider, selection) in packages {
        let provider: ProviderId = provider.parse()?;
        if provider == ProviderId::Script {
            return Err(Error::Unsupported(
                "script recipes require a separate verified schema".into(),
            ));
        }
        let mut seen = BTreeSet::new();
        for name in &selection.install {
            // A package may share its provider's name (pacman:x86_64, apt:amd64);
            // that reading needs a known architecture, otherwise it is a prefix.
            if let Some(rest) = name.strip_prefix(&format!("{provider}:")) {
                let first = rest.split(':').next().unwrap_or_default();
                if !crate::identity::known_scope(provider, first) || rest.contains(':') {
                    return Err(Error::InvalidInput(
                        "manifest names are already provider-scoped; omit the provider prefix"
                            .into(),
                    ));
                }
            }
            let id = parse_name(provider, name)?;
            if !seen.insert(id.to_string()) {
                return Err(Error::InvalidInput(
                    "duplicate package in manifest layer".into(),
                ));
            }
        }
    }
    Ok(())
}
fn merge(
    target: &mut BTreeMap<String, PackageSelection>,
    layer: &BTreeMap<String, PackageSelection>,
) {
    for (provider, selection) in layer {
        let target = target
            .entry(provider.clone())
            .or_insert_with(|| PackageSelection { install: vec![] });
        target.install.extend(selection.install.iter().cloned());
        target.install.sort();
        target.install.dedup();
    }
}
impl DesiredState {
    pub fn parse(input: &str) -> Result<Self, Error> {
        if input.len() > 1024 * 1024 {
            return Err(Error::InvalidInput("manifest exceeds 1 MiB".into()));
        }
        let state: Self = toml::from_str(input).map_err(|error: toml::de::Error| {
            let line = error
                .span()
                .map(|span| {
                    input.as_bytes()[..span.start.min(input.len())]
                        .iter()
                        .copied()
                        .filter(|b| *b == b'\n')
                        .count()
                        + 1
                })
                .unwrap_or(1);
            Error::InvalidInput(format!(
                "invalid manifest at line {line}: expected strict v1 schema (see docs/manifest.md)"
            ))
        })?;
        state.validate()?;
        Ok(state)
    }
    pub fn validate(&self) -> Result<(), Error> {
        if self.version != 1 {
            return Err(Error::InvalidInput(
                "only manifest version 1 is supported".into(),
            ));
        }
        validate_packages(&self.packages)?;
        crate::files::validate_declarations(&self.files)?;
        for (os, layer) in &self.platforms {
            if !matches!(os.as_str(), "linux" | "macos") {
                return Err(Error::InvalidInput(
                    "platform sections support linux and macos only".into(),
                ));
            }
            validate_packages(&layer.packages)?;
            crate::files::validate_declarations(&layer.files)?;
        }
        for (name, profile) in &self.profiles {
            if name.is_empty()
                || name.len() > 64
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            {
                return Err(Error::InvalidInput("invalid profile name".into()));
            }
            validate_packages(&profile.packages)?;
            crate::files::validate_declarations(&profile.files)?;
            self.visit_profile(
                name,
                &mut BTreeSet::new(),
                &mut BTreeSet::new(),
                &mut vec![],
            )?;
        }
        Ok(())
    }
    fn visit_profile(
        &self,
        name: &str,
        visiting: &mut BTreeSet<String>,
        done: &mut BTreeSet<String>,
        order: &mut Vec<String>,
    ) -> Result<(), Error> {
        if done.contains(name) {
            return Ok(());
        }
        if visiting.len() >= 64 || !visiting.insert(name.into()) {
            return Err(Error::InvalidInput(
                "profile inheritance cycle or depth above 64".into(),
            ));
        }
        let profile = self
            .profiles
            .get(name)
            .ok_or_else(|| Error::InvalidInput(format!("unknown profile: {name}")))?;
        for parent in &profile.extends {
            self.visit_profile(parent, visiting, done, order)?;
        }
        visiting.remove(name);
        done.insert(name.into());
        order.push(name.into());
        Ok(())
    }
    /// Stable additive union: base, matching platform, inherited profiles then
    /// requested profiles in CLI order. No implicit profile activation.
    pub fn resolve(&self, os: &str, selected: &[String]) -> Result<Self, Error> {
        self.validate()?;
        if !matches!(os, "linux" | "macos") {
            return Err(Error::Unsupported("manifest host platform".into()));
        }
        let mut result = self.clone();
        result.profiles.clear();
        result.platforms.clear();
        let mut packages = BTreeMap::new();
        merge(&mut packages, &self.packages);
        let mut files = BTreeMap::new();
        crate::files::merge(&mut files, &self.files)?;
        for (platform, layer) in &self.platforms {
            if platform == os {
                merge(&mut packages, &layer.packages);
                crate::files::merge(&mut files, &layer.files)?;
            } else {
                result.resolution_notes.push(format!(
                    "Skipped platform section: {platform} (host: {os})."
                ));
            }
        }
        let mut order = vec![];
        let mut done = BTreeSet::new();
        for name in selected {
            self.visit_profile(name, &mut BTreeSet::new(), &mut done, &mut order)?;
        }
        for name in order {
            merge(&mut packages, &self.profiles[&name].packages);
            crate::files::merge(&mut files, &self.profiles[&name].files)?;
            result
                .resolution_notes
                .push(format!("Included profile: {name}."));
        }
        result.packages = packages;
        result.files = files;
        Ok(result)
    }
    pub fn identities(&self) -> Result<Vec<PackageId>, Error> {
        self.validate()?;
        if !self.profiles.is_empty() || !self.platforms.is_empty() {
            return Err(Error::InvalidInput(
                "resolve manifest profiles/platforms before planning".into(),
            ));
        }
        self.packages
            .iter()
            .flat_map(|(provider, selection)| {
                selection.install.iter().map(move |name| (provider, name))
            })
            .map(|(provider, name)| parse_name(provider.parse()?, name))
            .collect()
    }
    /// File planning and writes are not implemented; refuse rather than ignore them.
    pub fn require_no_files(&self) -> Result<(), Error> {
        if self.files.is_empty() {
            return Ok(());
        }
        Err(Error::Unsupported(
            "managed configuration files (ADR 013) are observed by diff only; file plans and writes are not implemented yet".into(),
        ))
    }
    pub fn providers(&self) -> Result<Vec<ProviderId>, Error> {
        self.packages.keys().map(|p| p.parse()).collect()
    }
    pub fn diff(&self, observed: &[InstalledPackage]) -> Result<Diff, Error> {
        if self.system.mode == SyncMode::Strict {
            return Err(Error::Safety(
                "strict synchronization requires an explicit managed registry snapshot".into(),
            ));
        }
        let desired = self.identities()?;
        let matches = |want: &PackageId, have: &PackageId| {
            want.provider == have.provider
                && want.name == have.name
                && want
                    .architecture_or_scope
                    .as_ref()
                    .is_none_or(|arch| have.architecture_or_scope.as_ref() == Some(arch))
        };
        let mut missing = vec![];
        let mut present = vec![];
        for want in &desired {
            if observed.iter().any(|p| matches(want, &p.id)) {
                present.push(want.clone());
            } else {
                missing.push(want.clone());
            }
        }
        let mut undeclared: Vec<_> = observed
            .iter()
            .filter(|p| !desired.iter().any(|want| matches(want, &p.id)))
            .map(|p| p.id.clone())
            .collect();
        undeclared.sort_by_key(ToString::to_string);
        undeclared.dedup();
        let mut warnings = self.resolution_notes.clone();
        warnings.push(
            "Additive mode: undeclared software is retained; versions are not pinned.".into(),
        );
        Ok(Diff {
            missing,
            present,
            undeclared,
            removals: vec![],
            files: vec![],
            warnings,
        })
    }
    pub fn diff_managed(
        &self,
        observed: &[InstalledPackage],
        managed: &crate::managed::ManagedState,
    ) -> Result<Diff, Error> {
        managed.validate()?;
        let mut additive = self.clone();
        additive.system.mode = SyncMode::Additive;
        let mut diff = additive.diff(observed)?;
        if self.system.mode == SyncMode::Additive {
            return Ok(diff);
        }
        let providers = self.providers()?;
        if providers.iter().any(|p| *p != ProviderId::Apt) {
            return Err(Error::Unsupported(
                "strict synchronization currently supports explicit APT sections only".into(),
            ));
        }
        for id in &diff.undeclared {
            if !providers.contains(&id.provider) {
                continue;
            }
            if let Some(granted) = managed.packages.iter().find(|p| p.id == *id) {
                let package = crate::dependency::installed_match(observed.to_vec(), id)?;
                if granted.version != package.version
                    || granted.install_reason != package.install_reason
                {
                    return Err(Error::Safety("managed package changed; review a new management grant before strict removal".into()));
                }
                diff.removals.push(id.clone());
            }
        }
        diff.undeclared.retain(|id| !diff.removals.contains(id));
        diff.warnings.retain(|w| !w.starts_with("Additive mode:"));
        diff.warnings.push("Strict mode removes only explicitly managed APT identities in manifest provider sections; ungranted software is retained. Native removal cascades outside this scope are refused.".into());
        Ok(diff)
    }
    pub fn export(observed: &[InstalledPackage]) -> Result<String, Error> {
        let mut packages = BTreeMap::<String, PackageSelection>::new();
        for package in observed {
            crate::identity::validate(&package.id)?;
            let name = package
                .id
                .to_string()
                .split_once(':')
                .expect("qualified identity")
                .1
                .to_owned();
            packages
                .entry(package.id.provider.to_string())
                .or_insert_with(|| PackageSelection { install: vec![] })
                .install
                .push(name);
        }
        for selection in packages.values_mut() {
            selection.install.sort();
            selection.install.dedup();
        }
        let state = Self {
            version: 1,
            system: System::default(),
            packages,
            files: BTreeMap::new(),
            profiles: BTreeMap::new(),
            platforms: BTreeMap::new(),
            resolution_notes: vec![],
        };
        state.validate()?;
        toml::to_string_pretty(&state)
            .map_err(|_| Error::Parse("manifest serialization failed".into()))
    }
}

/// Write a complete manifest atomically; never follow an existing destination
/// symlink or truncate a file before serialization/staging has succeeded.
pub fn write_export(path: &std::path::Path, text: &str) -> Result<(), Error> {
    use std::io::Write;
    let io = |source| Error::Io {
        program: "manifest export".into(),
        source,
    };
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => {
            return Err(Error::Safety(
                "export destination must be a regular file, not a symlink or directory".into(),
            ))
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(io(error)),
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    let mut staged = tempfile::NamedTempFile::new_in(parent).map_err(io)?;
    staged.write_all(text.as_bytes()).map_err(io)?;
    staged.as_file().sync_all().map_err(io)?;
    staged.persist(path).map_err(|error| io(error.error))?;
    Ok(())
}
