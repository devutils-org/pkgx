use pkgx_core::{
    apt,
    command::{CommandOutput, CommandRunner, CommandSpec, ProcessRunner},
    detection, Error, InstallReason,
};
use std::{
    sync::{atomic::AtomicBool, Mutex},
    time::Duration,
};
struct Mock {
    calls: Mutex<Vec<String>>,
    fail_marks: bool,
}
impl CommandRunner for Mock {
    fn run(&self, spec: &CommandSpec, _: Duration, _: &AtomicBool) -> Result<CommandOutput, Error> {
        self.calls.lock().unwrap().push(spec.program.clone());
        let stdout = match spec.program.as_str() {
            "dpkg-query" if spec.args[0] == "-W" => {
                include_str!("../../../tests/fixtures/dpkg.tsv")
            }
            "apt-mark" if !self.fail_marks => {
                if spec.args[0] == "showmanual" {
                    "curl\n"
                } else {
                    "zlib1g:amd64\nzlib1g:i386\n"
                }
            }
            _ => return Err(Error::Unavailable(spec.program.clone())),
        };
        Ok(CommandOutput {
            status: Some(0),
            stdout: stdout.as_bytes().to_vec(),
            stderr: vec![],
        })
    }
}
#[test]
fn inventory_preserves_architectures_and_native_marks() {
    let mock = Mock {
        calls: Mutex::new(vec![]),
        fail_marks: false,
    };
    let packages = apt::inventory(&mock).unwrap();
    assert_eq!(packages.len(), 3);
    assert_eq!(packages[0].id.name, "curl");
    assert_eq!(packages[0].install_reason, InstallReason::Explicit);
    assert_eq!(packages[1].install_reason, InstallReason::Dependency);
    assert_ne!(packages[1].id, packages[2].id);
    assert_eq!(
        *mock.calls.lock().unwrap(),
        ["dpkg-query", "apt-mark", "apt-mark"]
    );
}
#[test]
fn missing_marks_do_not_imply_manual_install() {
    let mock = Mock {
        calls: Mutex::new(vec![]),
        fail_marks: true,
    };
    assert!(apt::inventory(&mock)
        .unwrap()
        .iter()
        .all(|p| p.install_reason == InstallReason::Unknown));
}
#[test]
fn malformed_inventory_is_rejected() {
    assert!(apt::parse_inventory("curl\tamd64\n").is_err());
    assert!(apt::parse_inventory("curl\tamd64\t\tinstalled\n").is_err());
    assert!(apt::parse_inventory("").unwrap().is_empty());
}
#[test]
fn detection_works_without_package_managers() {
    let mock = Mock {
        calls: Mutex::new(vec![]),
        fail_marks: true,
    };
    assert!(detection::detect(&mock, "linux")
        .iter()
        .all(|p| !p.available && p.capabilities.is_empty()));
    assert!(detection::detect(&mock, "windows")
        .iter()
        .all(|p| !p.available));
}
#[test]
fn canceled_commands_never_spawn() {
    let result = ProcessRunner.run(
        &CommandSpec::new("nonexistent-pkgx-command", &[]),
        Duration::from_secs(1),
        &AtomicBool::new(true),
    );
    assert!(matches!(result, Err(Error::Canceled(_))));
}
#[cfg(unix)]
#[test]
fn subprocess_timeout_is_bounded() {
    let result = ProcessRunner.run(
        &CommandSpec::new("/bin/sleep", &["2"]),
        Duration::from_millis(20),
        &AtomicBool::new(false),
    );
    assert!(matches!(result, Err(Error::Timeout(_))));
}

#[test]
fn uninstalled_entries_can_have_empty_metadata() {
    assert!(apt::parse_inventory("removed\t\t\tnot-installed\n")
        .unwrap()
        .is_empty());
}
#[test]
fn installed_identities_must_be_valid_and_unique() {
    assert!(apt::parse_inventory("bad*\tamd64\t1\tinstalled\n").is_err());
    assert!(
        apt::parse_inventory("curl\tamd64\t1\tinstalled\ncurl\tamd64\t1\tinstalled\n").is_err()
    );
}
#[cfg(unix)]
#[test]
fn process_runner_preserves_arguments_without_a_shell() {
    let output = ProcessRunner
        .run(
            &CommandSpec::new("/bin/echo", &["$(id); *", "two words"]),
            Duration::from_secs(1),
            &AtomicBool::new(false),
        )
        .unwrap();
    assert_eq!(output.stdout, b"$(id); * two words\n");
}
#[cfg(unix)]
#[test]
fn excessive_output_is_rejected_and_process_is_stopped() {
    let result = ProcessRunner.run(
        &CommandSpec::new("/usr/bin/yes", &[]),
        Duration::from_secs(2),
        &AtomicBool::new(false),
    );
    assert!(matches!(result, Err(Error::Io { .. })));
}

#[test]
fn ambiguous_native_marks_do_not_invent_multiarch_provenance() {
    struct AmbiguousMarks;
    impl CommandRunner for AmbiguousMarks {
        fn run(
            &self,
            spec: &CommandSpec,
            _: Duration,
            _: &AtomicBool,
        ) -> Result<CommandOutput, Error> {
            let data = if spec.program == "dpkg-query" {
                include_str!("../../../tests/fixtures/dpkg.tsv")
            } else if spec.args[0] == "showmanual" {
                "zlib1g\n"
            } else {
                ""
            };
            Ok(CommandOutput {
                status: Some(0),
                stdout: data.as_bytes().to_vec(),
                stderr: vec![],
            })
        }
    }
    assert!(apt::inventory(&AmbiguousMarks)
        .unwrap()
        .iter()
        .filter(|p| p.id.name == "zlib1g")
        .all(|p| p.install_reason == InstallReason::Unknown));
}

#[cfg(unix)]
#[test]
fn cancellation_during_execution_stops_the_child() {
    use std::sync::Arc;
    let canceled = Arc::new(AtomicBool::new(false));
    let flag = canceled.clone();
    let trigger = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(30));
        flag.store(true, std::sync::atomic::Ordering::Relaxed);
    });
    let start = std::time::Instant::now();
    let result = ProcessRunner.run(
        &CommandSpec::new("/bin/sleep", &["3"]),
        Duration::from_secs(2),
        &canceled,
    );
    trigger.join().unwrap();
    assert!(matches!(result, Err(Error::Canceled(_))));
    assert!(start.elapsed() < Duration::from_secs(2));
}

#[test]
fn residual_configuration_records_are_scoped_and_never_inferred_from_files() {
    let ids = apt::parse_residual_configurations(
        "curl\tamd64\tinstalled\nold-tool\tamd64\tconfig-files\nabsent\t\tnot-installed\n",
    )
    .unwrap();
    assert_eq!(ids.len(), 1);
    assert_eq!(ids[0].to_string(), "apt:old-tool:amd64");
    assert!(apt::parse_residual_configurations("old-tool\t\tconfig-files").is_err());
    assert!(apt::parse_residual_configurations("broken record").is_err());
}

#[cfg(unix)]
#[test]
#[ignore = "subprocess fixture invoked by the bounded pipe-lifetime test"]
#[allow(clippy::zombie_processes)]
fn pipe_holder_fixture() {
    if std::env::var("PKGX_PIPE_FIXTURE").as_deref() == Ok("1") {
        // Intentionally outlive this helper process. ProcessRunner must terminate
        // this same process group when the inherited output pipe stays open.
        let _child = std::process::Command::new("/bin/sleep")
            .arg("3")
            .spawn()
            .unwrap();
    }
}
#[cfg(unix)]
#[test]
fn inherited_pipe_lifetime_cannot_escape_the_command_timeout() {
    let executable = std::env::current_exe().unwrap();
    let mut command = CommandSpec::new(
        executable.to_str().unwrap(),
        &["--ignored", "--exact", "pipe_holder_fixture"],
    );
    command.env.insert("PKGX_PIPE_FIXTURE".into(), "1".into());
    let start = std::time::Instant::now();
    let result = ProcessRunner.run(
        &command,
        Duration::from_millis(200),
        &AtomicBool::new(false),
    );
    assert!(matches!(result, Err(Error::Timeout(_))));
    assert!(start.elapsed() < Duration::from_secs(2));
}
