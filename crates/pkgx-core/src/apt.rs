use crate::{
    command::{checked, CommandRunner, CommandSpec},
    Error, InstallReason, InstalledPackage, PackageId, ProviderId,
};
use std::collections::{BTreeMap, BTreeSet};
pub fn inventory(runner: &dyn CommandRunner) -> Result<Vec<InstalledPackage>, Error> {
    let output = checked(
        runner,
        &CommandSpec::new(
            "dpkg-query",
            &[
                "-W",
                "-f=${Package}\t${Architecture}\t${Version}\t${db:Status-Status}\n",
            ],
        ),
    )?;
    let text = String::from_utf8(output.stdout).map_err(|e| Error::Parse(e.to_string()))?;
    let mut packages = parse_inventory(&text)?;
    // Failure to read APT marks must never imply that packages were explicitly installed.
    let marks = ["showmanual", "showauto"].map(|kind| {
        checked(runner, &CommandSpec::new("apt-mark", &[kind]))
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.lines().map(str::to_owned).collect::<BTreeSet<_>>())
    });
    if let [Some(manual), Some(auto)] = marks {
        let counts =
            packages
                .iter()
                .fold(BTreeMap::<String, usize>::new(), |mut counts, package| {
                    *counts.entry(package.id.name.clone()).or_default() += 1;
                    counts
                });
        for package in &mut packages {
            let name = &package.id.name;
            let qualified = format!(
                "{name}:{}",
                package
                    .id
                    .architecture_or_scope
                    .as_deref()
                    .unwrap_or_default()
            );
            let contains = |set: &BTreeSet<String>| {
                set.contains(&qualified) || (counts.get(name) == Some(&1) && set.contains(name))
            };
            package.install_reason = match (contains(&manual), contains(&auto)) {
                (true, false) => InstallReason::Explicit,
                (false, true) => InstallReason::Dependency,
                _ => InstallReason::Unknown,
            };
        }
    }
    Ok(packages)
}
pub fn parse_inventory(text: &str) -> Result<Vec<InstalledPackage>, Error> {
    let mut result = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let fields: Vec<_> = line.split('\t').collect();
        if fields.len() != 4 {
            return Err(Error::Parse(format!(
                "dpkg row {}: expected four nonempty fields",
                i + 1
            )));
        }
        if fields[3] != "installed" {
            continue;
        }
        if fields.iter().any(|s| s.is_empty()) {
            return Err(Error::Parse(format!(
                "dpkg row {}: installed package has empty fields",
                i + 1
            )));
        }
        let id = package_id(&format!("{}:{}", fields[0], fields[1]))?;
        if result.iter().any(|p: &InstalledPackage| p.id == id) {
            return Err(Error::Parse("duplicate installed package identity".into()));
        }
        result.push(InstalledPackage {
            id,
            version: fields[2].into(),
            install_reason: InstallReason::Unknown,
            dependencies: vec![],
            metadata: BTreeMap::from([("dependencies_status".into(), "not_queried".into())]),
        });
    }
    result.sort_by(|a, b| {
        (&a.id.name, &a.id.architecture_or_scope).cmp(&(&b.id.name, &b.id.architecture_or_scope))
    });
    Ok(result)
}

use crate::{
    backend::{Backend, PackageInfo, PackageProvenance, SearchResult},
    Capability, Operation, OperationKind, Plan, RiskLevel,
};

pub struct AptBackend<'a> {
    pub runner: &'a dyn CommandRunner,
}

/// Accept only Debian binary package names and optional concrete architecture.
/// APT's option/regex/version/release/action selector syntax is intentionally excluded.
pub fn package_id(input: &str) -> Result<PackageId, Error> {
    if ["brew:", "pacman:", "dnf:", "flatpak:", "script:"]
        .iter()
        .any(|prefix| input.starts_with(prefix))
    {
        return Err(Error::InvalidInput(
            "APT cannot handle another provider".into(),
        ));
    }
    let input = input.strip_prefix("apt:").unwrap_or(input);
    let mut parts = input.split(':');
    let name = parts.next().unwrap_or_default();
    let arch = parts.next();
    let valid_name = name.len() >= 2
        && name.as_bytes()[0].is_ascii_lowercase()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"+.-".contains(&b));
    // Debian allows a digit as the first character too.
    let valid_name = valid_name
        || (name.len() >= 2
            && name.as_bytes()[0].is_ascii_digit()
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"+.-".contains(&b)));
    let valid_arch = arch.is_none_or(|a| {
        !a.is_empty()
            && a != "any"
            && a != "native"
            && a.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    });
    if !valid_name || !valid_arch || parts.next().is_some() {
        return Err(Error::InvalidInput(
            "expected apt:package[:architecture] with a concrete Debian package name".into(),
        ));
    }
    Ok(PackageId {
        provider: ProviderId::Apt,
        name: name.into(),
        architecture_or_scope: arch.map(str::to_owned),
    })
}
pub fn native_name(id: &PackageId) -> Result<String, Error> {
    if id.provider != ProviderId::Apt {
        return Err(Error::InvalidInput(
            "APT cannot handle another provider".into(),
        ));
    }
    let name = match &id.architecture_or_scope {
        Some(a) => format!("{}:{a}", id.name),
        None => id.name.clone(),
    };
    package_id(&name)?;
    Ok(name)
}
fn text(runner: &dyn CommandRunner, spec: CommandSpec) -> Result<String, Error> {
    String::from_utf8(checked(runner, &spec)?.stdout)
        .map_err(|_| Error::Parse("backend output is not UTF-8".into()))
}
fn cache(args: &[&str]) -> CommandSpec {
    // Disable on-disk cache generation, including in custom APT configurations.
    let mut spec = CommandSpec::new(
        "apt-cache",
        &[
            "-o",
            "Dir::Cache::pkgcache=",
            "-o",
            "Dir::Cache::srcpkgcache=",
        ],
    );
    spec.args.extend(args.iter().map(|s| (*s).into()));
    spec
}
pub fn parse_control(input: &str) -> Result<Vec<BTreeMap<String, String>>, Error> {
    let mut records = vec![];
    let mut record = BTreeMap::<String, String>::new();
    let mut last = None::<String>;
    for line in input.lines().chain(std::iter::once("")) {
        if line.is_empty() {
            if !record.is_empty() {
                records.push(std::mem::take(&mut record));
            }
            last = None;
        } else if line.starts_with([' ', '\t']) {
            let key = last
                .as_ref()
                .ok_or_else(|| Error::Parse("control continuation without field".into()))?;
            let field = record.get_mut(key).expect("last field exists");
            field.push('\n');
            field.push_str(line);
        } else {
            let (key, value) = line
                .split_once(':')
                .ok_or_else(|| Error::Parse("malformed control field".into()))?;
            if key.is_empty()
                || record
                    .insert(key.into(), value.trim_start().into())
                    .is_some()
            {
                return Err(Error::Parse("duplicate or empty control field".into()));
            }
            last = Some(key.into());
        }
    }
    Ok(records)
}
impl Backend for AptBackend<'_> {
    fn provider(&self) -> ProviderId {
        ProviderId::Apt
    }
    fn capabilities(&self) -> Vec<Capability> {
        vec![
            Capability::Inventory,
            Capability::Install,
            Capability::Remove,
            Capability::Search,
            Capability::Dependencies,
            Capability::DryRun,
            Capability::Orphans,
            Capability::ConfigInspection,
            Capability::PackageCache,
        ]
    }
    fn inventory(&self) -> Result<Vec<InstalledPackage>, Error> {
        inventory(self.runner)
    }
    fn search(&self, query: &str) -> Result<Vec<SearchResult>, Error> {
        if query.is_empty() || query.len() > 256 || query.chars().any(char::is_control) {
            return Err(Error::InvalidInput(
                "search requires 1–256 printable characters".into(),
            ));
        }
        // Search is explicitly a POSIX regex, passed after -- to prevent options.
        let output = text(self.runner, cache(&["search", "--", query]))?;
        output
            .lines()
            .map(|line| {
                let (name, summary) = line
                    .split_once(" - ")
                    .ok_or_else(|| Error::Parse("malformed search row".into()))?;
                Ok(SearchResult {
                    id: package_id(name)?,
                    summary: summary.into(),
                })
            })
            .collect()
    }
    fn info(&self, id: &PackageId) -> Result<PackageInfo, Error> {
        let name = native_name(id)?;
        let records = parse_control(&text(self.runner, cache(&["show", "--", &name]))?)?;
        if records.is_empty() {
            return Err(Error::Unavailable(
                "package has no available control records".into(),
            ));
        }
        Ok(PackageInfo {
            id: id.clone(),
            records,
        })
    }
    fn dependencies(&self, id: &PackageId) -> Result<crate::dependency::DependencyReport, Error> {
        let name = native_name(id)?;
        // Query the installed dpkg record, never an arbitrary available version.
        let raw = text(self.runner, CommandSpec::new("dpkg-query", &[
            "--show", "--showformat=${Package}\t${Architecture}\t${Version}\t${db:Status-Status}\t${Pre-Depends}\t${Depends}\t${Recommends}\t${Suggests}\t${Enhances}\t${Provides}\t${Conflicts}\t${Breaks}\t${Replaces}\n",
            "--", &name,
        ]))?;
        parse_installed_dependencies(&raw, id)
    }
    fn why(&self, id: &PackageId) -> Result<PackageProvenance, Error> {
        let name = native_name(id)?;
        let mut matches: Vec<_> = self
            .inventory()?
            .into_iter()
            .filter(|p| {
                p.id.name == id.name
                    && id
                        .architecture_or_scope
                        .as_ref()
                        .is_none_or(|a| p.id.architecture_or_scope.as_ref() == Some(a))
            })
            .collect();
        if matches.len() != 1 {
            return Err(Error::InvalidInput(
                "package is absent or ambiguous; specify installed architecture".into(),
            ));
        }
        let package = matches.remove(0);
        let confidence = if package.install_reason == InstallReason::Unknown {
            "unknown"
        } else {
            "native_mark_only"
        }
        .into();
        let reverse = text(
            self.runner,
            cache(&["--installed", "rdepends", "--", &name]),
        )?;
        Ok(PackageProvenance { package, origin: None, confidence, installed_reverse_dependencies: reverse, limitations: vec!["Native marks do not identify the person, script or transaction that installed a package.".into(), "Reverse dependencies are native APT output, not proof of historical installation cause.".into()] })
    }
    fn residual_configurations(&self) -> Result<Vec<PackageId>, Error> {
        let records = text(
            self.runner,
            CommandSpec::new(
                "dpkg-query",
                &[
                    "-W",
                    "-f=${Package}\t${Architecture}\t${db:Status-Status}\n",
                ],
            ),
        )?;
        parse_residual_configurations(&records)
    }
    fn package_cache(&self) -> Result<crate::cache::CacheReport, Error> {
        crate::cache::measure(self.runner, ProviderId::Apt)
    }
    fn orphan_candidates(&self) -> Result<Vec<PackageId>, Error> {
        let simulation = text(
            self.runner,
            CommandSpec::new(
                "apt-get",
                &[
                    "--simulate",
                    "-o",
                    "Dir::Cache::pkgcache=",
                    "-o",
                    "Dir::Cache::srcpkgcache=",
                    "-o",
                    "Debug::NoLocking=1",
                    "autoremove",
                ],
            ),
        )?;
        let operations = parse_simulation(&simulation)?;
        if operations
            .iter()
            .any(|operation| !matches!(operation.kind, OperationKind::Remove))
        {
            return Err(Error::Safety("native orphan simulation includes non-removal actions; review broken native state manually".into()));
        }
        if operations.is_empty() {
            return Ok(vec![]);
        }
        let ids: Vec<_> = operations
            .into_iter()
            .map(|operation| operation.package)
            .collect();
        // Reuse holds, Essential/Protected and architecture checks. No execution.
        let checked = self.plan(&[], &ids)?;
        Ok(checked
            .operations
            .into_iter()
            .filter(|operation| matches!(operation.kind, OperationKind::Remove))
            .map(|operation| operation.package)
            .collect())
    }
    fn plan(&self, install: &[PackageId], remove: &[PackageId]) -> Result<Plan, Error> {
        if install.is_empty() && remove.is_empty() {
            return Err(Error::InvalidInput(
                "plan requires at least one requested operation".into(),
            ));
        }
        let install: Vec<_> = install.iter().map(native_name).collect::<Result<_, _>>()?;
        let remove: Vec<_> = remove.iter().map(native_name).collect::<Result<_, _>>()?;
        if install.iter().any(|p| {
            remove
                .iter()
                .any(|r| p.split(':').next() == r.split(':').next())
        }) {
            return Err(Error::InvalidInput(
                "package requested for both install and removal".into(),
            ));
        }
        let observed = self.inventory()?;
        let holds = text(self.runner, CommandSpec::new("apt-mark", &["showhold"]))?;
        let protected = text(
            self.runner,
            CommandSpec::new(
                "dpkg-query",
                &[
                    "-W",
                    "-f=${Package}\t${Architecture}\t${Essential}\t${Protected}\n",
                ],
            ),
        )?;
        let mut spec = CommandSpec::new(
            "apt-get",
            &[
                "--simulate",
                "-o",
                "Dir::Cache::pkgcache=",
                "-o",
                "Dir::Cache::srcpkgcache=",
                "-o",
                "Debug::NoLocking=1",
                "-o",
                "APT::Get::allow-change-held-packages=false",
                "-o",
                "APT::Get::allow-remove-essential=false",
                "-o",
                "APT::Get::Purge=false",
                "-o",
                "APT::Get::force-yes=false",
                "install",
                "--",
            ],
        );
        if remove.is_empty() {
            spec.args.insert(0, "--no-remove".into());
        }
        spec.args.extend(install.iter().cloned());
        spec.args.extend(remove.iter().map(|p| format!("{p}-")));
        let simulation = text(self.runner, spec)?;
        let mut operations = parse_simulation(&simulation)?;
        for requested in &install {
            let requested = package_id(requested)?;
            let matches = |id: &PackageId| {
                id.name == requested.name
                    && requested
                        .architecture_or_scope
                        .as_ref()
                        .is_none_or(|a| id.architecture_or_scope.as_ref() == Some(a))
            };
            if !observed.iter().any(|package| matches(&package.id))
                && !operations.iter().any(|operation| {
                    matches!(operation.kind, OperationKind::Install) && matches(&operation.package)
                })
            {
                return Err(Error::Unsupported("APT resolved a virtual/replacement package instead of the requested concrete binary identity".into()));
            }
        }
        // Every executable action must identify one concrete architecture.
        let native_actions = operations.clone();
        for operation in &mut operations {
            if operation.package.architecture_or_scope.is_none() {
                let mut scopes: BTreeSet<_> = observed
                    .iter()
                    .filter(|p| p.id.name == operation.package.name)
                    .filter_map(|p| p.id.architecture_or_scope.clone())
                    .collect();
                for action in &native_actions {
                    if action.package.name == operation.package.name {
                        if let Some(scope) = &action.package.architecture_or_scope {
                            scopes.insert(scope.clone());
                        }
                    }
                }
                if scopes.len() != 1 {
                    return Err(Error::Safety(
                        "cannot unambiguously identify architecture of proposed native action"
                            .into(),
                    ));
                }
                operation.package.architecture_or_scope = scopes.into_iter().next();
            }
        }
        for requested in &install {
            let requested = package_id(requested)?;
            let matches: Vec<_> = observed
                .iter()
                .filter(|p| {
                    p.id.name == requested.name
                        && requested
                            .architecture_or_scope
                            .as_ref()
                            .is_none_or(|arch| p.id.architecture_or_scope.as_ref() == Some(arch))
                })
                .collect();
            if matches.len() > 1 {
                return Err(Error::Safety(
                    "manual-mark request is ambiguous; specify an installed architecture".into(),
                ));
            }
            for package in matches {
                if package.install_reason != InstallReason::Explicit {
                    // Installing a concrete already-installed dependency has a
                    // native manual-mark effect even without an unpack action.
                    operations.push(Operation {
                        kind: OperationKind::MarkExplicit,
                        package: package.id.clone(),
                        risk: RiskLevel::RequiresReview,
                        native_detail: format!(
                            "MarkExplicit {} ({}) previous_reason={:?}",
                            native_name(&package.id)?,
                            package.version,
                            package.install_reason
                        ),
                    });
                }
            }
        }
        for operation in &operations {
            let name = native_name(&operation.package)?;
            if holds
                .lines()
                .any(|h| h == name || h.split(':').next() == Some(operation.package.name.as_str()))
            {
                return Err(Error::Safety(format!("held package would change: {name}")));
            }
            if matches!(operation.kind, OperationKind::Remove) {
                if remove.is_empty() {
                    return Err(Error::Safety(
                        "additive installation would remove installed software".into(),
                    ));
                }
                let mut found_metadata = false;
                for line in protected.lines() {
                    let f: Vec<_> = line.split('\t').collect();
                    if f.len() != 4 {
                        return Err(Error::Parse("malformed protected-package metadata".into()));
                    }
                    if f[0] == operation.package.name {
                        found_metadata = true;
                    }
                    if f[0] == operation.package.name && (f[2] == "yes" || f[3] == "yes") {
                        return Err(Error::Safety(format!(
                            "protected package would be removed: {name}"
                        )));
                    }
                }
                if !found_metadata {
                    return Err(Error::Safety(format!(
                        "no protection metadata for proposed removal: {name}"
                    )));
                }
            }
        }
        Ok(Plan {
            operations,
            observed_state_fingerprint: crate::planner::inventory_token(&observed), observed_packages: observed.clone(),
            uncertain: true,
            warnings: vec![
                "Native preview: execution requires confirmation and a matching fresh simulation immediately before execution.".into(),
                "APT simulation does not lock state; versions, repositories and native configuration may change.".into(),
            ],
        })
    }
}
pub fn parse_simulation(input: &str) -> Result<Vec<Operation>, Error> {
    let mut operations = vec![];
    for line in input.lines() {
        let mut fields = line.split_whitespace();
        let kind = match fields.next() {
            Some("Inst") => OperationKind::Install,
            Some("Remv") => OperationKind::Remove,
            Some("Purg") => {
                return Err(Error::Safety(
                    "native simulation proposes a purge; configuration deletion is unsupported"
                        .into(),
                ))
            }
            Some("Conf") => OperationKind::Configure,
            _ => continue,
        };
        let name = fields
            .next()
            .ok_or_else(|| Error::Parse("simulation action missing package".into()))?;
        let mut package = package_id(name)?;
        if package.architecture_or_scope.is_none() && line.contains(" (") {
            if let Some((_, tail)) = line.rsplit_once('[') {
                if let Some((arch, suffix)) = tail.split_once(']') {
                    if suffix.starts_with(')') {
                        package = package_id(&format!("{name}:{arch}"))?;
                    }
                }
            }
        }
        operations.push(Operation {
            kind,
            package,
            risk: RiskLevel::RequiresReview,
            native_detail: line.into(),
        });
    }
    Ok(operations)
}

/// Native config-files status identifies a residual package record, not owned
/// filesystem paths and never permission to purge or delete those paths.
pub fn parse_residual_configurations(input: &str) -> Result<Vec<PackageId>, Error> {
    let mut result = vec![];
    for line in input.lines() {
        let fields: Vec<_> = line.split('\t').collect();
        if fields.len() != 3 {
            return Err(Error::Parse(
                "residual configuration query expected three fields".into(),
            ));
        }
        if fields[2] != "config-files" {
            continue;
        }
        let id = package_id(&format!("{}:{}", fields[0], fields[1]))?;
        if result.contains(&id) {
            return Err(Error::Parse("duplicate residual package record".into()));
        }
        result.push(id);
    }
    result.sort_by_key(ToString::to_string);
    Ok(result)
}

/// dpkg's installed relationship fields retain alternatives and version qualifiers.
pub fn parse_installed_dependencies(
    raw: &str,
    requested: &PackageId,
) -> Result<crate::dependency::DependencyReport, Error> {
    native_name(requested)?;
    let mut matches = vec![];
    for line in raw.lines() {
        let fields: Vec<_> = line.split('\t').collect();
        if fields.len() != 13 {
            return Err(Error::Parse(
                "dpkg dependency row requires thirteen columns".into(),
            ));
        }
        let id = package_id(&format!("{}:{}", fields[0], fields[1]))?;
        if id.name != requested.name
            || requested
                .architecture_or_scope
                .as_ref()
                .is_some_and(|a| id.architecture_or_scope.as_ref() != Some(a))
        {
            return Err(Error::Parse(
                "dpkg dependency identity does not match request".into(),
            ));
        }
        if fields[3] != "installed" {
            continue;
        }
        if fields[2].is_empty() {
            return Err(Error::Parse("installed version is empty".into()));
        }
        let package = InstalledPackage {
            id,
            version: fields[2].into(),
            install_reason: InstallReason::Unknown,
            dependencies: vec![],
            metadata: BTreeMap::new(),
        };
        let mut report =
            crate::dependency::DependencyReport::native(package, "dpkg installed control fields");
        for (relation, expression) in [
            "Pre-Depends",
            "Depends",
            "Recommends",
            "Suggests",
            "Enhances",
            "Provides",
            "Conflicts",
            "Breaks",
            "Replaces",
        ]
        .into_iter()
        .zip(&fields[4..])
        {
            report.push(relation, expression);
        }
        matches.push(report);
    }
    if matches.len() != 1 {
        return Err(Error::InvalidInput(
            "installed dependency record is absent or ambiguous; specify concrete architecture"
                .into(),
        ));
    }
    Ok(matches.remove(0))
}
