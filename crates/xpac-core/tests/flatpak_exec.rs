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
    providers::NativeBackend,
    Error, PackageId, ProviderId,
};

const APP: &str = "app/org.xpac.Hello/x86_64/stable";
const RUNTIME: &str = "runtime/org.xpac.Platform/x86_64/1";
const APP_COMMIT: &str = "9122cefcef7762a85c0498d145978684972c34d7e1a3bce2e335ca5692226697";
const RT_COMMIT: &str = "22abde9fa0b7aa85c0498d145978684972c34d7e1a3bce2e335ca5692226697";

/// User-scope refs: ref -> (origin, active commit). Remotes -> offered refs.
struct Flatpak {
    installed: Mutex<BTreeMap<String, (String, String)>>,
    remotes: Vec<(&'static str, Vec<&'static str>)>,
    wrong_commit: bool,
    executed: Mutex<Vec<CommandSpec>>,
}
fn flatpak(remotes: Vec<(&'static str, Vec<&'static str>)>) -> Flatpak {
    Flatpak {
        installed: Mutex::new(BTreeMap::new()),
        remotes,
        wrong_commit: false,
        executed: Mutex::new(vec![]),
    }
}
fn commit(reference: &str) -> &'static str {
    if reference == APP {
        APP_COMMIT
    } else {
        RT_COMMIT
    }
}
impl CommandRunner for Flatpak {
    fn run(&self, spec: &CommandSpec, _: Duration, _: &AtomicBool) -> Result<CommandOutput, Error> {
        assert_eq!(spec.program, "flatpak");
        assert!(!spec.requires_privilege);
        let a: Vec<&str> = spec.args.iter().map(String::as_str).collect();
        let mut installed = self.installed.lock().unwrap();
        let (status, out) = match a.as_slice() {
            ["list", "--all", scope, kind, _] => {
                let kind = kind.trim_start_matches("--");
                let rows: String = installed
                    .iter()
                    .filter(|(r, _)| *scope == "--user" && r.starts_with(&format!("{kind}/")))
                    .map(|(r, (origin, c))| {
                        format!(
                            "{}\t\t{}\t{origin}\n",
                            r.split_once('/').unwrap().1,
                            &c[..12]
                        )
                    })
                    .collect();
                (0, rows)
            }
            ["remotes", "--user", "--columns=name"] => (
                0,
                self.remotes.iter().map(|(r, _)| format!("{r}\n")).collect(),
            ),
            ["remote-info", "--user", flag, remote, reference] => {
                let offers = self
                    .remotes
                    .iter()
                    .any(|(r, refs)| r == remote && refs.contains(reference));
                match (offers, *flag) {
                    (false, _) => (1, String::new()),
                    (true, "--show-commit") => (0, format!("{}\n", commit(reference))),
                    (true, _) => (0, "org.xpac.Platform/x86_64/1\n".into()),
                }
            }
            ["install", "--user", "--noninteractive", "--no-related", "--", remote, refs @ ..] => {
                self.executed.lock().unwrap().push(spec.clone());
                for reference in refs {
                    let c = if self.wrong_commit {
                        "f".repeat(64)
                    } else {
                        commit(reference).into()
                    };
                    installed.insert((*reference).into(), ((*remote).into(), c));
                    if reference.starts_with("app/") {
                        installed.insert(RUNTIME.into(), ((*remote).into(), RT_COMMIT.into()));
                    }
                }
                (0, String::new())
            }
            ["uninstall", "--user", "--noninteractive", "--", refs @ ..] => {
                self.executed.lock().unwrap().push(spec.clone());
                for reference in refs {
                    installed.remove(*reference);
                }
                (0, String::new())
            }
            other => panic!("unexpected flatpak {other:?}"),
        };
        Ok(CommandOutput {
            status: Some(status),
            stdout: out.into_bytes(),
            stderr: vec![],
        })
    }
}
fn id(reference: &str, scope: &str) -> PackageId {
    parse_name(ProviderId::Flatpak, &format!("{reference}:{scope}")).unwrap()
}
fn plan(
    mock: &Flatpak,
    install: &[PackageId],
    remove: &[PackageId],
) -> Result<xpac_core::Plan, Error> {
    NativeBackend {
        provider: ProviderId::Flatpak,
        runner: mock,
    }
    .plan(install, remove)
}
fn apply(
    mock: &Flatpak,
    preview: &xpac_core::Plan,
    install: &[PackageId],
    remove: &[PackageId],
) -> xpac_core::ExecutionRecord {
    apply_recorded(
        mock,
        preview,
        install,
        remove,
        true,
        &AtomicBool::new(false),
        None,
    )
    .unwrap()
}

#[test]
fn install_previews_the_single_remote_commit_and_missing_runtime_then_verifies() {
    let mock = flatpak(vec![("local", vec![APP, RUNTIME])]);
    let request = [id(APP, "user")];
    let preview = plan(&mock, &request, &[]).unwrap();
    assert_eq!(preview.operations.len(), 2);
    assert!(preview.operations[0]
        .native_detail
        .contains("runtime dependency"));
    let record = apply(&mock, &preview, &request, &[]);
    assert!(record.success, "{:?}", record.errors);
    assert_eq!(
        mock.executed.lock().unwrap()[0].args,
        [
            "install",
            "--user",
            "--noninteractive",
            "--no-related",
            "--",
            "local",
            APP
        ]
    );
    // Installed: a verified no-op.
    let again = plan(&mock, &request, &[]).unwrap();
    assert!(again.operations.is_empty());
    assert!(apply(&mock, &again, &request, &[]).success);
}
#[test]
fn ambiguous_or_missing_origins_and_runtime_removal_are_refused() {
    let both = flatpak(vec![("a", vec![APP, RUNTIME]), ("b", vec![APP])]);
    assert!(
        matches!(plan(&both, &[id(APP, "user")], &[]), Err(Error::Safety(m)) if m.contains("exactly one"))
    );
    let none = flatpak(vec![("a", vec![])]);
    assert!(plan(&none, &[id(APP, "user")], &[]).is_err());
    let split = flatpak(vec![("a", vec![APP])]);
    assert!(
        matches!(plan(&split, &[id(APP, "user")], &[]), Err(Error::Safety(m)) if m.contains("install it explicitly"))
    );
    let mock = flatpak(vec![]);
    mock.installed
        .lock()
        .unwrap()
        .insert(RUNTIME.into(), ("local".into(), RT_COMMIT.into()));
    assert!(matches!(
        plan(&mock, &[], &[id(RUNTIME, "user")]),
        Err(Error::Safety(_))
    ));
    assert!(plan(&mock, &[id(APP, "user"), id(APP, "system")], &[]).is_err());
    assert!(mock.executed.lock().unwrap().is_empty());
}
#[test]
fn unexpected_commits_fail_and_app_removal_keeps_the_runtime() {
    let mut mock = flatpak(vec![("local", vec![APP, RUNTIME])]);
    mock.wrong_commit = true;
    let request = [id(APP, "user")];
    let preview = plan(&mock, &request, &[]).unwrap();
    assert!(!apply(&mock, &preview, &request, &[]).verified);
    mock.wrong_commit = false;
    let removal = plan(&mock, &[], &request).unwrap();
    let record = apply(&mock, &removal, &[], &request);
    assert!(record.success, "{:?}", record.errors);
    assert!(mock.installed.lock().unwrap().contains_key(RUNTIME));
}
