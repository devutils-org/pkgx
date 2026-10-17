use crate::{backend::Backend, Capability, Error, InstallReason, PackageId, ProviderId, RiskLevel};
use serde::Serialize;
#[derive(Debug, Serialize)]
pub struct AuditFinding {
    pub package: PackageId,
    pub kind: String,
    pub risk: RiskLevel,
    pub confidence: String,
    pub estimated_reclaimable_bytes: Option<u64>,
    pub evidence: String,
    pub automatic_removal: bool,
}
#[derive(Debug, Serialize)]
pub struct AuditReport {
    pub provider: ProviderId,
    pub installed_count: usize,
    pub unknown_reason_count: usize,
    pub findings: Vec<AuditFinding>,
    pub limitations: Vec<String>,
}
/// No paths are scanned, no native cleanup is invoked and no executor is exposed.
pub fn inspect(backend: &dyn Backend) -> Result<AuditReport, Error> {
    let inventory = backend.inventory()?;
    let mut report = AuditReport { provider: backend.provider(), installed_count: inventory.len(), unknown_reason_count: inventory.iter().filter(|p| p.install_reason == InstallReason::Unknown).count(), findings: vec![], limitations: vec!["Native orphan status is a current dependency suggestion, not proof that removal is safe for the user.".into(), "Unmanaged files, home data and shared resources are not scanned or deleted. Reclaimable space is unknown.".into()] };
    if backend.capabilities().contains(&Capability::Orphans) {
        // Native query errors are reported as errors, never an empty successful audit.
        for id in backend.orphan_candidates()? {
            if !inventory.iter().any(|p| {
                p.id.provider == id.provider
                    && p.id.name == id.name
                    && id
                        .architecture_or_scope
                        .as_ref()
                        .is_none_or(|a| p.id.architecture_or_scope.as_ref() == Some(a))
            }) {
                return Err(Error::Parse(
                    "orphan candidate absent from observed inventory".into(),
                ));
            }
            report.findings.push(AuditFinding { package: id, kind: "native_orphan_candidate".into(), risk: RiskLevel::RequiresReview, confidence: "native_metadata".into(), estimated_reclaimable_bytes: None, evidence: match backend.provider() { ProviderId::Apt => "APT autoremove simulation, checked against holds and Essential/Protected metadata", ProviderId::Pacman => "pacman -Qdtq excludes packages required or optionally required by installed packages", ProviderId::Brew => "Homebrew dependency leaves, including native dependency-mark validation", _ => "native orphan query" }.into(), automatic_removal: false });
        }
    } else {
        report
            .limitations
            .push("No orphan-query capability implemented for this provider.".into());
    }
    if backend
        .capabilities()
        .contains(&Capability::ConfigInspection)
    {
        for id in backend.residual_configurations()? {
            if id.provider != backend.provider() {
                return Err(Error::Parse(
                    "residual query returned another provider".into(),
                ));
            }
            report.findings.push(AuditFinding { package: id, kind: "native_residual_configuration_record".into(), risk: RiskLevel::RequiresReview, confidence: "native_metadata".into(), estimated_reclaimable_bytes: None, evidence: "Native config-files status attributes a residual package record; actual paths, contents and ownership are not inspected.".into(), automatic_removal: false });
        }
        report.limitations.push("Residual records do not prove ownership of current configuration files; no purge or filesystem deletion is supported.".into());
    }
    Ok(report)
}
