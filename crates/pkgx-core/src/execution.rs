//! Transaction execution is deliberately separate from inspection and planning.
use crate::{
    apt::{self, AptBackend},
    backend::Backend,
    command::{CommandRunner, CommandSpec},
    Error, OperationKind, PackageId, Plan,
};
use serde::{Deserialize, Serialize};
use std::{sync::atomic::AtomicBool, time::Duration};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionRecord {
    pub success: bool,
    pub native_status: Option<i32>,
    pub verified: bool,
    pub errors: Vec<String>,
    pub rollback_attempted: bool,
    #[serde(default)]
    pub observed_before: String,
    #[serde(default)]
    pub observed_after: Option<String>,
}
/// Never accepts a deserialized command. Rebuilds native arguments from validated
/// identities and repeats both native simulation and inventory after confirmation.
pub fn apply_apt(
    runner: &dyn CommandRunner,
    preview: &Plan,
    install: &[PackageId],
    remove: &[PackageId],
    confirmed: bool,
    canceled: &AtomicBool,
) -> Result<ExecutionRecord, Error> {
    apply_apt_recorded(runner, preview, install, remove, confirmed, canceled, None)
}
/// Persist intent before execution and replace it with the verified result.
/// An intent left without a result is reported as interrupted/unknown, never success.
#[allow(clippy::too_many_arguments)]
pub fn apply_apt_recorded(
    runner: &dyn CommandRunner,
    preview: &Plan,
    install: &[PackageId],
    remove: &[PackageId],
    confirmed: bool,
    canceled: &AtomicBool,
    journal: Option<&crate::journal::Journal>,
) -> Result<ExecutionRecord, Error> {
    apply_apt_guarded(
        runner, preview, install, remove, confirmed, canceled, journal, None,
    )
}
/// Strict execution holds a registry writer lock and rechecks its grants at every guard point.
#[allow(clippy::too_many_arguments)]
pub fn apply_managed_apt(
    runner: &dyn CommandRunner,
    preview: &Plan,
    install: &[PackageId],
    remove: &[PackageId],
    confirmed: bool,
    canceled: &AtomicBool,
    journal: Option<&crate::journal::Journal>,
    managed: &crate::managed::ManagedGuard<'_>,
) -> Result<ExecutionRecord, Error> {
    managed.state.authorize_removals(preview, remove)?;
    let check = || managed.validate();
    apply_apt_guarded(
        runner,
        preview,
        install,
        remove,
        confirmed,
        canceled,
        journal,
        Some(&check),
    )
}
pub fn apply_saved_apt(
    runner: &dyn CommandRunner,
    saved: &crate::saved_plan::SavedPlan,
    confirmed: bool,
    canceled: &AtomicBool,
    journal: Option<&crate::journal::Journal>,
) -> Result<ExecutionRecord, Error> {
    if saved.provider != crate::ProviderId::Apt {
        return Err(Error::Unsupported(
            "saved-plan execution supports APT only".into(),
        ));
    }
    let check = || {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| Error::Safety("system clock predates Unix epoch".into()))?
            .as_secs();
        saved.validate_at(now)
    };
    apply_apt_guarded(
        runner,
        &saved.plan,
        &saved.install,
        &saved.remove,
        confirmed,
        canceled,
        journal,
        Some(&check),
    )
}
#[allow(clippy::too_many_arguments)]
fn apply_apt_guarded(
    runner: &dyn CommandRunner,
    preview: &Plan,
    install: &[PackageId],
    remove: &[PackageId],
    confirmed: bool,
    canceled: &AtomicBool,
    journal: Option<&crate::journal::Journal>,
    guard: Option<&dyn Fn() -> Result<(), Error>>,
) -> Result<ExecutionRecord, Error> {
    if let Some(check) = guard {
        check()?;
    }
    if canceled.load(std::sync::atomic::Ordering::Relaxed) {
        return Err(Error::Canceled("APT execution".into()));
    }
    if !confirmed {
        return Err(Error::Safety(
            "execution requires explicit confirmation".into(),
        ));
    }
    let backend = AptBackend { runner };
    let current = backend.plan(install, remove)?;
    if &current != preview {
        return Err(Error::Safety(
            "plan is stale; inspect a new plan before applying".into(),
        ));
    }
    if let Some(check) = guard {
        check()?;
    }
    if current.operations.is_empty() {
        return Ok(ExecutionRecord {
            success: true,
            native_status: None,
            verified: true,
            errors: vec![],
            rollback_attempted: false,
            observed_before: current.observed_state_fingerprint.clone(),
            observed_after: Some(current.observed_state_fingerprint.clone()),
        });
    }
    let mut spec = CommandSpec::new(
        "apt-get",
        &[
            "--yes",
            "-o",
            "APT::Get::allow-change-held-packages=false",
            "-o",
            "APT::Get::allow-remove-essential=false",
            "-o",
            "APT::Get::Purge=false",
            "-o",
            "APT::Get::force-yes=false",
            "-o",
            "APT::Get::allow-downgrades=false",
            "install",
            "--",
        ],
    );
    spec.requires_privilege = true;
    if remove.is_empty() {
        spec.args.insert(0, "--no-remove".into());
    }
    for id in install {
        let name = apt::native_name(id)?;
        // Pin requested packages to the version reviewed in the simulation.
        let action = current.operations.iter().find(|operation| {
            matches!(
                operation.kind,
                OperationKind::Install | OperationKind::MarkExplicit
            ) && operation.package.name == id.name
                && (id.architecture_or_scope.is_none()
                    || operation.package.architecture_or_scope.is_none()
                    || operation.package.architecture_or_scope == id.architecture_or_scope)
        });
        let argument = if let Some(action) = action {
            let version = planned_version(&action.native_detail)
                .ok_or_else(|| Error::Safety("cannot determine reviewed native version".into()))?;
            format!("{name}={version}")
        } else {
            let installed: Vec<_> = current
                .observed_packages
                .iter()
                .filter(|p| {
                    p.id.name == id.name
                        && id
                            .architecture_or_scope
                            .as_ref()
                            .is_none_or(|a| p.id.architecture_or_scope.as_ref() == Some(a))
                })
                .collect();
            if installed.len() != 1 {
                return Err(Error::Safety(
                    "cannot pin an already-installed request unambiguously".into(),
                ));
            }
            format!("{name}={}", installed[0].version)
        };
        spec.args.push(argument);
    }
    for id in remove {
        spec.args.push(format!("{}-", apt::native_name(id)?));
    }
    let journal_id = journal.map(|journal| journal.begin(&current)).transpose()?;
    if let Some(check) = guard {
        if let Err(error) = check() {
            let mut record = ExecutionRecord {
                success: false,
                native_status: None,
                verified: false,
                errors: vec![format!("{error}; native transaction was not started")],
                rollback_attempted: false,
                observed_before: current.observed_state_fingerprint.clone(),
                observed_after: None,
            };
            if let (Some(journal), Some(id)) = (journal, &journal_id) {
                if let Err(error) = journal.finish(id, &record) {
                    record
                        .errors
                        .push(format!("journal finalization failed: {error}"));
                }
            }
            return Ok(record);
        }
    }
    let output = runner.run(&spec, Duration::from_secs(1800), canceled);
    let mut errors = vec![];
    let native_status = match output {
        Ok(output) => {
            if output.status != Some(0) {
                errors.push(format!(
                    "native transaction failed with status {:?}; partial changes may remain",
                    output.status
                ));
            }
            output.status
        }
        Err(error) => {
            errors.push(format!("{error}; partial changes may remain"));
            None
        }
    };
    let mut observed_after = None;
    let verified = match backend.inventory() {
        Ok(observed) => {
            observed_after = Some(crate::planner::inventory_token(&observed));
            let matches = |id: &PackageId, p: &&crate::InstalledPackage| {
                p.id.provider == id.provider
                    && p.id.name == id.name
                    && id
                        .architecture_or_scope
                        .as_ref()
                        .is_none_or(|a| p.id.architecture_or_scope.as_ref() == Some(a))
            };
            let mut verified = true;
            for operation in &current.operations {
                match operation.kind {
                    OperationKind::Remove => {
                        if observed.iter().any(|p| matches(&operation.package, &p)) {
                            verified = false;
                        }
                    }
                    OperationKind::MarkExplicit => {
                        if !observed
                            .iter()
                            .filter(|p| matches(&operation.package, p))
                            .any(|p| p.install_reason == crate::InstallReason::Explicit)
                        {
                            verified = false;
                        }
                    }
                    OperationKind::Install | OperationKind::Configure => {
                        let expected = planned_version(&operation.native_detail);
                        if !observed
                            .iter()
                            .filter(|p| matches(&operation.package, p))
                            .any(|p| expected == Some(p.version.as_str()))
                        {
                            verified = false;
                        }
                    }
                }
            }
            let touched = |id: &PackageId| {
                current
                    .operations
                    .iter()
                    .any(|operation| operation.package == *id)
            };
            let untouched = |packages: &[crate::InstalledPackage]| {
                let mut rows: Vec<_> = packages
                    .iter()
                    .filter(|p| !touched(&p.id))
                    .map(|p| {
                        (
                            p.id.to_string(),
                            p.version.clone(),
                            format!("{:?}", p.install_reason),
                        )
                    })
                    .collect();
                rows.sort();
                rows
            };
            if untouched(&current.observed_packages) != untouched(&observed) {
                verified = false;
                errors.push("inventory changed outside reviewed package actions; concurrent or unexpected native effects require inspection".into());
            }
            if !verified {
                errors
                    .push("post-transaction inventory does not match every planned action".into());
            }
            verified
        }
        Err(error) => {
            errors.push(format!(
                "post-transaction verification unavailable: {error}"
            ));
            false
        }
    };
    let mut record = ExecutionRecord {
        success: native_status == Some(0) && verified,
        native_status,
        verified,
        errors,
        rollback_attempted: false,
        observed_before: current.observed_state_fingerprint.clone(),
        observed_after,
    };
    if let (Some(journal), Some(id)) = (journal, journal_id) {
        if let Err(error) = journal.finish(&id, &record) {
            record.success = false;
            record.errors.push(format!("transaction journal finalization failed: {error}; saved intent remains interrupted/unknown"));
        }
    }
    Ok(record)
}
fn planned_version(detail: &str) -> Option<&str> {
    let version = detail
        .split_once(" (")?
        .1
        .split_whitespace()
        .next()?
        .trim_end_matches(')');
    // Debian versions may contain digits, letters and . + - : ~ only. Never
    // forward backend text containing selectors or whitespace into native args.
    if version.is_empty()
        || !version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".+-:~".contains(&b))
    {
        return None;
    }
    Some(version)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, sync::Mutex};
    struct Mock {
        mutated: Mutex<bool>,
    }
    impl CommandRunner for Mock {
        fn run(
            &self,
            spec: &CommandSpec,
            _: Duration,
            _: &AtomicBool,
        ) -> Result<crate::command::CommandOutput, Error> {
            let output = match spec.program.as_str() {
                "dpkg-query" if spec.args.iter().any(|s| s.contains("Essential")) => {
                    "curl\tamd64\tno\tno\n"
                }
                "dpkg-query" => "curl\tamd64\t1\tinstalled\n",
                "apt-mark" if spec.args[0] == "showmanual" => "curl\n",
                "apt-mark" => "",
                "apt-get" if spec.args.contains(&"--simulate".into()) => {
                    "Inst curl (2 Debian [amd64])\n"
                }
                "apt-get" => {
                    *self.mutated.lock().unwrap() = true;
                    ""
                }
                _ => panic!("unexpected command"),
            };
            Ok(crate::command::CommandOutput {
                status: Some(0),
                stdout: output.as_bytes().to_vec(),
                stderr: vec![],
            })
        }
    }
    #[test]
    fn expiration_during_revalidation_or_journaling_prevents_native_execution() {
        for fail_at in [2, 3] {
            let runner = Mock {
                mutated: Mutex::new(false),
            };
            let requested = [apt::package_id("curl").unwrap()];
            let preview = AptBackend { runner: &runner }
                .plan(&requested, &[])
                .unwrap();
            let calls = Cell::new(0);
            let guard = || {
                calls.set(calls.get() + 1);
                if calls.get() >= fail_at {
                    Err(Error::Safety("expired".into()))
                } else {
                    Ok(())
                }
            };
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("journal");
            let journal = crate::journal::Journal::new(&path);
            let result = apply_apt_guarded(
                &runner,
                &preview,
                &requested,
                &[],
                true,
                &AtomicBool::new(false),
                Some(&journal),
                Some(&guard),
            );
            assert!(!*runner.mutated.lock().unwrap());
            if fail_at == 2 {
                assert!(result.is_err());
                assert!(!path.exists());
            } else {
                assert!(!result.unwrap().success);
                let history = journal.history().unwrap();
                assert!(history[0]
                    .result
                    .as_ref()
                    .is_some_and(|r| !r.success && r.native_status.is_none()));
            }
        }
    }
}
