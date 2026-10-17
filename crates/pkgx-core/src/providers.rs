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
                (Some(true), Some(false)) => InstallReason::Explicit,
                (Some(false), Some(true)) => InstallReason::Dependency,
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
                Capability::Dependencies,
                Capability::Orphans,
                Capability::DryRun,
            ],
            ProviderId::Dnf => vec![Capability::Inventory, Capability::Dependencies],
            ProviderId::Flatpak => vec![Capability::Inventory],
            ProviderId::Brew => vec![
                Capability::Inventory,
                Capability::Dependencies,
                Capability::Orphans,
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
        if operations.is_empty() {
            return Err(Error::Parse(
                "Pacman print produced no operation records".into(),
            ));
        }
        Ok(Plan { operations, observed_state_fingerprint: crate::planner::inventory_token(&observed), observed_packages: observed.clone(), uncertain: true, warnings: vec!["Pacman --print is a target/dependency estimate, not a complete transaction simulation; conflict replacements and hooks are not modeled.".into(), "Pacman execution is not implemented. HoldPkg and system-critical package policy must be reviewed before an executor is added.".into()] })
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
