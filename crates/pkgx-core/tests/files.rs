use pkgx_core::{
    files::{self, FileReceipt, FileRegistry, FileRegistryState, FileState},
    manifest::DesiredState,
    Error,
};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, os::unix::fs::PermissionsExt, path::Path};

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn manifest(text: &str) -> DesiredState {
    DesiredState::parse(text)
        .unwrap()
        .resolve("linux", &[])
        .unwrap()
}
fn receipt(target: &str, written: &[u8], mode: &str) -> FileReceipt {
    FileReceipt {
        target: target.into(),
        sha256: digest(written),
        bytes: written.len() as u64,
        mode: mode.into(),
        source_sha256: digest(written),
    }
}
fn write(path: &Path, bytes: &[u8], mode: u32) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}
fn states(
    state: &DesiredState,
    dir: &Path,
    home: &Path,
    registry: &FileRegistryState,
) -> Vec<(String, FileState)> {
    let sources = files::load_sources(dir, &state.files).unwrap();
    files::observe(home, &sources, registry)
        .unwrap()
        .into_iter()
        .map(|s| (s.target, s.state))
        .collect()
}

#[test]
fn declarations_are_strict_literal_home_paths() {
    let ok = manifest(
        "version = 1\n[files.\"~/.tmux.conf\"]\nsource = \"dotfiles/tmux.conf\"\nmode = \"0644\"\n",
    );
    assert_eq!(ok.files["~/.tmux.conf"].mode, "0644");
    let default = manifest("version = 1\n[files.\"~/.a\"]\nsource = \"a\"\n");
    assert_eq!(default.files["~/.a"].mode, "0600");
    for bad in [
        "[files.\"/etc/tmux.conf\"]\nsource = \"a\"",
        "[files.\"~/../x\"]\nsource = \"a\"",
        "[files.\"~/a//b\"]\nsource = \"a\"",
        "[files.\"~/a/\"]\nsource = \"a\"",
        "[files.\"~/\"]\nsource = \"a\"",
        "[files.\"~/a/./b\"]\nsource = \"a\"",
        "[files.\"~/a\"]\nsource = \"../a\"",
        "[files.\"~/a\"]\nsource = \"/etc/passwd\"",
        "[files.\"~/a\"]\nsource = \"a\"\nmode = \"4755\"",
        "[files.\"~/a\"]\nsource = \"a\"\nmode = \"0666\"",
        "[files.\"~/a\"]\nsource = \"a\"\ncontent = \"x\"",
        "[profiles.dev.files.\"/etc/x\"]\nsource = \"a\"",
        "[platforms.macos.files.\"~/a\"]\nsource = \"a\"\nmode = \"0777\"",
    ] {
        let input = format!("version = 1\n{bad}\n");
        assert!(DesiredState::parse(&input).is_err(), "accepted {bad}");
    }
}

#[test]
fn layers_collapse_identical_and_refuse_conflicting_declarations() {
    let same = DesiredState::parse(
        "version = 1\n[files.\"~/.a\"]\nsource = \"a\"\n[profiles.dev.files.\"~/.a\"]\nsource = \"a\"\n[profiles.dev.files.\"~/.b\"]\nsource = \"b\"\n[platforms.macos.files.\"~/.a\"]\nsource = \"other\"\n",
    )
    .unwrap();
    let resolved = same.resolve("linux", &["dev".into()]).unwrap();
    assert_eq!(resolved.files.len(), 2);
    assert!(same.resolve("linux", &[]).unwrap().files.len() == 1);
    assert!(matches!(
        same.resolve("macos", &[]),
        Err(Error::InvalidInput(message)) if message.contains("conflicting")
    ));
}

#[test]
fn file_manifests_refuse_planning_until_writes_exist() {
    let state = manifest("version = 1\n[files.\"~/.a\"]\nsource = \"a\"\n");
    assert!(matches!(
        state.require_no_files(),
        Err(Error::Unsupported(_))
    ));
    assert!(matches!(
        pkgx_core::planner::plan_manifest_multi(&[], &state),
        Err(Error::Unsupported(_))
    ));
    let exported = DesiredState::export(&[]).unwrap();
    assert!(!exported.contains("files"));
}

#[test]
fn observation_classifies_every_state_without_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    for name in ["missing", "sync", "outdated", "drifted", "conflict", "mode"] {
        fs::write(dir.path().join(name), format!("new {name}")).unwrap();
    }
    let state = manifest(
        "version = 1\n\
         [files.\"~/missing\"]\nsource = \"missing\"\n\
         [files.\"~/.config/sync\"]\nsource = \"sync\"\nmode = \"0644\"\n\
         [files.\"~/outdated\"]\nsource = \"outdated\"\n\
         [files.\"~/drifted\"]\nsource = \"drifted\"\n\
         [files.\"~/conflict\"]\nsource = \"conflict\"\n\
         [files.\"~/mode\"]\nsource = \"mode\"\n",
    );
    let h = home.path();
    write(&h.join(".config/sync"), b"new sync", 0o644);
    write(&h.join("outdated"), b"old outdated", 0o600);
    write(&h.join("drifted"), b"edited", 0o600);
    write(&h.join("conflict"), b"new conflict", 0o600);
    write(&h.join("mode"), b"new mode", 0o644);
    let registry = FileRegistryState {
        schema_version: 1,
        files: vec![
            receipt("~/.config/sync", b"older sync", "0644"),
            receipt("~/outdated", b"old outdated", "0600"),
            receipt("~/drifted", b"old drifted", "0600"),
            receipt("~/mode", b"new mode", "0600"),
            receipt("~/gone-from-manifest", b"x", "0600"),
        ],
    };
    let before: Vec<_> = fs::read_dir(h)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(
        states(&state, dir.path(), h, &registry),
        vec![
            ("~/.config/sync".into(), FileState::InSync),
            ("~/conflict".into(), FileState::UnmanagedConflict),
            ("~/drifted".into(), FileState::Drifted),
            ("~/gone-from-manifest".into(), FileState::Undeclared),
            ("~/missing".into(), FileState::Missing),
            ("~/mode".into(), FileState::Drifted),
            ("~/outdated".into(), FileState::Outdated),
        ]
    );
    let after: Vec<_> = fs::read_dir(h)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(before, after);
    assert_eq!(fs::read(h.join("outdated")).unwrap(), b"old outdated");
}

#[test]
fn symlinks_and_special_targets_are_unsupported() {
    let dir = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a"), "a").unwrap();
    let h = home.path();
    std::os::unix::fs::symlink(outside.path(), h.join("linked")).unwrap();
    std::os::unix::fs::symlink(dir.path().join("a"), h.join("link")).unwrap();
    fs::create_dir(h.join("dir")).unwrap();
    fs::write(h.join("plain"), "x").unwrap();
    let state = manifest(
        "version = 1\n\
         [files.\"~/linked/a\"]\nsource = \"a\"\n\
         [files.\"~/link\"]\nsource = \"a\"\n\
         [files.\"~/dir\"]\nsource = \"a\"\n\
         [files.\"~/plain/a\"]\nsource = \"a\"\n",
    );
    let observed = states(&state, dir.path(), h, &FileRegistryState::default());
    assert!(observed.iter().all(|(_, s)| *s == FileState::Unsupported));
    assert!(!outside.path().join("a").exists());
}

#[test]
fn sources_stay_inside_the_manifest_directory() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("secret"), "s").unwrap();
    std::os::unix::fs::symlink(outside.path().join("secret"), dir.path().join("link")).unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join("linkdir")).unwrap();
    fs::create_dir(dir.path().join("sub")).unwrap();
    fs::write(dir.path().join("big"), vec![b'x'; 1024 * 1024 + 1]).unwrap();
    for source in ["link", "linkdir/secret", "sub", "absent", "big"] {
        let mut declared = BTreeMap::new();
        declared.insert(
            "~/.a".to_string(),
            files::FileDeclaration {
                source: source.into(),
                mode: "0600".into(),
            },
        );
        assert!(
            matches!(
                files::load_sources(dir.path(), &declared),
                Err(Error::Safety(_))
            ),
            "accepted {source}"
        );
    }
}

#[test]
fn registry_reads_fail_closed_and_never_create_state() {
    let root = tempfile::tempdir().unwrap();
    let registry = FileRegistry::new(root.path().join("files"));
    assert_eq!(registry.read().unwrap(), FileRegistryState::default());
    assert!(!root.path().join("files").exists());
    fs::create_dir(root.path().join("files")).unwrap();
    fs::set_permissions(root.path().join("files"), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(registry.read(), Err(Error::Safety(_))));
    fs::set_permissions(root.path().join("files"), fs::Permissions::from_mode(0o700)).unwrap();
    write(
        &root.path().join("files/registry.json"),
        b"{\"schema_version\":1,\"files\":[],\"x\":1}",
        0o600,
    );
    assert!(matches!(registry.read(), Err(Error::Parse(_))));
    let duplicate = FileRegistryState {
        schema_version: 1,
        files: vec![receipt("~/.a", b"a", "0600"), receipt("~/.a", b"b", "0600")],
    };
    write(
        &root.path().join("files/registry.json"),
        &serde_json::to_vec(&duplicate).unwrap(),
        0o600,
    );
    assert!(matches!(registry.read(), Err(Error::Safety(_))));
}

struct Fixture {
    repo: tempfile::TempDir,
    home: tempfile::TempDir,
    state: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        Self {
            repo: tempfile::tempdir().unwrap(),
            home: tempfile::tempdir().unwrap(),
            state: tempfile::tempdir().unwrap(),
        }
    }
    fn registry(&self) -> FileRegistry {
        FileRegistry::new(self.state.path().join("files"))
    }
    fn sources(&self, manifest_text: &str) -> Vec<files::LoadedSource> {
        files::load_sources(self.repo.path(), &manifest(manifest_text).files).unwrap()
    }
    fn plan(&self, manifest_text: &str) -> Result<files::FilePlan, Error> {
        files::plan(
            self.home.path(),
            &self.sources(manifest_text),
            &self.registry().read().unwrap(),
        )
    }
    fn apply(&self, manifest_text: &str, plan: &files::FilePlan) -> Result<(), Error> {
        let registry = self.registry();
        let mut guard = registry.lock(&registry.read().unwrap()).unwrap();
        files::apply(
            self.home.path(),
            &self.sources(manifest_text),
            plan,
            &mut guard,
        )
        .map(|_| ())
    }
}
const TMUX: &str =
    "version = 1\n[files.\"~/.config/tmux/tmux.conf\"]\nsource = \"tmux.conf\"\nmode = \"0644\"\n";

#[test]
fn apply_creates_files_with_private_parents_and_records_receipts() {
    let f = Fixture::new();
    fs::write(f.repo.path().join("tmux.conf"), "set -g mouse on\n").unwrap();
    let plan = f.plan(TMUX).unwrap();
    assert_eq!(plan.writes.len(), 1);
    assert!(plan.writes[0].diff.contains("+set -g mouse on"));
    f.apply(TMUX, &plan).unwrap();
    let target = f.home.path().join(".config/tmux/tmux.conf");
    assert_eq!(fs::read_to_string(&target).unwrap(), "set -g mouse on\n");
    assert_eq!(
        fs::metadata(&target).unwrap().permissions().mode() & 0o7777,
        0o644
    );
    for dir in [".config", ".config/tmux"] {
        let mode = fs::metadata(f.home.path().join(dir))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700, "{dir}");
    }
    let receipts = f.registry().read().unwrap();
    assert_eq!(receipts.files[0].sha256, digest(b"set -g mouse on\n"));
    assert!(f.plan(TMUX).unwrap().writes.is_empty());
    assert!(!f.state.path().join("files/lock").exists());

    fs::write(f.repo.path().join("tmux.conf"), "set -g mouse off\n").unwrap();
    let update = f.plan(TMUX).unwrap();
    assert!(update.writes[0].diff.contains("-set -g mouse on"));
    f.apply(TMUX, &update).unwrap();
    assert_eq!(fs::read_to_string(&target).unwrap(), "set -g mouse off\n");
}

#[test]
fn conflicts_block_the_whole_plan() {
    let f = Fixture::new();
    fs::write(f.repo.path().join("tmux.conf"), "x").unwrap();
    write(&f.home.path().join(".config/tmux/tmux.conf"), b"x", 0o644);
    let error = f.plan(TMUX).unwrap_err().to_string();
    assert!(
        error.contains("~/.config/tmux/tmux.conf (unmanaged_conflict)"),
        "{error}"
    );
}

#[test]
fn apply_refuses_targets_or_sources_changed_after_review() {
    let f = Fixture::new();
    fs::write(f.repo.path().join("tmux.conf"), "declared\n").unwrap();
    let target = f.home.path().join(".config/tmux/tmux.conf");

    let plan = f.plan(TMUX).unwrap();
    write(&target, b"created meanwhile\n", 0o644);
    assert!(matches!(f.apply(TMUX, &plan), Err(Error::Safety(_))));
    assert_eq!(fs::read_to_string(&target).unwrap(), "created meanwhile\n");
    fs::remove_file(&target).unwrap();

    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path().join("victim"), &target).unwrap();
    assert!(matches!(f.apply(TMUX, &plan), Err(Error::Safety(_))));
    assert!(!outside.path().join("victim").exists());
    fs::remove_file(&target).unwrap();
    fs::remove_dir(target.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(outside.path(), target.parent().unwrap()).unwrap();
    assert!(matches!(f.apply(TMUX, &plan), Err(Error::Safety(_))));
    assert!(fs::read_dir(outside.path()).unwrap().next().is_none());
    fs::remove_file(target.parent().unwrap()).unwrap();

    let reviewed = f.plan(TMUX).unwrap();
    fs::write(f.repo.path().join("tmux.conf"), "edited after review\n").unwrap();
    assert!(matches!(f.apply(TMUX, &reviewed), Err(Error::Safety(_))));
    assert!(!target.exists());
    assert!(f.registry().read().unwrap().files.is_empty());
}

#[test]
fn adoption_backs_up_content_and_enables_replacement() {
    let f = Fixture::new();
    fs::write(f.repo.path().join("tmux.conf"), "declared\n").unwrap();
    let target = f.home.path().join(".config/tmux/tmux.conf");
    write(&target, b"handwritten\n", 0o664);
    let registry = f.registry();
    let preview = files::preview_adoption(
        f.home.path(),
        "~/.config/tmux/tmux.conf",
        &registry.read().unwrap(),
    )
    .unwrap();
    assert_eq!(preview.mode, "0664");
    {
        let mut guard = registry.lock(&registry.read().unwrap()).unwrap();
        assert!(matches!(
            registry.lock(&registry.read().unwrap()),
            Err(Error::Safety(_))
        ));
        files::adopt(f.home.path(), &preview, &mut guard).unwrap();
    }
    let backup = f
        .state
        .path()
        .join("files/backups")
        .join(digest(b"handwritten\n"));
    assert_eq!(fs::read(&backup).unwrap(), b"handwritten\n");
    assert_eq!(
        fs::metadata(&backup).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(fs::read(&target).unwrap(), b"handwritten\n");
    let plan = f.plan(TMUX).unwrap();
    assert_eq!(plan.files[0].state, FileState::Outdated);
    f.apply(TMUX, &plan).unwrap();
    assert_eq!(fs::read(&target).unwrap(), b"declared\n");

    // Content changed between preview and confirmation: nothing is adopted.
    write(&target, b"local edit\n", 0o644);
    let stale = files::preview_adoption(
        f.home.path(),
        "~/.config/tmux/tmux.conf",
        &registry.read().unwrap(),
    )
    .unwrap();
    assert!(stale.replaces_receipt);
    write(&target, b"another edit\n", 0o644);
    let snapshot = registry.read().unwrap();
    let mut guard = registry.lock(&snapshot).unwrap();
    assert!(matches!(
        files::adopt(f.home.path(), &stale, &mut guard),
        Err(Error::Safety(_))
    ));
    assert_eq!(guard.state, snapshot);
    guard.forget("~/.config/tmux/tmux.conf").unwrap();
    drop(guard);
    assert!(target.exists());
    assert!(registry.read().unwrap().files.is_empty());
}

#[test]
fn adoption_refuses_unsupported_targets_and_stale_registry_snapshots() {
    let f = Fixture::new();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("x"), "x").unwrap();
    std::os::unix::fs::symlink(outside.path().join("x"), f.home.path().join(".link")).unwrap();
    let empty = FileRegistryState::default();
    assert!(files::preview_adoption(f.home.path(), "~/.link", &empty).is_err());
    assert!(files::preview_adoption(f.home.path(), "~/.absent", &empty).is_err());
    fs::write(f.home.path().join(".big"), vec![b'x'; 1024 * 1024 + 1]).unwrap();
    assert!(files::preview_adoption(f.home.path(), "~/.big", &empty).is_err());

    let registry = f.registry();
    let stale = FileRegistryState {
        schema_version: 1,
        files: vec![receipt("~/.a", b"a", "0600")],
    };
    assert!(matches!(registry.lock(&stale), Err(Error::Safety(_))));
}

#[test]
fn removal_deletes_only_unchanged_managed_files_after_backup() {
    let f = Fixture::new();
    fs::write(f.repo.path().join("tmux.conf"), "declared\n").unwrap();
    f.apply(TMUX, &f.plan(TMUX).unwrap()).unwrap();
    let target = f.home.path().join(".config/tmux/tmux.conf");
    let registry = f.registry();
    let name = "~/.config/tmux/tmux.conf";

    let unmanaged = f.home.path().join(".bashrc");
    write(&unmanaged, b"mine\n", 0o644);
    assert!(files::preview_removal(f.home.path(), "~/.bashrc", &registry.read().unwrap()).is_err());

    write(&target, b"edited\n", 0o644);
    assert!(matches!(
        files::preview_removal(f.home.path(), name, &registry.read().unwrap()),
        Err(Error::Safety(_))
    ));
    write(&target, b"declared\n", 0o644);

    let preview = files::preview_removal(f.home.path(), name, &registry.read().unwrap()).unwrap();
    write(&target, b"changed after review\n", 0o644);
    {
        let mut guard = registry.lock(&registry.read().unwrap()).unwrap();
        assert!(matches!(
            files::remove(f.home.path(), &preview, &mut guard),
            Err(Error::Safety(_))
        ));
    }
    assert_eq!(fs::read(&target).unwrap(), b"changed after review\n");
    write(&target, b"declared\n", 0o644);

    let preview = files::preview_removal(f.home.path(), name, &registry.read().unwrap()).unwrap();
    let mut guard = registry.lock(&registry.read().unwrap()).unwrap();
    files::remove(f.home.path(), &preview, &mut guard).unwrap();
    drop(guard);
    assert!(!target.exists());
    assert_eq!(fs::read(&unmanaged).unwrap(), b"mine\n");
    assert!(registry.read().unwrap().files.is_empty());
    let backup = f
        .state
        .path()
        .join("files/backups")
        .join(digest(b"declared\n"));
    assert_eq!(fs::read(backup).unwrap(), b"declared\n");
    assert!(matches!(
        files::preview_removal(f.home.path(), name, &registry.read().unwrap()),
        Err(Error::Safety(_))
    ));
}

#[test]
fn receipt_audit_reports_current_state_without_a_manifest() {
    let f = Fixture::new();
    let h = f.home.path();
    write(&h.join("intact"), b"a", 0o600);
    write(&h.join("drifted"), b"edited", 0o600);
    std::os::unix::fs::symlink("/etc/hostname", h.join("link")).unwrap();
    let registry = FileRegistryState {
        schema_version: 1,
        files: vec![
            receipt("~/drifted", b"b", "0600"),
            receipt("~/gone", b"c", "0600"),
            receipt("~/intact", b"a", "0600"),
            receipt("~/link", b"d", "0600"),
        ],
    };
    let audit = files::audit_receipts(h, &registry).unwrap();
    let states: Vec<_> = audit.files.iter().map(|s| s.state).collect();
    use files::ReceiptState::*;
    assert_eq!(states, vec![Drifted, Missing, Intact, Unsupported]);
    let json = serde_json::to_value(&audit).unwrap();
    assert_eq!(json["files"][2]["target"], "~/intact");
    assert_eq!(json["files"][2]["state"], "intact");
}
