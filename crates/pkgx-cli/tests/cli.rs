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
