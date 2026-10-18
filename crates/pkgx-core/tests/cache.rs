use pkgx_core::{
    cache::{self, APT_CLEAN, PACMAN_CLEAN},
    command::{CommandOutput, CommandRunner, CommandSpec},
    privilege::SudoAptRunner,
    Error, ProviderId,
};
use std::{
    fs,
    path::PathBuf,
    sync::{atomic::AtomicBool, Mutex},
    time::Duration,
};

/// Reports temporary cache directories; the native cleanup deletes per `keep`.
struct Native {
    dirs: Vec<PathBuf>,
    /// File names the native cleanup leaves in place.
    keep: Vec<&'static str>,
    status: i32,
    cleanups: Mutex<Vec<CommandSpec>>,
}
impl CommandRunner for Native {
    fn run(&self, spec: &CommandSpec, _: Duration, _: &AtomicBool) -> Result<CommandOutput, Error> {
        let stdout = match (spec.program.as_str(), spec.args.first().map(String::as_str)) {
            ("apt-config", Some("shell")) => {
                format!("ARCHIVES='{}/'\n", self.dirs[0].display())
            }
            ("pacman-conf", Some("CacheDir")) => self
                .dirs
                .iter()
                .map(|d| format!("{}/\n", d.display()))
                .collect(),
            _ => {
                assert!(spec.requires_privilege);
                self.cleanups.lock().unwrap().push(spec.clone());
                for dir in &self.dirs {
                    for entry in fs::read_dir(dir).unwrap().flatten() {
                        let name = entry.file_name().into_string().unwrap();
                        if entry.path().is_file() && !self.keep.contains(&name.as_str()) {
                            fs::remove_file(entry.path()).unwrap();
                        }
                    }
                }
                String::new()
            }
        };
        Ok(CommandOutput {
            status: Some(if spec.requires_privilege {
                self.status
            } else {
                0
            }),
            stdout: stdout.into_bytes(),
            stderr: vec![],
        })
    }
}
fn native(dirs: Vec<PathBuf>) -> Native {
    Native {
        dirs,
        keep: vec![],
        status: 0,
        cleanups: Mutex::new(vec![]),
    }
}

#[test]
fn measurement_counts_only_regular_package_files_in_native_directories() {
    let temp = tempfile::tempdir().unwrap();
    let (a, b) = (temp.path().join("a"), temp.path().join("b"));
    fs::create_dir_all(a.join("partial")).unwrap();
    fs::create_dir(&b).unwrap();
    fs::write(a.join("curl_8.0_amd64.deb"), [0u8; 10]).unwrap();
    fs::write(a.join("partial/next.deb"), [0u8; 99]).unwrap();
    fs::write(a.join("lock"), b"").unwrap();
    fs::write(a.join("x-1-1-x86_64.pkg.tar.zst"), [0u8; 7]).unwrap();
    fs::write(b.join("y-1-1-any.pkg.tar.zst.sig"), [0u8; 3]).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(a.join("curl_8.0_amd64.deb"), a.join("link.deb")).unwrap();
    let apt = cache::measure(&native(vec![a.clone()]), ProviderId::Apt).unwrap();
    assert_eq!((apt.files, apt.bytes), (1, 10));
    assert_eq!(apt.category, "safe_native_cleanup");
    assert_eq!(apt.native_command, ["apt-get", "clean"]);
    let pacman = cache::measure(&native(vec![a, b]), ProviderId::Pacman).unwrap();
    assert_eq!((pacman.files, pacman.bytes), (2, 10));
    assert!(cache::measure(&native(vec![]), ProviderId::Flatpak).is_err());
}
#[test]
fn cleanup_requires_confirmation_rejects_stale_previews_and_verifies_reduction() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().to_path_buf();
    fs::write(dir.join("a-1-1-x86_64.pkg.tar.zst"), [0u8; 5]).unwrap();
    fs::write(dir.join("b-1-1-x86_64.pkg.tar.zst"), [0u8; 5]).unwrap();
    let runner = native(vec![dir.clone()]);
    let preview = cache::measure(&runner, ProviderId::Pacman).unwrap();
    let canceled = AtomicBool::new(false);
    assert!(matches!(
        cache::clean(&runner, &preview, false, &canceled, None),
        Err(Error::Safety(_))
    ));
    fs::write(dir.join("c-1-1-x86_64.pkg.tar.zst"), [0u8; 5]).unwrap();
    assert!(matches!(
        cache::clean(&runner, &preview, true, &canceled, None),
        Err(Error::Safety(m)) if m.contains("changed")
    ));
    assert!(runner.cleanups.lock().unwrap().is_empty());
    let preview = cache::measure(&runner, ProviderId::Pacman).unwrap();
    let journal_dir = tempfile::tempdir().unwrap();
    let journal = pkgx_core::journal::Journal::new(journal_dir.path().join("j"));
    let record = cache::clean(&runner, &preview, true, &canceled, Some(&journal)).unwrap();
    assert!(record.success && record.verified, "{:?}", record.errors);
    assert_eq!(
        runner.cleanups.lock().unwrap()[0].args,
        PACMAN_CLEAN[1..]
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
    );
    assert!(
        journal.history().unwrap()[0]
            .result
            .as_ref()
            .unwrap()
            .success
    );
    // Nothing left: a verified no-op that runs no native command.
    let empty = cache::measure(&runner, ProviderId::Pacman).unwrap();
    assert!(
        cache::clean(&runner, &empty, true, &canceled, None)
            .unwrap()
            .success
    );
    assert_eq!(runner.cleanups.lock().unwrap().len(), 1);
}
#[test]
fn apt_cleanup_that_leaves_archives_or_fails_is_reported() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().to_path_buf();
    fs::write(dir.join("a_1_amd64.deb"), [0u8; 5]).unwrap();
    let mut runner = native(vec![dir.clone()]);
    runner.keep = vec!["a_1_amd64.deb"];
    let preview = cache::measure(&runner, ProviderId::Apt).unwrap();
    let record = cache::clean(&runner, &preview, true, &AtomicBool::new(false), None).unwrap();
    assert!(!record.success && !record.verified);
    runner.keep.clear();
    runner.status = 100;
    let record = cache::clean(&runner, &preview, true, &AtomicBool::new(false), None).unwrap();
    assert!(!record.success && record.native_status == Some(100));
}
#[test]
fn sudo_accepts_only_the_exact_cleanup_vectors() {
    #[derive(Default)]
    struct Inner(Mutex<Vec<CommandSpec>>);
    impl CommandRunner for Inner {
        fn run(
            &self,
            spec: &CommandSpec,
            _: Duration,
            _: &AtomicBool,
        ) -> Result<CommandOutput, Error> {
            self.0.lock().unwrap().push(spec.clone());
            Ok(CommandOutput {
                status: Some(0),
                stdout: vec![],
                stderr: vec![],
            })
        }
    }
    let inner = Inner::default();
    let runner = SudoAptRunner::new(&inner);
    let run = |template: &[&str], extra: Option<&str>| {
        let mut spec = CommandSpec::new(template[0], &template[1..]);
        spec.args.extend(extra.map(str::to_owned));
        spec.requires_privilege = true;
        runner.run(&spec, Duration::from_secs(1), &AtomicBool::new(false))
    };
    assert!(run(APT_CLEAN, None).is_ok());
    assert!(run(PACMAN_CLEAN, None).is_ok());
    assert!(run(APT_CLEAN, Some("--option")).is_err());
    assert!(run(PACMAN_CLEAN, Some("--clean")).is_err());
    assert!(run(&["apt-get", "autoclean"], None).is_err());
    let calls = inner.0.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].args[4], pkgx_core::privilege::APT);
    assert_eq!(calls[1].args[4], pkgx_core::privilege::PACMAN);
}
