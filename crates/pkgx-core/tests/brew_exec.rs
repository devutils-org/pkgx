use pkgx_core::{
    backend::Backend,
    command::{CommandOutput, CommandRunner, CommandSpec},
    execution::apply_recorded,
    identity::parse_name,
    providers::{parse_brew_dry_run, NativeBackend},
    Error, PackageId, ProviderId,
};
use std::{
    collections::BTreeMap,
    sync::{atomic::AtomicBool, Mutex},
    time::Duration,
};

/// formula -> (version, on_request). `jq` depends on `oniguruma`.
struct Brew {
    installed: Mutex<BTreeMap<String, (String, bool)>>,
    executed: Mutex<Vec<CommandSpec>>,
}
fn brew(installed: &[(&str, &str, bool)]) -> Brew {
    Brew {
        installed: Mutex::new(
            installed
                .iter()
                .map(|(n, v, r)| ((*n).into(), ((*v).into(), *r)))
                .collect(),
        ),
        executed: Mutex::new(vec![]),
    }
}
fn version(name: &str) -> &'static str {
    if name == "jq" {
        "1.8.2"
    } else {
        "6.9.10"
    }
}
impl CommandRunner for Brew {
    fn run(&self, spec: &CommandSpec, _: Duration, _: &AtomicBool) -> Result<CommandOutput, Error> {
        assert_eq!(spec.program, "brew");
        assert_eq!(spec.env["HOMEBREW_NO_AUTO_UPDATE"], "1");
        assert_eq!(spec.env["HOMEBREW_NO_AUTOREMOVE"], "1");
        assert!(!spec.requires_privilege);
        let a: Vec<&str> = spec.args.iter().map(String::as_str).collect();
        let mut installed = self.installed.lock().unwrap();
        let out = match a.as_slice() {
            ["info", "--json=v2", "--installed"] => {
                let formulae: Vec<_> = installed
                    .iter()
                    .map(|(n, (v, r))| serde_json::json!({"full_name": n, "installed": [{"version": v, "installed_on_request": r, "installed_as_dependency": !r}], "dependencies": if n == "jq" {vec!["oniguruma"]} else {vec![]}}))
                    .collect();
                serde_json::json!({"formulae": formulae, "casks": []}).to_string()
            }
            ["install", "--dry-run", "--formula", names @ ..] => {
                let mut text = String::new();
                let missing: Vec<_> = names
                    .iter()
                    .filter(|n| !installed.contains_key(**n))
                    .collect();
                if !missing.is_empty() {
                    text.push_str(&format!("==> Would install {} formula:\n", missing.len()));
                    for n in &missing {
                        text.push_str(&format!("{n} {}\n", version(n)));
                    }
                    text.push_str(
                        "==> Downloading https://ghcr.io/v2/homebrew/core/jq/manifests/1.8.2-1\n",
                    );
                    if missing.iter().any(|n| **n == "jq") && !installed.contains_key("oniguruma") {
                        // Homebrew 7 omits dependency versions.
                        text.push_str("==> Would install 1 dependency for jq:\noniguruma\n");
                    }
                } else {
                    text.push_str("Warning: already installed\n");
                }
                text
            }
            ["info", "--json=v2", "--formula", "oniguruma"] => {
                serde_json::json!({"formulae": [{"name": "oniguruma", "versions": {"stable": "6.9.10"}, "revision": 0}]}).to_string()
            }
            ["uses", "--installed", "--formula", name] => {
                if *name == "oniguruma" && installed.contains_key("jq") {
                    "jq\n".into()
                } else {
                    String::new()
                }
            }
            ["install", "--formula", names @ ..] => {
                self.executed.lock().unwrap().push(spec.clone());
                for n in names {
                    installed.insert((*n).into(), (version(n).into(), true));
                    if *n == "jq" {
                        installed
                            .entry("oniguruma".into())
                            .or_insert(("6.9.10".into(), false));
                    }
                }
                String::new()
            }
            ["uninstall", "--formula", names @ ..] => {
                self.executed.lock().unwrap().push(spec.clone());
                for n in names {
                    installed.remove(*n);
                }
                String::new()
            }
            other => panic!("unexpected brew {other:?}"),
        };
        Ok(CommandOutput {
            status: Some(0),
            stdout: out.into_bytes(),
            stderr: vec![],
        })
    }
}
fn id(name: &str) -> PackageId {
    parse_name(ProviderId::Brew, &format!("{name}:formula")).unwrap()
}
fn plan(
    mock: &Brew,
    install: &[PackageId],
    remove: &[PackageId],
) -> Result<pkgx_core::Plan, Error> {
    NativeBackend {
        provider: ProviderId::Brew,
        runner: mock,
    }
    .plan(install, remove)
}

#[test]
fn dry_run_sections_parse_and_unknown_actions_fail_closed() {
    let rows = parse_brew_dry_run(
        "==> Would install 1 formula:\njq 1.8.2\n==> Downloading x\n==> Would install 2 dependencies for jq:\noniguruma 6.9.10, libfoo 1.0_1\n",
    )
    .unwrap();
    assert_eq!(rows.len(), 3);
    assert!(!rows[0].2 && rows[1].2);
    assert!(parse_brew_dry_run("==> Would upgrade 1 outdated package:\nx 1\n").is_err());
    assert_eq!(
        parse_brew_dry_run("==> Would install 1 dependency for jq:\noniguruma\n").unwrap()[0].1,
        ""
    );
    assert!(parse_brew_dry_run("==> Would install 1 formula:\nbad;rm 1\n").is_err());
}
#[test]
fn install_verifies_versions_and_removal_respects_dependents() {
    let mock = brew(&[]);
    let preview = plan(&mock, &[id("jq")], &[]).unwrap();
    assert_eq!(preview.operations.len(), 2);
    let record = apply_recorded(
        &mock,
        &preview,
        &[id("jq")],
        &[],
        true,
        &AtomicBool::new(false),
        None,
    )
    .unwrap();
    assert!(record.success, "{:?}", record.errors);
    assert_eq!(
        mock.executed.lock().unwrap()[0].args,
        ["install", "--formula", "jq"]
    );
    assert!(plan(&mock, &[id("jq")], &[]).unwrap().operations.is_empty());
    assert!(
        matches!(plan(&mock, &[], &[id("oniguruma")]), Err(Error::Safety(m)) if m.contains("required by"))
    );
    let removal = plan(&mock, &[], &[id("jq")]).unwrap();
    let record = apply_recorded(
        &mock,
        &removal,
        &[],
        &[id("jq")],
        true,
        &AtomicBool::new(false),
        None,
    )
    .unwrap();
    assert!(record.success, "{:?}", record.errors);
    assert!(mock.installed.lock().unwrap().contains_key("oniguruma"));
    let cask = parse_name(ProviderId::Brew, "firefox:cask").unwrap();
    assert!(matches!(
        plan(&mock, &[cask], &[]),
        Err(Error::Unsupported(_))
    ));
}
