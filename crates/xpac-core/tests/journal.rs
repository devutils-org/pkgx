use xpac_core::{execution::ExecutionRecord, journal::Journal, Plan};
fn plan() -> Plan {
    Plan {
        operations: vec![],
        observed_state_fingerprint: "before".into(),
        observed_packages: vec![],
        uncertain: true,
        warnings: vec![],
    }
}
#[test]
fn reading_missing_history_is_read_only() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("missing");
    assert!(Journal::new(&path).history().unwrap().is_empty());
    assert!(!path.exists());
}
#[test]
fn intent_is_unknown_until_finalized_and_cannot_be_rewritten() {
    let temp = tempfile::tempdir().unwrap();
    let journal = Journal::new(temp.path().join("transactions"));
    let id = journal.begin(&plan()).unwrap();
    assert!(journal.history().unwrap()[0].result.is_none());
    let record = ExecutionRecord {
        success: false,
        native_status: Some(100),
        verified: false,
        errors: vec!["partial failure".into()],
        rollback_attempted: false,
        observed_before: "before".into(),
        observed_after: Some("after".into()),
    };
    journal.finish(&id, &record).unwrap();
    let history = journal.history().unwrap();
    let saved = history[0].result.as_ref().unwrap();
    assert!(!saved.success && !saved.rollback_attempted);
    assert_eq!(saved.native_status, Some(100));
    assert!(journal.finish(&id, &record).is_err());
    assert!(journal.finish("../../outside", &record).is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(temp.path().join("transactions").join(format!("{id}.json")))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
}
#[cfg(unix)]
#[test]
fn journal_rejects_symlinked_directories_and_entries() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("real");
    std::fs::create_dir(&target).unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    symlink(&target, temp.path().join("link")).unwrap();
    assert!(Journal::new(temp.path().join("link"))
        .begin(&plan())
        .is_err());
    let journal = Journal::new(&target);
    let id = journal.begin(&plan()).unwrap();
    symlink(target.join(format!("{id}.json")), target.join("123.json")).unwrap();
    assert!(journal.history().is_err());
}

#[cfg(unix)]
#[test]
fn existing_shared_directory_is_rejected_without_changing_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let shared = temp.path().join("shared");
    std::fs::create_dir(&shared).unwrap();
    std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(Journal::new(&shared).begin(&plan()).is_err());
    assert_eq!(
        std::fs::metadata(shared).unwrap().permissions().mode() & 0o777,
        0o755
    );
}

#[test]
fn paginated_history_and_streamed_status_preserve_pending_failures() {
    let temp = tempfile::tempdir().unwrap();
    let journal = Journal::new(temp.path().join("records"));
    let first = journal.begin(&plan()).unwrap();
    let success = ExecutionRecord {
        success: true,
        native_status: Some(0),
        verified: true,
        errors: vec![],
        rollback_attempted: false,
        observed_before: "before".into(),
        observed_after: Some("after".into()),
    };
    journal.finish(&first, &success).unwrap();
    let second = journal.begin(&plan()).unwrap();
    let mut failed = success;
    failed.success = false;
    failed.native_status = Some(100);
    journal.finish(&second, &failed).unwrap();
    let third = journal.begin(&plan()).unwrap();
    let status = journal.status().unwrap();
    assert_eq!(
        (
            status.total,
            status.successful,
            status.failed,
            status.pending_or_unknown
        ),
        (3, 1, 1, 1)
    );
    assert_eq!(journal.history_page(0, 1).unwrap()[0].id, third);
    assert_eq!(journal.history_page(1, 1).unwrap()[0].id, second);
    assert_eq!(journal.history_page(2, 1).unwrap()[0].id, first);
    assert!(journal.history_page(3, 1).unwrap().is_empty());
    assert!(journal.history_page(0, 0).is_err());
}
