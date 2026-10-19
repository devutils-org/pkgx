use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Mutex,
    },
    time::Duration,
};
use xpac_core::{
    command::{CommandOutput, CommandRunner, CommandSpec},
    privilege::{authorize_apt, validate_environment, SudoAptRunner, SudoAuthenticator, APT, SUDO},
    Error, Operation, OperationKind, Plan, RiskLevel,
};
fn plan() -> Plan {
    Plan {
        operations: vec![Operation {
            kind: OperationKind::Install,
            package: "apt:curl:amd64".parse().unwrap(),
            risk: RiskLevel::Native,
            native_detail: "reviewed".into(),
        }],
        observed_state_fingerprint: String::new(),
        observed_packages: vec![],
        uncertain: false,
        warnings: vec![],
    }
}
struct Auth {
    calls: AtomicUsize,
    fail: bool,
    cancel: bool,
}
impl Auth {
    fn new(fail: bool, cancel: bool) -> Self {
        Self {
            calls: AtomicUsize::new(0),
            fail,
            cancel,
        }
    }
}
impl SudoAuthenticator for Auth {
    fn authenticate(&self, canceled: &AtomicBool) -> Result<(), Error> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        if self.cancel {
            canceled.store(true, Ordering::Relaxed);
        }
        if self.fail {
            Err(Error::Safety("refused credentials".into()))
        } else {
            Ok(())
        }
    }
}
#[test]
fn credentials_need_explicit_opt_in_confirmation_and_nonempty_plan() {
    let auth = Auth::new(false, false);
    let canceled = AtomicBool::new(false);
    assert!(!authorize_apt(&auth, false, &plan(), true, &canceled).unwrap());
    assert!(authorize_apt(&auth, true, &plan(), false, &canceled).is_err());
    let mut empty = plan();
    empty.operations.clear();
    assert!(!authorize_apt(&auth, true, &empty, true, &canceled).unwrap());
    assert_eq!(auth.calls.load(Ordering::Relaxed), 0);
    assert!(authorize_apt(&auth, true, &plan(), true, &canceled).unwrap());
    assert_eq!(auth.calls.load(Ordering::Relaxed), 1);
}
#[test]
fn refused_or_canceled_authentication_never_authorizes_execution() {
    let auth = Auth::new(true, false);
    assert!(authorize_apt(&auth, true, &plan(), true, &AtomicBool::new(false)).is_err());
    let auth = Auth::new(false, true);
    assert!(matches!(
        authorize_apt(&auth, true, &plan(), true, &AtomicBool::new(false)),
        Err(Error::Canceled(_))
    ));
    let auth = Auth::new(false, false);
    assert!(authorize_apt(&auth, true, &plan(), true, &AtomicBool::new(true)).is_err());
    assert_eq!(auth.calls.load(Ordering::Relaxed), 0);
}
#[derive(Default)]
struct Runner {
    calls: Mutex<Vec<CommandSpec>>,
}
impl CommandRunner for Runner {
    fn run(&self, spec: &CommandSpec, _: Duration, _: &AtomicBool) -> Result<CommandOutput, Error> {
        self.calls.lock().unwrap().push(spec.clone());
        Ok(CommandOutput {
            status: Some(0),
            stdout: vec![],
            stderr: vec![],
        })
    }
}
#[test]
fn inspection_and_simulation_are_passed_through_without_sudo() {
    let inner = Runner::default();
    let runner = SudoAptRunner::new(&inner);
    for (program, args) in [
        ("apt-get", vec!["--version"]),
        ("apt-get", vec!["--simulate", "install", "--", "curl"]),
        ("dpkg-query", vec!["-W"]),
        ("apt-mark", vec!["showmanual"]),
        ("brew", vec!["info", "--installed"]),
    ] {
        let mut spec = CommandSpec::new(program, &args);
        spec.env
            .insert("TEST_INSPECTION_ENV".into(), "preserved".into());
        runner
            .run(&spec, Duration::from_secs(30), &AtomicBool::new(false))
            .unwrap();
    }
    let calls = inner.calls.lock().unwrap();
    assert_eq!(calls.len(), 5);
    assert!(calls
        .iter()
        .all(|c| c.program != SUDO && c.env["TEST_INSPECTION_ENV"] == "preserved"));
}
#[test]
fn only_the_marked_native_transaction_is_delegated_with_fixed_paths_and_argv() {
    let inner = Runner::default();
    let runner = SudoAptRunner::new(&inner);
    let mut spec = CommandSpec::new(
        "apt-get",
        &[
            "--yes",
            "-o",
            "APT::Get::Purge=false",
            "install",
            "--",
            "curl:amd64=8.0-1",
            "zlib1g:i386-",
        ],
    );
    spec.requires_privilege = true;
    runner
        .run(&spec, Duration::from_secs(1800), &AtomicBool::new(false))
        .unwrap();
    let calls = inner.calls.lock().unwrap();
    let command = &calls[0];
    assert_eq!(command.program, SUDO);
    assert_eq!(&command.args[..5], &["-n", "-u", "root", "--", APT]);
    assert_eq!(&command.args[5..], spec.args);
    assert!(!command.requires_privilege);
    assert!(command.env.is_empty());
    assert_eq!(command.termination_grace, Duration::from_secs(3));
}
#[test]
fn delegation_refuses_other_programs_inspection_and_environment_injection() {
    let inner = Runner::default();
    let runner = SudoAptRunner::new(&inner);
    for mut spec in [
        CommandSpec::new("sh", &["-c", "anything"]),
        CommandSpec::new("apt-get", &["--simulate", "--yes", "install", "--", "curl"]),
        CommandSpec::new("apt-get", &["install", "--", "curl"]),
    ] {
        spec.requires_privilege = true;
        assert!(runner
            .run(&spec, Duration::from_secs(30), &AtomicBool::new(false))
            .is_err());
    }
    let mut spec = CommandSpec::new("apt-get", &["--yes", "install", "--", "curl"]);
    spec.requires_privilege = true;
    spec.env.insert("APT_CONFIG".into(), "untrusted".into());
    assert!(runner
        .run(&spec, Duration::from_secs(30), &AtomicBool::new(false))
        .is_err());
    assert!(inner.calls.lock().unwrap().is_empty());
}
#[test]
fn system_apt_identity_and_database_overrides_are_checked_before_authentication() {
    assert!(validate_environment(Path::new(APT), Path::new(APT), &[]).is_ok());
    assert!(
        validate_environment(Path::new("/home/user/bin/apt-get"), Path::new(APT), &[]).is_err()
    );
    for key in ["APT_CONFIG", "DPKG_ROOT", "DPKG_ADMINDIR"] {
        assert!(validate_environment(Path::new(APT), Path::new(APT), &[key]).is_err());
    }
}

#[cfg(unix)]
#[test]
fn cooperative_cleanup_of_a_harmless_child_remains_bounded() {
    let mut spec = CommandSpec::new("/bin/sleep", &["5"]);
    spec.termination_grace = Duration::from_millis(30);
    let start = std::time::Instant::now();
    assert!(matches!(
        xpac_core::command::ProcessRunner.run(
            &spec,
            Duration::from_millis(20),
            &AtomicBool::new(false)
        ),
        Err(Error::Timeout(_))
    ));
    assert!(start.elapsed() < Duration::from_secs(2));
}
