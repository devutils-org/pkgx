use crate::{Capability, Error, InstalledPackage, PackageId, Plan, ProviderId};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct SearchResult {
    pub id: PackageId,
    pub summary: String,
}
#[derive(Debug, Serialize)]
pub struct PackageInfo {
    pub id: PackageId,
    /// Native control records; dependency alternatives and constraints are preserved.
    pub records: Vec<std::collections::BTreeMap<String, String>>,
}
#[derive(Debug, Serialize)]
pub struct PackageProvenance {
    pub package: InstalledPackage,
    pub origin: Option<String>,
    pub confidence: String,
    pub installed_reverse_dependencies: String,
    pub limitations: Vec<String>,
}
pub trait Backend {
    fn provider(&self) -> ProviderId;
    fn capabilities(&self) -> Vec<Capability>;
    fn inventory(&self) -> Result<Vec<InstalledPackage>, Error>;
    fn search(&self, _query: &str) -> Result<Vec<SearchResult>, Error> {
        Err(Error::Unsupported("search".into()))
    }
    fn info(&self, _package: &PackageId) -> Result<PackageInfo, Error> {
        Err(Error::Unsupported("info".into()))
    }
    fn dependencies(
        &self,
        _package: &PackageId,
    ) -> Result<crate::dependency::DependencyReport, Error> {
        Err(Error::Unsupported("installed dependency query".into()))
    }
    fn why(&self, _package: &PackageId) -> Result<PackageProvenance, Error> {
        Err(Error::Unsupported("why".into()))
    }
    fn residual_configurations(&self) -> Result<Vec<PackageId>, Error> {
        Err(Error::Unsupported("residual configuration query".into()))
    }
    fn orphan_candidates(&self) -> Result<Vec<PackageId>, Error> {
        Err(Error::Unsupported("orphan query".into()))
    }
    fn plan(&self, _install: &[PackageId], _remove: &[PackageId]) -> Result<Plan, Error> {
        Err(Error::Unsupported("dry_run".into()))
    }
}
