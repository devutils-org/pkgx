//! Alpine's apk (apk-tools 2.12 and later, 3.x included): installed inventory,
//! a native `--simulate` preview and the WORLD constraints that say which
//! packages were asked for (ADR 014).
use crate::{
    command::{CommandRunner, CommandSpec},
    identity::{parse_name, validate},
    providers::output,
    Error, InstallReason, InstalledPackage, Operation, OperationKind, PackageId, Plan, ProviderId,
    RiskLevel,
};
use std::collections::{BTreeMap, BTreeSet};

/// The WORLD file apk records every requested constraint in.
pub const WORLD: &str = "/etc/apk/world";
/// Executor-built apk transactions; sudo delegation accepts only these prefixes.
/// No `--update-cache`, `--upgrade`, `--force*` or `--allow-untrusted`, ever.
pub const APK_ADD: &[&str] = &["apk", "add", "--no-interactive", "--no-progress", "--"];
pub const APK_DEL: &[&str] = &["apk", "del", "--no-interactive", "--no-progress", "--"];

/// Split `name-version-rN` at the version: apk versions hold no `-` but the
/// one before the mandatory release, so the name is everything before the
/// second hyphen from the end.
pub fn split_name_version(text: &str) -> Result<(&str, &str), Error> {
    let invalid = || Error::Parse(format!("invalid apk package {text:?}"));
    let (rest, release) = text.rsplit_once('-').ok_or_else(invalid)?;
    let (name, version) = rest.rsplit_once('-').ok_or_else(invalid)?;
    if name.is_empty()
        || !release
            .strip_prefix('r')
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        || !version.as_bytes().first().is_some_and(u8::is_ascii_digit)
        || !version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._+~".contains(&b))
    {
        return Err(invalid());
    }
    Ok((name, &text[name.len() + 1..]))
}

/// The package names WORLD asks for. Version or repository constraints
/// (`foo>=1`, `foo@edge`) name their package; conflicts (`!foo`) and
/// provides (`so:libz.so.1`, `cmd:sh`) name none.
pub fn world_names(world: &str) -> BTreeSet<String> {
    world
        .split_whitespace()
        .filter(|atom| !atom.starts_with('!') && !atom.contains(':'))
        .map(|atom| {
            atom.split(|c: char| "=<>~@".contains(c))
                .next()
                .unwrap_or_default()
                .to_owned()
        })
        .filter(|name| !name.is_empty())
        .collect()
}

/// Parse `apk list --installed`: `name-version-rN arch {origin} (license) [installed]`.
/// The license may hold spaces and parentheses, so only the first three
/// columns are read.
pub fn parse_installed(list: &str, world: &str) -> Result<Vec<InstalledPackage>, Error> {
    let explicit = world_names(world);
    let mut packages = vec![];
    for line in list.lines().filter(|line| !line.trim().is_empty()) {
        let mut columns = line.split_whitespace();
        let (Some(name_version), Some(arch), Some(origin)) =
            (columns.next(), columns.next(), columns.next())
        else {
            return Err(Error::Parse(
                "apk list row expected package, arch, origin".into(),
            ));
        };
        let origin = origin
            .strip_prefix('{')
            .and_then(|o| o.strip_suffix('}'))
            .ok_or_else(|| Error::Parse("apk list row lacks its {origin}".into()))?;
        if !line.trim_end().ends_with("[installed]") {
            return Err(Error::Parse(
                "apk list --installed row is not [installed]".into(),
            ));
        }
        let (name, version) = split_name_version(name_version)?;
        packages.push(InstalledPackage {
            id: parse_name(ProviderId::Apk, &format!("{name}:{arch}"))?,
            version: version.into(),
            install_reason: if explicit.contains(name) {
                InstallReason::Explicit
            } else {
                // apk installs nothing else: what WORLD does not name is there
                // because something it names depends on it.
                InstallReason::Dependency
            },
            dependencies: vec![],
            metadata: BTreeMap::from([
                ("origin".into(), origin.into()),
                (
                    "reason_status".into(),
                    "explicit means a WORLD constraint names the package".into(),
                ),
                ("dependencies_status".into(), "not_queried".into()),
            ]),
        });
    }
    packages.sort_by_key(|p| p.id.to_string());
    if packages.windows(2).any(|w| w[0].id == w[1].id) {
        return Err(Error::Parse("duplicate installed identity".into()));
    }
    Ok(packages)
}

pub fn inventory(runner: &dyn CommandRunner) -> Result<Vec<InstalledPackage>, Error> {
    let list = output(runner, CommandSpec::new("apk", &["list", "--installed"]))?;
    let world = output(runner, CommandSpec::new("cat", &["--", WORLD]))?;
    parse_installed(&list, &world)
}

/// One `(i/n) Verb name (detail)` row of an apk transaction.
fn transaction_row(line: &str) -> Option<(&str, &str, &str)> {
    let rest = line.strip_prefix('(')?;
    let (counter, rest) = rest.split_once(") ")?;
    // apk right-aligns the counter from ten operations on: `( 1/10)`.
    let (done, total) = counter.trim_start().split_once('/')?;
    if !done
        .bytes()
        .chain(total.bytes())
        .all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let (verb, rest) = rest.split_once(' ')?;
    let (name, detail) = rest.split_once(" (")?;
    Some((verb, name, detail.strip_suffix(')')?))
}

/// `apk add --simulate` / `apk del --simulate`, which resolve against the
/// local indexes without the database lock and without root. apk has no way
/// to pin a version without writing the constraint into WORLD for good, so
/// the executor installs by name and verifies the reviewed versions after.
pub fn plan(
    runner: &dyn CommandRunner,
    install: &[PackageId],
    remove: &[PackageId],
) -> Result<Plan, Error> {
    if install.is_empty() == remove.is_empty() {
        return Err(Error::Unsupported(
            "apk preview accepts either installs or removals, not an empty or mixed transaction"
                .into(),
        ));
    }
    let requests = if remove.is_empty() { install } else { remove };
    for id in requests {
        validate(id)?;
        if id.provider != ProviderId::Apk {
            return Err(Error::InvalidInput("provider mismatch".into()));
        }
    }
    if requests.iter().any(|id| id.architecture_or_scope.is_some()) {
        let native = output(runner, CommandSpec::new("apk", &["--print-arch"]))?;
        if requests.iter().any(|id| {
            id.architecture_or_scope
                .as_deref()
                .is_some_and(|arch| arch != native.trim() && arch != "noarch")
        }) {
            return Err(Error::Safety(format!(
                "apk installs {} packages here; another architecture is not requested",
                native.trim()
            )));
        }
    }
    let observed = inventory(runner)?;
    let template = if remove.is_empty() { APK_ADD } else { APK_DEL };
    let mut spec = CommandSpec::new(template[0], &[template[1], "--simulate"]);
    spec.args
        .extend(template[2..].iter().map(|s| (*s).to_owned()));
    spec.args.extend(requests.iter().map(|id| id.name.clone()));
    let native = output(runner, spec)?;
    if native.contains("are not removed due to") {
        // apk then exits 0 having changed WORLD alone: refused before it can.
        let held: Vec<_> = native
            .lines()
            .skip_while(|line| !line.contains("are not removed due to"))
            .skip(1)
            .take_while(|line| !line.trim().is_empty())
            .map(str::trim)
            .collect();
        return Err(Error::Safety(format!(
            "apk would update WORLD without removing the package, which installed packages need: {}",
            held.join(" ")
        )));
    }
    let mut operations = vec![];
    let mut warnings = vec![
        "apk --simulate resolves against the local indexes; execution never refreshes them (no --update-cache).".into(),
        "apk cannot pin a version without recording it in WORLD; execution installs by name and verifies the reviewed versions afterwards.".into(),
    ];
    for line in native.lines() {
        let Some((verb, name, detail)) = transaction_row(line) else {
            if line.starts_with('(') {
                return Err(Error::Parse(format!(
                    "unexpected apk transaction row {line:?}"
                )));
            }
            continue;
        };
        let kind = match (verb, remove.is_empty()) {
            ("Installing", true) => OperationKind::Install,
            ("Upgrading", true) => {
                warnings.push(format!(
                    "{name} ({detail}) upgrades an installed package; review partial-upgrade risk."
                ));
                OperationKind::Install
            }
            ("Purging", false) => OperationKind::Remove,
            _ => {
                return Err(Error::Safety(format!(
                    "apk transaction action {verb:?} on {name} is not supported for this request"
                )))
            }
        };
        // `old -> new` for an upgrade: the version that will be installed.
        let version = detail.rsplit(" -> ").next().unwrap_or(detail);
        split_name_version(&format!("{name}-{version}"))?;
        operations.push(Operation {
            kind,
            package: parse_name(ProviderId::Apk, name)?,
            risk: RiskLevel::RequiresReview,
            native_detail: format!("{name}\t{version}\t{verb}"),
        });
    }
    if remove.is_empty() {
        for id in install {
            if operations.iter().any(|op| op.package.name == id.name) {
                continue;
            }
            let installed: Vec<_> = observed.iter().filter(|p| p.id.name == id.name).collect();
            match installed.as_slice() {
                [package] if package.install_reason == InstallReason::Explicit => warnings.push(
                    format!("{id} is already installed and in WORLD; nothing to do."),
                ),
                // Installed for another package: `apk add` records it in WORLD.
                [package] => operations.push(Operation {
                    kind: OperationKind::MarkExplicit,
                    package: package.id.clone(),
                    risk: RiskLevel::RequiresReview,
                    native_detail: format!("{}\t{}\tWORLD", id.name, package.version),
                }),
                _ => {
                    return Err(Error::Safety(format!(
                        "apk resolved no action for {id} and it is not installed exactly once"
                    )))
                }
            }
        }
    } else {
        for id in remove {
            if !operations
                .iter()
                .any(|op| op.package.name == id.name && op.kind == OperationKind::Remove)
            {
                return Err(Error::Safety(format!("apk would not remove {id}")));
            }
        }
        warnings.push("apk del also purges the dependencies nothing else needs any more; every package it removes is listed.".into());
    }
    Ok(Plan {
        operations,
        observed_state_fingerprint: crate::planner::inventory_token(&observed),
        observed_packages: observed,
        uncertain: true,
        warnings,
    })
}

/// The version an apk operation reviewed (`name\tversion\tverb`).
pub fn reviewed_version(operation: &Operation) -> Option<&str> {
    operation
        .native_detail
        .split('\t')
        .nth(1)
        .filter(|version| {
            !version.is_empty()
                && version
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._+~-".contains(&b))
        })
}
