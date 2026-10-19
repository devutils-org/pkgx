//! Conservative native inspection adapters. Unsupported solvers are never simulated.
use crate::{
    backend::{Backend, PackageInfo, PackageProvenance, SearchResult},
    command::{checked, CommandRunner, CommandSpec},
    identity::{parse_name, validate},
    Capability, Error, InstallReason, InstalledPackage, Operation, OperationKind, PackageId, Plan,
    ProviderId, RiskLevel,
};
use std::collections::{BTreeMap, BTreeSet};

pub struct NativeBackend<'a> {
    pub provider: ProviderId,
    pub runner: &'a dyn CommandRunner,
}
pub fn backend<'a>(
    provider: ProviderId,
    runner: &'a dyn CommandRunner,
    os: &str,
) -> Result<Box<dyn Backend + 'a>, Error> {
    if os != "linux" && !(os == "macos" && provider == ProviderId::Brew) {
        return Err(Error::Unavailable(format!(
            "{provider} is not supported on {os}"
        )));
    }
    if provider == ProviderId::Script {
        return Err(Error::Unsupported(
            "script backend is not implemented".into(),
        ));
    }
    let binary = match provider {
        ProviderId::Apt => "apt-get",
        ProviderId::Pacman => "pacman",
        ProviderId::Dnf => "dnf",
        ProviderId::Flatpak => "flatpak",
        ProviderId::Brew => "brew",
        ProviderId::Script => unreachable!(),
    };
    crate::version::require_target(
        provider,
        &output(runner, CommandSpec::new(binary, &["--version"]))?,
    )?;
    if provider == ProviderId::Apt {
        return Ok(Box::new(crate::apt::AptBackend { runner }));
    }
    Ok(Box::new(NativeBackend { provider, runner }))
}
pub fn output(runner: &dyn CommandRunner, mut spec: CommandSpec) -> Result<String, Error> {
    if spec.program == "brew" {
        for key in [
            "HOMEBREW_NO_AUTO_UPDATE",
            "HOMEBREW_NO_ANALYTICS",
            "HOMEBREW_NO_INSTALL_CLEANUP",
            "HOMEBREW_NO_ENV_HINTS",
            "HOMEBREW_NO_COLOR",
            "HOMEBREW_NO_AUTOREMOVE",
        ] {
            spec.env.insert(key.into(), "1".into());
        }
    }
    String::from_utf8(checked(runner, &spec)?.stdout)
        .map_err(|_| Error::Parse("backend output is not UTF-8".into()))
}
fn package(
    id: PackageId,
    version: String,
    reason: InstallReason,
    metadata: BTreeMap<String, String>,
) -> InstalledPackage {
    InstalledPackage {
        id,
        version,
        install_reason: reason,
        dependencies: vec![],
        metadata,
    }
}
fn sorted(
    mut packages: Vec<InstalledPackage>,
    unique: bool,
) -> Result<Vec<InstalledPackage>, Error> {
    packages.sort_by_key(|p| (p.id.to_string(), p.version.clone()));
    if unique && packages.windows(2).any(|w| w[0].id == w[1].id) {
        return Err(Error::Parse("duplicate installed identity".into()));
    }
    Ok(packages)
}
/// Parse pacman's C-locale aligned key/value output without flattening native
/// dependency alternatives into fabricated resolved edges.
pub fn parse_pacman(input: &str) -> Result<Vec<InstalledPackage>, Error> {
    let mut records = vec![];
    let mut record = BTreeMap::<String, String>::new();
    let mut last = None::<String>;
    for line in input.lines().chain(std::iter::once("")) {
        if line.is_empty() {
            if !record.is_empty() {
                records.push(std::mem::take(&mut record));
            }
            last = None;
        } else if line.starts_with(' ') {
            let key = last
                .as_ref()
                .ok_or_else(|| Error::Parse("pacman continuation without field".into()))?;
            let field = record.get_mut(key).expect("last field exists");
            field.push('\n');
            field.push_str(line.trim());
        } else {
            let (key, value) = line
                .split_once(':')
                .ok_or_else(|| Error::Parse("malformed pacman field".into()))?;
            let key = key.trim().to_string();
            if key.is_empty() || record.insert(key.clone(), value.trim().into()).is_some() {
                return Err(Error::Parse("duplicate pacman field".into()));
            }
            last = Some(key);
        }
    }
    let packages = records
        .into_iter()
        .map(|mut fields| {
            let mut required = |key: &str| {
                fields
                    .remove(key)
                    .filter(|v| !v.is_empty())
                    .ok_or_else(|| Error::Parse(format!("pacman missing {key}")))
            };
            let name = required("Name")?;
            let version = required("Version")?;
            let arch = required("Architecture")?;
            let id = parse_name(ProviderId::Pacman, &format!("{name}:{arch}"))?;
            let reason = match fields.get("Install Reason").map(String::as_str) {
                Some("Explicitly installed") => InstallReason::Explicit,
                Some("Installed as a dependency for another package") => InstallReason::Dependency,
                _ => InstallReason::Unknown,
            };
            fields.insert(
                "dependencies_status".into(),
                "native_expression_only".into(),
            );
            Ok(package(id, version, reason, fields))
        })
        .collect::<Result<Vec<_>, Error>>()?;
    sorted(packages, true)
}
pub fn parse_rpm(input: &str) -> Result<Vec<InstalledPackage>, Error> {
    let packages = input
        .lines()
        // Imported OpenPGP keys appear as `gpg-pubkey` headers with no
        // architecture; they are keyring entries, not installable packages.
        .filter(|line| !line.starts_with("gpg-pubkey\t(none)\t"))
        .map(|line| {
            let f: Vec<_> = line.split('\t').collect();
            if f.len() != 3 || f.iter().any(|s| s.is_empty()) {
                return Err(Error::Parse(
                    "RPM inventory expected name, architecture and EVR".into(),
                ));
            }
            Ok(package(
                parse_name(ProviderId::Dnf, &format!("{}:{}", f[0], f[1]))?,
                f[2].into(),
                InstallReason::Unknown,
                BTreeMap::from([
                    (
                        "reason_status".into(),
                        "RPM does not establish DNF installation reason".into(),
                    ),
                    ("dependencies_status".into(), "not_queried".into()),
                ]),
            ))
        })
        .collect::<Result<Vec<_>, Error>>()?;
    // RPM legitimately permits multiple installonly versions of one identity.
    sorted(packages, false)
}
pub fn parse_flatpak(input: &str) -> Result<Vec<InstalledPackage>, Error> {
    let packages = input
        .lines()
        .map(|line| {
            let f: Vec<_> = line.split('\t').collect();
            if f.len() != 5 {
                return Err(Error::Parse(
                    "Flatpak inventory expected ref, version, active, origin, installation".into(),
                ));
            }
            if f[2].is_empty() {
                return Err(Error::Parse("Flatpak active commit missing".into()));
            }
            let id = parse_name(ProviderId::Flatpak, &format!("{}:{}", f[0], f[4]))?;
            Ok(package(
                id,
                if f[1].is_empty() { f[2] } else { f[1] }.into(),
                InstallReason::Unknown,
                BTreeMap::from([
                    ("active_commit_prefix".into(), f[2].into()),
                    ("origin".into(), f[3].into()),
                    (
                        "version_source".into(),
                        if f[1].is_empty() {
                            "native_active_commit_prefix"
                        } else {
                            "native_appdata_version"
                        }
                        .into(),
                    ),
                    ("dependencies_status".into(), "not_queried".into()),
                ]),
            ))
        })
        .collect::<Result<Vec<_>, Error>>()?;
    sorted(packages, true)
}
pub fn parse_brew(input: &str) -> Result<Vec<InstalledPackage>, Error> {
    let value: serde_json::Value =
        serde_json::from_str(input).map_err(|_| Error::Parse("invalid Homebrew JSON".into()))?;
    let formulae = value
        .get("formulae")
        .and_then(|v| v.as_array())
        .ok_or_else(|| Error::Parse("Homebrew formulae array missing".into()))?;
    let casks = value
        .get("casks")
        .and_then(|v| v.as_array())
        .ok_or_else(|| Error::Parse("Homebrew casks array missing".into()))?;
    let string = |value: &serde_json::Value, key: &str| {
        value
            .get(key)
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| Error::Parse(format!("Homebrew {key} missing")))
    };
    let mut packages = vec![];
    for formula in formulae {
        let name = string(formula, "full_name")?;
        let installed = formula
            .get("installed")
            .and_then(|v| v.as_array())
            .ok_or_else(|| Error::Parse("Homebrew installed array missing".into()))?;
        for receipt in installed {
            let reason = match (
                receipt
                    .get("installed_on_request")
                    .and_then(|v| v.as_bool()),
                receipt
                    .get("installed_as_dependency")
                    .and_then(|v| v.as_bool()),
            ) {
                (Some(true), Some(false) | None) => InstallReason::Explicit,
                // Homebrew 7 receipts no longer carry installed_as_dependency.
                (Some(false), Some(true) | None) => InstallReason::Dependency,
                _ => InstallReason::Unknown,
            };
            let mut metadata = BTreeMap::from([(
                "dependencies_status".into(),
                "native_expression_only".into(),
            )]);
            if let Some(deps) = formula.get("dependencies") {
                metadata.insert("native_dependencies".into(), deps.to_string());
            }
            packages.push(package(
                parse_name(ProviderId::Brew, &format!("{name}:formula"))?,
                string(receipt, "version")?,
                reason,
                metadata,
            ));
        }
    }
    for cask in casks {
        let name = string(cask, "full_token").or_else(|_| string(cask, "token"))?;
        let installed = cask
            .get("installed")
            .filter(|v| !v.is_null())
            .ok_or_else(|| Error::Parse("Homebrew installed cask version missing".into()))?;
        let versions: Vec<String> = if let Some(version) = installed.as_str() {
            vec![version.into()]
        } else if let Some(versions) = installed.as_array() {
            versions
                .iter()
                .map(|v| {
                    v.as_str()
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .ok_or_else(|| Error::Parse("invalid cask installed version".into()))
                })
                .collect::<Result<_, _>>()?
        } else {
            return Err(Error::Parse(
                "unsupported cask installed version shape".into(),
            ));
        };
        for version in versions {
            packages.push(package(
                parse_name(ProviderId::Brew, &format!("{name}:cask"))?,
                version,
                InstallReason::Unknown,
                BTreeMap::from([("dependencies_status".into(), "not_queried".into())]),
            ));
        }
    }
    sorted(packages, false)
}
impl Backend for NativeBackend<'_> {
    fn provider(&self) -> ProviderId {
        self.provider
    }
    fn capabilities(&self) -> Vec<Capability> {
        match self.provider {
            ProviderId::Pacman => vec![
                Capability::Inventory,
                Capability::Search,
                Capability::Install,
                Capability::Remove,
                Capability::Dependencies,
                Capability::Orphans,
                Capability::DryRun,
                Capability::PackageCache,
            ],
            ProviderId::Dnf => vec![
                Capability::Inventory,
                Capability::Install,
                Capability::Remove,
                Capability::Dependencies,
                Capability::DryRun,
            ],
            ProviderId::Flatpak => vec![
                Capability::Inventory,
                Capability::Install,
                Capability::Remove,
                Capability::DryRun,
            ],
            ProviderId::Brew => vec![
                Capability::Inventory,
                Capability::Install,
                Capability::Remove,
                Capability::Dependencies,
                Capability::Orphans,
                Capability::DryRun,
            ],
            _ => vec![],
        }
    }
    fn inventory(&self) -> Result<Vec<InstalledPackage>, Error> {
        match self.provider {
            ProviderId::Pacman => parse_pacman(&output(
                self.runner,
                CommandSpec::new("pacman", &["-Qi", "--color", "never"]),
            )?),
            ProviderId::Dnf => parse_rpm(&output(
                self.runner,
                CommandSpec::new(
                    "rpm",
                    &[
                        "-qa",
                        "--qf",
                        "%{NAME}\t%{ARCH}\t%{EPOCHNUM}:%{VERSION}-%{RELEASE}\n",
                    ],
                ),
            )?),
            ProviderId::Flatpak => {
                let mut normalized = String::new();
                // Query kind and scope explicitly: Flatpak's ref column is a
                // partial ID/arch/branch and does not identify app vs runtime.
                for scope in ["user", "system"] {
                    for kind in ["app", "runtime"] {
                        let raw = output(
                            self.runner,
                            CommandSpec::new(
                                "flatpak",
                                &[
                                    "list",
                                    "--all",
                                    &format!("--{scope}"),
                                    &format!("--{kind}"),
                                    "--columns=ref:full,version:full,active:full,origin:full",
                                ],
                            ),
                        )?;
                        for line in raw.lines() {
                            let f: Vec<_> = line.split('\t').collect();
                            if f.len() != 4 {
                                return Err(Error::Parse(
                                    "Flatpak row expected four columns".into(),
                                ));
                            }
                            normalized.push_str(&format!(
                                "{kind}/{}\t{}\t{}\t{}\t{scope}\n",
                                f[0], f[1], f[2], f[3]
                            ));
                        }
                    }
                }
                parse_flatpak(&normalized)
            }
            ProviderId::Brew => parse_brew(&output(
                self.runner,
                CommandSpec::new("brew", &["info", "--json=v2", "--installed"]),
            )?),
            _ => Err(Error::Unsupported("native inventory adapter".into())),
        }
    }
    fn info(&self, id: &PackageId) -> Result<PackageInfo, Error> {
        validate(id)?;
        if id.provider != self.provider {
            return Err(Error::InvalidInput("provider mismatch".into()));
        }
        let matches: Vec<_> = self
            .inventory()?
            .into_iter()
            .filter(|p| {
                p.id.name == id.name
                    && id
                        .architecture_or_scope
                        .as_ref()
                        .is_none_or(|s| p.id.architecture_or_scope.as_ref() == Some(s))
            })
            .collect();
        if matches.is_empty() {
            return Err(Error::Unavailable(
                "package is not installed; this adapter provides installed info only".into(),
            ));
        }
        let records = matches
            .into_iter()
            .map(|p| {
                let mut fields = p.metadata;
                fields.insert("version".into(), p.version);
                fields.insert("identity".into(), p.id.to_string());
                fields
            })
            .collect();
        Ok(PackageInfo {
            id: id.clone(),
            records,
        })
    }
    fn dependencies(&self, id: &PackageId) -> Result<crate::dependency::DependencyReport, Error> {
        validate(id)?;
        if id.provider != self.provider {
            return Err(Error::InvalidInput("provider mismatch".into()));
        }
        if !matches!(
            self.provider,
            ProviderId::Pacman | ProviderId::Dnf | ProviderId::Brew
        ) {
            return Err(Error::Unsupported(
                "installed dependency query for this provider".into(),
            ));
        }
        let package = crate::dependency::installed_match(self.inventory()?, id)?;
        let metadata = package.metadata.clone();
        let source = match self.provider {
            ProviderId::Pacman => "pacman installed metadata",
            ProviderId::Dnf => "RPM installed requirements",
            _ => "Homebrew formula metadata",
        };
        let mut report = crate::dependency::DependencyReport::native(package, source);
        match self.provider {
            ProviderId::Pacman => {
                for relation in [
                    "Depends On",
                    "Optional Deps",
                    "Required By",
                    "Optional For",
                    "Provides",
                    "Conflicts With",
                    "Replaces",
                ] {
                    if let Some(value) = metadata.get(relation) {
                        if value != "None" {
                            report.push(relation, value);
                        }
                    }
                }
            }
            ProviderId::Dnf => {
                let name = format!(
                    "{}.{}",
                    report.package.name,
                    report
                        .package
                        .architecture_or_scope
                        .as_deref()
                        .expect("RPM inventory carries architecture")
                );
                let raw = output(
                    self.runner,
                    CommandSpec::new("rpm", &["-q", "--requires", "--", &name]),
                )?;
                for requirement in raw.lines() {
                    report.push("Requires", requirement);
                }
                report.limitations.push("RPM requirements include virtual/file/rpmlib capabilities and are not DNF installation reasons.".into());
            }
            ProviderId::Brew => {
                if report.package.architecture_or_scope.as_deref() != Some("formula") {
                    return Err(Error::Unsupported(
                        "Homebrew cask dependency metadata".into(),
                    ));
                }
                let raw = metadata.get("native_dependencies").ok_or_else(|| {
                    Error::Unavailable("formula dependency metadata is absent".into())
                })?;
                let deps: Vec<String> = serde_json::from_str(raw).map_err(|_| {
                    Error::Parse("formula dependencies must be an array of strings".into())
                })?;
                for dep in deps {
                    report.push("Depends", &dep);
                }
                report.limitations.push("Formula metadata may describe the current formula rather than the installed keg's historical dependency versions.".into());
            }
            _ => unreachable!(),
        }
        Ok(report)
    }
    fn why(&self, id: &PackageId) -> Result<PackageProvenance, Error> {
        validate(id)?;
        if id.provider != self.provider {
            return Err(Error::InvalidInput("provider mismatch".into()));
        }
        let mut matches: Vec<_> = self
            .inventory()?
            .into_iter()
            .filter(|p| {
                p.id.name == id.name
                    && id
                        .architecture_or_scope
                        .as_ref()
                        .is_none_or(|s| p.id.architecture_or_scope.as_ref() == Some(s))
            })
            .collect();
        if matches.len() != 1 {
            return Err(Error::InvalidInput("installed package is absent or ambiguous; specify scope; multiple installed versions cannot be attributed individually".into()));
        }
        let package = matches.remove(0);
        let origin = package.metadata.get("origin").cloned();
        let reverse = package
            .metadata
            .get("Required By")
            .cloned()
            .unwrap_or_else(|| "not queried".into());
        Ok(PackageProvenance {
            confidence: if package.install_reason == InstallReason::Unknown {
                "unknown"
            } else {
                "native_mark_only"
            }
            .into(),
            package,
            origin,
            installed_reverse_dependencies: reverse,
            limitations: vec![
                "Native inventory is not historical proof of who installed this package.".into(),
            ],
        })
    }
    fn plan(&self, install: &[PackageId], remove: &[PackageId]) -> Result<Plan, Error> {
        if self.provider == ProviderId::Dnf {
            return dnf_plan(self.runner, install, remove);
        }
        if self.provider == ProviderId::Flatpak {
            return flatpak_plan(self.runner, install, remove);
        }
        if self.provider == ProviderId::Brew {
            return brew_plan(self.runner, install, remove);
        }
        if self.provider != ProviderId::Pacman {
            return Err(Error::Unsupported(
                "native preview for this provider".into(),
            ));
        }
        if install.is_empty() == remove.is_empty() {
            return Err(Error::Unsupported("Pacman preview accepts either installs or removals, not an empty or mixed transaction".into()));
        }
        let requests = if remove.is_empty() { install } else { remove };
        for id in requests {
            validate(id)?;
            if id.provider != self.provider {
                return Err(Error::InvalidInput("provider mismatch".into()));
            }
        }
        let observed = self.inventory()?;
        let mut spec = CommandSpec::new(
            "pacman",
            &[
                if remove.is_empty() {
                    "--sync"
                } else {
                    "--remove"
                },
                "--print",
                "--print-format",
                "%n\t%v\t%a\t%r",
                "--color",
                "never",
                "--",
            ],
        );
        if remove.is_empty() {
            // Matches the executor: already-installed targets are not reinstalled.
            spec.args.insert(1, "--needed".into());
        }
        spec.args.extend(requests.iter().map(|id| id.name.clone()));
        let native = output(self.runner, spec)?;
        let mut operations = vec![];
        for line in native.lines() {
            let fields: Vec<_> = line.split('\t').collect();
            if fields.len() != 4 || fields[..3].iter().any(|s| s.is_empty()) {
                return Err(Error::Parse(
                    "Pacman preview expected name, version, architecture, repository".into(),
                ));
            }
            let id = parse_name(ProviderId::Pacman, &format!("{}:{}", fields[0], fields[2]))?;
            if requests.iter().any(|request| {
                request.name == id.name
                    && request
                        .architecture_or_scope
                        .as_ref()
                        .is_some_and(|arch| id.architecture_or_scope.as_ref() != Some(arch))
            }) {
                return Err(Error::Safety(
                    "Pacman resolved a different architecture than requested".into(),
                ));
            }
            operations.push(Operation {
                kind: if remove.is_empty() {
                    OperationKind::Install
                } else {
                    OperationKind::Remove
                },
                package: id,
                risk: RiskLevel::RequiresReview,
                native_detail: line.into(),
            });
        }
        if operations.is_empty() && !remove.is_empty() {
            return Err(Error::Parse(
                "Pacman print produced no operation records".into(),
            ));
        }
        pacman_policy(self.runner, &operations, !remove.is_empty())?;
        let mut warnings = vec!["Pacman --print is a target/dependency estimate, not a complete transaction simulation; conflict replacements and hooks are not modeled.".into(), "Execution never refreshes sync databases or upgrades the system; dependency upgrades listed here are partial-upgrade risks to review.".into(), "Pacman cannot pin reviewed versions; execution pins the reviewed repository and verifies installed versions afterwards.".into()];
        if remove.is_empty() {
            for id in install {
                if !operations.iter().any(|op| op.package.name == id.name) {
                    warnings.push(format!("{id} is already installed; --needed leaves it and its install reason unchanged."));
                }
            }
        } else {
            warnings.push("Removal uses pacman --remove without --recursive or --cascade; dependencies stay installed and modified configuration files are kept as native .pacsave backups.".into());
        }
        Ok(Plan {
            operations,
            observed_state_fingerprint: crate::planner::inventory_token(&observed),
            observed_packages: observed.clone(),
            uncertain: true,
            warnings,
        })
    }
    fn package_cache(&self) -> Result<crate::cache::CacheReport, Error> {
        crate::cache::measure(self.runner, self.provider)
    }
    fn orphan_candidates(&self) -> Result<Vec<PackageId>, Error> {
        let names = match self.provider {
            ProviderId::Pacman => {
                let spec = CommandSpec::new("pacman", &["-Qdtq", "--color", "never"]);
                let output = self.runner.run(
                    &spec,
                    std::time::Duration::from_secs(30),
                    &std::sync::atomic::AtomicBool::new(false),
                )?;
                if output.status == Some(1) && output.stdout.is_empty() && output.stderr.is_empty()
                {
                    return Ok(vec![]);
                }
                if output.status != Some(0) {
                    return Err(Error::CommandFailed {
                        program: spec.program,
                        status: output.status,
                    });
                }
                String::from_utf8(output.stdout)
                    .map_err(|_| Error::Parse("pacman orphan output is not UTF-8".into()))?
            }
            ProviderId::Brew => output(
                self.runner,
                CommandSpec::new("brew", &["leaves", "--installed-as-dependency"]),
            )?,
            _ => return Err(Error::Unsupported("orphan query for this provider".into())),
        };
        let inventory = self.inventory()?;
        let mut candidates = vec![];
        for name in names.lines() {
            let matching: Vec<_> = inventory
                .iter()
                .filter(|p| {
                    p.id.name == name
                        && (self.provider != ProviderId::Brew
                            || p.id.architecture_or_scope.as_deref() == Some("formula"))
                })
                .collect();
            if matching.is_empty() {
                return Err(Error::Parse(
                    "orphan query returned an identity absent from inventory".into(),
                ));
            }
            for package in matching {
                if package.install_reason != InstallReason::Dependency {
                    return Err(Error::Safety(
                        "orphan candidate lacks a native dependency installation mark".into(),
                    ));
                }
                if !candidates.contains(&package.id) {
                    candidates.push(package.id.clone());
                }
            }
        }
        Ok(candidates)
    }
    fn search(&self, query: &str) -> Result<Vec<SearchResult>, Error> {
        if self.provider != ProviderId::Pacman {
            return Err(Error::Unsupported(
                "repository search for this provider".into(),
            ));
        }
        if query.is_empty() || query.len() > 256 || query.chars().any(char::is_control) {
            return Err(Error::InvalidInput(
                "search requires 1–256 printable characters".into(),
            ));
        }
        let spec = CommandSpec::new("pacman", &["-Ss", "--color", "never", "--", query]);
        let result = self.runner.run(
            &spec,
            std::time::Duration::from_secs(30),
            &std::sync::atomic::AtomicBool::new(false),
        )?;
        if result.status == Some(1) && result.stdout.is_empty() && result.stderr.is_empty() {
            return Ok(vec![]);
        }
        if result.status != Some(0) {
            return Err(Error::CommandFailed {
                program: spec.program,
                status: result.status,
            });
        }
        let output = String::from_utf8(result.stdout)
            .map_err(|_| Error::Parse("pacman search output is not UTF-8".into()))?;
        let mut results: Vec<SearchResult> = vec![];
        let mut seen = BTreeSet::new();
        for line in output.lines() {
            if line.starts_with(' ') {
                let last = results
                    .last_mut()
                    .ok_or_else(|| Error::Parse("search description without package".into()))?;
                if !last.summary.is_empty() {
                    last.summary.push(' ');
                }
                last.summary.push_str(line.trim());
            } else {
                let name = line
                    .split_whitespace()
                    .next()
                    .and_then(|s| s.split_once('/'))
                    .map(|(_, name)| name)
                    .ok_or_else(|| Error::Parse("invalid pacman search row".into()))?;
                let id = parse_name(ProviderId::Pacman, name)?;
                // Repository duplicates are real candidates; do not lose repository attribution.
                if !seen.insert(line.to_owned()) {
                    continue;
                }
                results.push(SearchResult {
                    id,
                    summary: format!("[{line}]"),
                });
            }
        }
        Ok(results)
    }
}

/// Packages xpac never removes through Pacman, even when HoldPkg is empty.
pub const PACMAN_CRITICAL: &[&str] = &["pacman", "glibc", "filesystem"];
fn pacman_conf(runner: &dyn CommandRunner, key: &str) -> Result<Vec<String>, Error> {
    Ok(output(runner, CommandSpec::new("pacman-conf", &[key]))?
        .split_whitespace()
        .map(str::to_owned)
        .collect())
}
/// pacman-conf patterns use fnmatch; only `*` and `?` are evaluated, and any
/// other glob syntax fails closed instead of guessing.
fn pacman_pattern(pattern: &str, name: &str) -> Result<bool, Error> {
    if pattern.contains(['[', ']', '\\', '!']) {
        return Err(Error::Safety(format!(
            "pacman pattern {pattern:?} uses unsupported glob syntax; review it manually"
        )));
    }
    fn matches(p: &[u8], n: &[u8]) -> bool {
        match (p.first(), n.first()) {
            (None, None) => true,
            (Some(b'*'), _) => matches(&p[1..], n) || (!n.is_empty() && matches(p, &n[1..])),
            (Some(b'?'), Some(_)) => matches(&p[1..], &n[1..]),
            (Some(a), Some(b)) if a == b => matches(&p[1..], &n[1..]),
            _ => false,
        }
    }
    Ok(matches(pattern.as_bytes(), name.as_bytes()))
}
/// Refuse what pacman would otherwise decide through prompts answered by
/// --noconfirm: held removals and installation of ignored packages/groups.
fn pacman_policy(
    runner: &dyn CommandRunner,
    operations: &[Operation],
    removing: bool,
) -> Result<(), Error> {
    if removing {
        let mut held = pacman_conf(runner, "HoldPkg")?;
        held.extend(PACMAN_CRITICAL.iter().map(|s| (*s).to_owned()));
        for op in operations {
            for pattern in &held {
                if pacman_pattern(pattern, &op.package.name)? {
                    return Err(Error::Safety(format!(
                        "{} is protected by HoldPkg or xpac critical-package policy",
                        op.package
                    )));
                }
            }
        }
    } else if !operations.is_empty() {
        if !pacman_conf(runner, "IgnoreGroup")?.is_empty() {
            return Err(Error::Safety("IgnoreGroup is configured; group membership is not evaluated, so Pacman installation is refused".into()));
        }
        let ignored = pacman_conf(runner, "IgnorePkg")?;
        for op in operations {
            for pattern in &ignored {
                if pacman_pattern(pattern, &op.package.name)? {
                    return Err(Error::Safety(format!(
                        "{} matches IgnorePkg; xpac does not override native ignore policy",
                        op.package
                    )));
                }
            }
        }
    }
    Ok(())
}

/// Executor-built DNF5 transactions; sudo delegation accepts only these prefixes.
pub const DNF_INSTALL: &[&str] = &["dnf5", "install", "--assumeyes"];
pub const DNF_REMOVE: &[&str] = &["dnf5", "remove", "--assumeyes", "--no-autoremove"];
/// Split a DNF NEVRA (`name-[epoch:]version-release.arch`) into name, arch and the
/// `epoch:version-release` form used by the RPM inventory.
pub fn split_nevra(nevra: &str) -> Result<(String, String, String), Error> {
    let invalid = || Error::Parse(format!("invalid NEVRA {nevra:?}"));
    let (nevr, arch) = nevra.rsplit_once('.').ok_or_else(invalid)?;
    let (nev, release) = nevr.rsplit_once('-').ok_or_else(invalid)?;
    let (name, version) = nev.rsplit_once('-').ok_or_else(invalid)?;
    let (epoch, version) = version.split_once(':').unwrap_or(("0", version));
    if [name, arch, release, version, epoch]
        .iter()
        .any(|s| s.is_empty())
        || !epoch.bytes().all(|b| b.is_ascii_digit())
        || ![version, release].iter().all(|s| {
            s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._+~^".contains(&b))
        })
    {
        return Err(invalid());
    }
    parse_name(ProviderId::Dnf, &format!("{name}:{arch}"))?;
    Ok((
        name.into(),
        arch.into(),
        format!("{epoch}:{version}-{release}"),
    ))
}
/// Resolve with `dnf5 --store`, which writes the resolved transaction as JSON
/// (downloading packages into a discarded temporary directory) without running it.
fn dnf_plan(
    runner: &dyn CommandRunner,
    install: &[PackageId],
    remove: &[PackageId],
) -> Result<Plan, Error> {
    if install.is_empty() == remove.is_empty() {
        return Err(Error::Unsupported(
            "DNF preview accepts either installs or removals, not an empty or mixed transaction"
                .into(),
        ));
    }
    let requests = if remove.is_empty() { install } else { remove };
    for id in requests {
        validate(id)?;
        if id.provider != ProviderId::Dnf {
            return Err(Error::InvalidInput("provider mismatch".into()));
        }
    }
    let observed = NativeBackend {
        provider: ProviderId::Dnf,
        runner,
    }
    .inventory()?;
    let directory = tempfile::tempdir().map_err(|source| Error::Io {
        program: "DNF preview staging".into(),
        source,
    })?;
    let store = directory.path().join("transaction");
    let template = if remove.is_empty() {
        DNF_INSTALL
    } else {
        DNF_REMOVE
    };
    let mut spec = CommandSpec::new(template[0], &template[1..]);
    spec.args.push(format!(
        "--store={}",
        store
            .to_str()
            .ok_or_else(|| Error::InvalidInput("temporary path is not UTF-8".into()))?
    ));
    for id in requests {
        spec.args.push(match &id.architecture_or_scope {
            Some(arch) => format!("{}.{arch}", id.name),
            None => id.name.clone(),
        });
    }
    output(runner, spec)?;
    let json = store.join("transaction.json");
    let mut operations = vec![];
    let mut warnings = vec![
        "DNF preview is a resolved dnf5 transaction stored without running it; execution re-resolves with reviewed NEVRAs pinned and verifies the result.".into(),
        "Packages were downloaded into a temporary directory for the preview and discarded.".into(),
    ];
    if json.exists() {
        use std::io::Read;
        let mut text = String::new();
        std::fs::File::open(&json)
            .and_then(|f| f.take(16 * 1024 * 1024).read_to_string(&mut text))
            .map_err(|source| Error::Io {
                program: "DNF stored transaction".into(),
                source,
            })?;
        let value: serde_json::Value = serde_json::from_str(&text)
            .map_err(|_| Error::Parse("invalid dnf5 transaction.json".into()))?;
        if value.get("version").and_then(|v| v.as_str()) != Some("1.0") {
            return Err(Error::Unsupported("dnf5 stored transaction version".into()));
        }
        if value
            .as_object()
            .is_some_and(|o| o.keys().any(|k| k != "rpms" && k != "version"))
        {
            return Err(Error::Unsupported(
                "dnf5 transaction contains groups, environments or modules".into(),
            ));
        }
        for rpm in value
            .get("rpms")
            .and_then(|v| v.as_array())
            .ok_or_else(|| Error::Parse("dnf5 transaction lacks rpms".into()))?
        {
            let field = |key: &str| {
                rpm.get(key)
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty() && !s.contains(['\t', '\n']))
                    .ok_or_else(|| Error::Parse(format!("dnf5 rpm entry lacks {key}")))
            };
            let (nevra, action, reason, repo) = (
                field("nevra")?,
                field("action")?,
                field("reason")?,
                field("repo_id")?,
            );
            let (name, arch, _) = split_nevra(nevra)?;
            let kind = match (action, remove.is_empty()) {
                ("Install" | "Upgrade", true) => OperationKind::Install,
                ("Remove", false) => OperationKind::Remove,
                // The old side of an upgrade; verified through the new version.
                ("Replaced", true) => continue,
                _ => {
                    return Err(Error::Safety(format!(
                        "dnf5 transaction action {action:?} is not supported for this request"
                    )))
                }
            };
            if action == "Upgrade" {
                warnings.push(format!(
                    "{nevra} upgrades an installed package; review partial-upgrade risk."
                ));
            }
            operations.push(Operation {
                kind,
                package: parse_name(ProviderId::Dnf, &format!("{name}:{arch}"))?,
                risk: RiskLevel::RequiresReview,
                native_detail: format!("{nevra}\t{action}\t{reason}\t{repo}"),
            });
        }
    }
    if operations.is_empty() {
        if !remove.is_empty() {
            return Err(Error::Parse("dnf5 removal produced no operations".into()));
        }
        warnings.push("Requested packages are already installed; nothing to do.".into());
    }
    if !remove.is_empty() {
        warnings.push("Removal uses --no-autoremove: dependencies stay installed.".into());
    }
    Ok(Plan {
        operations,
        observed_state_fingerprint: crate::planner::inventory_token(&observed),
        observed_packages: observed,
        uncertain: true,
        warnings,
    })
}

/// Split a validated Flatpak identity into its native ref and installation scope.
pub fn flatpak_ref(id: &PackageId) -> Result<(&str, &str), Error> {
    validate(id)?;
    let scope = id
        .architecture_or_scope
        .as_deref()
        .filter(|s| matches!(*s, "user" | "system"))
        .ok_or_else(|| {
            Error::InvalidInput("Flatpak identity requires user or system scope".into())
        })?;
    Ok((&id.name, scope))
}
fn flatpak_query(runner: &dyn CommandRunner, args: &[&str]) -> Result<Option<String>, Error> {
    let spec = CommandSpec::new("flatpak", args);
    let result = runner.run(
        &spec,
        std::time::Duration::from_secs(60),
        &std::sync::atomic::AtomicBool::new(false),
    )?;
    if result.status != Some(0) {
        return Ok(None);
    }
    String::from_utf8(result.stdout)
        .map(|s| Some(s.trim().to_owned()))
        .map_err(|_| Error::Parse("flatpak output is not UTF-8".into()))
}
/// Flatpak has no dry run. The preview names exactly one configured remote that
/// offers each ref, its current commit and any missing runtime from that remote.
/// Execution uses --no-related, so locale/extension refs are never added implicitly.
fn flatpak_plan(
    runner: &dyn CommandRunner,
    install: &[PackageId],
    remove: &[PackageId],
) -> Result<Plan, Error> {
    if install.is_empty() == remove.is_empty() {
        return Err(Error::Unsupported(
            "Flatpak preview accepts either installs or removals, not an empty or mixed transaction".into(),
        ));
    }
    let requests = if remove.is_empty() { install } else { remove };
    let mut scopes = BTreeSet::new();
    for id in requests {
        if id.provider != ProviderId::Flatpak {
            return Err(Error::InvalidInput("provider mismatch".into()));
        }
        scopes.insert(flatpak_ref(id)?.1);
    }
    if scopes.len() != 1 {
        return Err(Error::Safety(
            "one Flatpak transaction targets one installation scope".into(),
        ));
    }
    let scope = *scopes.first().expect("one scope");
    let scope_flag = format!("--{scope}");
    let observed = NativeBackend {
        provider: ProviderId::Flatpak,
        runner,
    }
    .inventory()?;
    let installed = |name: &str, any_scope: bool| {
        observed.iter().any(|p| {
            p.id.name == name && (any_scope || p.id.architecture_or_scope.as_deref() == Some(scope))
        })
    };
    let mut operations = vec![];
    let mut warnings = vec![
        "Flatpak has no native dry run: this preview is the remote's current ref and commit, not a simulated transaction.".into(),
        "Execution uses --no-related: locale and extension refs are not installed implicitly.".into(),
    ];
    if remove.is_empty() {
        let remotes = flatpak_query(runner, &["remotes", &scope_flag, "--columns=name"])?
            .ok_or_else(|| Error::Unavailable("cannot list Flatpak remotes".into()))?;
        let remotes: Vec<_> = remotes
            .lines()
            .map(str::trim)
            .filter(|r| !r.is_empty())
            .collect();
        let mut chosen = None::<String>;
        for id in install {
            let (reference, _) = flatpak_ref(id)?;
            if installed(reference, false) {
                warnings.push(format!("{id} is already installed; nothing to do."));
                continue;
            }
            let mut offered = vec![];
            for remote in &remotes {
                if !crate::identity::token_like(remote, b"._-") {
                    return Err(Error::Parse("unexpected Flatpak remote name".into()));
                }
                if let Some(commit) = flatpak_query(
                    runner,
                    &[
                        "remote-info",
                        &scope_flag,
                        "--show-commit",
                        remote,
                        reference,
                    ],
                )? {
                    offered.push(((*remote).to_owned(), commit));
                }
            }
            let [(remote, commit)] = offered.as_slice() else {
                return Err(Error::Safety(format!(
                    "{id} must be offered by exactly one configured {scope} remote (found {}); xpac never chooses an origin",
                    offered.len()
                )));
            };
            if commit.len() != 64 || !commit.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(Error::Parse(
                    "Flatpak remote commit is not a full digest".into(),
                ));
            }
            if chosen.get_or_insert_with(|| remote.clone()) != remote {
                return Err(Error::Safety(
                    "one Flatpak transaction uses one remote".into(),
                ));
            }
            if reference.starts_with("app/") {
                let runtime = flatpak_query(
                    runner,
                    &[
                        "remote-info",
                        &scope_flag,
                        "--show-runtime",
                        remote,
                        reference,
                    ],
                )?
                .ok_or_else(|| Error::Unavailable("cannot read the app's runtime".into()))?;
                let runtime_ref = format!("runtime/{runtime}");
                if !installed(&runtime_ref, true) {
                    let runtime_id =
                        parse_name(ProviderId::Flatpak, &format!("{runtime_ref}:{scope}"))?;
                    let runtime_commit = flatpak_query(
                        runner,
                        &["remote-info", &scope_flag, "--show-commit", remote, &runtime_ref],
                    )?
                    .ok_or_else(|| Error::Safety(format!(
                        "{runtime_id} is required but not offered by {remote}; install it explicitly first"
                    )))?;
                    if !operations
                        .iter()
                        .any(|op: &Operation| op.package == runtime_id)
                    {
                        operations.push(Operation {
                            kind: OperationKind::Install,
                            package: runtime_id,
                            risk: RiskLevel::RequiresReview,
                            native_detail: format!(
                                "{remote}\t{runtime_commit}\truntime dependency"
                            ),
                        });
                    }
                }
            }
            operations.push(Operation {
                kind: OperationKind::Install,
                package: id.clone(),
                risk: RiskLevel::RequiresReview,
                native_detail: format!("{remote}\t{commit}\trequested"),
            });
        }
    } else {
        for id in remove {
            let (reference, _) = flatpak_ref(id)?;
            if !reference.starts_with("app/") {
                return Err(Error::Safety(
                    "Flatpak runtime removal is not supported; apps may depend on it".into(),
                ));
            }
            if !installed(reference, false) {
                return Err(Error::Unavailable(format!("{id} is not installed")));
            }
            operations.push(Operation {
                kind: OperationKind::Remove,
                package: id.clone(),
                risk: RiskLevel::RequiresReview,
                native_detail: "uninstall; its runtime stays installed".into(),
            });
        }
    }
    Ok(Plan {
        operations,
        observed_state_fingerprint: crate::planner::inventory_token(&observed),
        observed_packages: observed,
        uncertain: true,
        warnings,
    })
}

/// Homebrew environment for every xpac-built brew command: no auto-update,
/// analytics, implicit cleanup, hints or colors.
pub fn brew_spec(args: &[&str]) -> CommandSpec {
    let mut spec = CommandSpec::new("brew", args);
    for key in [
        "HOMEBREW_NO_AUTO_UPDATE",
        "HOMEBREW_NO_ANALYTICS",
        "HOMEBREW_NO_INSTALL_CLEANUP",
        "HOMEBREW_NO_ENV_HINTS",
        "HOMEBREW_NO_COLOR",
        "HOMEBREW_NO_AUTOREMOVE",
    ] {
        spec.env.insert(key.into(), "1".into());
    }
    spec
}
fn brew_formula(id: &PackageId) -> Result<&str, Error> {
    validate(id)?;
    if id.architecture_or_scope.as_deref() != Some("formula") {
        return Err(Error::Unsupported(
            "Homebrew cask execution; only formulae are supported".into(),
        ));
    }
    Ok(&id.name)
}
/// Parse `brew install --dry-run` sections ("Would install N formula(e)" and
/// "Would install N dependencies for X") into name/version rows. Recent Homebrew
/// omits dependency versions; those rows carry an empty version.
pub fn parse_brew_dry_run(text: &str) -> Result<Vec<(String, String, bool)>, Error> {
    let mut rows = vec![];
    let mut section = None::<bool>;
    for line in text.lines() {
        if let Some(header) = line.strip_prefix("==> ") {
            section = if header.starts_with("Would install") {
                Some(header.contains(" for "))
            } else if header.starts_with("Would ") {
                return Err(Error::Safety(format!(
                    "Homebrew dry run reports an unsupported action: {header}"
                )));
            } else {
                None
            };
            continue;
        }
        let Some(dependency) = section else { continue };
        if line.trim().is_empty() {
            continue;
        }
        for item in line.split(", ") {
            let mut fields = item.split_whitespace();
            let (Some(name), version, None) = (fields.next(), fields.next(), fields.next()) else {
                return Err(Error::Parse("unexpected Homebrew dry-run row".into()));
            };
            let version = version.unwrap_or_default();
            parse_name(ProviderId::Brew, &format!("{name}:formula"))?;
            if !version
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b))
            {
                return Err(Error::Parse("unexpected Homebrew version".into()));
            }
            rows.push((name.to_owned(), version.to_owned(), dependency));
        }
    }
    Ok(rows)
}
fn brew_plan(
    runner: &dyn CommandRunner,
    install: &[PackageId],
    remove: &[PackageId],
) -> Result<Plan, Error> {
    if install.is_empty() == remove.is_empty() {
        return Err(Error::Unsupported(
            "Homebrew preview accepts either installs or removals, not an empty or mixed transaction".into(),
        ));
    }
    let requests = if remove.is_empty() { install } else { remove };
    for id in requests {
        if id.provider != ProviderId::Brew {
            return Err(Error::InvalidInput("provider mismatch".into()));
        }
        brew_formula(id)?;
    }
    let observed = NativeBackend {
        provider: ProviderId::Brew,
        runner,
    }
    .inventory()?;
    let mut operations = vec![];
    let mut warnings = vec![
        "Homebrew --dry-run lists formulae to install; post-install steps, linking and source builds are not modeled.".into(),
        "Homebrew cannot pin versions; execution verifies the installed versions against this preview.".into(),
    ];
    if remove.is_empty() {
        let mut spec = brew_spec(&["install", "--dry-run", "--formula"]);
        spec.args.extend(install.iter().map(|id| id.name.clone()));
        let mut rows = parse_brew_dry_run(&output(runner, spec)?)?;
        let unversioned: Vec<String> = rows
            .iter()
            .filter(|row| row.1.is_empty())
            .map(|row| row.0.clone())
            .collect();
        if !unversioned.is_empty() {
            let mut info = brew_spec(&["info", "--json=v2", "--formula"]);
            info.args.extend(unversioned.iter().cloned());
            let value: serde_json::Value = serde_json::from_str(&output(runner, info)?)
                .map_err(|_| Error::Parse("invalid Homebrew JSON".into()))?;
            for formula in value["formulae"].as_array().into_iter().flatten() {
                let (Some(name), Some(stable)) = (
                    formula["name"].as_str(),
                    formula["versions"]["stable"].as_str(),
                ) else {
                    continue;
                };
                let version = match formula["revision"].as_u64() {
                    Some(0) | None => stable.to_owned(),
                    Some(revision) => format!("{stable}_{revision}"),
                };
                for row in rows
                    .iter_mut()
                    .filter(|row| row.0 == name && row.1.is_empty())
                {
                    row.1 = version.clone();
                }
            }
            if rows.iter().any(|row| row.1.is_empty()) {
                return Err(Error::Parse(
                    "cannot determine a Homebrew dependency version".into(),
                ));
            }
        }
        for (name, version, dependency) in rows {
            operations.push(Operation {
                kind: OperationKind::Install,
                package: parse_name(ProviderId::Brew, &format!("{name}:formula"))?,
                risk: RiskLevel::RequiresReview,
                native_detail: format!(
                    "{name}\t{version}\t{}",
                    if dependency {
                        "dependency"
                    } else {
                        "requested"
                    }
                ),
            });
        }
        for id in install {
            if !operations.iter().any(|op| op.package.name == id.name) {
                warnings.push(format!("{id} is already installed; nothing to do."));
            }
        }
    } else {
        for id in remove {
            let kegs = observed.iter().filter(|p| p.id == *id).count();
            if kegs != 1 {
                return Err(Error::Safety(format!(
                    "{id} must have exactly one installed keg to remove (found {kegs})"
                )));
            }
            let dependents = output(
                runner,
                brew_spec(&["uses", "--installed", "--formula", &id.name]),
            )?;
            if !dependents.trim().is_empty() {
                return Err(Error::Safety(format!(
                    "{id} is required by installed formulae: {}",
                    dependents.split_whitespace().collect::<Vec<_>>().join(", ")
                )));
            }
            operations.push(Operation {
                kind: OperationKind::Remove,
                package: id.clone(),
                risk: RiskLevel::RequiresReview,
                native_detail: "brew uninstall --formula; dependencies stay installed".into(),
            });
        }
    }
    Ok(Plan {
        operations,
        observed_state_fingerprint: crate::planner::inventory_token(&observed),
        observed_packages: observed,
        uncertain: true,
        warnings,
    })
}
