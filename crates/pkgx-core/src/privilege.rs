//! Explicit delegation of one reviewed native transaction; never elevate inspection.
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
pub const PACMAN: &str = "/usr/bin/pacman";
pub const DNF: &str = "/usr/bin/dnf5";
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
        if !spec.env.is_empty() {
            return Err(Error::Safety(
                "sudo delegation refuses environment overrides".into(),
            ));
        }
        let program = match spec.program.as_str() {
            "apt-get"
                if spec.args.iter().any(|a| a == "--yes")
                    && spec.args.iter().any(|a| a == "install")
                    && spec.args.iter().any(|a| a == "--")
                    && !spec
                        .args
                        .iter()
                        .any(|a| a == "--simulate" || a == "--version") =>
            {
                APT
            }
            "apt-get" if crate::cache::sudo_template("apt-get", &spec.args) => APT,
            "dnf5" if dnf_transaction(&spec.args) => DNF,
            "pacman"
                if pacman_transaction(&spec.args)
                    || crate::cache::sudo_template("pacman", &spec.args) =>
            {
                PACMAN
            }
            _ => return Err(Error::Safety("sudo delegation accepts only an executor-marked APT, Pacman or DNF transaction or cache cleanup with no environment overrides".into())),
        };
        let mut elevated = CommandSpec::new(SUDO, &["-n", "-u", "root", "--", program]);
        elevated.args.extend(spec.args.iter().cloned());
        elevated.termination_grace = Duration::from_secs(3);
        self.runner.run(&elevated, timeout, canceled)
    }
}

fn dnf_transaction(args: &[String]) -> bool {
    [crate::providers::DNF_INSTALL, crate::providers::DNF_REMOVE]
        .iter()
        .any(|template| {
            let prefix = &template[1..];
            args.len() > prefix.len()
                && args.iter().zip(prefix.iter()).all(|(a, b)| a == b)
                && args[prefix.len()..]
                    .iter()
                    .all(|t| crate::providers::split_nevra(t).is_ok())
        })
}
/// Exactly an executor template followed by non-option targets.
fn pacman_transaction(args: &[String]) -> bool {
    [
        crate::execution::PACMAN_INSTALL,
        crate::execution::PACMAN_REMOVE,
    ]
    .iter()
    .any(|template| {
        let prefix = &template[1..];
        args.len() > prefix.len()
            && args.iter().zip(prefix.iter()).all(|(a, b)| a == b)
            && args[prefix.len()..]
                .iter()
                .all(|target| !target.is_empty() && !target.starts_with('-'))
    })
}
/// Authenticates for the provider whose system binary the transaction uses.
pub struct ProcessSudoAuthenticator {
    pub provider: crate::ProviderId,
}
impl ProcessSudoAuthenticator {
    pub const APT: Self = Self {
        provider: crate::ProviderId::Apt,
    };
}
impl SudoAuthenticator for ProcessSudoAuthenticator {
    fn authenticate(&self, canceled: &AtomicBool) -> Result<(), Error> {
        verify_environment(self.provider)?;
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
        Err(Error::Safety("sudo authorization failed; use an interactive terminal or obtain sudo authorization first; no native transaction started".into()))
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
            "--sudo requires the system package-manager binary selected on PATH".into(),
        ));
    }
    if !overrides.is_empty() {
        return Err(Error::Safety("--sudo refuses APT_CONFIG, DPKG_ROOT and DPKG_ADMINDIR overrides; inspection and privileged execution must use the same native database/configuration".into()));
    }
    Ok(())
}
fn verify_environment(provider: crate::ProviderId) -> Result<(), Error> {
    if std::env::consts::OS != "linux" {
        return Err(Error::Unsupported(
            "sudo native execution requires Linux".into(),
        ));
    }
    let (binary, system, variables): (&str, &str, &[&str]) = match provider {
        crate::ProviderId::Apt => (
            "apt-get",
            APT,
            &["APT_CONFIG", "DPKG_ROOT", "DPKG_ADMINDIR"],
        ),
        crate::ProviderId::Pacman => ("pacman", PACMAN, &[]),
        crate::ProviderId::Dnf => ("dnf5", DNF, &[]),
        _ => {
            return Err(Error::Unsupported(format!(
                "sudo delegation for {provider}"
            )))
        }
    };
    let expected = std::fs::canonicalize(system).map_err(|source| Error::Io {
        program: format!("system {binary}"),
        source,
    })?;
    let path = std::env::var_os("PATH").ok_or_else(|| {
        Error::Safety(format!(
            "--sudo requires a trusted PATH selecting system {binary}"
        ))
    })?;
    let selected: PathBuf = std::env::split_paths(&path)
        .map(|dir| dir.join(binary))
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
        .ok_or_else(|| Error::Safety(format!("{binary} is absent from PATH")))?;
    let selected = std::fs::canonicalize(selected).map_err(|source| Error::Io {
        program: format!("selected {binary}"),
        source,
    })?;
    let overrides = variables
        .iter()
        .copied()
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
    crate::command::reap_bounded(child).map_err(|_| Error::Safety("sudo authorization could not be stopped; its process may still be running; no native transaction started".into()))
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
                "sudo authorization; no native transaction started".into(),
            ));
        }
        if start.elapsed() >= timeout {
            stop_authentication(child)?;
            return Err(Error::Timeout(
                "sudo authorization; no native transaction started".into(),
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
