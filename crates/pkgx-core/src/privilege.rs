//! Explicit delegation of one reviewed APT transaction; never elevate inspection.
use crate::{
    command::{CommandOutput, CommandRunner, CommandSpec, ProcessRunner},
    Error, Plan,
};
use std::{
    io::IsTerminal,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};

pub const SUDO: &str = "/usr/bin/sudo";
pub const APT: &str = "/usr/bin/apt-get";
pub trait SudoAuthenticator {
    fn authenticate(&self, canceled: &AtomicBool) -> Result<(), Error>;
}
/// Called only after operation confirmation. A no-op never requests credentials.
pub fn authorize_apt(
    authenticator: &dyn SudoAuthenticator,
    requested: bool,
    plan: &Plan,
    confirmed: bool,
    canceled: &AtomicBool,
) -> Result<bool, Error> {
    if canceled.load(Ordering::Relaxed) {
        return Err(Error::Canceled("APT authorization".into()));
    }
    if !confirmed {
        return Err(Error::Safety(
            "authorization requires operation confirmation".into(),
        ));
    }
    if !requested || plan.operations.is_empty() {
        return Ok(false);
    }
    authenticator.authenticate(canceled)?;
    if canceled.load(Ordering::Relaxed) {
        return Err(Error::Canceled("APT authorization".into()));
    }
    Ok(true)
}
/// sudo is deliberately noninteractive here: native revalidation occurs after
/// authentication, and credential expiry must fail rather than ask midway through.
pub struct SudoAptRunner<'a> {
    runner: &'a dyn CommandRunner,
}
impl<'a> SudoAptRunner<'a> {
    pub fn new(runner: &'a dyn CommandRunner) -> Self {
        Self { runner }
    }
}
impl CommandRunner for SudoAptRunner<'_> {
    fn run(
        &self,
        spec: &CommandSpec,
        timeout: Duration,
        canceled: &AtomicBool,
    ) -> Result<CommandOutput, Error> {
        if !spec.requires_privilege {
            return self.runner.run(spec, timeout, canceled);
        }
        if spec.program != "apt-get"
            || !spec.env.is_empty()
            || !spec.args.iter().any(|a| a == "--yes")
            || !spec.args.iter().any(|a| a == "install")
            || !spec.args.iter().any(|a| a == "--")
            || spec
                .args
                .iter()
                .any(|a| a == "--simulate" || a == "--version")
        {
            return Err(Error::Safety("sudo delegation accepts only an executor-marked APT transaction with no environment overrides".into()));
        }
        let mut elevated = CommandSpec::new(SUDO, &["-n", "-u", "root", "--", APT]);
        elevated.args.extend(spec.args.iter().cloned());
        elevated.termination_grace = Duration::from_secs(3);
        self.runner.run(&elevated, timeout, canceled)
    }
}

pub struct ProcessSudoAuthenticator;
impl SudoAuthenticator for ProcessSudoAuthenticator {
    fn authenticate(&self, canceled: &AtomicBool) -> Result<(), Error> {
        verify_environment()?;
        if canceled.load(Ordering::Relaxed) {
            return Err(Error::Canceled("sudo authorization".into()));
        }
        if std::io::stdin().is_terminal() && std::io::stderr().is_terminal() {
            // Keep the foreground process group: sudo reads /dev/tty and a separate
            // background group could receive SIGTTIN. Password entry belongs only
            // to sudo; pkgx never reads or logs it. stdout remains machine-readable.
            let child = Command::new(SUDO)
                .args(["-v", "-u", "root"])
                .env("LC_ALL", "C")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .spawn()
                .map_err(|source| Error::Io {
                    program: "sudo authorization".into(),
                    source,
                })?;
            wait_authentication(child, canceled, Duration::from_secs(120))
        } else {
            let output = ProcessRunner.run(
                &CommandSpec::new(SUDO, &["-n", "-v", "-u", "root"]),
                Duration::from_secs(30),
                canceled,
            )?;
            authorization_status(output.status)
        }
    }
}
fn authorization_status(status: Option<i32>) -> Result<(), Error> {
    if status == Some(0) {
        Ok(())
    } else {
        Err(Error::Safety("sudo authorization failed; use an interactive terminal or obtain sudo authorization first; no APT transaction started".into()))
    }
}
/// Require the same system APT executable used by inspection; sudo's secure_path
/// must not silently select another binary or a different native database.
pub fn validate_environment(
    resolved_apt: &Path,
    expected_apt: &Path,
    overrides: &[&str],
) -> Result<(), Error> {
    if resolved_apt != expected_apt {
        return Err(Error::Safety(
            "--sudo requires the system /usr/bin/apt-get selected on PATH".into(),
        ));
    }
    if !overrides.is_empty() {
        return Err(Error::Safety("--sudo refuses APT_CONFIG, DPKG_ROOT and DPKG_ADMINDIR overrides; inspection and privileged execution must use the same native database/configuration".into()));
    }
    Ok(())
}
fn verify_environment() -> Result<(), Error> {
    if std::env::consts::OS != "linux" {
        return Err(Error::Unsupported(
            "sudo APT execution requires Linux".into(),
        ));
    }
    let expected = std::fs::canonicalize(APT).map_err(|source| Error::Io {
        program: "system apt-get".into(),
        source,
    })?;
    let path = std::env::var_os("PATH").ok_or_else(|| {
        Error::Safety("--sudo requires a trusted PATH selecting system apt-get".into())
    })?;
    let selected: PathBuf = std::env::split_paths(&path)
        .map(|dir| dir.join("apt-get"))
        .find(|path| {
            let Ok(metadata) = std::fs::metadata(path) else {
                return false;
            };
            if !metadata.is_file() {
                return false;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                metadata.permissions().mode() & 0o111 != 0
            }
            #[cfg(not(unix))]
            {
                true
            }
        })
        .ok_or_else(|| Error::Safety("apt-get is absent from PATH".into()))?;
    let selected = std::fs::canonicalize(selected).map_err(|source| Error::Io {
        program: "selected apt-get".into(),
        source,
    })?;
    let overrides = ["APT_CONFIG", "DPKG_ROOT", "DPKG_ADMINDIR"]
        .into_iter()
        .filter(|key| std::env::var_os(key).is_some())
        .collect::<Vec<_>>();
    validate_environment(&selected, &expected, &overrides)
}
fn stop_authentication(mut child: Child) -> Result<(), Error> {
    // Do not signal a child already reaped (or one whose status is unknown).
    match child.try_wait() {
        Ok(Some(_)) => return Ok(()),
        Ok(None) => {}
        Err(source) => {
            return Err(Error::Io {
                program: "sudo authorization cleanup; child status unknown".into(),
                source,
            })
        }
    }
    #[cfg(unix)]
    {
        if let Ok(pid) = i32::try_from(child.id()) {
            // SAFETY: the unreaped direct child reserves its PID; scalar arguments.
            unsafe {
                libc::kill(pid, libc::SIGTERM);
            }
        }
    }
    let deadline = Instant::now() + Duration::from_millis(200);
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(_)) => return Ok(()),
            Ok(None) => {}
            Err(source) => {
                return Err(Error::Io {
                    program: "sudo authorization cleanup; child status unknown".into(),
                    source,
                })
            }
        }
        thread::sleep(Duration::from_millis(10));
    }
    let _ = child.kill();
    crate::command::reap_bounded(child).map_err(|_| Error::Safety("sudo authorization could not be stopped; its process may still be running; no APT transaction started".into()))
}

fn wait_authentication(
    mut child: Child,
    canceled: &AtomicBool,
    timeout: Duration,
) -> Result<(), Error> {
    let start = Instant::now();
    loop {
        if canceled.load(Ordering::Relaxed) {
            stop_authentication(child)?;
            return Err(Error::Canceled(
                "sudo authorization; no APT transaction started".into(),
            ));
        }
        if start.elapsed() >= timeout {
            stop_authentication(child)?;
            return Err(Error::Timeout(
                "sudo authorization; no APT transaction started".into(),
            ));
        }
        match child.try_wait() {
            Ok(Some(status)) => return authorization_status(status.code()),
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(source) => {
                stop_authentication(child)?;
                return Err(Error::Io {
                    program: "sudo authorization".into(),
                    source,
                });
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn authentication_wait_reaps_harmless_child_on_timeout_and_cancel() {
        for canceled in [false, true] {
            let child = Command::new("/bin/sleep")
                .arg("5")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let result =
                wait_authentication(child, &AtomicBool::new(canceled), Duration::from_millis(20));
            assert!(if canceled {
                matches!(result, Err(Error::Canceled(_)))
            } else {
                matches!(result, Err(Error::Timeout(_)))
            });
        }
    }
}
