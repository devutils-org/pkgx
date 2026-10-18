use pkgx_core::{
    backend::Backend,
    command::{CommandOutput, CommandRunner, CommandSpec},
    identity::parse_name,
    providers::{self, NativeBackend},
    Error, InstallReason, PackageId, ProviderId,
};
use std::{
    sync::{atomic::AtomicBool, Mutex},
    time::Duration,
};
#[test]
fn pacman_preserves_native_reasons_and_expressions() {
    let packages =
        providers::parse_pacman(include_str!("../../../tests/fixtures/pacman-info.txt")).unwrap();
    assert_eq!(packages.len(), 2);
    assert_eq!(packages[0].install_reason, InstallReason::Explicit);
    assert_eq!(packages[1].install_reason, InstallReason::Dependency);
    assert_eq!(packages[0].metadata["Depends On"], "glibc  libcurl.so=4-64");
    assert_eq!(packages[0].id.to_string(), "pacman:curl:x86_64");
    assert!(providers::parse_pacman("Name : curl\nVersion : 1").is_err());
}
#[test]
fn rpm_keeps_epoch_architecture_and_multiple_installonly_versions() {
    let packages =
        providers::parse_rpm("kernel\tx86_64\t0:6.1-1\nkernel\tx86_64\t0:6.2-1\n").unwrap();
    assert_eq!(packages.len(), 2);
    assert_eq!(packages[0].id, packages[1].id);
    assert_eq!(packages[0].version, "0:6.1-1");
    assert_eq!(packages[0].install_reason, InstallReason::Unknown);
    assert!(providers::parse_rpm("curl x86_64 1").is_err());
}
#[test]
fn flatpak_kind_branch_and_scope_do_not_collide() {
    let data = "app/org.example.App/x86_64/stable\t1\tabcdef\tflathub\tuser\napp/org.example.App/x86_64/stable\t1\tabcdef\tflathub\tsystem\nruntime/org.example.App/x86_64/stable\t\tdeadbeef\tflathub\tuser\n";
    let packages = providers::parse_flatpak(data).unwrap();
    assert_eq!(packages.len(), 3);
    assert_ne!(packages[0].id, packages[1].id);
    assert_eq!(packages[2].version, "deadbeef");
    assert!(
        providers::parse_flatpak("app/org.example.App/x86_64/stable\t1\t\tflathub\tuser\n")
            .is_err()
    );
}
#[test]
fn brew_preserves_kegs_and_cask_formula_identity() {
    let packages =
        providers::parse_brew(include_str!("../../../tests/fixtures/brew-info.json")).unwrap();
    assert_eq!(packages.len(), 3);
    assert_eq!(
        packages
            .iter()
            .filter(|p| p.id.architecture_or_scope.as_deref() == Some("formula"))
            .count(),
        2
    );
    assert_eq!(
        packages
            .iter()
            .find(|p| p.version == "8.10.1")
            .unwrap()
            .install_reason,
        InstallReason::Explicit
    );
    assert_eq!(
        packages
            .iter()
            .find(|p| p.version == "131.0")
            .unwrap()
            .id
            .to_string(),
        "brew:homebrew/cask/firefox:cask"
    );
    assert!(providers::parse_brew("{}").is_err());
}
#[test]
fn identities_reject_selectors_and_require_ambiguous_scopes() {
    for text in [
        "curl",
        "unknown:curl",
        "pacman:--help",
        "dnf:curl*",
        "dnf:curl.x86_64=1",
        "brew:curl",
        "brew:../curl:formula",
        "flatpak:org.example.App",
        "flatpak:app/org.example.App/x86_64/stable:all",
        "script:unverified",
    ] {
        assert!(text.parse::<PackageId>().is_err(), "accepted {text}");
    }
    for text in [
        "apt:curl:amd64",
        "pacman:curl:x86_64",
        "dnf:curl:x86_64",
        "brew:curl:formula",
        "brew:homebrew/cask/firefox:cask",
        "flatpak:app/org.example.App/x86_64/stable:user",
    ] {
        assert_eq!(text.parse::<PackageId>().unwrap().to_string(), text);
    }
    assert!(parse_name(ProviderId::Pacman, "curl/stable").is_err());
}
struct Mock {
    calls: Mutex<Vec<CommandSpec>>,
}
impl CommandRunner for Mock {
    fn run(&self, spec: &CommandSpec, _: Duration, _: &AtomicBool) -> Result<CommandOutput, Error> {
        self.calls.lock().unwrap().push(spec.clone());
        let text = match spec.program.as_str() {
            "brew" => include_str!("../../../tests/fixtures/brew-info.json"),
            "pacman" => include_str!("../../../tests/fixtures/pacman-info.txt"),
            "flatpak" if spec.args.contains(&"--app".into()) => {
                "org.example.App/x86_64/stable\t1\tabcdef\tflathub\n"
            }
            "flatpak" => "",
            _ => return Err(Error::Unavailable("mock".into())),
        };
        Ok(CommandOutput {
            status: Some(0),
            stdout: text.as_bytes().to_vec(),
            stderr: vec![],
        })
    }
}
#[test]
fn adapters_query_explicit_scopes_and_disable_brew_auto_update() {
    let mock = Mock {
        calls: Mutex::new(vec![]),
    };
    assert_eq!(
        NativeBackend {
            provider: ProviderId::Brew,
            runner: &mock
        }
        .inventory()
        .unwrap()
        .len(),
        3
    );
    assert_eq!(
        mock.calls.lock().unwrap()[0].env["HOMEBREW_NO_AUTO_UPDATE"],
        "1"
    );
    assert_eq!(
        NativeBackend {
            provider: ProviderId::Flatpak,
            runner: &mock
        }
        .inventory()
        .unwrap()
        .len(),
        2
    );
    assert_eq!(mock.calls.lock().unwrap().len(), 5);
}
#[test]
fn unsupported_solver_never_runs_a_native_command() {
    let mock = Mock {
        calls: Mutex::new(vec![]),
    };
    let backend = NativeBackend {
        provider: ProviderId::Script,
        runner: &mock,
    };
    assert!(matches!(
        backend.plan(&[parse_name(ProviderId::Pacman, "curl").unwrap()], &[]),
        Err(Error::Unsupported(_))
    ));
    // DNF validates identities before any native command.
    let dnf = NativeBackend {
        provider: ProviderId::Dnf,
        runner: &mock,
    };
    assert!(dnf
        .plan(&[parse_name(ProviderId::Pacman, "curl").unwrap()], &[])
        .is_err());
    assert!(mock.calls.lock().unwrap().is_empty());
}

#[test]
fn pacman_print_is_an_uncertain_preview_not_an_executable_plan() {
    struct PrintMock;
    impl CommandRunner for PrintMock {
        fn run(
            &self,
            spec: &CommandSpec,
            _: Duration,
            _: &AtomicBool,
        ) -> Result<CommandOutput, Error> {
            let text = if spec.program == "pacman-conf" {
                ""
            } else if spec.args.contains(&"-Qi".into()) {
                include_str!("../../../tests/fixtures/pacman-info.txt")
            } else {
                assert_eq!(spec.program, "pacman");
                assert!(spec.args.contains(&"--print".into()));
                assert!(spec.args.contains(&"--needed".into()));
                assert!(!spec.args.contains(&"--refresh".into()));
                "curl\t8.10.1-1\tx86_64\tcore\n"
            };
            Ok(CommandOutput {
                status: Some(0),
                stdout: text.as_bytes().to_vec(),
                stderr: vec![],
            })
        }
    }
    let backend = NativeBackend {
        provider: ProviderId::Pacman,
        runner: &PrintMock,
    };
    let plan = backend
        .plan(
            &[parse_name(ProviderId::Pacman, "curl:x86_64").unwrap()],
            &[],
        )
        .unwrap();
    assert!(plan.uncertain);
    assert_eq!(plan.operations.len(), 1);
    assert!(plan
        .warnings
        .iter()
        .any(|s| s.contains("not a complete transaction simulation")));
    assert!(backend
        .plan(
            &[parse_name(ProviderId::Pacman, "curl:aarch64").unwrap()],
            &[]
        )
        .is_err());
}
#[test]
fn rpm_inventory_ignores_imported_openpgp_key_headers() {
    // Found in the disposable Fedora root after the first signed install.
    let rows = "bash\tx86_64\t0:5.3.9-3.fc44\ngpg-pubkey\t(none)\t0:6d9f90a6-6786af3b\n";
    let packages = pkgx_core::providers::parse_rpm(rows).unwrap();
    assert_eq!(packages.len(), 1);
    assert!(pkgx_core::providers::parse_rpm("other\t(none)\t0:1-1\n").is_err());
}
#[test]
fn homebrew_7_receipts_without_dependency_flag_keep_native_reasons() {
    let json = r#"{"formulae":[{"full_name":"jq","installed":[{"version":"1.8.2","installed_on_request":true}]},{"full_name":"oniguruma","installed":[{"version":"6.9.10","installed_on_request":false}]},{"full_name":"x","installed":[{"version":"1","installed_on_request":true,"installed_as_dependency":true}]}],"casks":[]}"#;
    let packages = pkgx_core::providers::parse_brew(json).unwrap();
    let reasons: Vec<_> = packages.iter().map(|p| p.install_reason).collect();
    assert_eq!(
        reasons,
        [
            pkgx_core::InstallReason::Explicit,
            pkgx_core::InstallReason::Dependency,
            pkgx_core::InstallReason::Unknown
        ]
    );
}
