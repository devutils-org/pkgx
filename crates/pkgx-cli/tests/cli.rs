use std::process::Command;
#[test]
fn version() {
    let output = Command::new(env!("CARGO_BIN_EXE_pkgx"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "pkgx 0.1.0\n");
}
#[test]
fn doctor_json_without_any_provider() {
    let output = Command::new(env!("CARGO_BIN_EXE_pkgx"))
        .args(["doctor", "--json"])
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema_version"], 1);
    let providers = value["providers"].as_array().unwrap();
    assert_eq!(providers.len(), 5);
    assert!(providers.iter().all(|p| p["available"] == false));
}
#[test]
fn provider_selection_is_required() {
    let output = Command::new(env!("CARGO_BIN_EXE_pkgx"))
        .arg("list")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
}
#[test]
fn inventory_failure_is_structured() {
    let output = Command::new(env!("CARGO_BIN_EXE_pkgx"))
        .args(["list", "--provider", "apt", "--json"])
        .env("PATH", "")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert!(value["error"].is_string());
}

#[test]
fn help_matches_documented_command_surface() {
    let output = Command::new(env!("CARGO_BIN_EXE_pkgx"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        include_str!("../../../tests/fixtures/help.txt")
    );
}
#[test]
fn yes_and_dry_run_are_mutually_exclusive() {
    for command in ["install", "remove"] {
        let output = Command::new(env!("CARGO_BIN_EXE_pkgx"))
            .args([command, "apt:curl", "--yes", "--dry-run"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
    }
}
#[test]
fn plan_requires_exactly_one_input() {
    for args in [
        vec!["plan"],
        vec!["plan", "apt:curl", "-f", "manifest.toml"],
        vec!["plan", "-f", "manifest.toml", "--remove"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_pkgx"))
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
    }
}

#[test]
fn clean_requires_explicit_preview_and_tui_refuses_noninteractive_mode() {
    let output = Command::new(env!("CARGO_BIN_EXE_pkgx"))
        .arg("clean")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let output = Command::new(env!("CARGO_BIN_EXE_pkgx"))
        .arg("tui")
        .env("PATH", "")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("interactive terminal"));
}
#[test]
fn missing_history_and_doctor_never_create_state() {
    let base = std::env::temp_dir().join(format!("pkgx-read-only-{}", std::process::id()));
    assert!(!base.exists());
    for command in ["history", "doctor"] {
        let output = Command::new(env!("CARGO_BIN_EXE_pkgx"))
            .args([command, "--json", "--state-dir"])
            .arg(&base)
            .env("PATH", "")
            .output()
            .unwrap();
        assert!(output.status.success());
    }
    assert!(!base.exists());
}
#[test]
fn providers_work_without_home_or_state_directory() {
    let output = Command::new(env!("CARGO_BIN_EXE_pkgx"))
        .args(["providers", "--json"])
        .env("PATH", "")
        .env_remove("HOME")
        .env_remove("XDG_STATE_HOME")
        .output()
        .unwrap();
    assert!(output.status.success());
}

#[test]
fn saved_plan_grammar_is_unambiguous_and_expiry_defaults_do_not_require_save() {
    let output = Command::new(env!("CARGO_BIN_EXE_pkgx"))
        .args(["plan", "apt:curl"])
        .env("PATH", "")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    for args in [
        vec!["apply"],
        vec!["apply", "-f", "system.toml", "--plan", "preview.json"],
        vec!["plan", "-f", "system.toml", "--save", "preview.json"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_pkgx"))
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
    }
}

#[test]
fn saved_estimate_preview_needs_no_native_binaries_or_journal() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("preview.json");
    let state = temp.path().join("state");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let bundle = pkgx_core::saved_plan::SavedPlan::new(
        pkgx_core::ProviderId::Pacman,
        vec!["pacman:curl:x86_64".parse().unwrap()],
        vec![],
        pkgx_core::Plan {
            operations: vec![],
            observed_state_fingerprint: String::new(),
            observed_packages: vec![],
            uncertain: true,
            warnings: vec![],
        },
        now,
        300,
    )
    .unwrap();
    std::fs::write(&file, bundle.to_json().unwrap()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_pkgx"))
        .args(["apply", "--dry-run", "--json", "--plan"])
        .arg(&file)
        .arg("--state-dir")
        .arg(&state)
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(value["data"]["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|warning| warning.as_str().unwrap().contains("Stored preview only")));
    assert!(!state.exists());
}

#[test]
fn demo_snapshot_is_portable_and_does_not_create_state() {
    let temp = tempfile::tempdir().unwrap();
    let state = temp.path().join("unused-state");
    let output = Command::new(env!("CARGO_BIN_EXE_pkgx"))
        .args(["demo", "--snapshot", "--json", "--state-dir"])
        .arg(&state)
        .env("PATH", "")
        .env_remove("HOME")
        .env_remove("XDG_STATE_HOME")
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output);
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["data"]["demo"], true);
    let packages = value["data"]["packages"].as_array().unwrap();
    assert_eq!(packages.len(), 5);
    for provider in ["apt", "brew", "flatpak"] {
        assert!(packages.iter().any(|p| p["id"]["provider"] == provider));
    }
    assert!(!state.exists());
}

#[test]
fn demo_has_no_execution_switch_and_requires_terminal_or_snapshot() {
    for option in ["--yes", "--allow-apply"] {
        let output = Command::new(env!("CARGO_BIN_EXE_pkgx"))
            .args(["demo", option])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
    }
    let output = Command::new(env!("CARGO_BIN_EXE_pkgx"))
        .args(["demo", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("requires --snapshot"));
    let output = Command::new(env!("CARGO_BIN_EXE_pkgx"))
        .arg("demo")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("interactive"));
}

#[test]
fn dependencies_require_qualified_identity_and_surface_provider_failures() {
    let output = Command::new(env!("CARGO_BIN_EXE_pkgx"))
        .args(["deps", "curl", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let output = Command::new(env!("CARGO_BIN_EXE_pkgx"))
        .args(["deps", "apt:curl", "--json"])
        .env("PATH", "")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert!(error["error"].is_string());
}

#[test]
fn managed_inspection_and_forget_preview_do_not_write_or_query_native_tools() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("state");
    let command = || {
        let mut command = Command::new(env!("CARGO_BIN_EXE_pkgx"));
        command.env("PATH", "").arg("--state-dir").arg(&root);
        command
    };
    let output = command().args(["managed", "--json"]).output().unwrap();
    assert!(output.status.success());
    assert!(!root.exists());
    let registry = pkgx_core::managed::ManagedRegistry::new(root.join("managed"));
    let package = pkgx_core::apt::parse_inventory("curl\tamd64\t8.0-1\tinstalled\n")
        .unwrap()
        .remove(0);
    {
        let mut guard = registry.lock(&registry.read().unwrap()).unwrap();
        guard.grant(&package).unwrap();
    }
    let output = command()
        .args([
            "manage",
            "apt:curl:amd64",
            "--forget",
            "--dry-run",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(registry.read().unwrap().packages.len(), 1);
    let output = command()
        .args(["manage", "apt:curl:amd64", "--forget"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(registry.read().unwrap().packages.len(), 1);
    let output = command()
        .args(["manage", "apt:curl:amd64", "--forget", "--yes", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output);
    assert!(registry.read().unwrap().packages.is_empty());
}

#[test]
fn sudo_is_refused_for_inspection_and_tui_without_execution_opt_in() {
    for command in [
        vec!["--sudo", "providers"],
        vec!["list", "--provider", "apt", "--sudo"],
        vec!["tui", "--sudo"],
        vec!["managed", "--sudo"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_pkgx"))
            .args(command)
            .env("PATH", "")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(String::from_utf8(output.stderr)
            .unwrap()
            .contains("inspection is never elevated"));
    }
}
#[test]
fn no_op_manifest_with_sudo_never_authenticates_or_creates_state() {
    let temp = tempfile::tempdir().unwrap();
    let manifest = temp.path().join("empty.toml");
    std::fs::write(&manifest, "version = 1\n").unwrap();
    let state = temp.path().join("missing-state");
    let output = Command::new(env!("CARGO_BIN_EXE_pkgx"))
        .args(["apply", "--sudo", "--yes", "--json", "--file"])
        .arg(manifest)
        .arg("--state-dir")
        .arg(&state)
        .env("PATH", "")
        .env_remove("HOME")
        .env_remove("XDG_STATE_HOME")
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output);
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["data"]["success"], true);
    assert!(!state.exists());
}
#[test]
fn recipe_install_previews_and_refuses_without_trust_before_any_download() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let recipe = temp.path().join("tool.toml");
    std::fs::write(
        &recipe,
        "version = 1\nname = \"demo-tool\"\npinned_version = \"1.0\"\nurl = \"https://example.org/i.sh\"\nsha256 = \"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\"\ninterpreter = \"sh\"\ninstall = { args = [] }\nuninstall = { args = [\"--uninstall\"] }\nresources = [{ kind = \"file\", path = \"~/.local/bin/demo-tool\" }]\n",
    )
    .unwrap();
    let state = temp.path().join("state");
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_pkgx"))
            .args(args)
            .arg("--state-dir")
            .arg(&state)
            .env("HOME", &home)
            // No curl or interpreter can be found: nothing may be downloaded or run.
            .env("PATH", "")
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap()
    };
    let file = recipe.to_str().unwrap();
    let preview = run(&["recipe", "install", "-f", file, "--dry-run", "--json"]);
    assert!(preview.status.success());
    let value: serde_json::Value = serde_json::from_slice(&preview.stdout).unwrap();
    assert_eq!(
        value["data"]["resolved_resources"][0],
        home.join(".local/bin/demo-tool").to_str().unwrap()
    );
    let refused = run(&["recipe", "install", "-f", file]);
    assert_eq!(refused.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&refused.stderr).contains("--yes"));
    assert!(!state.exists());
    let listed = run(&["recipe", "list", "--json"]);
    assert!(listed.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&listed.stdout).unwrap()["data"],
        serde_json::json!([])
    );
    assert_eq!(
        run(&["recipe", "uninstall", "demo-tool", "--dry-run"])
            .status
            .code(),
        Some(1)
    );
    assert_eq!(run(&["--sudo", "recipe", "list"]).status.code(), Some(1));
}
#[test]
fn declared_files_reconcile_through_review_and_adoption() {
    let dir = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("dotfiles")).unwrap();
    std::fs::write(dir.path().join("dotfiles/tmux.conf"), "set -g mouse on\n").unwrap();
    std::fs::write(dir.path().join("dotfiles/bashrc"), "alias ll='ls -l'\n").unwrap();
    std::fs::write(home.path().join(".bashrc"), "existing\n").unwrap();
    let manifest = dir.path().join("system.toml");
    std::fs::write(
        &manifest,
        "version = 1\n[files.\"~/.tmux.conf\"]\nsource = \"dotfiles/tmux.conf\"\nmode = \"0644\"\n[files.\"~/.bashrc\"]\nsource = \"dotfiles/bashrc\"\n",
    )
    .unwrap();
    let pkgx = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_pkgx"))
            .args(args)
            .arg("--state-dir")
            .arg(state.path())
            .env("HOME", home.path())
            .env("PATH", "")
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap()
    };
    let with_manifest = |args: &[&str]| {
        let mut all = args.to_vec();
        all.extend(["-f", manifest.to_str().unwrap()]);
        pkgx(&all)
    };
    let output = with_manifest(&["diff", "--json"]);
    assert!(output.status.success(), "{output:?}");
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["data"]["files"][0]["target"], "~/.bashrc");
    assert_eq!(value["data"]["files"][0]["state"], "unmanaged_conflict");
    assert_eq!(value["data"]["files"][1]["state"], "missing");
    let text = with_manifest(&["diff"]);
    assert!(String::from_utf8_lossy(&text.stdout).contains("file missing\t~/.tmux.conf"));
    for args in [&["plan"][..], &["apply", "--dry-run"], &["apply", "--yes"]] {
        let refused = with_manifest(args);
        assert_eq!(refused.status.code(), Some(1), "{args:?}");
        assert!(String::from_utf8_lossy(&refused.stderr).contains("~/.bashrc (unmanaged_conflict)"));
    }
    assert!(!home.path().join(".tmux.conf").exists());
    assert!(!state.path().join("files").exists());

    let bashrc = home.path().join(".bashrc");
    let refused = pkgx(&["file", "adopt", bashrc.to_str().unwrap()]);
    assert_eq!(
        refused.status.code(),
        Some(1),
        "noninteractive adoption needs --yes"
    );
    assert!(pkgx(&["file", "adopt", bashrc.to_str().unwrap(), "--yes"])
        .status
        .success());

    let plan = with_manifest(&["plan", "--json"]);
    let value: serde_json::Value = serde_json::from_slice(&plan.stdout).unwrap();
    assert_eq!(
        value["data"]["files"]["writes"].as_array().unwrap().len(),
        2
    );
    assert!(with_manifest(&["apply"]).status.code() == Some(1));
    assert_eq!(std::fs::read_to_string(&bashrc).unwrap(), "existing\n");
    let applied = with_manifest(&["apply", "--yes", "--json"]);
    assert!(applied.status.success(), "{applied:?}");
    assert_eq!(
        std::fs::read_to_string(&bashrc).unwrap(),
        "alias ll='ls -l'\n"
    );
    assert_eq!(
        std::fs::read_to_string(home.path().join(".tmux.conf")).unwrap(),
        "set -g mouse on\n"
    );
    let again = with_manifest(&["apply", "--yes", "--json"]);
    let value: serde_json::Value = serde_json::from_slice(&again.stdout).unwrap();
    assert!(value["data"]["writes"].as_array().unwrap().is_empty());

    std::fs::write(home.path().join(".tmux.conf"), "local edit\n").unwrap();
    let drifted = with_manifest(&["apply", "--yes"]);
    assert!(String::from_utf8_lossy(&drifted.stderr).contains("~/.tmux.conf (drifted)"));
    assert!(pkgx(&["file", "forget", "~/.tmux.conf", "--yes"])
        .status
        .success());
    assert_eq!(
        std::fs::read_to_string(home.path().join(".tmux.conf")).unwrap(),
        "local edit\n"
    );
    let list = pkgx(&["file", "list", "--json"]);
    let value: serde_json::Value = serde_json::from_slice(&list.stdout).unwrap();
    assert_eq!(value["data"]["files"].as_array().unwrap().len(), 1);
    assert_eq!(value["data"]["files"][0]["state"], "intact");

    assert_eq!(
        pkgx(&["file", "remove", "~/.tmux.conf", "--yes"])
            .status
            .code(),
        Some(1)
    );
    assert!(home.path().join(".tmux.conf").exists());
    assert!(pkgx(&["file", "remove", "~/.bashrc", "--dry-run"])
        .status
        .success());
    assert!(bashrc.exists());
    assert!(pkgx(&["file", "remove", "~/.bashrc", "--yes"])
        .status
        .success());
    assert!(!bashrc.exists());
}
