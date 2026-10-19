//! Script lifecycle against a simulated interpreter. No script is ever executed:
//! the mock interprets install/uninstall arguments by editing a temporary home.
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, Mutex},
    time::Duration,
};
use xpac_core::{
    command::{CommandOutput, CommandRunner, CommandSpec},
    recipe::Recipe,
    script::{self, ReceiptStatus, ReceiptStore},
    Error,
};

const ARTIFACT: &[u8] = b"abc";
const DIGEST: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

#[derive(Default)]
struct Sim {
    home: PathBuf,
    tampered_download: bool,
    /// Do not create this file on install / do not remove it on uninstall.
    skip: Option<&'static str>,
    exit: i32,
    undeclared: bool,
    interpreted: Mutex<Vec<CommandSpec>>,
}
impl CommandRunner for Sim {
    fn run(&self, spec: &CommandSpec, _: Duration, _: &AtomicBool) -> Result<CommandOutput, Error> {
        let stdout = match spec.program.as_str() {
            "curl" => ARTIFACT.to_vec(),
            "sha256sum" => format!(
                "{}  artifact\n",
                if self.tampered_download {
                    "1".repeat(64)
                } else {
                    DIGEST.into()
                }
            )
            .into_bytes(),
            "/bin/sh" => {
                // The stored artifact is passed by path; it is never a shell string.
                assert_eq!(fs::read(&spec.args[0]).unwrap(), ARTIFACT);
                assert_eq!(spec.env["XPAC_RECIPE_NAME"], "demo-tool");
                assert!(!spec.requires_privilege);
                self.interpreted.lock().unwrap().push(spec.clone());
                let bin = self.home.join(".local/bin");
                let config = self.home.join(".config/demo-tool.toml");
                match spec.args[1].as_str() {
                    "install" => {
                        fs::create_dir_all(&bin).unwrap();
                        if self.skip != Some("tool") {
                            fs::write(bin.join("demo-tool"), b"#!/bin/true\n").unwrap();
                        }
                        #[cfg(unix)]
                        std::os::unix::fs::symlink("demo-tool", bin.join("dt")).unwrap();
                        fs::create_dir_all(config.parent().unwrap()).unwrap();
                        fs::write(&config, b"x = 1\n").unwrap();
                        if self.undeclared {
                            fs::create_dir_all(self.home.join(".cache/demo")).unwrap();
                            fs::write(self.home.join(".cache/demo/index"), b"x").unwrap();
                        }
                    }
                    "uninstall" => {
                        if self.skip != Some("tool") {
                            let _ = fs::remove_file(bin.join("demo-tool"));
                        }
                        let _ = fs::remove_file(bin.join("dt"));
                    }
                    other => panic!("unexpected action {other}"),
                }
                vec![]
            }
            other => panic!("unexpected program {other}"),
        };
        let interpreter = spec.program.starts_with('/');
        Ok(CommandOutput {
            status: Some(if interpreter { self.exit } else { 0 }),
            stdout,
            stderr: vec![],
        })
    }
}
fn recipe() -> Recipe {
    Recipe::parse(&format!(
        r#"version = 1
name = "demo-tool"
pinned_version = "1.2.0"
url = "https://example.org/install.sh"
sha256 = "{DIGEST}"
interpreter = "sh"
install = {{ args = ["install"] }}
uninstall = {{ args = ["uninstall"] }}
resources = [
  {{ kind = "file", path = "~/.local/bin/demo-tool" }},
  {{ kind = "link", path = "~/.local/bin/dt" }},
  {{ kind = "configuration", path = "~/.config/demo-tool.toml" }},
]
"#
    ))
    .unwrap()
}
struct Env {
    _temp: tempfile::TempDir,
    home: PathBuf,
    store: ReceiptStore,
}
fn env() -> Env {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    fs::create_dir(&home).unwrap();
    let store = ReceiptStore::new(temp.path().join("state/scripts"));
    Env {
        home,
        store,
        _temp: temp,
    }
}
fn sim(home: &Path) -> Sim {
    Sim {
        home: home.into(),
        ..Default::default()
    }
}
fn install(sim: &Sim, env: &Env, confirmed: bool) -> Result<script::ScriptRecord, Error> {
    script::install(
        sim,
        &recipe(),
        &env.store,
        &env.home,
        "linux",
        confirmed,
        &AtomicBool::new(false),
    )
}

#[cfg(unix)]
#[test]
fn install_records_created_resources_and_uninstall_verifies_their_removal() {
    let env = env();
    let sim = sim(&env.home);
    let record = install(&sim, &env, true).unwrap();
    assert!(record.success, "{:?}", record.errors);
    let listed = env.store.list().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].classification, "tracked");
    assert!(listed[0].receipt.resources.iter().all(|r| r.owned()));
    let record =
        script::uninstall(&sim, &env.store, "demo-tool", true, &AtomicBool::new(false)).unwrap();
    assert!(record.success, "{:?}", record.errors);
    // Configuration is retained for review, never deleted by xpac.
    assert_eq!(
        record.retained,
        vec![env.home.join(".config/demo-tool.toml")]
    );
    assert!(env.home.join(".config/demo-tool.toml").exists());
    assert!(env.store.list().unwrap().is_empty());
    assert_eq!(sim.interpreted.lock().unwrap().len(), 2);
}
#[test]
fn existing_outside_or_symlinked_paths_and_missing_uninstall_are_refused_before_download() {
    let env = env();
    let sim = sim(&env.home);
    fs::create_dir_all(env.home.join(".local/bin")).unwrap();
    fs::write(env.home.join(".local/bin/demo-tool"), b"user file").unwrap();
    assert!(
        matches!(install(&sim, &env, true), Err(Error::Safety(m)) if m.contains("already exists"))
    );
    assert_eq!(
        fs::read(env.home.join(".local/bin/demo-tool")).unwrap(),
        b"user file"
    );

    let outside = Recipe::parse(&toml_with_resource(
        "{ kind = \"file\", path = \"/usr/local/bin/demo-tool\" }",
    ))
    .unwrap();
    assert!(script::preview_install(&outside, &env.store, &env.home).is_err());
    let no_uninstall = Recipe::parse(
        &toml_with_resource("{ kind = \"file\", path = \"~/bin/x\" }")
            .replace("uninstall = { args = [\"uninstall\"] }\n", ""),
    )
    .unwrap();
    assert!(matches!(
        script::preview_install(&no_uninstall, &env.store, &env.home),
        Err(Error::Safety(m)) if m.contains("uninstall")
    ));
    #[cfg(unix)]
    {
        let elsewhere = env.home.parent().unwrap().join("elsewhere");
        fs::create_dir(&elsewhere).unwrap();
        std::os::unix::fs::symlink(&elsewhere, env.home.join("linked")).unwrap();
        let linked = Recipe::parse(&toml_with_resource(
            "{ kind = \"file\", path = \"~/linked/x\" }",
        ))
        .unwrap();
        assert!(script::preview_install(&linked, &env.store, &env.home).is_err());
    }
    assert!(sim.interpreted.lock().unwrap().is_empty());
}
fn toml_with_resource(resource: &str) -> String {
    format!(
        r#"version = 1
name = "demo-tool"
pinned_version = "1.2.0"
url = "https://example.org/install.sh"
sha256 = "{DIGEST}"
interpreter = "sh"
install = {{ args = ["install"] }}
uninstall = {{ args = ["uninstall"] }}
resources = [{resource}]
"#
    )
}
#[test]
fn tampered_or_unconfirmed_installs_never_execute_or_leave_receipts() {
    let env = env();
    let mut sim = sim(&env.home);
    assert!(matches!(install(&sim, &env, false), Err(Error::Safety(_))));
    sim.tampered_download = true;
    assert!(matches!(install(&sim, &env, true), Err(Error::Safety(m)) if m.contains("mismatch")));
    assert!(sim.interpreted.lock().unwrap().is_empty());
    assert!(env.store.list().unwrap().is_empty());
}
#[cfg(unix)]
#[test]
fn partial_installs_are_incomplete_and_never_claim_missing_resources() {
    let env = env();
    let mut sim = sim(&env.home);
    sim.skip = Some("tool");
    let record = install(&sim, &env, true).unwrap();
    assert!(!record.success);
    assert_eq!(record.status, Some(ReceiptStatus::InstallIncomplete));
    let receipt = env.store.read("demo-tool").unwrap();
    assert!(!receipt.resources[0].owned());
    assert_eq!(
        env.store.list().unwrap()[0].classification,
        "tracked_incomplete"
    );
    // A second install is refused while a receipt exists.
    assert!(install(&sim, &env, true).is_err());
}
#[cfg(unix)]
#[test]
fn modified_resources_or_artifacts_block_uninstall_before_execution() {
    let env = env();
    let sim = sim(&env.home);
    install(&sim, &env, true).unwrap();
    let tool = env.home.join(".local/bin/demo-tool");
    fs::write(&tool, b"edited by the user").unwrap();
    assert!(matches!(
        script::uninstall(&sim, &env.store, "demo-tool", true, &AtomicBool::new(false)),
        Err(Error::Safety(m)) if m.contains("changed since installation")
    ));
    fs::write(&tool, b"#!/bin/true\n").unwrap();
    // Configuration edits are expected and do not block uninstall.
    fs::write(env.home.join(".config/demo-tool.toml"), b"x = 2\n").unwrap();
    let artifacts = env.store_dir().join("artifacts").join(DIGEST);
    fs::write(&artifacts, b"evil").unwrap();
    assert!(matches!(
        script::uninstall(&sim, &env.store, "demo-tool", true, &AtomicBool::new(false)),
        Err(Error::Safety(m)) if m.contains("artifact")
    ));
    assert!(matches!(
        script::uninstall(
            &sim,
            &env.store,
            "demo-tool",
            false,
            &AtomicBool::new(false)
        ),
        Err(Error::Safety(_))
    ));
    assert_eq!(sim.interpreted.lock().unwrap().len(), 1);
    assert!(tool.exists());
}
impl Env {
    fn store_dir(&self) -> PathBuf {
        self.home.parent().unwrap().join("state/scripts")
    }
}
#[cfg(unix)]
#[test]
fn residual_owned_files_keep_an_incomplete_receipt() {
    let env = env();
    let mut sim = sim(&env.home);
    install(&sim, &env, true).unwrap();
    sim.skip = Some("tool");
    let record =
        script::uninstall(&sim, &env.store, "demo-tool", true, &AtomicBool::new(false)).unwrap();
    assert!(!record.success);
    assert_eq!(
        env.store.read("demo-tool").unwrap().status,
        ReceiptStatus::UninstallIncomplete
    );
    assert!(env.home.join(".local/bin/demo-tool").exists());
}
#[test]
fn interrupted_receipts_require_manual_review_and_forget_touches_no_files() {
    let env = env();
    let mut sim = sim(&env.home);
    sim.exit = 1;
    let _ = install(&sim, &env, true);
    let mut receipt = env.store.read("demo-tool").unwrap();
    assert_eq!(receipt.status, ReceiptStatus::InstallIncomplete);
    receipt.status = ReceiptStatus::InstallStarted;
    fs::write(
        env.store_dir().join("receipts/demo-tool.json"),
        serde_json::to_vec(&receipt).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        script::preview_uninstall(&env.store, "demo-tool"),
        Err(Error::Safety(m)) if m.contains("interrupted")
    ));
    let before: Vec<_> = walk(&env.home);
    env.store.forget("demo-tool").unwrap();
    assert_eq!(walk(&env.home), before);
    assert!(env.store.list().unwrap().is_empty());
    assert!(!env.store_dir().join("artifacts").join(DIGEST).exists());
}
fn walk(path: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            out.push(entry.path());
            out.extend(walk(&entry.path()));
        }
    }
    out.sort();
    out
}
#[cfg(unix)]
#[test]
fn experimental_observation_reports_undeclared_effects_without_owning_them() {
    let env = env();
    let mut sim = sim(&env.home);
    sim.undeclared = true;
    fs::create_dir_all(env.home.join("Documents")).unwrap();
    fs::write(env.home.join("Documents/unrelated"), b"user").unwrap();
    let record = script::install_observed(
        &sim,
        &recipe(),
        &env.store,
        &env.home,
        "linux",
        true,
        &AtomicBool::new(false),
        true,
    )
    .unwrap();
    assert!(record.success, "{:?}", record.errors);
    assert_eq!(
        record.observed,
        vec![
            env.home.join(".cache"),
            env.home.join(".cache/demo"),
            env.home.join(".cache/demo/index")
        ]
    );
    assert_eq!(env.store.list().unwrap()[0].classification, "observed");
    let record =
        script::uninstall(&sim, &env.store, "demo-tool", true, &AtomicBool::new(false)).unwrap();
    assert!(record.success, "{:?}", record.errors);
    // Observed paths are reported, never removed.
    assert!(env.home.join(".cache/demo/index").exists());
    assert!(record
        .observed
        .contains(&env.home.join(".cache/demo/index")));
    assert!(env.home.join("Documents/unrelated").exists());
}
