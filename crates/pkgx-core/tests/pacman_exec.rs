use pkgx_core::{
    backend::Backend,
    command::{CommandOutput, CommandRunner, CommandSpec},
    execution::{apply_pacman_recorded, apply_recorded, apply_saved, PACMAN_INSTALL},
    identity::parse_name,
    privilege::SudoAptRunner,
    providers::NativeBackend,
    saved_plan::SavedPlan,
    Error, PackageId, ProviderId,
};
use std::{
    collections::BTreeMap,
    sync::{atomic::AtomicBool, Mutex},
    time::Duration,
};

/// Stateful Pacman model: name -> (version, explicit). Transactions mutate it.
struct Pacman {
    installed: Mutex<BTreeMap<String, (String, bool)>>,
    /// Packages added by `--sync` for each requested target (target first).
    resolves: BTreeMap<String, Vec<(String, String)>>,
    hold: String,
    ignore: String,
    ignore_group: String,
    /// Version actually installed by the native transaction (tamper/race model).
    installed_version_override: Option<String>,
    transactions: Mutex<Vec<CommandSpec>>,
}
impl Pacman {
    fn new(installed: &[(&str, &str, bool)]) -> Self {
        Self {
            installed: Mutex::new(
                installed
                    .iter()
                    .map(|(n, v, e)| ((*n).into(), ((*v).into(), *e)))
                    .collect(),
            ),
            resolves: BTreeMap::from([(
                "ripgrep".into(),
                vec![
                    ("ripgrep".into(), "14.1.1-1".into()),
                    ("pcre2".into(), "10.44-1".into()),
                ],
            )]),
            hold: "pacman glibc\n".into(),
            ignore: String::new(),
            ignore_group: String::new(),
            installed_version_override: None,
            transactions: Mutex::new(vec![]),
        }
    }
    fn info(&self) -> String {
        self.installed
            .lock()
            .unwrap()
            .iter()
            .map(|(name, (version, explicit))| {
                format!(
                    "Name            : {name}\nVersion         : {version}\nArchitecture    : x86_64\nInstall Reason  : {}\n\n",
                    if *explicit {
                        "Explicitly installed"
                    } else {
                        "Installed as a dependency for another package"
                    }
                )
            })
            .collect()
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
impl CommandRunner for Pacman {
    fn run(&self, spec: &CommandSpec, _: Duration, _: &AtomicBool) -> Result<CommandOutput, Error> {
        assert!(spec.env.is_empty());
        match spec.program.as_str() {
            "pacman-conf" => ok(match spec.args[0].as_str() {
                "HoldPkg" => self.hold.clone(),
                "IgnorePkg" => self.ignore.clone(),
                "IgnoreGroup" => self.ignore_group.clone(),
                _ => panic!("unexpected pacman-conf key"),
            }),
            "pacman" if spec.args[0] == "-Qi" => ok(self.info()),
            "pacman" if spec.args.contains(&"--print".into()) => {
                assert!(!spec.requires_privilege);
                assert!(!spec.args.iter().any(|a| a == "--refresh" || a == "-y"));
                let installed = self.installed.lock().unwrap();
                let mut rows = String::new();
                for target in Self::targets(spec) {
                    if spec.args[0] == "--remove" {
                        let (version, _) = &installed[&target];
                        rows.push_str(&format!("{target}\t{version}\tx86_64\t\n"));
                    } else {
                        for (name, version) in &self.resolves[&target] {
                            if !installed.contains_key(name) {
                                rows.push_str(&format!("{name}\t{version}\tx86_64\textra\n"));
                            }
                        }
                    }
                }
                ok(rows)
            }
            "pacman" => {
                assert!(spec.requires_privilege);
                self.transactions.lock().unwrap().push(spec.clone());
                let mut installed = self.installed.lock().unwrap();
                for target in Self::targets(spec) {
                    if spec.args[0] == "--remove" {
                        installed.remove(&target);
                    } else {
                        let name = target.split_once('/').unwrap().1;
                        for (dep, version) in &self.resolves[name] {
                            let version = self
                                .installed_version_override
                                .clone()
                                .unwrap_or(version.clone());
                            installed
                                .entry(dep.clone())
                                .or_insert((version, dep == name));
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
    parse_name(ProviderId::Pacman, name).unwrap()
}
fn preview(mock: &Pacman, install: &[PackageId], remove: &[PackageId]) -> pkgx_core::Plan {
    NativeBackend {
        provider: ProviderId::Pacman,
        runner: mock,
    }
    .plan(install, remove)
    .unwrap()
}

#[test]
fn reviewed_install_pins_repository_never_refreshes_and_verifies_versions() {
    let mock = Pacman::new(&[("curl", "8.10.1-1", true)]);
    let request = [id("ripgrep")];
    let plan = preview(&mock, &request, &[]);
    assert_eq!(plan.operations.len(), 2);
    assert!(plan.uncertain);
    let temp = tempfile::tempdir().unwrap();
    let journal = pkgx_core::journal::Journal::new(temp.path().join("journal"));
    let record = apply_recorded(
        &mock,
        &plan,
        &request,
        &[],
        true,
        &AtomicBool::new(false),
        Some(&journal),
    )
    .unwrap();
    assert!(record.success && record.verified, "{:?}", record.errors);
    let transactions = mock.transactions.lock().unwrap();
    assert_eq!(transactions.len(), 1);
    let mut expected: Vec<String> = PACMAN_INSTALL[1..].iter().map(|s| (*s).into()).collect();
    expected.push("extra/ripgrep".into());
    assert_eq!(transactions[0].args, expected);
    assert!(journal.history().unwrap()[0]
        .result
        .as_ref()
        .is_some_and(|r| r.success));
}
#[test]
fn removal_keeps_dependencies_and_configuration_backups() {
    let mock = Pacman::new(&[("curl", "8.10.1-1", true), ("zlib", "1:1.3.1-2", false)]);
    let request = [id("curl")];
    let plan = preview(&mock, &[], &request);
    let record = apply_pacman_recorded(
        &mock,
        &plan,
        &[],
        &request,
        true,
        &AtomicBool::new(false),
        None,
    )
    .unwrap();
    assert!(record.success, "{:?}", record.errors);
    let transactions = mock.transactions.lock().unwrap();
    let args = &transactions[0].args;
    assert_eq!(args.last().unwrap(), "curl");
    for forbidden in ["--recursive", "--cascade", "--nosave", "-s", "-c", "-n"] {
        assert!(!args.iter().any(|a| a == forbidden));
    }
    assert!(mock.installed.lock().unwrap().contains_key("zlib"));
}
#[test]
fn stale_unconfirmed_or_canceled_requests_never_start_a_transaction() {
    let mock = Pacman::new(&[("curl", "8.10.1-1", true)]);
    let request = [id("ripgrep")];
    let plan = preview(&mock, &request, &[]);
    let run = |confirmed, canceled| {
        apply_recorded(
            &mock,
            &plan,
            &request,
            &[],
            confirmed,
            &AtomicBool::new(canceled),
            None,
        )
    };
    assert!(matches!(run(false, false), Err(Error::Safety(_))));
    assert!(matches!(run(true, true), Err(Error::Canceled(_))));
    mock.installed
        .lock()
        .unwrap()
        .insert("pcre2".into(), ("10.44-1".into(), false));
    assert!(matches!(run(true, false), Err(Error::Safety(m)) if m.contains("stale")));
    assert!(mock.transactions.lock().unwrap().is_empty());
}
#[test]
fn held_critical_and_ignored_packages_are_refused_at_preview() {
    let backend = |mock: &Pacman, install: &[PackageId], remove: &[PackageId]| {
        NativeBackend {
            provider: ProviderId::Pacman,
            runner: mock,
        }
        .plan(install, remove)
    };
    let mut mock = Pacman::new(&[("glibc", "2.40-1", true), ("filesystem", "1-1", true)]);
    assert!(matches!(
        backend(&mock, &[], &[id("glibc")]),
        Err(Error::Safety(_))
    ));
    mock.hold.clear();
    // Built-in critical packages stay protected even without HoldPkg.
    assert!(matches!(
        backend(&mock, &[], &[id("filesystem")]),
        Err(Error::Safety(_))
    ));
    mock.ignore = "pcre*\n".into();
    assert!(matches!(
        backend(&mock, &[id("ripgrep")], &[]),
        Err(Error::Safety(m)) if m.contains("IgnorePkg")
    ));
    mock.ignore = "pcre[0-9]\n".into();
    assert!(matches!(
        backend(&mock, &[id("ripgrep")], &[]),
        Err(Error::Safety(m)) if m.contains("glob")
    ));
    mock.ignore.clear();
    mock.ignore_group = "base-devel\n".into();
    assert!(matches!(
        backend(&mock, &[id("ripgrep")], &[]),
        Err(Error::Safety(m)) if m.contains("IgnoreGroup")
    ));
    assert!(mock.transactions.lock().unwrap().is_empty());
}
#[test]
fn already_installed_targets_are_a_verified_noop() {
    let mock = Pacman::new(&[("ripgrep", "14.1.1-1", true), ("pcre2", "10.44-1", false)]);
    let request = [id("ripgrep")];
    let plan = preview(&mock, &request, &[]);
    assert!(plan.operations.is_empty());
    assert!(plan
        .warnings
        .iter()
        .any(|w| w.contains("already installed")));
    let record = apply_recorded(
        &mock,
        &plan,
        &request,
        &[],
        true,
        &AtomicBool::new(false),
        None,
    )
    .unwrap();
    assert!(record.success && record.native_status.is_none());
    assert!(mock.transactions.lock().unwrap().is_empty());
}
#[test]
fn unexpected_installed_versions_fail_verification() {
    let mut mock = Pacman::new(&[]);
    mock.installed_version_override = Some("99-1".into());
    let request = [id("ripgrep")];
    let plan = preview(&mock, &request, &[]);
    let record = apply_recorded(
        &mock,
        &plan,
        &request,
        &[],
        true,
        &AtomicBool::new(false),
        None,
    )
    .unwrap();
    assert!(!record.success && !record.verified);
    assert_eq!(record.native_status, Some(0));
}
#[test]
fn providers_without_executor_and_mixed_requests_are_refused() {
    let mock = Pacman::new(&[]);
    let empty = preview(
        &Pacman::new(&[("ripgrep", "1", true)]),
        &[id("ripgrep")],
        &[],
    );
    let dnf = parse_name(ProviderId::Brew, "curl:formula").unwrap();
    assert!(matches!(
        apply_recorded(
            &mock,
            &empty,
            std::slice::from_ref(&dnf),
            &[],
            true,
            &AtomicBool::new(false),
            None
        ),
        Err(Error::Unsupported(_))
    ));
    assert!(matches!(
        apply_recorded(
            &mock,
            &empty,
            &[id("ripgrep")],
            &[dnf],
            true,
            &AtomicBool::new(false),
            None
        ),
        Err(Error::Safety(_))
    ));
}
#[test]
fn expired_saved_pacman_plan_is_refused_before_native_inspection() {
    let mock = Pacman::new(&[]);
    let request = vec![id("ripgrep")];
    let plan = preview(&mock, &request, &[]);
    let saved = SavedPlan::new(ProviderId::Pacman, request, vec![], plan, 100, 60).unwrap();
    assert!(apply_saved(&mock, &saved, true, &AtomicBool::new(false), None).is_err());
    assert!(mock.transactions.lock().unwrap().is_empty());
}
#[test]
fn sudo_delegates_only_exact_pacman_executor_templates() {
    #[derive(Default)]
    struct Inner(Mutex<Vec<CommandSpec>>);
    impl CommandRunner for Inner {
        fn run(
            &self,
            spec: &CommandSpec,
            _: Duration,
            _: &AtomicBool,
        ) -> Result<CommandOutput, Error> {
            self.0.lock().unwrap().push(spec.clone());
            ok(String::new())
        }
    }
    let inner = Inner::default();
    let runner = SudoAptRunner::new(&inner);
    let mut spec = CommandSpec::new(PACMAN_INSTALL[0], &PACMAN_INSTALL[1..]);
    spec.args.push("extra/ripgrep".into());
    spec.requires_privilege = true;
    runner
        .run(&spec, Duration::from_secs(1), &AtomicBool::new(false))
        .unwrap();
    {
        let calls = inner.0.lock().unwrap();
        assert_eq!(
            &calls[0].args[..5],
            &["-n", "-u", "root", "--", pkgx_core::privilege::PACMAN]
        );
        assert_eq!(&calls[0].args[5..], spec.args);
    }
    let variants: Vec<Vec<String>> = vec![
        // Database refresh or system upgrade injected into the template.
        {
            let mut a = spec.args.clone();
            a.insert(1, "--refresh".into());
            a
        },
        {
            let mut a = spec.args.clone();
            a.insert(1, "--sysupgrade".into());
            a
        },
        // Option smuggled as a target or no target at all.
        {
            let mut a = spec.args.clone();
            a.push("--overwrite=*".into());
            a
        },
        PACMAN_INSTALL[1..].iter().map(|s| (*s).into()).collect(),
        vec!["-Qi".into()],
    ];
    for args in variants {
        let mut bad = spec.clone();
        bad.args = args;
        assert!(runner
            .run(&bad, Duration::from_secs(1), &AtomicBool::new(false))
            .is_err());
    }
    let mut env = spec.clone();
    env.env.insert("PATH".into(), "/tmp".into());
    assert!(runner
        .run(&env, Duration::from_secs(1), &AtomicBool::new(false))
        .is_err());
    assert_eq!(inner.0.lock().unwrap().len(), 1);
}
