//! Gentoo's Portage (3.0 and later): the installed package database, an
//! `emerge --pretend` preview and the world set that says which packages were
//! asked for (ADR 015).
use crate::{
    command::{CommandRunner, CommandSpec},
    identity::{parse_name, validate},
    providers::output,
    Error, InstallReason, InstalledPackage, Operation, OperationKind, PackageId, Plan, ProviderId,
    RiskLevel,
};
use std::collections::BTreeMap;

/// Portage's installed package database (`VDB_PATH` under `/`).
pub const VDB: &str = "/var/db/pkg";
/// The world set: the atoms the user asked for.
pub const WORLD: &str = "/var/lib/portage/world";
/// Executor-built emerge transactions; sudo delegation accepts only these
/// prefixes. Never `--update`, `--deep`, `--newuse`, `--oneshot`, an
/// autounmask that writes configuration, or `--unmerge`, which removes what
/// installed packages need. `--depclean` removes a package only when nothing
/// installed needs it, and deselects it from world.
pub const EMERGE_INSTALL: &[&str] = &[
    "emerge",
    "--ask=n",
    "--noreplace",
    "--autounmask=n",
    "--nospinner",
    "--color=n",
    "--quiet",
    "--",
];
pub const EMERGE_REMOVE: &[&str] = &[
    "emerge",
    "--ask=n",
    "--depclean",
    "--nospinner",
    "--color=n",
    "--",
];

/// A version by the package manager specification: digits and dots, one
/// optional letter, `_alpha|_beta|_pre|_rc|_p` suffixes with optional
/// numbers, and an optional `-rN` revision.
pub fn is_version(text: &str) -> bool {
    let body = match text.rsplit_once("-r") {
        Some((body, n)) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => body,
        _ => text,
    };
    let mut parts = body.split('_');
    let numeric = parts.next().unwrap_or_default();
    let numbers = numeric
        .strip_suffix(|c: char| c.is_ascii_lowercase())
        .unwrap_or(numeric);
    if numbers.is_empty()
        || !numbers
            .split('.')
            .all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    {
        return false;
    }
    parts.all(|suffix| {
        ["alpha", "beta", "pre", "rc", "p"].iter().any(|kind| {
            suffix
                .strip_prefix(kind)
                .is_some_and(|n| n.bytes().all(|b| b.is_ascii_digit()))
        })
    })
}

/// Split `name-version` (a VDB directory, or a merge-list entry without its
/// category) at the first hyphen whose remainder is a version: a package
/// name may not end in a hyphen and a version.
pub fn split_name_version(text: &str) -> Result<(&str, &str), Error> {
    text.match_indices('-')
        .map(|(at, _)| (&text[..at], &text[at + 1..]))
        .find(|(name, version)| !name.is_empty() && is_version(version))
        .ok_or_else(|| Error::Parse(format!("invalid Portage package {text:?}")))
}

/// The world set's atoms, by `category/package`, each with the slot it is
/// restricted to (`dev-lang/python:3.12`), if any. A repository qualifier
/// (`::gentoo`) names no different package.
pub fn world_atoms(world: &str) -> Vec<(String, Option<String>)> {
    world
        .split_whitespace()
        .filter(|atom| !atom.starts_with('@') && atom.contains('/'))
        .map(|atom| {
            let atom = atom.split("::").next().unwrap_or(atom);
            match atom.split_once(':') {
                Some((name, slot)) => (name.to_owned(), Some(slot.to_owned())),
                None => (atom.to_owned(), None),
            }
        })
        .collect()
}

/// Parse `grep -r --include=SLOT --include=repository -H . VDB`: one
/// `VDB/category/name-version/FILE:value` row per file, grouped by package.
pub fn parse_installed(rows: &str, world: &str) -> Result<Vec<InstalledPackage>, Error> {
    let atoms = world_atoms(world);
    let mut found = BTreeMap::<(String, String), BTreeMap<String, String>>::new();
    for row in rows.lines().filter(|row| !row.is_empty()) {
        let (path, value) = row
            .split_once(':')
            .ok_or_else(|| Error::Parse("Portage database row lacks a value".into()))?;
        let mut parts = path
            .strip_prefix(VDB)
            .and_then(|p| p.strip_prefix('/'))
            .ok_or_else(|| Error::Parse("Portage database row outside the VDB".into()))?
            .split('/');
        let (Some(category), Some(package), Some(file), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(Error::Parse(format!(
                "unexpected Portage database path {path:?}"
            )));
        };
        if found
            .entry((category.into(), package.into()))
            .or_default()
            .insert(file.into(), value.trim().into())
            .is_some()
        {
            return Err(Error::Parse("duplicate Portage database file".into()));
        }
    }
    let mut packages = vec![];
    for ((category, package), files) in found {
        let (name, version) = split_name_version(&package)?;
        let slot_line = files
            .get("SLOT")
            .ok_or_else(|| Error::Parse(format!("{category}/{package} has no SLOT")))?;
        let (slot, subslot) = slot_line.split_once('/').unwrap_or((slot_line, ""));
        let atom = format!("{category}/{name}");
        let id = parse_name(ProviderId::Portage, &format!("{atom}:{slot}"))?;
        let selected = atoms
            .iter()
            .any(|(n, s)| *n == atom && s.as_deref().is_none_or(|s| s == slot));
        let mut metadata = BTreeMap::from([
            (
                "reason_status".into(),
                "Portage records selections (the world set), not reasons: not selected means a dependency or a member of the system set".into(),
            ),
            ("dependencies_status".into(), "not_queried".into()),
        ]);
        if let Some(repository) = files.get("repository") {
            metadata.insert("repository".into(), repository.clone());
        }
        if !subslot.is_empty() {
            metadata.insert("subslot".into(), subslot.into());
        }
        packages.push(InstalledPackage {
            id,
            version: version.into(),
            install_reason: if selected {
                InstallReason::Explicit
            } else {
                InstallReason::Unknown
            },
            dependencies: vec![],
            metadata,
        });
    }
    packages.sort_by_key(|p| p.id.to_string());
    if packages.windows(2).any(|w| w[0].id == w[1].id) {
        return Err(Error::Parse("duplicate installed identity".into()));
    }
    Ok(packages)
}

pub fn inventory(runner: &dyn CommandRunner) -> Result<Vec<InstalledPackage>, Error> {
    let rows = output(
        runner,
        CommandSpec::new(
            "grep",
            &[
                "-r",
                "--include=SLOT",
                "--include=repository",
                "-H",
                ".",
                "--",
                VDB,
            ],
        ),
    )?;
    let world = output(runner, CommandSpec::new("cat", &["--", WORLD]))?;
    parse_installed(&rows, &world)
}

/// One `[ebuild  N    ] category/name-version[::repo]` row of a merge list,
/// as `(type, flags, category/name, version)`. A binary package's
/// `-BUILD_ID` (`strace-7.1-3`) is no part of its version.
pub fn merge_row(line: &str) -> Result<Option<(String, String, String, String)>, Error> {
    let Some(rest) = line.strip_prefix('[') else {
        return Ok(None);
    };
    let (inside, rest) = rest
        .split_once(']')
        .ok_or_else(|| Error::Parse(format!("unexpected emerge row {line:?}")))?;
    let (kind, flags) = inside.split_once(' ').unwrap_or((inside, ""));
    let cpv = rest
        .split_whitespace()
        .next()
        .ok_or_else(|| Error::Parse(format!("emerge row names no package: {line:?}")))?;
    let cpv = cpv.split("::").next().unwrap_or(cpv);
    let (category, package) = cpv
        .split_once('/')
        .ok_or_else(|| Error::Parse(format!("emerge row package lacks a category: {cpv:?}")))?;
    let package = match (kind, package.rsplit_once('-')) {
        ("binary", Some((rest, build))) if build.bytes().all(|b| b.is_ascii_digit()) => rest,
        _ => package,
    };
    let (name, version) = split_name_version(package)?;
    Ok(Some((
        kind.into(),
        flags.trim().into(),
        format!("{category}/{name}"),
        version.into(),
    )))
}

/// `emerge --pretend` resolves without root and writes nothing. Installs are
/// pinned at execution to the reviewed `=category/name-version`; a removal is
/// `--depclean` of exactly the reviewed versions.
pub fn plan(
    runner: &dyn CommandRunner,
    install: &[PackageId],
    remove: &[PackageId],
) -> Result<Plan, Error> {
    if install.is_empty() == remove.is_empty() {
        return Err(Error::Unsupported(
            "Portage preview accepts either installs or removals, not an empty or mixed transaction".into(),
        ));
    }
    let requests = if remove.is_empty() { install } else { remove };
    for id in requests {
        validate(id)?;
        if id.provider != ProviderId::Portage {
            return Err(Error::InvalidInput("provider mismatch".into()));
        }
    }
    let atom = |id: &PackageId| match &id.architecture_or_scope {
        Some(slot) => format!("{}:{slot}", id.name),
        None => id.name.clone(),
    };
    let observed = inventory(runner)?;
    let mut operations = vec![];
    let mut warnings = vec![
        "emerge --pretend resolves against the synced repositories; execution never syncs them."
            .into(),
    ];
    if remove.is_empty() {
        let mut spec = CommandSpec::new(
            "emerge",
            &[
                "--pretend",
                "--noreplace",
                "--autounmask=n",
                "--nospinner",
                "--color=n",
                "--quiet",
                "--",
            ],
        );
        spec.args.extend(requests.iter().map(atom));
        for line in output(runner, spec)?.lines() {
            let Some((kind, flags, name, version)) = merge_row(line)? else {
                continue;
            };
            if !matches!(kind.as_str(), "ebuild" | "binary") {
                return Err(Error::Safety(format!(
                    "emerge would also {kind} {name}-{version}; xpac runs only installs"
                )));
            }
            let flag = |f: char| flags.split_whitespace().any(|word| word.contains(f));
            if flag('D') || flag('B') || flag('b') {
                return Err(Error::Safety(format!(
                    "emerge would downgrade or unblock {name}-{version} [{flags}]; review it natively"
                )));
            }
            if flag('U') {
                warnings.push(format!(
                    "{name}-{version} upgrades an installed package; review partial-upgrade risk."
                ));
            } else if flag('R') {
                // A dependency rebuilt at the version it has (a binary built
                // with other flags, a subslot change): bpftrace's zlib on
                // Gentoo. Any other version is no rebuild and is refused.
                if !observed
                    .iter()
                    .any(|p| p.id.name == name && p.version == version)
                {
                    return Err(Error::Safety(format!(
                        "emerge would replace {name} with {version}, which is not the installed version"
                    )));
                }
                warnings.push(format!(
                    "{name}-{version} is reinstalled at the version it has; its build may differ."
                ));
            } else if !flag('N') {
                return Err(Error::Safety(format!(
                    "emerge row [{kind} {flags}] for {name} is neither new nor an upgrade"
                )));
            }
            if kind == "ebuild" {
                warnings.push(format!("{name}-{version} is built from source."));
            }
            operations.push(Operation {
                kind: OperationKind::Install,
                package: parse_name(ProviderId::Portage, &name)?,
                risk: RiskLevel::RequiresReview,
                native_detail: format!("{name}\t{version}\t{kind}\t{flags}"),
            });
        }
        for id in install {
            if operations.iter().any(|op| op.package.name == id.name) {
                continue;
            }
            let installed: Vec<_> = observed
                .iter()
                .filter(|p| {
                    p.id.name == id.name
                        && id
                            .architecture_or_scope
                            .as_ref()
                            .is_none_or(|s| p.id.architecture_or_scope.as_ref() == Some(s))
                })
                .collect();
            match installed.as_slice() {
                [package] if package.install_reason == InstallReason::Explicit => warnings.push(
                    format!("{id} is already installed and in the world set; nothing to do."),
                ),
                // Installed for another package: emerge records it in world.
                [package] => operations.push(Operation {
                    kind: OperationKind::MarkExplicit,
                    package: package.id.clone(),
                    risk: RiskLevel::RequiresReview,
                    native_detail: format!("{}\t{}\tworld", id.name, package.version),
                }),
                _ => {
                    return Err(Error::Safety(format!(
                        "emerge resolved no action for {id} and it is not installed exactly once"
                    )))
                }
            }
        }
    } else {
        // Exactly the installed version of each request, by slot if given.
        let mut targets = vec![];
        for id in remove {
            let installed: Vec<_> = observed
                .iter()
                .filter(|p| {
                    p.id.name == id.name
                        && id
                            .architecture_or_scope
                            .as_ref()
                            .is_none_or(|s| p.id.architecture_or_scope.as_ref() == Some(s))
                })
                .collect();
            let [package] = installed.as_slice() else {
                return Err(Error::Safety(format!(
                    "{id} must be installed exactly once to remove (found {}); name its slot",
                    installed.len()
                )));
            };
            targets.push(format!("={}-{}", id.name, package.version));
            operations.push(Operation {
                kind: OperationKind::Remove,
                package: package.id.clone(),
                risk: RiskLevel::RequiresReview,
                native_detail: format!("{}\t{}\tdepclean", id.name, package.version),
            });
        }
        let mut spec = CommandSpec::new(
            "emerge",
            &["--pretend", "--depclean", "--nospinner", "--color=n", "--"],
        );
        spec.args.extend(targets.iter().cloned());
        let native = output(runner, spec)?;
        let selected: Vec<&str> = native
            .lines()
            .find_map(|line| line.trim().strip_prefix("All selected packages:"))
            .map(|list| list.split_whitespace().collect())
            .unwrap_or_default();
        let mut wanted: Vec<&str> = targets.iter().map(String::as_str).collect();
        let mut chosen = selected.clone();
        wanted.sort_unstable();
        chosen.sort_unstable();
        if wanted != chosen {
            return Err(Error::Safety(format!(
                "emerge --depclean would remove [{}] where [{}] was asked; what installed packages need is never removed",
                selected.join(" "),
                targets.join(" ")
            )));
        }
        warnings.push("Removal is emerge --depclean of the reviewed versions: it removes them from the world set and leaves their dependencies installed.".into());
    }
    Ok(Plan {
        operations,
        observed_state_fingerprint: crate::planner::inventory_token(&observed),
        observed_packages: observed,
        uncertain: true,
        warnings,
    })
}

/// The version a Portage operation reviewed (`category/name\tversion\t...`).
pub fn reviewed_version(operation: &Operation) -> Option<&str> {
    operation
        .native_detail
        .split('\t')
        .nth(1)
        .filter(|version| is_version(version))
}
