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
    privilege::SudoAptRunner,
    providers::{
        dnf_generation, split_nevra, DnfGeneration, NativeBackend, DNF4_INSTALL, DNF4_REMOVE,
        DNF_INSTALL,
    },
    Error, PackageId, ProviderId,
};

/// name -> EVR. `hello` resolves to itself plus the weak dependency `info`.
struct Dnf {
    /// DNF4 (`dnf` with its Python resolver) rather than DNF5.
    four: bool,
    installed: Mutex<BTreeMap<String, String>>,
    upgrade_glibc: bool,
    drift: Option<&'static str>,
    executed: Mutex<Vec<CommandSpec>>,
}
fn dnf(installed: &[(&str, &str)]) -> Dnf {
    Dnf {
        four: false,
        installed: Mutex::new(
            installed
                .iter()
                .map(|(n, v)| ((*n).into(), (*v).into()))
                .collect(),
        ),
        upgrade_glibc: false,
        drift: None,
        executed: Mutex::new(vec![]),
    }
}
const AVAILABLE: &[(&str, &str)] = &[("hello", "2.12.3-1.fc44"), ("info", "1:7.2-9.fc44")];
impl CommandRunner for Dnf {
    fn run(&self, spec: &CommandSpec, _: Duration, _: &AtomicBool) -> Result<CommandOutput, Error> {
        let mut installed = self.installed.lock().unwrap();
        let program = match spec.program.as_str() {
            "dnf" if spec.args == ["--version"] => {
                let version = if self.four {
                    "4.14.0\n"
                } else {
                    "dnf5 version 5.2.7.0\n"
                };
                return Ok(CommandOutput {
                    status: Some(0),
                    stdout: version.into(),
                    stderr: vec![],
                });
            }
            "dnf" | "python3" | "/usr/libexec/platform-python" => {
                assert!(self.four, "DNF5 machine asked DNF4's {}", spec.program);
                spec.program.as_str()
            }
            "dnf5" => {
                assert!(!self.four, "DNF4 machine asked dnf5");
                "dnf5"
            }
            other => other,
        };
        // DNF4's resolver: `python3 -c SCRIPT install|remove TARGETS`, printed
        // as dnf5 stores it.
        if program != "dnf" && program != "dnf5" && program != "rpm" {
            assert_eq!(spec.args[0], "-c");
            assert!(!spec.requires_privilege);
            let rpms = self.resolve(&installed, &spec.args[2], &spec.args[3..]);
            let mut rpms: Vec<serde_json::Value> = rpms;
            for rpm in &mut rpms {
                let reason = rpm["reason"].as_str().unwrap().to_lowercase();
                rpm["reason"] = reason.into();
            }
            return Ok(CommandOutput {
                status: Some(0),
                stdout: serde_json::json!({"version": "dnf4", "rpms": rpms})
                    .to_string()
                    .into_bytes(),
                stderr: vec![],
            });
        }
        let stdout = match program {
            "rpm" => installed
                .iter()
                .map(|(n, v)| {
                    format!(
                        "{n}\tx86_64\t{}\n",
                        if v.contains(':') {
                            v.clone()
                        } else {
                            format!("0:{v}")
                        }
                    )
                })
                .collect(),
            "dnf5" | "dnf" => {
                let store = spec.args.iter().find_map(|a| a.strip_prefix("--store="));
                let targets: Vec<_> = spec.args[2..]
                    .iter()
                    .filter(|a| !a.starts_with("--"))
                    .cloned()
                    .collect();
                if let Some(store) = store {
                    assert!(!spec.requires_privilege);
                    let rpms = self.resolve(&installed, &spec.args[0], &targets);
                    if !rpms.is_empty() {
                        std::fs::create_dir_all(store).unwrap();
                        std::fs::write(
                            format!("{store}/transaction.json"),
                            serde_json::json!({"rpms": rpms, "version": "1.0"}).to_string(),
                        )
                        .unwrap();
                    }
                } else {
                    assert!(spec.requires_privilege);
                    self.executed.lock().unwrap().push(spec.clone());
                    for target in &targets {
                        let (name, _, evr) = split_nevra(target).unwrap();
                        if spec.args[0] == "remove" {
                            installed.remove(&name);
                        } else {
                            for (n, v) in AVAILABLE {
                                if *n == name || (*n == "info" && name == "hello") {
                                    installed.insert((*n).into(), (*v).into());
                                }
                            }
                            assert!(installed[&name] == evr || evr.ends_with(&installed[&name]));
                        }
                    }
                    if let Some(name) = self.drift {
                        installed.insert(name.into(), "9-9".into());
                    }
                }
                String::new()
            }
            other => panic!("unexpected {other}"),
        };
        Ok(CommandOutput {
            status: Some(0),
            stdout: stdout.into_bytes(),
            stderr: vec![],
        })
    }
}
impl Dnf {
    /// The transaction either DNF resolves for `op` on `targets`.
    fn resolve(
        &self,
        installed: &BTreeMap<String, String>,
        op: &str,
        targets: &[String],
    ) -> Vec<serde_json::Value> {
        let mut rpms = vec![];
        for target in targets {
            let name = target.split('.').next().unwrap();
            if op == "remove" {
                let v = &installed[name];
                rpms.push(serde_json::json!({"nevra": format!("{name}-{v}.x86_64"), "action":"Remove", "reason":"User", "repo_id":"@System"}));
            } else {
                for (n, v) in AVAILABLE {
                    if (*n == name || (*n == "info" && name == "hello"))
                        && !installed.contains_key(*n)
                    {
                        rpms.push(serde_json::json!({"nevra": format!("{n}-{v}.x86_64"), "action":"Install", "reason": if *n == name {"User"} else {"Weak Dependency"}, "repo_id":"fedora"}));
                    }
                }
            }
        }
        if self.upgrade_glibc {
            rpms.push(serde_json::json!({"nevra":"glibc-2.43-2.fc44.x86_64","action":"Upgrade","reason":"Dependency","repo_id":"updates"}));
            rpms.push(serde_json::json!({"nevra":"glibc-2.43-1.fc44.x86_64","action":"Replaced","reason":"Dependency","repo_id":"@System"}));
        }
        rpms
    }
}
fn id(name: &str) -> PackageId {
    parse_name(ProviderId::Dnf, name).unwrap()
}
fn plan(mock: &Dnf, install: &[PackageId], remove: &[PackageId]) -> Result<xpac_core::Plan, Error> {
    NativeBackend {
        provider: ProviderId::Dnf,
        runner: mock,
    }
    .plan(install, remove)
}

#[test]
fn nevras_split_with_and_without_epochs() {
    assert_eq!(
        split_nevra("hello-2.12.3-1.fc44.x86_64").unwrap(),
        ("hello".into(), "x86_64".into(), "0:2.12.3-1.fc44".into())
    );
    assert_eq!(
        split_nevra("java-21-openjdk-1:21.0.5.0.11-1.fc44.x86_64").unwrap(),
        (
            "java-21-openjdk".into(),
            "x86_64".into(),
            "1:21.0.5.0.11-1.fc44".into()
        )
    );
    for bad in [
        "hello",
        "hello-1.x86_64",
        "-1-1.x86_64",
        "a-1-1;rm.x86_64",
        "--x-1-1.noarch",
    ] {
        assert!(split_nevra(bad).is_err(), "{bad}");
    }
}
#[test]
fn stored_transaction_preview_pins_reviewed_nevras_and_verifies_versions() {
    let mock = dnf(&[("bash", "5.3-1.fc44")]);
    let request = [id("hello")];
    let preview = plan(&mock, &request, &[]).unwrap();
    assert_eq!(preview.operations.len(), 2);
    assert!(preview.operations[1]
        .native_detail
        .contains("Weak Dependency"));
    let record = apply_recorded(
        &mock,
        &preview,
        &request,
        &[],
        true,
        &AtomicBool::new(false),
        None,
    )
    .unwrap();
    assert!(record.success, "{:?}", record.errors);
    let executed = mock.executed.lock().unwrap();
    let mut expected: Vec<String> = DNF_INSTALL[1..].iter().map(|s| (*s).into()).collect();
    expected.push("hello-2.12.3-1.fc44.x86_64".into());
    assert_eq!(executed[0].args, expected);
}
#[test]
fn removal_keeps_dependencies_and_noop_and_stale_plans_run_nothing() {
    let mock = dnf(&[("hello", "2.12.3-1.fc44"), ("info", "1:7.2-9.fc44")]);
    let preview = plan(&mock, &[id("hello")], &[]).unwrap();
    assert!(preview.operations.is_empty());
    assert!(
        apply_recorded(
            &mock,
            &preview,
            &[id("hello")],
            &[],
            true,
            &AtomicBool::new(false),
            None
        )
        .unwrap()
        .success
    );
    let removal = plan(&mock, &[], &[id("hello")]).unwrap();
    mock.installed
        .lock()
        .unwrap()
        .insert("zsh".into(), "5.9-1".into());
    assert!(matches!(
        apply_recorded(&mock, &removal, &[], &[id("hello")], true, &AtomicBool::new(false), None),
        Err(Error::Safety(m)) if m.contains("stale")
    ));
    assert!(mock.executed.lock().unwrap().is_empty());
    let removal = plan(&mock, &[], &[id("hello")]).unwrap();
    let record = apply_recorded(
        &mock,
        &removal,
        &[],
        &[id("hello")],
        true,
        &AtomicBool::new(false),
        None,
    )
    .unwrap();
    assert!(record.success, "{:?}", record.errors);
    assert!(mock.executed.lock().unwrap()[0]
        .args
        .contains(&"--no-autoremove".into()));
    assert!(mock.installed.lock().unwrap().contains_key("info"));
}
#[test]
fn upgrades_are_flagged_and_unexpected_effects_fail_verification() {
    let mut mock = dnf(&[("glibc", "2.43-1.fc44")]);
    mock.upgrade_glibc = true;
    let preview = plan(&mock, &[id("hello")], &[]).unwrap();
    assert!(preview.warnings.iter().any(|w| w.contains("upgrades")));
    assert_eq!(preview.operations.len(), 3);
    let mut mock = dnf(&[]);
    mock.drift = Some("bash");
    let preview = plan(&mock, &[id("hello")], &[]).unwrap();
    let record = apply_recorded(
        &mock,
        &preview,
        &[id("hello")],
        &[],
        true,
        &AtomicBool::new(false),
        None,
    )
    .unwrap();
    assert!(!record.success && !record.verified);
}
#[test]
fn sudo_delegates_only_dnf_templates_with_nevra_targets() {
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
            Ok(CommandOutput {
                status: Some(0),
                stdout: vec![],
                stderr: vec![],
            })
        }
    }
    let inner = Inner::default();
    let runner = SudoAptRunner::new(&inner);
    let run = |args: &[&str]| {
        let mut spec = CommandSpec::new(DNF_INSTALL[0], args);
        spec.requires_privilege = true;
        runner.run(&spec, Duration::from_secs(1), &AtomicBool::new(false))
    };
    assert!(run(&["install", "--assumeyes", "hello-2.12.3-1.fc44.x86_64"]).is_ok());
    assert!(run(&[
        "install",
        "--assumeyes",
        "--nogpgcheck",
        "hello-2.12.3-1.fc44.x86_64"
    ])
    .is_err());
    assert!(run(&["install", "--assumeyes", "hello"]).is_err());
    assert!(run(&["install", "--assumeyes"]).is_err());
    assert_eq!(
        inner.0.lock().unwrap()[0].args[4],
        xpac_core::privilege::DNF
    );
}
fn dnf4(installed: &[(&str, &str)]) -> Dnf {
    let mut mock = dnf(installed);
    mock.four = true;
    mock
}
#[test]
fn the_generation_is_read_from_dnf_itself() {
    assert_eq!(dnf_generation(&dnf(&[])).unwrap(), DnfGeneration::Five);
    assert_eq!(dnf_generation(&dnf4(&[])).unwrap(), DnfGeneration::Four);
}
#[test]
fn dnf4_resolves_through_its_own_module_and_pins_the_reviewed_nevras() {
    let mock = dnf4(&[("bash", "5.1.8-9.el9")]);
    let request = [id("hello")];
    let preview = plan(&mock, &request, &[]).unwrap();
    assert_eq!(preview.operations.len(), 2);
    assert!(preview.warnings.iter().any(|w| w.contains("DNF4 preview")));
    assert!(preview.operations[1]
        .native_detail
        .contains("weak dependency"));
    let record = apply_recorded(
        &mock,
        &preview,
        &request,
        &[],
        true,
        &AtomicBool::new(false),
        None,
    )
    .unwrap();
    assert!(record.success, "{:?}", record.errors);
    let executed = mock.executed.lock().unwrap();
    assert_eq!(executed[0].program, "dnf");
    let mut expected: Vec<String> = DNF4_INSTALL[1..].iter().map(|s| (*s).into()).collect();
    expected.push("hello-2.12.3-1.fc44.x86_64".into());
    assert_eq!(executed[0].args, expected);
}
#[test]
fn dnf4_removal_keeps_dependencies() {
    let mock = dnf4(&[("hello", "2.12.3-1.fc44"), ("info", "1:7.2-9.fc44")]);
    let removal = plan(&mock, &[], &[id("hello")]).unwrap();
    let record = apply_recorded(
        &mock,
        &removal,
        &[],
        &[id("hello")],
        true,
        &AtomicBool::new(false),
        None,
    )
    .unwrap();
    assert!(record.success, "{:?}", record.errors);
    let executed = mock.executed.lock().unwrap();
    let mut expected: Vec<String> = DNF4_REMOVE[1..].iter().map(|s| (*s).into()).collect();
    expected.push("hello-2.12.3-1.fc44.x86_64".into());
    assert_eq!(executed[0].args, expected);
    assert!(mock.installed.lock().unwrap().contains_key("info"));
}
#[test]
fn sudo_delegates_dnf4_templates_alone_to_dnf() {
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
            Ok(CommandOutput {
                status: Some(0),
                stdout: vec![],
                stderr: vec![],
            })
        }
    }
    let inner = Inner::default();
    let runner = SudoAptRunner::new(&inner);
    let run = |args: &[&str]| {
        let mut spec = CommandSpec::new("dnf", args);
        spec.requires_privilege = true;
        runner.run(&spec, Duration::from_secs(1), &AtomicBool::new(false))
    };
    assert!(run(&["install", "--assumeyes", "hello-2.12.3-1.el9.x86_64"]).is_ok());
    assert!(run(&[
        "remove",
        "--assumeyes",
        "--noautoremove",
        "hello-0:2.12.3-1.el9.x86_64"
    ])
    .is_ok());
    // dnf5's spelling, an option between, or a name that is no NEVRA: refused.
    assert!(run(&[
        "remove",
        "--assumeyes",
        "--no-autoremove",
        "hello-2.12.3-1.el9.x86_64"
    ])
    .is_err());
    assert!(run(&[
        "install",
        "--assumeyes",
        "--nogpgcheck",
        "hello-2.12.3-1.el9.x86_64"
    ])
    .is_err());
    assert!(run(&["install", "--assumeyes", "hello"]).is_err());
    assert_eq!(
        inner.0.lock().unwrap()[0].args[4],
        xpac_core::privilege::DNF4
    );
}
