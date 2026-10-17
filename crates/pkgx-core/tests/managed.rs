use pkgx_core::{
    apt,
    backend::Backend,
    command::{CommandOutput, CommandRunner, CommandSpec},
    managed::{ManagedPackage, ManagedRegistry, ManagedState},
    manifest::DesiredState,
    planner::{inventory_token, plan_manifest_multi, plan_manifest_multi_managed},
    Error, InstalledPackage, Operation, OperationKind, PackageId, Plan, ProviderId, RiskLevel,
};
use std::{sync::atomic::AtomicBool, time::Duration};
fn packages() -> Vec<InstalledPackage> {
    apt::parse_inventory("curl\tamd64\t8.0-1\tinstalled\ngit\tamd64\t2.0-1\tinstalled\nlibcurl4\tamd64\t8.0-1\tinstalled\n").unwrap()
}
fn state(packages: &[InstalledPackage]) -> ManagedState {
    ManagedState {
        schema_version: 1,
        packages: packages
            .iter()
            .map(|p| ManagedPackage {
                id: p.id.clone(),
                version: p.version.clone(),
                install_reason: p.install_reason,
            })
            .collect(),
    }
}
fn strict(names: &str) -> DesiredState {
    DesiredState::parse(&format!(
        "version = 1\n[system]\nmode = 'strict'\n[packages.apt]\ninstall = [{names}]\n"
    ))
    .unwrap()
}
struct Mock {
    cascade: Option<&'static str>,
}
impl Backend for Mock {
    fn provider(&self) -> ProviderId {
        ProviderId::Apt
    }
    fn capabilities(&self) -> Vec<pkgx_core::Capability> {
        vec![]
    }
    fn inventory(&self) -> Result<Vec<InstalledPackage>, Error> {
        Ok(packages())
    }
    fn plan(&self, install: &[PackageId], remove: &[PackageId]) -> Result<Plan, Error> {
        assert!(install.is_empty());
        let mut removals = remove.to_vec();
        if let Some(id) = self.cascade {
            removals.push(id.parse().unwrap());
        }
        let observed = packages();
        Ok(Plan {
            operations: removals
                .into_iter()
                .map(|id| Operation {
                    kind: OperationKind::Remove,
                    package: id,
                    risk: RiskLevel::RequiresReview,
                    native_detail: "native removal".into(),
                })
                .collect(),
            observed_state_fingerprint: inventory_token(&observed),
            observed_packages: observed,
            uncertain: false,
            warnings: vec![],
        })
    }
}
#[test]
fn missing_registry_is_read_only_and_grants_are_explicit() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("missing");
    let registry = ManagedRegistry::new(&path);
    let initial = registry.read().unwrap();
    assert!(!path.exists());
    assert!(initial.packages.is_empty());
    {
        let mut guard = registry.lock(&initial).unwrap();
        assert!(registry.lock(&initial).is_err());
        guard.grant(&packages()[0]).unwrap();
        assert_eq!(registry.read().unwrap().packages.len(), 1);
        guard.forget(&packages()[0].id).unwrap();
        assert!(registry.read().unwrap().packages.is_empty());
    }
    assert!(!path.join("lock").exists());
    registry.lock(&initial).unwrap();
}
#[test]
fn stale_registry_snapshot_and_external_edits_are_refused() {
    let temp = tempfile::tempdir().unwrap();
    let registry = ManagedRegistry::new(temp.path().join("managed"));
    let before = registry.read().unwrap();
    {
        let mut guard = registry.lock(&before).unwrap();
        guard.grant(&packages()[0]).unwrap();
    }
    assert!(registry.lock(&before).is_err());
    let current = registry.read().unwrap();
    let guard = registry.lock(&current).unwrap();
    std::fs::write(temp.path().join("managed/registry.json"), "{}").unwrap();
    assert!(guard.validate().is_err());
}
#[test]
fn strict_diff_retains_every_ungranted_package_and_default_api_refuses_strict() {
    let desired = strict("'git'");
    let managed = state(&packages()[..1]);
    assert!(desired.diff(&packages()).is_err());
    assert!(plan_manifest_multi(&[&Mock { cascade: None }], &desired).is_err());
    let diff = desired.diff_managed(&packages(), &managed).unwrap();
    assert_eq!(diff.removals, [packages()[0].id.clone()]);
    assert_eq!(diff.undeclared, [packages()[2].id.clone()]);
    let empty = desired
        .diff_managed(&packages(), &ManagedState::default())
        .unwrap();
    assert!(empty.removals.is_empty());
    assert_eq!(empty.undeclared.len(), 2);
}
#[test]
fn strict_planner_refuses_unmanaged_and_desired_managed_cascades() {
    let desired = strict("'git'");
    let managed = state(&packages()[..1]);
    let review =
        plan_manifest_multi_managed(&[&Mock { cascade: None }], &desired, &managed).unwrap();
    assert_eq!(review.providers[0].plan.operations.len(), 1);
    assert!(plan_manifest_multi_managed(
        &[&Mock {
            cascade: Some("apt:libcurl4:amd64")
        }],
        &desired,
        &managed
    )
    .is_err());
    // Even a granted package cannot be removed if it remains in desired state.
    let managed = state(&packages()[..2]);
    assert!(plan_manifest_multi_managed(
        &[&Mock {
            cascade: Some("apt:git:amd64")
        }],
        &desired,
        &managed
    )
    .is_err());
}
#[test]
fn changed_versions_reasons_other_providers_and_duplicate_grants_fail_closed() {
    let desired = strict("'git'");
    let mut managed = state(&packages()[..1]);
    managed.packages[0].version = "older".into();
    assert!(desired.diff_managed(&packages(), &managed).is_err());
    managed = state(&packages()[..1]);
    managed.packages[0].install_reason = pkgx_core::InstallReason::Explicit;
    assert!(desired.diff_managed(&packages(), &managed).is_err());
    managed.packages.push(managed.packages[0].clone());
    assert!(managed.validate().is_err());
    let foreign = DesiredState::parse(
        "version = 1\n[system]\nmode = 'strict'\n[packages.pacman]\ninstall = []",
    )
    .unwrap();
    assert!(foreign.diff_managed(&[], &ManagedState::default()).is_err());
}
#[test]
fn registry_scope_does_not_extend_to_omitted_provider_sections() {
    let desired = DesiredState::parse("version = 1\n[system]\nmode = 'strict'").unwrap();
    assert!(desired
        .diff_managed(&packages(), &state(&packages()))
        .unwrap()
        .removals
        .is_empty());
    let review = plan_manifest_multi_managed(&[], &desired, &state(&packages())).unwrap();
    assert!(review.providers.is_empty());
}
#[cfg(unix)]
#[test]
fn shared_directories_and_symlinked_entries_are_never_repaired_or_followed() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("shared");
    std::fs::create_dir(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    let registry = ManagedRegistry::new(&path);
    assert!(registry.read().is_err());
    assert!(registry.lock(&ManagedState::default()).is_err());
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o755
    );
    let private = temp.path().join("private");
    std::fs::create_dir(&private).unwrap();
    std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o700)).unwrap();
    let target = temp.path().join("target");
    std::fs::write(&target, "keep").unwrap();
    symlink(&target, private.join("registry.json")).unwrap();
    assert!(ManagedRegistry::new(&private).read().is_err());
    assert_eq!(std::fs::read_to_string(target).unwrap(), "keep");
    let link = temp.path().join("directory-link");
    symlink(&private, &link).unwrap();
    assert!(ManagedRegistry::new(link).read().is_err());
}
struct NoCommands;
impl CommandRunner for NoCommands {
    fn run(&self, _: &CommandSpec, _: Duration, _: &AtomicBool) -> Result<CommandOutput, Error> {
        panic!("unauthorized strict execution must not invoke native commands")
    }
}
#[test]
fn strict_executor_refuses_an_unauthorized_preview_before_native_work() {
    let temp = tempfile::tempdir().unwrap();
    let registry = ManagedRegistry::new(temp.path().join("managed"));
    let guard = registry.lock(&ManagedState::default()).unwrap();
    let plan = Mock { cascade: None }
        .plan(&[], &[packages()[0].id.clone()])
        .unwrap();
    assert!(pkgx_core::execution::apply_managed_apt(
        &NoCommands,
        &plan,
        &[],
        &[packages()[0].id.clone()],
        true,
        &AtomicBool::new(false),
        None,
        &guard
    )
    .is_err());
}
