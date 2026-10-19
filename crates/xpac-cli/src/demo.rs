//! Synthetic read-only data: this module never invokes a native command.
use std::collections::BTreeMap;
use xpac_core::{
    backend::{Backend, PackageInfo, PackageProvenance},
    identity::parse_name,
    Capability, Error, InstallReason, InstalledPackage, PackageId, ProviderId,
};

pub const NOTICE: &str = "DEMO — synthetic packages, no native inspection or execution";
pub struct DemoBackend(pub ProviderId);
impl Backend for DemoBackend {
    fn provider(&self) -> ProviderId {
        self.0
    }
    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::Inventory]
    }
    fn inventory(&self) -> Result<Vec<InstalledPackage>, Error> {
        let entries: &[(&str, &str, InstallReason)] = match self.0 {
            ProviderId::Apt => &[
                ("curl:amd64", "8.0-demo", InstallReason::Explicit),
                ("libcurl4:amd64", "8.0-demo", InstallReason::Dependency),
                ("git:amd64", "2.0-demo", InstallReason::Explicit),
            ],
            ProviderId::Brew => &[("ripgrep:formula", "14.0-demo", InstallReason::Explicit)],
            ProviderId::Flatpak => &[(
                "app/org.example.Editor/x86_64/stable:user",
                "1.0-demo",
                InstallReason::Unknown,
            )],
            _ => &[],
        };
        entries
            .iter()
            .map(|(name, version, reason)| {
                Ok(InstalledPackage {
                    id: parse_name(self.0, name)?,
                    version: (*version).into(),
                    install_reason: *reason,
                    dependencies: vec![],
                    metadata: BTreeMap::from([
                        ("description".into(), NOTICE.into()),
                        ("dependencies_status".into(), "not_queried".into()),
                    ]),
                })
            })
            .collect()
    }
    fn info(&self, id: &PackageId) -> Result<PackageInfo, Error> {
        let package = self.find(id)?;
        Ok(PackageInfo {
            id: package.id,
            records: vec![BTreeMap::from([
                ("Description".into(), NOTICE.into()),
                ("Version".into(), package.version),
            ])],
        })
    }
    fn dependencies(
        &self,
        id: &PackageId,
    ) -> Result<xpac_core::dependency::DependencyReport, Error> {
        let mut report = xpac_core::dependency::DependencyReport::native(
            self.find(id)?,
            "synthetic demo; no native query",
        );
        report
            .limitations
            .push("Demo dependency relationships are not modeled.".into());
        Ok(report)
    }
    fn why(&self, id: &PackageId) -> Result<PackageProvenance, Error> {
        Ok(PackageProvenance {
            package: self.find(id)?,
            origin: None,
            confidence: "synthetic".into(),
            installed_reverse_dependencies: "Not queried in demo".into(),
            limitations: vec![NOTICE.into()],
        })
    }
}
impl DemoBackend {
    fn find(&self, id: &PackageId) -> Result<InstalledPackage, Error> {
        self.inventory()?
            .into_iter()
            .find(|p| p.id == *id)
            .ok_or_else(|| Error::InvalidInput("package is absent from demo inventory".into()))
    }
}
pub fn backends() -> Vec<Box<dyn Backend>> {
    [ProviderId::Apt, ProviderId::Brew, ProviderId::Flatpak]
        .into_iter()
        .map(|p| Box::new(DemoBackend(p)) as Box<dyn Backend>)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn synthetic_backends_inspect_but_never_plan() {
        for backend in backends() {
            for package in backend.inventory().unwrap() {
                assert_eq!(
                    backend.info(&package.id).unwrap().records[0]["Description"],
                    NOTICE
                );
                assert_eq!(backend.why(&package.id).unwrap().confidence, "synthetic");
                assert!(matches!(
                    backend.plan(&[package.id], &[]),
                    Err(Error::Unsupported(_))
                ));
            }
            assert!(matches!(
                backend.orphan_candidates(),
                Err(Error::Unsupported(_))
            ));
        }
    }
}
