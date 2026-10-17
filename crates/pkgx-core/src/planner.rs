use crate::{
    backend::Backend,
    manifest::{DesiredState, Diff},
    Error, InstalledPackage, Plan,
};

/// Canonical comparison token, not a cryptographic signature or authorization.
pub fn inventory_token(observed: &[InstalledPackage]) -> String {
    let mut rows: Vec<_> = observed
        .iter()
        .map(|p| {
            format!(
                "{:?}:{}:{}={} ({:?})",
                p.id.provider,
                p.id.name,
                p.id.architecture_or_scope.as_deref().unwrap_or_default(),
                p.version,
                p.install_reason
            )
        })
        .collect();
    rows.sort();
    rows.join("\n")
}
pub fn plan_manifest(backend: &dyn Backend, desired: &DesiredState) -> Result<(Diff, Plan), Error> {
    desired.identities()?;
    if desired
        .providers()?
        .iter()
        .any(|p| *p != backend.provider())
    {
        return Err(Error::Unsupported(
            "single-backend planner cannot reconcile another provider".into(),
        ));
    }
    let observed = backend.inventory()?;
    let diff = desired.diff(&observed)?;
    let plan = if diff.missing.is_empty() {
        Plan {
            operations: vec![],
            observed_state_fingerprint: inventory_token(&observed),
            observed_packages: observed.clone(),
            uncertain: false,
            warnings: diff.warnings.clone(),
        }
    } else {
        backend.plan(&diff.missing, &[])?
    };
    Ok((diff, plan))
}

#[derive(Debug, serde::Serialize)]
pub struct ProviderPlan {
    pub provider: crate::ProviderId,
    pub plan: Plan,
}
#[derive(Debug, serde::Serialize)]
pub struct MultiPlan {
    pub diff: Diff,
    pub providers: Vec<ProviderPlan>,
}
/// Validate all providers before any executor is considered. Mixed-provider
/// execution is deliberately unsupported until each native executor is reviewed.
pub fn plan_manifest_multi(
    backends: &[&dyn Backend],
    desired: &DesiredState,
) -> Result<MultiPlan, Error> {
    if desired.system.mode == crate::manifest::SyncMode::Strict {
        return Err(Error::Safety(
            "strict planning requires a managed registry snapshot".into(),
        ));
    }
    plan_manifest_multi_managed(backends, desired, &crate::managed::ManagedState::default())
}
pub fn plan_manifest_multi_managed(
    backends: &[&dyn Backend],
    desired: &DesiredState,
    managed: &crate::managed::ManagedState,
) -> Result<MultiPlan, Error> {
    desired.identities()?;
    let mut observed = vec![];
    let mut providers = vec![];
    let wanted = desired.providers()?;
    for provider in &wanted {
        if backends
            .iter()
            .filter(|b| b.provider() == *provider)
            .count()
            != 1
        {
            return Err(Error::Unavailable(format!(
                "exactly one backend required for {provider}"
            )));
        }
    }
    for provider in wanted {
        let backend = *backends
            .iter()
            .find(|b| b.provider() == provider)
            .expect("validated backend");
        let inventory = backend.inventory()?;
        let mut subset = desired.clone();
        subset
            .packages
            .retain(|name, _| name == &provider.to_string());
        let diff = subset.diff_managed(&inventory, managed)?;
        let plan = if diff.missing.is_empty() && diff.removals.is_empty() {
            Plan {
                operations: vec![],
                observed_state_fingerprint: inventory_token(&inventory),
                observed_packages: inventory.clone(),
                uncertain: false,
                warnings: diff.warnings,
            }
        } else {
            backend.plan(&diff.missing, &diff.removals)?
        };
        if desired.system.mode == crate::manifest::SyncMode::Strict {
            managed.authorize_removals(&plan, &diff.removals)?;
        }
        observed.extend(inventory);
        providers.push(ProviderPlan { provider, plan });
    }
    Ok(MultiPlan {
        diff: desired.diff_managed(&observed, managed)?,
        providers,
    })
}
