use crate::Error;
use std::{
    io::{self, Read},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

const OUTPUT_LIMIT: u64 = 8 * 1024 * 1024;
#[derive(Debug, Clone)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub env: std::collections::BTreeMap<String, String>,
    /// Only the executor marks a native transaction; inspection defaults to false.
    pub requires_privilege: bool,
    /// Give a trusted delegate time to relay SIGTERM before forced cleanup.
    pub termination_grace: Duration,
}
impl CommandSpec {
    pub fn new(program: &str, args: &[&str]) -> Self {
        Self {
            program: program.into(),
            args: args.iter().map(|s| (*s).into()).collect(),
            env: Default::default(),
            requires_privilege: false,
            termination_grace: Duration::ZERO,
        }
    }
}
#[derive(Debug)]
pub struct CommandOutput {
    pub status: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}
pub trait CommandRunner {
    fn run(
        &self,
        spec: &CommandSpec,
        timeout: Duration,
        canceled: &AtomicBool,
    ) -> Result<CommandOutput, Error>;
}
pub struct ProcessRunner;
fn stop(mut child: Child, grace: Duration) -> io::Result<()> {
    #[cfg(unix)]
    if !grace.is_zero() {
        if let Ok(pid) = i32::try_from(child.id()) {
            // SAFETY: the unreaped leader reserves this process group identifier.
            // In particular, sudo must relay a catchable signal before SIGKILL.
            unsafe {
                libc::kill(-pid, libc::SIGTERM);
            }
        }
        // Do not reap during the grace period: a descendant may still retain pipes
        // and a later group signal must never target a recycled process group.
        thread::sleep(grace.min(Duration::from_secs(3)));
    }
    #[cfg(unix)]
    {
        if let Ok(pid) = i32::try_from(child.id()) {
            // SAFETY: scalar arguments, with the leader still unreaped.
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
        }
    }
    let kill_error = child.kill().err();
    reap_bounded(child).map_err(|error| kill_error.unwrap_or(error))
}
/// Even a successful kill can leave a child in uninterruptible kernel sleep.
/// Keep a reaper, but never block the caller indefinitely on wait().
pub(crate) fn reap_bounded(mut child: Child) -> io::Result<()> {
    let deadline = Instant::now() + Duration::from_millis(200);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return Ok(()),
            Err(error) => return Err(error),
            Ok(None) if Instant::now() >= deadline => {
                thread::spawn(move || {
                    let _ = child.wait();
                });
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "subprocess has not exited; background reaper retained",
                ));
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
        }
    }
}
fn cleanup(child: Child, spec: &CommandSpec, original: Error) -> Error {
    match stop(child, spec.termination_grace) {
        Ok(()) => original,
        Err(error) => Error::Safety(format!(
            "{original}; subprocess cleanup failed ({error}); process may still be running"
        )),
    }
}

fn read_pipe(pipe: impl Read) -> io::Result<Vec<u8>> {
    let mut bytes = vec![];
    pipe.take(OUTPUT_LIMIT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > OUTPUT_LIMIT {
        return Err(io::Error::other(
            "subprocess output exceeds 8 MiB per stream",
        ));
    }
    Ok(bytes)
}
impl CommandRunner for ProcessRunner {
    fn run(
        &self,
        spec: &CommandSpec,
        timeout: Duration,
        canceled: &AtomicBool,
    ) -> Result<CommandOutput, Error> {
        let io_error = |source| Error::Io {
            program: spec.program.clone(),
            source,
        };
        if canceled.load(Ordering::Relaxed) {
            return Err(Error::Canceled(spec.program.clone()));
        }
        let mut command = Command::new(&spec.program);
        command
            .args(&spec.args)
            .envs(&spec.env)
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command.spawn().map_err(io_error)?;
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        let (sender, receiver) = mpsc::channel();
        let err_sender = sender.clone();
        thread::spawn(move || {
            let _ = sender.send((true, read_pipe(stdout)));
        });
        thread::spawn(move || {
            let _ = err_sender.send((false, read_pipe(stderr)));
        });
        let start = Instant::now();
        let mut stdout = None;
        let mut stderr = None;
        let result = loop {
            if canceled.load(Ordering::Relaxed) {
                break Err(Error::Canceled(spec.program.clone()));
            }
            if start.elapsed() >= timeout {
                break Err(Error::Timeout(spec.program.clone()));
            }
            while let Ok((is_stdout, result)) = receiver.try_recv() {
                match result {
                    Ok(bytes) => {
                        if is_stdout {
                            stdout = Some(bytes);
                        } else {
                            stderr = Some(bytes);
                        }
                    }
                    Err(e) => {
                        return Err(cleanup(child, spec, io_error(e)));
                    }
                }
            }
            // Retain the unreaped leader until both pipes close. Its PID/PGID
            // cannot be reused while a descendant retains inherited pipe handles,
            // so a later timeout cannot signal an unrelated reused process group.
            if stdout.is_some() && stderr.is_some() {
                match child.try_wait() {
                    Ok(Some(exit)) => {
                        break Ok(CommandOutput {
                            status: exit.code(),
                            stdout: stdout.take().expect("received stdout"),
                            stderr: stderr.take().expect("received stderr"),
                        })
                    }
                    Ok(None) => {}
                    Err(error) => break Err(io_error(error)),
                }
            }
            thread::sleep(Duration::from_millis(10));
        };
        result.map_err(|error| cleanup(child, spec, error))
    }
}
pub fn checked(runner: &dyn CommandRunner, spec: &CommandSpec) -> Result<CommandOutput, Error> {
    let output = runner.run(spec, Duration::from_secs(30), &AtomicBool::new(false))?;
    if output.status != Some(0) {
        return Err(Error::CommandFailed {
            program: spec.program.clone(),
            status: output.status,
        });
    }
    Ok(output)
}

#[cfg(test)]
mod cleanup_tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn a_slow_child_is_reaped_in_the_background_without_blocking_the_caller() {
        let child = Command::new("/bin/sleep")
            .arg("0.4")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let start = Instant::now();
        assert_eq!(
            reap_bounded(child).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        assert!(start.elapsed() < Duration::from_secs(2));
        thread::sleep(Duration::from_millis(250));
    }
}
