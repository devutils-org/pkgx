use std::{
    collections::BTreeMap,
    sync::{atomic::AtomicBool, Mutex},
    time::Duration,
};
use xpac_core::{
    backend::Backend,
    command::{CommandOutput, CommandRunner, CommandSpec},
    execution::apply_recorded,
    identity::parse_name,
    portage::{
        is_version, merge_row, parse_installed, split_name_version, EMERGE_INSTALL, EMERGE_REMOVE,
    },
    providers::NativeBackend,
    Error, InstallReason, OperationKind, PackageId, ProviderId,
};

/// Stateful Portage model: category/name -> (version, slot, in world).
struct Portage {
    installed: Mutex<BTreeMap<String, (String, String, bool)>>,
    /// The merge list `emerge --pretend` prints for each atom (deps first).
    resolves: BTreeMap<String, Vec<String>>,
    /// Installed packages other installed packages need: depclean keeps them.
    needed: Vec<String>,
    installed_version_override: Option<String>,
    transactions: Mutex<Vec<CommandSpec>>,
}
impl Portage {
    fn new(installed: &[(&str, &str, bool)]) -> Self {
        Self {
            installed: Mutex::new(
                installed
                    .iter()
                    .map(|(n, v, w)| ((*n).into(), ((*v).into(), "0".into(), *w)))
                    .collect(),
            ),
            resolves: BTreeMap::from([(
                "dev-debug/strace".into(),
                vec![
                    "[binary  N g  ] dev-libs/elfutils-0.193-2 ".into(),
                    "[binary  N g  ] dev-debug/strace-7.1-3 ".into(),
                ],
            )]),
            needed: vec!["sys-libs/glibc".into()],
            installed_version_override: None,
            transactions: Mutex::new(vec![]),
        }
    }
    fn targets(spec: &CommandSpec) -> Vec<String> {
        let split = spec.args.iter().position(|a| a == "--").unwrap();
        spec.args[split + 1..].to_vec()
    }
}
fn ok(text: String) -> Result<CommandOutput, Error> {
    Ok(CommandOutput {
        status: Some(0),
        stdout: text.into_bytes(),
        stderr: vec![],
    })
}
/// `=category/name-version` -> (category/name, version).
fn exact(atom: &str) -> (String, String) {
    let (category, package) = atom.trim_start_matches('=').split_once('/').unwrap();
    let (name, version) = split_name_version(package).unwrap();
    (format!("{category}/{name}"), version.into())
}
impl CommandRunner for Portage {
    fn run(&self, spec: &CommandSpec, _: Duration, _: &AtomicBool) -> Result<CommandOutput, Error> {
        assert!(spec.env.is_empty());
        let installed = || self.installed.lock().unwrap();
        match spec.program.as_str() {
            "grep" => ok(installed()
                .iter()
                .flat_map(|(atom, (version, slot, _))| {
                    [
                        format!("/var/db/pkg/{atom}-{version}/SLOT:{slot}\n"),
                        format!("/var/db/pkg/{atom}-{version}/repository:gentoo\n"),
                    ]
                })
                .collect()),
            "cat" => ok(installed()
                .iter()
                .filter(|(_, (_, _, world))| *world)
                .map(|(atom, _)| format!("{atom}::gentoo\n"))
                .collect()),
            "emerge" if spec.args[0] == "--pretend" => {
                assert!(!spec.requires_privilege);
                assert!(!spec.args.iter().any(|a| a == "--sync" || a == "--update"));
                let installed = installed();
                if spec.args.contains(&"--depclean".into()) {
                    let selected: Vec<_> = Self::targets(spec)
                        .into_iter()
                        .filter(|t| !self.needed.contains(&exact(t).0))
                        .collect();
                    return ok(format!(
                        ">>> These are the packages that would be unmerged:\n\nAll selected packages: {}\n\nNumber to remove: {}\n",
                        selected.join(" "),
                        selected.len()
                    ));
                }
                let mut rows = String::new();
                for target in Self::targets(spec) {
                    for row in self.resolves.get(&target).into_iter().flatten() {
                        let (_, flags, atom, _) = merge_row(row).unwrap().unwrap();
                        // emerge lists an installed package only to rebuild it.
                        if !installed.contains_key(&atom) || flags.contains('R') {
                            rows.push_str(row);
                            rows.push('\n');
                        }
                    }
                }
                ok(rows)
            }
            "emerge" => {
                assert!(spec.requires_privilege);
                self.transactions.lock().unwrap().push(spec.clone());
                let mut installed = installed();
                for target in Self::targets(spec) {
                    if spec.args.contains(&"--depclean".into()) {
                        installed.remove(&exact(&target).0);
                    } else if let Some(entry) = installed.get_mut(&target) {
                        entry.2 = true;
                    } else {
                        let (atom, _) = exact(&target);
                        for row in &self.resolves[&atom] {
                            let (_, _, dep, version) = merge_row(row).unwrap().unwrap();
                            let version =
                                self.installed_version_override.clone().unwrap_or(version);
                            installed.insert(dep.clone(), (version, "0".into(), dep == atom));
                        }
                    }
                }
                ok(String::new())
            }
            other => panic!("unexpected command {other}"),
        }
    }
}
fn id(name: &str) -> PackageId {
    parse_name(ProviderId::Portage, name).unwrap()
}
fn backend(mock: &Portage) -> NativeBackend<'_> {
    NativeBackend {
        provider: ProviderId::Portage,
        runner: mock,
    }
}

#[test]
fn versions_follow_the_package_manager_specification() {
    for valid in [
        "1",
        "9.11-r1",
        "3.12.1_p1-r2",
        "1.0a",
        "2.11_rc3_p",
        "0.1_alpha_pre2",
    ] {
        assert!(is_version(valid), "{valid}");
    }
    for invalid in [
        "",
        "r1",
        "7.1-3",
        "1.0-rc1",
        "1..2",
        "1.0_beta-x",
        "completion-2.11",
    ] {
        assert!(!is_version(invalid), "{invalid}");
    }
    assert_eq!(
        split_name_version("coreutils-9.11-r1").unwrap(),
        ("coreutils", "9.11-r1")
    );
    assert_eq!(
        split_name_version("bash-completion-2.11").unwrap(),
        ("bash-completion", "2.11")
    );
    assert_eq!(
        split_name_version("foo-1-bar-2").unwrap(),
        ("foo-1-bar", "2")
    );
    assert!(split_name_version("coreutils").is_err());
}

#[test]
fn a_binary_build_id_is_no_part_of_the_version() {
    assert_eq!(
        merge_row("[binary  N g  ] dev-debug/strace-7.1-3 ")
            .unwrap()
            .unwrap(),
        (
            "binary".into(),
            "N g".into(),
            "dev-debug/strace".into(),
            "7.1".into()
        )
    );
    assert_eq!(
        merge_row("[ebuild  N     ] dev-lang/python-3.12.1_p1-r2::gentoo  USE=\"ssl\"")
            .unwrap()
            .unwrap()
            .3,
        "3.12.1_p1-r2"
    );
    assert!(merge_row(" * IMPORTANT: 24 news items need reading")
        .unwrap()
        .is_none());
}

#[test]
fn the_vdb_and_world_give_slots_repositories_and_selections() {
    let rows = "/var/db/pkg/dev-lang/python-3.12.1_p1-r2/SLOT:3.12/3.12\n\
                /var/db/pkg/dev-lang/python-3.12.1_p1-r2/repository:gentoo\n\
                /var/db/pkg/dev-lang/python-3.13.0/SLOT:3.13/3.13\n\
                /var/db/pkg/sys-apps/coreutils-9.11-r1/SLOT:0\n";
    let packages = parse_installed(
        rows,
        "dev-lang/python:3.12\nsys-apps/coreutils::gentoo\n@extra\n",
    )
    .unwrap();
    let rows: Vec<_> = packages
        .iter()
        .map(|p| (p.id.to_string(), p.version.as_str(), p.install_reason))
        .collect();
    assert_eq!(
        rows,
        [
            (
                "portage:dev-lang/python:3.12".to_owned(),
                "3.12.1_p1-r2",
                InstallReason::Explicit
            ),
            (
                "portage:dev-lang/python:3.13".to_owned(),
                "3.13.0",
                InstallReason::Unknown
            ),
            (
                "portage:sys-apps/coreutils:0".to_owned(),
                "9.11-r1",
                InstallReason::Explicit
            ),
        ]
    );
    assert_eq!(packages[0].metadata["repository"], "gentoo");
    assert_eq!(packages[0].metadata["subslot"], "3.12");
    assert!(parse_installed(
        "/var/db/pkg/sys-apps/coreutils-9.11-r1/repository:gentoo\n",
        ""
    )
    .is_err());
}

#[test]
fn an_install_is_pinned_to_the_reviewed_version_and_verified() {
    let mock = Portage::new(&[
        ("sys-libs/glibc", "2.43-r4", true),
        ("app-misc/screen", "5.0", false),
    ]);
    let requests = [id("dev-debug/strace"), id("app-misc/screen")];
    let plan = backend(&mock).plan(&requests, &[]).unwrap();
    let ops: Vec<_> = plan
        .operations
        .iter()
        .map(|op| (op.kind.clone(), op.package.name.as_str()))
        .collect();
    assert_eq!(
        ops,
        [
            (OperationKind::Install, "dev-libs/elfutils"),
            (OperationKind::Install, "dev-debug/strace"),
            (OperationKind::MarkExplicit, "app-misc/screen"),
        ]
    );
    let record = apply_recorded(
        &mock,
        &plan,
        &requests,
        &[],
        true,
        &AtomicBool::new(false),
        None,
    )
    .unwrap();
    assert!(record.success && record.verified, "{:?}", record.errors);
    let transactions = mock.transactions.lock().unwrap();
    let mut expected: Vec<String> = EMERGE_INSTALL[1..]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    expected.extend(["=dev-debug/strace-7.1".into(), "app-misc/screen".into()]);
    assert_eq!(transactions[0].args, expected);
}

#[test]
fn a_different_installed_version_fails_verification() {
    let mut mock = Portage::new(&[("sys-libs/glibc", "2.43-r4", true)]);
    mock.installed_version_override = Some("7.2".into());
    let plan = backend(&mock).plan(&[id("dev-debug/strace")], &[]).unwrap();
    let record = apply_recorded(
        &mock,
        &plan,
        &[id("dev-debug/strace")],
        &[],
        true,
        &AtomicBool::new(false),
        None,
    )
    .unwrap();
    assert!(!record.success && !record.verified);
}

#[test]
fn downgrades_rebuilds_blockers_and_uninstalls_are_refused() {
    for row in [
        "[ebuild     UD ] dev-debug/strace-6.0",
        "[binary  N   B ] dev-debug/strace-7.1-3",
        "[blocks B      ] dev-debug/strace-7.1",
        "[uninstall     ] dev-debug/strace-7.1",
    ] {
        let mut mock = Portage::new(&[("sys-libs/glibc", "2.43-r4", true)]);
        mock.resolves
            .insert("dev-debug/strace".into(), vec![row.into()]);
        assert!(
            matches!(
                backend(&mock).plan(&[id("dev-debug/strace")], &[]),
                Err(Error::Safety(_))
            ),
            "{row}"
        );
    }
}

#[test]
fn a_removal_is_depclean_of_exactly_the_reviewed_versions() {
    let mock = Portage::new(&[
        ("sys-libs/glibc", "2.43-r4", true),
        ("dev-debug/strace", "7.1", true),
    ]);
    let plan = backend(&mock).plan(&[], &[id("dev-debug/strace")]).unwrap();
    assert_eq!(plan.operations[0].kind, OperationKind::Remove);
    let record = apply_recorded(
        &mock,
        &plan,
        &[],
        &[id("dev-debug/strace")],
        true,
        &AtomicBool::new(false),
        None,
    )
    .unwrap();
    assert!(record.success && record.verified, "{:?}", record.errors);
    let transactions = mock.transactions.lock().unwrap();
    let mut expected: Vec<String> = EMERGE_REMOVE[1..].iter().map(|s| (*s).to_owned()).collect();
    expected.push("=dev-debug/strace-7.1".into());
    assert_eq!(transactions[0].args, expected);
}

#[test]
fn what_installed_packages_need_is_never_removed() {
    let mock = Portage::new(&[("sys-libs/glibc", "2.43-r4", true)]);
    assert!(matches!(
        backend(&mock).plan(&[], &[id("sys-libs/glibc")]),
        Err(Error::Safety(ref m)) if m.contains("never removed")
    ));
}

#[test]
fn a_dependency_rebuilt_at_its_installed_version_is_an_install_to_review() {
    let rebuild = |row: &str| {
        let mut mock = Portage::new(&[
            ("sys-libs/glibc", "2.43-r4", true),
            ("sys-libs/zlib", "1.3.2-r1", false),
        ]);
        mock.resolves.insert(
            "dev-debug/bpftrace".into(),
            vec![
                row.into(),
                "[binary  N   g] dev-debug/bpftrace-0.24.1-1 ".into(),
            ],
        );
        backend(&mock).plan(&[id("dev-debug/bpftrace")], &[])
    };
    let plan = rebuild("[binary   R  g] sys-libs/zlib-1.3.2-r1-2 ").unwrap();
    assert_eq!(plan.operations[0].package.name, "sys-libs/zlib");
    assert_eq!(plan.operations[0].kind, OperationKind::Install);
    assert!(plan
        .warnings
        .iter()
        .any(|w| w.contains("reinstalled at the version it has")));
    // A replacement by any other version is no rebuild.
    assert!(matches!(
        rebuild("[binary   R  g] sys-libs/zlib-1.3.3-1 "),
        Err(Error::Safety(_))
    ));
}
