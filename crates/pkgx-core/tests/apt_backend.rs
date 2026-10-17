use pkgx_core::{
    apt::{self, AptBackend},
    backend::Backend,
    command::{CommandOutput, CommandRunner, CommandSpec},
    Error, OperationKind,
};
use std::{
    collections::VecDeque,
    sync::{atomic::AtomicBool, Mutex},
    time::Duration,
};
struct Step {
    program: &'static str,
    args: Vec<&'static str>,
    output: &'static str,
    status: i32,
}
struct Mock(Mutex<VecDeque<Step>>);
impl Mock {
    fn new(steps: Vec<Step>) -> Self {
        Self(Mutex::new(steps.into()))
    }
    fn done(&self) {
        assert!(self.0.lock().unwrap().is_empty());
    }
}
impl CommandRunner for Mock {
    fn run(&self, spec: &CommandSpec, _: Duration, _: &AtomicBool) -> Result<CommandOutput, Error> {
        let step = self
            .0
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected subprocess");
        assert_eq!(spec.program, step.program);
        for arg in step.args {
            assert!(
                spec.args.iter().any(|a| a == arg),
                "missing argument {arg}: {:?}",
                spec.args
            );
        }
        assert!(!spec
            .args
            .iter()
            .any(|a| a == "--allow-remove-essential" || a == "--allow-change-held-packages"));
        Ok(CommandOutput {
            status: Some(step.status),
            stdout: step.output.as_bytes().to_vec(),
            stderr: vec![],
        })
    }
}
fn planning_steps(
    holds: &'static str,
    protected: &'static str,
    simulation: &'static str,
) -> Vec<Step> {
    vec![
        Step {
            program: "dpkg-query",
            args: vec!["-W"],
            output: include_str!("../../../tests/fixtures/dpkg.tsv"),
            status: 0,
        },
        Step {
            program: "apt-mark",
            args: vec!["showmanual"],
            output: "curl\n",
            status: 0,
        },
        Step {
            program: "apt-mark",
            args: vec!["showauto"],
            output: "zlib1g\n",
            status: 0,
        },
        Step {
            program: "apt-mark",
            args: vec!["showhold"],
            output: holds,
            status: 0,
        },
        Step {
            program: "dpkg-query",
            args: vec!["-W"],
            output: protected,
            status: 0,
        },
        Step {
            program: "apt-get",
            args: vec!["--simulate", "--", "install"],
            output: simulation,
            status: 0,
        },
    ]
}
#[test]
fn selectors_and_cross_provider_inputs_are_rejected() {
    for input in [
        "--help",
        "curl/stable",
        "curl=1",
        "./curl.deb",
        "curl*",
        "curl?",
        "curl$()",
        "curl;id",
        "brew:curl",
        "curl:amd64:extra",
        "curl:any",
        "curl:native",
        "a",
        "",
        "curl\n",
    ] {
        assert!(apt::package_id(input).is_err(), "accepted {input:?}");
    }
    for input in [
        "apt:curl:amd64",
        "libstdc++6",
        "0ad",
        "python3.13",
        "linux-image-amd64",
    ] {
        assert!(apt::package_id(input).is_ok(), "rejected {input}");
    }
}
#[test]
fn native_control_records_preserve_versions_and_dependency_alternatives() {
    let records = apt::parse_control(include_str!("../../../tests/fixtures/apt-show.txt")).unwrap();
    assert_eq!(records.len(), 2);
    assert!(records[0]["Depends"].contains(" | "));
    assert!(records[0]["Description"].contains("\n .\n"));
    assert!(apt::parse_control(" continuation without header").is_err());
    assert!(apt::parse_control("Package: curl\nPackage: other").is_err());
}
#[test]
fn search_never_interprets_query_as_an_option() {
    let mock = Mock::new(vec![Step {
        program: "apt-cache",
        args: vec!["Dir::Cache::pkgcache=", "search", "--", "--help"],
        output: "curl - transfer tool\n",
        status: 0,
    }]);
    let found = AptBackend { runner: &mock }.search("--help").unwrap();
    assert_eq!(found[0].id.name, "curl");
    mock.done();
}
#[test]
fn plan_exposes_all_native_actions_and_is_always_uncertain() {
    let mut steps = planning_steps(
        "",
        "curl\tamd64\tno\tno\n",
        include_str!("../../../tests/fixtures/apt-simulation.txt"),
    );
    steps
        .last_mut()
        .unwrap()
        .args
        .extend(["--no-remove", "curl"]);
    let mock = Mock::new(steps);
    let plan = AptBackend { runner: &mock }
        .plan(&[apt::package_id("curl").unwrap()], &[])
        .unwrap();
    assert_eq!(plan.operations.len(), 4);
    assert!(matches!(plan.operations[2].kind, OperationKind::Configure));
    assert!(plan.uncertain);
    assert!(!plan.observed_state_fingerprint.is_empty());
    mock.done();
}
#[test]
fn plans_refuse_held_changes_including_qualified_hold() {
    let mock = Mock::new(planning_steps(
        "curl:amd64\n",
        "",
        "Inst curl (8.0-1 Debian [amd64])\n",
    ));
    assert!(matches!(
        AptBackend { runner: &mock }.plan(&[apt::package_id("curl").unwrap()], &[]),
        Err(Error::Safety(_))
    ));
}
#[test]
fn plans_refuse_essential_protected_and_additive_removals() {
    for metadata in ["curl\tamd64\tyes\tno\n", "curl\tamd64\tno\tyes\n"] {
        let mock = Mock::new(planning_steps("", metadata, "Remv curl [8.0-1]\n"));
        assert!(matches!(
            AptBackend { runner: &mock }.plan(&[], &[apt::package_id("curl").unwrap()]),
            Err(Error::Safety(_))
        ));
    }
    let mock = Mock::new(planning_steps(
        "",
        "curl\tamd64\tno\tno\n",
        "Remv curl [8.0-1]\n",
    ));
    assert!(matches!(
        AptBackend { runner: &mock }.plan(&[apt::package_id("curl").unwrap()], &[]),
        Err(Error::Safety(_))
    ));
}
#[test]
fn conflicting_requests_fail_before_any_command() {
    let mock = Mock::new(vec![]);
    let id = apt::package_id("curl").unwrap();
    assert!(AptBackend { runner: &mock }
        .plan(std::slice::from_ref(&id), std::slice::from_ref(&id))
        .is_err());
    mock.done();
}

#[test]
fn execution_requires_confirmation_before_any_command() {
    let mock = Mock::new(vec![]);
    let plan = pkgx_core::Plan {
        operations: vec![],
        observed_state_fingerprint: String::new(),
        observed_packages: vec![],
        uncertain: false,
        warnings: vec![],
    };
    assert!(pkgx_core::execution::apply_apt(
        &mock,
        &plan,
        &[],
        &[],
        false,
        &AtomicBool::new(false)
    )
    .is_err());
    mock.done();
}
#[test]
fn stale_plan_aborts_before_mutation() {
    let mock = Mock::new(planning_steps("", "", "Inst curl (8.0-1 Debian [amd64])\n"));
    let preview = pkgx_core::Plan {
        operations: vec![],
        observed_state_fingerprint: "stale".into(),
        observed_packages: vec![],
        uncertain: true,
        warnings: vec![],
    };
    assert!(matches!(
        pkgx_core::execution::apply_apt(
            &mock,
            &preview,
            &[apt::package_id("curl").unwrap()],
            &[],
            true,
            &AtomicBool::new(false)
        ),
        Err(Error::Safety(_))
    ));
    mock.done();
}
#[test]
fn execution_revalidates_pins_and_verifies_inventory() {
    let mut steps = planning_steps(
        "",
        "",
        "Inst curl (8.0-1 Debian [amd64])\nConf curl (8.0-1 Debian [amd64])\n",
    );
    steps.extend(planning_steps(
        "",
        "",
        "Inst curl (8.0-1 Debian [amd64])\nConf curl (8.0-1 Debian [amd64])\n",
    ));
    steps.push(Step {
        program: "apt-get",
        args: vec!["--yes", "--no-remove", "curl=8.0-1"],
        output: "",
        status: 0,
    });
    steps.extend(planning_steps("", "", "").into_iter().take(3));
    let mock = Mock::new(steps);
    let requested = [apt::package_id("curl").unwrap()];
    let plan = AptBackend { runner: &mock }.plan(&requested, &[]).unwrap();
    let record = pkgx_core::execution::apply_apt(
        &mock,
        &plan,
        &requested,
        &[],
        true,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(record.success && record.verified);
    assert!(!record.rollback_attempted);
    mock.done();
}
#[test]
fn native_failure_remains_failure_even_if_inventory_matches() {
    let mut steps = planning_steps("", "", "Inst curl (8.0-1 Debian [amd64])\n");
    steps.extend(planning_steps("", "", "Inst curl (8.0-1 Debian [amd64])\n"));
    steps.push(Step {
        program: "apt-get",
        args: vec!["--yes"],
        output: "",
        status: 100,
    });
    steps.extend(planning_steps("", "", "").into_iter().take(3));
    let mock = Mock::new(steps);
    let requested = [apt::package_id("curl").unwrap()];
    let plan = AptBackend { runner: &mock }.plan(&requested, &[]).unwrap();
    let record = pkgx_core::execution::apply_apt(
        &mock,
        &plan,
        &requested,
        &[],
        true,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(!record.success);
    assert_eq!(record.native_status, Some(100));
    assert!(!record.errors.is_empty());
    mock.done();
}

#[test]
fn ordinary_removal_is_plannable_but_requires_review() {
    let mock = Mock::new(planning_steps(
        "",
        "curl\tamd64\tno\tno\n",
        "Remv curl [8.0-1]\n",
    ));
    let plan = AptBackend { runner: &mock }
        .plan(&[], &[apt::package_id("curl").unwrap()])
        .unwrap();
    assert_eq!(plan.operations.len(), 1);
    assert_eq!(
        plan.operations[0].package.architecture_or_scope.as_deref(),
        Some("amd64")
    );
    mock.done();
}
#[test]
fn removal_without_protection_metadata_is_refused() {
    let mock = Mock::new(planning_steps("", "", "Remv curl [8.0-1]\n"));
    assert!(matches!(
        AptBackend { runner: &mock }.plan(&[], &[apt::package_id("curl").unwrap()]),
        Err(Error::Safety(_))
    ));
    mock.done();
}
#[test]
fn successful_exit_with_wrong_inventory_is_not_success() {
    let mut steps = planning_steps("", "", "Inst curl (9.0-1 Debian [amd64])\n");
    steps.extend(planning_steps("", "", "Inst curl (9.0-1 Debian [amd64])\n"));
    steps.push(Step {
        program: "apt-get",
        args: vec!["--yes", "curl=9.0-1"],
        output: "",
        status: 0,
    });
    steps.extend(planning_steps("", "", "").into_iter().take(3));
    let mock = Mock::new(steps);
    let requested = [apt::package_id("curl").unwrap()];
    let plan = AptBackend { runner: &mock }.plan(&requested, &[]).unwrap();
    let record = pkgx_core::execution::apply_apt(
        &mock,
        &plan,
        &requested,
        &[],
        true,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(!record.success && !record.verified);
    mock.done();
}

#[test]
fn orphan_preview_reuses_hold_and_protected_package_checks() {
    for holds in ["", "curl\n"] {
        let mut steps = vec![Step {
            program: "apt-get",
            args: vec!["--simulate", "autoremove", "Dir::Cache::pkgcache="],
            output: "Remv curl [8.0-1]\n",
            status: 0,
        }];
        steps.extend(planning_steps(
            holds,
            "curl\tamd64\tno\tno\n",
            "Remv curl [8.0-1]\n",
        ));
        let mock = Mock::new(steps);
        let result = AptBackend { runner: &mock }.orphan_candidates();
        if holds.is_empty() {
            assert_eq!(
                result.unwrap()[0].architecture_or_scope.as_deref(),
                Some("amd64")
            );
        } else {
            assert!(matches!(result, Err(Error::Safety(_))));
        }
        mock.done();
    }
}
#[test]
fn execution_persists_intent_and_native_failure() {
    let mut steps = planning_steps("", "", "Inst curl (8.0-1 Debian [amd64])\n");
    steps.extend(planning_steps("", "", "Inst curl (8.0-1 Debian [amd64])\n"));
    steps.push(Step {
        program: "apt-get",
        args: vec!["--yes"],
        output: "",
        status: 100,
    });
    steps.extend(planning_steps("", "", "").into_iter().take(3));
    let mock = Mock::new(steps);
    let requested = [apt::package_id("curl").unwrap()];
    let plan = AptBackend { runner: &mock }.plan(&requested, &[]).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let journal = pkgx_core::journal::Journal::new(temp.path().join("records"));
    let result = pkgx_core::execution::apply_apt_recorded(
        &mock,
        &plan,
        &requested,
        &[],
        true,
        &AtomicBool::new(false),
        Some(&journal),
    )
    .unwrap();
    assert!(!result.success);
    let history = journal.history().unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].result.as_ref().unwrap().native_status, Some(100));
    mock.done();
}
#[test]
fn cancellation_aborts_before_revalidation_and_mutation() {
    let mock = Mock::new(vec![]);
    let plan = pkgx_core::Plan {
        operations: vec![],
        observed_state_fingerprint: String::new(),
        observed_packages: vec![],
        uncertain: false,
        warnings: vec![],
    };
    assert!(matches!(
        pkgx_core::execution::apply_apt(&mock, &plan, &[], &[], true, &AtomicBool::new(true)),
        Err(Error::Canceled(_))
    ));
    mock.done();
}

#[test]
fn native_lock_or_planning_failure_aborts_before_mutation() {
    let mut steps = planning_steps("", "", "");
    steps.last_mut().unwrap().status = 100;
    let mock = Mock::new(steps);
    assert!(matches!(
        AptBackend { runner: &mock }.plan(&[apt::package_id("curl").unwrap()], &[]),
        Err(Error::CommandFailed {
            status: Some(100),
            ..
        })
    ));
    mock.done();
}
#[test]
fn journal_failure_prevents_the_native_transaction() {
    let mut steps = planning_steps("", "", "Inst curl (8.0-1 Debian [amd64])\n");
    steps.extend(planning_steps("", "", "Inst curl (8.0-1 Debian [amd64])\n"));
    let mock = Mock::new(steps);
    let requested = [apt::package_id("curl").unwrap()];
    let plan = AptBackend { runner: &mock }.plan(&requested, &[]).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let invalid = temp.path().join("regular-file");
    std::fs::write(&invalid, b"keep").unwrap();
    let journal = pkgx_core::journal::Journal::new(&invalid);
    assert!(pkgx_core::execution::apply_apt_recorded(
        &mock,
        &plan,
        &requested,
        &[],
        true,
        &AtomicBool::new(false),
        Some(&journal)
    )
    .is_err());
    assert_eq!(std::fs::read(invalid).unwrap(), b"keep");
    mock.done();
}

#[test]
fn virtual_alias_cannot_satisfy_a_concrete_manifest_identity() {
    let mock = Mock::new(planning_steps(
        "",
        "",
        "Inst alternative-tool (1.0 Debian [amd64])\n",
    ));
    assert!(matches!(
        AptBackend { runner: &mock }.plan(&[apt::package_id("virtual-tool").unwrap()], &[]),
        Err(Error::Unsupported(_))
    ));
    mock.done();
}

#[test]
fn installing_an_existing_dependency_reviews_and_verifies_its_manual_mark() {
    for mark_verified in [true, false] {
        let before = || {
            let mut steps = planning_steps("", "", "curl set to manually installed.\n");
            steps[1].output = "";
            steps[2].output = "curl\nzlib1g\n";
            steps
        };
        let mut steps = before();
        steps.extend(before());
        steps.push(Step {
            program: "apt-get",
            args: vec!["--yes", "--no-remove", "curl=8.0-1"],
            output: "",
            status: 0,
        });
        let mut after = planning_steps("", "", "");
        if !mark_verified {
            after[1].output = "";
        }
        steps.extend(after.into_iter().take(3));
        let mock = Mock::new(steps);
        let requested = [apt::package_id("curl").unwrap()];
        let plan = AptBackend { runner: &mock }.plan(&requested, &[]).unwrap();
        assert_eq!(plan.operations.len(), 1);
        assert!(matches!(
            plan.operations[0].kind,
            OperationKind::MarkExplicit
        ));
        let record = pkgx_core::execution::apply_apt(
            &mock,
            &plan,
            &requested,
            &[],
            true,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(record.success, mark_verified);
        assert_eq!(record.verified, mark_verified);
        mock.done();
    }
}
#[test]
fn unexpected_additions_removals_and_upgrades_fail_verification() {
    for inventory in [
        "curl\tamd64\t8.0-1\tinstalled\nzlib1g\tamd64\t1:1.3-1\tinstalled\nzlib1g\ti386\t1:1.3-1\tinstalled\nextra-tool\tamd64\t1\tinstalled\n",
        "curl\tamd64\t8.0-1\tinstalled\n",
        "curl\tamd64\t8.0-1\tinstalled\nzlib1g\tamd64\t9.0\tinstalled\nzlib1g\ti386\t1:1.3-1\tinstalled\n",
    ] {
        let mut steps = planning_steps("", "", "Inst curl (8.0-1 Debian [amd64])\n");
        steps.extend(planning_steps("", "", "Inst curl (8.0-1 Debian [amd64])\n"));
        steps.push(Step { program: "apt-get", args: vec!["--yes"], output: "", status: 0 });
        let mut after = planning_steps("", "", ""); after[0].output = inventory;
        steps.extend(after.into_iter().take(3));
        let mock = Mock::new(steps); let requested = [apt::package_id("curl").unwrap()];
        let plan = AptBackend { runner: &mock }.plan(&requested,&[]).unwrap();
        let record = pkgx_core::execution::apply_apt(&mock,&plan,&requested,&[],true,&AtomicBool::new(false)).unwrap();
        assert!(!record.success && !record.verified);
        assert!(record.errors.iter().any(|e| e.contains("outside reviewed package actions")));
        mock.done();
    }
}
#[test]
fn ambiguous_multiarch_manual_request_is_refused() {
    let mock = Mock::new(planning_steps("", "", ""));
    assert!(matches!(
        AptBackend { runner: &mock }.plan(&[apt::package_id("zlib1g").unwrap()], &[]),
        Err(Error::Safety(_))
    ));
    mock.done();
}

#[test]
fn purge_actions_are_never_silently_dropped_from_a_plan() {
    assert!(matches!(
        apt::parse_simulation("Purg old-tool [1.0]\n"),
        Err(Error::Safety(_))
    ));
    let mock = Mock::new(planning_steps(
        "",
        "curl\tamd64\tno\tno\n",
        "Purg curl [8.0-1]\n",
    ));
    assert!(matches!(
        AptBackend { runner: &mock }.plan(&[], &[apt::package_id("curl").unwrap()]),
        Err(Error::Safety(_))
    ));
    mock.done();
}

#[test]
fn strict_execution_keeps_grants_locked_and_verifies_native_removal() {
    let make_steps = || planning_steps("", "curl\tamd64\tno\tno\n", "Remv curl [8.0-1]\n");
    let mut steps = make_steps();
    steps.extend(make_steps());
    steps.push(Step {
        program: "apt-get",
        args: vec!["--yes", "curl:amd64-"],
        output: "",
        status: 0,
    });
    steps.push(Step {
        program: "dpkg-query",
        args: vec!["-W"],
        output: "zlib1g\tamd64\t1:1.3-1\tinstalled\nzlib1g\ti386\t1:1.3-1\tinstalled\n",
        status: 0,
    });
    steps.push(Step {
        program: "apt-mark",
        args: vec!["showmanual"],
        output: "",
        status: 0,
    });
    steps.push(Step {
        program: "apt-mark",
        args: vec!["showauto"],
        output: "zlib1g\n",
        status: 0,
    });
    let mock = Mock::new(steps);
    let requested = [apt::package_id("curl:amd64").unwrap()];
    let plan = AptBackend { runner: &mock }.plan(&[], &requested).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let registry = pkgx_core::managed::ManagedRegistry::new(temp.path().join("managed"));
    let mut guard = registry.lock(&registry.read().unwrap()).unwrap();
    guard
        .grant(
            plan.observed_packages
                .iter()
                .find(|p| p.id == requested[0])
                .unwrap(),
        )
        .unwrap();
    let journal = pkgx_core::journal::Journal::new(temp.path().join("journal"));
    let result = pkgx_core::execution::apply_managed_apt(
        &mock,
        &plan,
        &[],
        &requested,
        true,
        &AtomicBool::new(false),
        Some(&journal),
        &guard,
    )
    .unwrap();
    assert!(result.success && result.verified);
    assert!(
        journal.history().unwrap()[0]
            .result
            .as_ref()
            .unwrap()
            .success
    );
    assert!(registry.lock(&guard.state).is_err());
    mock.done();
}

#[test]
fn strict_registry_change_during_revalidation_aborts_before_native_execution() {
    struct Mutator {
        mock: Mock,
        path: std::path::PathBuf,
    }
    impl CommandRunner for Mutator {
        fn run(
            &self,
            spec: &CommandSpec,
            timeout: Duration,
            canceled: &AtomicBool,
        ) -> Result<CommandOutput, Error> {
            let output = self.mock.run(spec, timeout, canceled)?;
            if spec.args.contains(&"--simulate".into()) {
                std::fs::write(
                    &self.path,
                    serde_json::to_vec(&pkgx_core::managed::ManagedState::default()).unwrap(),
                )
                .unwrap();
            }
            Ok(output)
        }
    }
    let make_steps = || planning_steps("", "curl\tamd64\tno\tno\n", "Remv curl [8.0-1]\n");
    let preview_mock = Mock::new(make_steps());
    let requested = [apt::package_id("curl:amd64").unwrap()];
    let plan = AptBackend {
        runner: &preview_mock,
    }
    .plan(&[], &requested)
    .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let registry = pkgx_core::managed::ManagedRegistry::new(temp.path().join("managed"));
    let mut guard = registry.lock(&registry.read().unwrap()).unwrap();
    guard
        .grant(
            plan.observed_packages
                .iter()
                .find(|p| p.id == requested[0])
                .unwrap(),
        )
        .unwrap();
    let runner = Mutator {
        mock: Mock::new(make_steps()),
        path: temp.path().join("managed/registry.json"),
    };
    assert!(pkgx_core::execution::apply_managed_apt(
        &runner,
        &plan,
        &[],
        &requested,
        true,
        &AtomicBool::new(false),
        None,
        &guard
    )
    .is_err());
    runner.mock.done();
}

#[test]
fn sudo_execution_authenticates_before_revalidation_and_elevates_only_the_transaction() {
    use pkgx_core::privilege::{authorize_apt, SudoAptRunner, SudoAuthenticator};
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Auth(AtomicUsize);
    impl SudoAuthenticator for Auth {
        fn authenticate(&self, _: &AtomicBool) -> Result<(), Error> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }
    struct AfterAuth<'a> {
        mock: Mock,
        auth: &'a Auth,
    }
    impl CommandRunner for AfterAuth<'_> {
        fn run(
            &self,
            spec: &CommandSpec,
            timeout: Duration,
            canceled: &AtomicBool,
        ) -> Result<CommandOutput, Error> {
            assert_eq!(
                self.auth.0.load(Ordering::Relaxed),
                1,
                "revalidation must follow authentication"
            );
            assert!(
                !spec.requires_privilege,
                "the wrapper must consume the transaction marker"
            );
            self.mock.run(spec, timeout, canceled)
        }
    }
    for status in [0, 1] {
        let preview_mock = Mock::new(planning_steps("", "", "Inst curl (8.0-1 Debian [amd64])\n"));
        let requested = [apt::package_id("curl").unwrap()];
        let preview = AptBackend {
            runner: &preview_mock,
        }
        .plan(&requested, &[])
        .unwrap();
        let auth = Auth(AtomicUsize::new(0));
        let canceled = AtomicBool::new(false);
        assert!(authorize_apt(&auth, true, &preview, true, &canceled).unwrap());
        let mut steps = planning_steps("", "", "Inst curl (8.0-1 Debian [amd64])\n");
        steps.push(Step {
            program: "/usr/bin/sudo",
            args: vec![
                "-n",
                "--",
                "/usr/bin/apt-get",
                "--yes",
                "--no-remove",
                "curl=8.0-1",
            ],
            output: "",
            status,
        });
        steps.extend(planning_steps("", "", "").into_iter().take(3));
        let inner = AfterAuth {
            mock: Mock::new(steps),
            auth: &auth,
        };
        let elevated = SudoAptRunner::new(&inner);
        let record =
            pkgx_core::execution::apply_apt(&elevated, &preview, &requested, &[], true, &canceled)
                .unwrap();
        assert_eq!(record.success, status == 0);
        assert_eq!(record.native_status, Some(status));
        assert_eq!(
            auth.0.load(Ordering::Relaxed),
            1,
            "never prompt again during execution"
        );
        inner.mock.done();
    }
}
