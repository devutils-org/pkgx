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
