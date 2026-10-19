use std::{
    collections::BTreeMap,
    sync::{atomic::AtomicBool, Mutex},
    time::Duration,
};
use xpac_core::{
    apk::{parse_installed, split_name_version, world_names, APK_ADD, APK_DEL},
    backend::Backend,
    command::{CommandOutput, CommandRunner, CommandSpec},
    execution::apply_recorded,
    identity::parse_name,
    providers::NativeBackend,
    Error, InstallReason, OperationKind, PackageId, ProviderId,
};

/// Stateful apk model: name -> (version, in WORLD). Transactions mutate it.
struct Apk {
    installed: Mutex<BTreeMap<String, (String, bool)>>,
    /// What `apk add` of each name brings in (the name last), as apk orders it.
    resolves: BTreeMap<String, Vec<(String, String)>>,
    /// Packages that installed packages depend on: `apk del` keeps them.
    needed: Vec<String>,
    /// Version actually installed by the native transaction (race model).
    installed_version_override: Option<String>,
    transactions: Mutex<Vec<CommandSpec>>,
}
impl Apk {
    fn new(installed: &[(&str, &str, bool)]) -> Self {
        Self {
            installed: Mutex::new(
                installed
                    .iter()
                    .map(|(n, v, e)| ((*n).into(), ((*v).into(), *e)))
                    .collect(),
            ),
            resolves: BTreeMap::from([(
                "strace".into(),
                vec![
                    ("libdw".into(), "0.195-r0".into()),
                    ("strace".into(), "6.19-r1".into()),
                ],
            )]),
            needed: vec!["musl".into()],
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
impl CommandRunner for Apk {
    fn run(&self, spec: &CommandSpec, _: Duration, _: &AtomicBool) -> Result<CommandOutput, Error> {
        assert!(spec.env.is_empty());
        let installed = || self.installed.lock().unwrap();
        match (spec.program.as_str(), spec.args[0].as_str()) {
            ("cat", _) => {
                assert_eq!(spec.args, ["--", "/etc/apk/world"]);
                ok(installed()
                    .iter()
                    .filter(|(_, (_, world))| *world)
                    .map(|(name, _)| format!("{name}\n"))
                    .collect())
            }
            ("apk", "list") => ok(installed()
                .iter()
                .map(|(name, (version, _))| {
                    format!("{name}-{version} x86_64 {{{name}}} (MIT AND ( GPL-2.0 OR BSD )) [installed]\n")
                })
                .collect()),
            ("apk", "--print-arch") => ok("x86_64\n".into()),
            ("apk", _) if spec.args.contains(&"--simulate".into()) => {
                assert!(!spec.requires_privilege);
                assert!(!spec.args.iter().any(|a| a.starts_with("--update") || a == "-U"));
                let installed = installed();
                let mut rows = vec![];
                for target in Self::targets(spec) {
                    if spec.args[0] == "del" {
                        if self.needed.contains(&target) {
                            return ok(format!("World updated, but the following packages are not removed due to:\n  {target}: busybox\n\nOK: 8 MiB in 16 packages\n"));
                        }
                        rows.push(format!("Purging {target} ({})", installed[&target].0));
                    } else {
                        for (name, version) in self.resolves.get(&target).into_iter().flatten() {
                            if !installed.contains_key(name) {
                                rows.push(format!("Installing {name} ({version})"));
                            }
                        }
                    }
                }
                let total = rows.len();
                ok(rows
                    .iter()
                    .enumerate()
                    .map(|(i, row)| {
                        let width = total.to_string().len();
                        format!("({:>width$}/{total}) {row}\n", i + 1)
                    })
                    .chain(["OK: 10 MiB in 23 packages\n".to_owned()])
                    .collect())
            }
            ("apk", "add" | "del") => {
                assert!(spec.requires_privilege);
                self.transactions.lock().unwrap().push(spec.clone());
                let mut installed = installed();
                for target in Self::targets(spec) {
                    if spec.args[0] == "del" {
                        installed.remove(&target);
                    } else if let Some(entry) = installed.get_mut(&target) {
                        entry.1 = true;
                    } else {
                        for (name, version) in &self.resolves[&target] {
                            let version = self
                                .installed_version_override
                                .clone()
                                .unwrap_or(version.clone());
                            installed.insert(name.clone(), (version, *name == target));
                        }
                    }
                }
                ok(String::new())
            }
            other => panic!("unexpected command {other:?}"),
        }
    }
}
fn id(name: &str) -> PackageId {
    parse_name(ProviderId::Apk, name).unwrap()
}
fn backend(mock: &Apk) -> NativeBackend<'_> {
    NativeBackend {
        provider: ProviderId::Apk,
        runner: mock,
    }
}

#[test]
fn names_split_at_the_version_whatever_their_hyphens_and_digits() {
    assert_eq!(
        split_name_version("strace-6.19-r1").unwrap(),
        ("strace", "6.19-r1")
    );
    assert_eq!(
        split_name_version("py3-foo-bar-1.2-r0").unwrap(),
        ("py3-foo-bar", "1.2-r0")
    );
    assert_eq!(
        split_name_version("lib2-x-0.1_git20240101-r3").unwrap(),
        ("lib2-x", "0.1_git20240101-r3")
    );
    for invalid in [
        "strace",
        "strace-6.19",
        "strace-6.19-1",
        "strace-x6-r1",
        "-6.19-r1",
    ] {
        assert!(split_name_version(invalid).is_err(), "{invalid}");
    }
}

#[test]
fn world_constraints_name_their_packages_and_nothing_else() {
    let names =
        world_names("alpine-base\nfoo>=1.2\nbar@edge baz=3-r0\n!qux\nso:libz.so.1\ncmd:sh\n");
    assert_eq!(
        names.into_iter().collect::<Vec<_>>(),
        ["alpine-base", "bar", "baz", "foo"]
    );
}

#[test]
fn inventory_marks_world_packages_explicit_and_the_rest_dependencies() {
    let list = "musl-1.2.6-r2 x86_64 {musl} (MIT) [installed]\n\
                libdw-0.195-r0 x86_64 {elfutils} (GPL-3.0-or-later AND ( GPL-2.0-or-later OR LGPL-3.0-or-later )) [installed]\n";
    let packages = parse_installed(list, "libdw\n").unwrap();
    assert_eq!(packages.len(), 2);
    assert_eq!(packages[0].id.to_string(), "apk:libdw:x86_64");
    assert_eq!(packages[0].version, "0.195-r0");
    assert_eq!(packages[0].install_reason, InstallReason::Explicit);
    assert_eq!(packages[0].metadata["origin"], "elfutils");
    assert_eq!(packages[1].install_reason, InstallReason::Dependency);
    assert!(parse_installed("musl-1.2.6-r2 x86_64 {musl} (MIT)\n", "").is_err());
    assert!(parse_installed("musl-1.2.6-r2 x86_64 (MIT) [installed]\n", "").is_err());
}

#[test]
fn an_install_previews_new_packages_and_marks_a_dependency_explicit() {
    let mock = Apk::new(&[("musl", "1.2.6-r2", false), ("zlib", "1.3-r0", false)]);
    let plan = backend(&mock)
        .plan(&[id("strace"), id("zlib")], &[])
        .unwrap();
    let ops: Vec<_> = plan
        .operations
        .iter()
        .map(|op| (op.kind.clone(), op.package.name.as_str()))
        .collect();
    assert_eq!(
        ops,
        [
            (OperationKind::Install, "libdw"),
            (OperationKind::Install, "strace"),
            (OperationKind::MarkExplicit, "zlib"),
        ]
    );
    assert!(plan.uncertain);
    let record = apply_recorded(
        &mock,
        &plan,
        &[id("strace"), id("zlib")],
        &[],
        true,
        &AtomicBool::new(false),
        None,
    )
    .unwrap();
    assert!(record.success && record.verified, "{:?}", record.errors);
    let transactions = mock.transactions.lock().unwrap();
    let mut expected: Vec<String> = APK_ADD[1..].iter().map(|s| (*s).to_owned()).collect();
    expected.extend(["strace".into(), "zlib".into()]);
    assert_eq!(transactions[0].program, "apk");
    assert_eq!(transactions[0].args, expected);
}

#[test]
fn a_different_installed_version_fails_verification() {
    let mut mock = Apk::new(&[("musl", "1.2.6-r2", false)]);
    mock.installed_version_override = Some("6.20-r0".into());
    let plan = backend(&mock).plan(&[id("strace")], &[]).unwrap();
    let record = apply_recorded(
        &mock,
        &plan,
        &[id("strace")],
        &[],
        true,
        &AtomicBool::new(false),
        None,
    )
    .unwrap();
    assert!(!record.success && !record.verified);
}

#[test]
fn a_stale_preview_or_no_confirmation_never_reaches_apk() {
    let mock = Apk::new(&[("musl", "1.2.6-r2", false)]);
    let plan = backend(&mock).plan(&[id("strace")], &[]).unwrap();
    assert!(matches!(
        apply_recorded(
            &mock,
            &plan,
            &[id("strace")],
            &[],
            false,
            &AtomicBool::new(false),
            None
        ),
        Err(Error::Safety(_))
    ));
    mock.installed
        .lock()
        .unwrap()
        .insert("zlib".into(), ("1.3-r0".into(), false));
    assert!(matches!(
        apply_recorded(
            &mock,
            &plan,
            &[id("strace")],
            &[],
            true,
            &AtomicBool::new(false),
            None
        ),
        Err(Error::Safety(_))
    ));
    assert!(mock.transactions.lock().unwrap().is_empty());
}

#[test]
fn a_removal_apk_would_turn_into_a_world_edit_is_refused() {
    let mock = Apk::new(&[("musl", "1.2.6-r2", true)]);
    let error = backend(&mock).plan(&[], &[id("musl")]).unwrap_err();
    assert!(
        matches!(error, Error::Safety(ref m) if m.contains("busybox")),
        "{error}"
    );
}

#[test]
fn a_removal_purges_and_is_verified() {
    let mock = Apk::new(&[("musl", "1.2.6-r2", false), ("strace", "6.19-r1", true)]);
    let plan = backend(&mock).plan(&[], &[id("strace")]).unwrap();
    assert_eq!(plan.operations[0].kind, OperationKind::Remove);
    let record = apply_recorded(
        &mock,
        &plan,
        &[],
        &[id("strace")],
        true,
        &AtomicBool::new(false),
        None,
    )
    .unwrap();
    assert!(record.success && record.verified, "{:?}", record.errors);
    let transactions = mock.transactions.lock().unwrap();
    assert_eq!(&transactions[0].args[..APK_DEL.len() - 1], &APK_DEL[1..]);
}

#[test]
fn another_architecture_or_a_mixed_transaction_is_refused() {
    let mock = Apk::new(&[("musl", "1.2.6-r2", false)]);
    assert!(matches!(
        backend(&mock).plan(&[id("strace:aarch64")], &[]),
        Err(Error::Safety(_))
    ));
    assert!(matches!(
        backend(&mock).plan(&[id("strace")], &[id("musl")]),
        Err(Error::Unsupported(_))
    ));
}

#[test]
fn ten_operations_and_more_are_read_with_their_aligned_counter() {
    let mut mock = Apk::new(&[("musl", "1.2.6-r2", false)]);
    let deps: Vec<(String, String)> = (0..11)
        .map(|i| (format!("lib{i}"), format!("1.{i}-r0")))
        .chain([("gcc".to_owned(), "15.2.0-r2".to_owned())])
        .collect();
    mock.resolves.insert("gcc".into(), deps);
    let plan = backend(&mock).plan(&[id("gcc")], &[]).unwrap();
    assert_eq!(plan.operations.len(), 12);
    assert_eq!(plan.operations[11].package.name, "gcc");
}
