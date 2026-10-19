use std::{
    collections::VecDeque,
    sync::{atomic::AtomicBool, Mutex},
    time::Duration,
};
use xpac_core::{
    apt::{self, AptBackend},
    backend::Backend,
    command::{CommandOutput, CommandRunner, CommandSpec},
    providers::NativeBackend,
    Error, ProviderId,
};
struct Mock(Mutex<VecDeque<(&'static str, &'static str)>>);
impl Mock {
    fn new(steps: Vec<(&'static str, &'static str)>) -> Self {
        Self(Mutex::new(steps.into()))
    }
    fn done(&self) {
        assert!(self.0.lock().unwrap().is_empty());
    }
}
impl CommandRunner for Mock {
    fn run(&self, spec: &CommandSpec, _: Duration, _: &AtomicBool) -> Result<CommandOutput, Error> {
        let (program, raw) = self
            .0
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected native command");
        assert_eq!(spec.program, program);
        if spec.program == "dpkg-query" {
            assert_eq!(spec.args[0], "--show");
            assert!(spec.args[1].contains("${Depends}"));
            assert_eq!(&spec.args[2..], &["--", "curl:amd64"]);
        }
        if spec.program == "rpm" && spec.args.contains(&"--requires".into()) {
            assert_eq!(spec.args, ["-q", "--requires", "--", "curl.x86_64"]);
        }
        Ok(CommandOutput {
            status: Some(0),
            stdout: raw.as_bytes().to_vec(),
            stderr: vec![],
        })
    }
}
const APT: &str = "curl\tamd64\t8.0-1\tinstalled\tlibc6 (>= 2.34)\tlibcurl4 (= 8.0-1) | libcurl4t64\tca-certificates\t\t\tvirtual-curl\t\t\t\n";
#[test]
fn apt_queries_installed_version_and_preserves_alternatives_and_relations() {
    let runner = Mock::new(vec![("dpkg-query", APT)]);
    let id = "apt:curl:amd64".parse().unwrap();
    let report = AptBackend { runner: &runner }.dependencies(&id).unwrap();
    assert_eq!(report.package, id);
    assert_eq!(report.installed_version, "8.0-1");
    assert!(!report.resolved_graph);
    assert_eq!(report.expressions.len(), 4);
    assert_eq!(report.expressions[1].relation, "Depends");
    assert_eq!(
        report.expressions[1].expression,
        "libcurl4 (= 8.0-1) | libcurl4t64"
    );
    runner.done();
}
#[test]
fn apt_refuses_missing_ambiguous_wrong_or_malformed_records() {
    let id = "apt:curl".parse().unwrap();
    for raw in [
        String::new(),
        format!("{APT}{}", APT.replace("amd64", "arm64")),
        APT.replace("curl\t", "other\t"),
        APT.replace("installed", "config-files"),
        APT.replace("8.0-1\tinstalled", "\tinstalled"),
        "wrong columns".into(),
    ] {
        assert!(apt::parse_installed_dependencies(&raw, &id).is_err());
    }
    assert!(apt::parse_installed_dependencies(APT, &"apt:curl:arm64".parse().unwrap()).is_err());
}
#[test]
fn pacman_preserves_capabilities_and_optional_dependency_descriptions() {
    let runner = Mock::new(vec![(
        "pacman",
        include_str!("../../../tests/fixtures/pacman-info.txt"),
    )]);
    let report = NativeBackend {
        provider: ProviderId::Pacman,
        runner: &runner,
    }
    .dependencies(&"pacman:curl".parse().unwrap())
    .unwrap();
    assert_eq!(report.package.to_string(), "pacman:curl:x86_64");
    assert_eq!(report.expressions[0].expression, "glibc  libcurl.so=4-64");
    assert!(!report.resolved_graph);
    runner.done();
}
#[test]
fn rpm_preserves_rich_file_and_versioned_requirements() {
    let runner = Mock::new(vec![
        ("rpm", "curl\tx86_64\t0:8.0-1\n"),
        ("rpm", "libc.so.6()(64bit)\n/bin/sh\n(foo >= 2 if bar)\n"),
    ]);
    let report = NativeBackend {
        provider: ProviderId::Dnf,
        runner: &runner,
    }
    .dependencies(&"dnf:curl:x86_64".parse().unwrap())
    .unwrap();
    assert_eq!(report.expressions.len(), 3);
    assert_eq!(report.expressions[2].expression, "(foo >= 2 if bar)");
    runner.done();
}
#[test]
fn rpm_multi_version_ambiguity_fails_before_requirements_query() {
    let runner = Mock::new(vec![(
        "rpm",
        "kernel\tx86_64\t0:6.1-1\nkernel\tx86_64\t0:6.2-1\n",
    )]);
    assert!(NativeBackend {
        provider: ProviderId::Dnf,
        runner: &runner
    }
    .dependencies(&"dnf:kernel:x86_64".parse().unwrap())
    .is_err());
    runner.done();
}
#[test]
fn absent_or_foreign_ids_never_fabricate_a_dependency_report() {
    let runner = Mock::new(vec![]);
    let backend = NativeBackend {
        provider: ProviderId::Pacman,
        runner: &runner,
    };
    assert!(backend.dependencies(&"apt:curl".parse().unwrap()).is_err());
    let backend = NativeBackend {
        provider: ProviderId::Flatpak,
        runner: &runner,
    };
    assert!(matches!(
        backend.dependencies(
            &"flatpak:app/org.example.App/x86_64/stable:user"
                .parse()
                .unwrap()
        ),
        Err(Error::Unsupported(_))
    ));
    runner.done();
}

#[test]
fn brew_reports_formula_expressions_but_refuses_multiple_kegs_and_casks() {
    const BREW: &str = r#"{"formulae":[{"full_name":"curl","dependencies":["openssl@3","zlib"],"installed":[{"version":"8.10.1","installed_on_request":true,"installed_as_dependency":false}]}],"casks":[{"full_token":"homebrew/cask/firefox","installed":"131.0"}]}"#;
    let runner = Mock::new(vec![("brew", BREW)]);
    let backend = NativeBackend {
        provider: ProviderId::Brew,
        runner: &runner,
    };
    let report = backend
        .dependencies(&"brew:curl:formula".parse().unwrap())
        .unwrap();
    assert_eq!(report.expressions.len(), 2);
    assert_eq!(report.expressions[0].expression, "openssl@3");
    assert!(report.limitations.iter().any(|s| s.contains("historical")));
    runner.done();
    let runner = Mock::new(vec![(
        "brew",
        include_str!("../../../tests/fixtures/brew-info.json"),
    )]);
    assert!(NativeBackend {
        provider: ProviderId::Brew,
        runner: &runner
    }
    .dependencies(&"brew:curl:formula".parse().unwrap())
    .is_err());
    runner.done();
    let runner = Mock::new(vec![("brew", BREW)]);
    assert!(matches!(
        NativeBackend {
            provider: ProviderId::Brew,
            runner: &runner
        }
        .dependencies(&"brew:homebrew/cask/firefox:cask".parse().unwrap()),
        Err(Error::Unsupported(_))
    ));
    runner.done();
}
