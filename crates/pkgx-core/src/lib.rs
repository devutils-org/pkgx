pub mod apt;
pub mod audit;
pub mod backend;
pub mod catalog;
pub mod command;
pub mod dependency;
pub mod detection;
pub mod execution;
pub mod identity;
pub mod journal;
pub mod managed;
pub mod manifest;
pub mod planner;
pub mod privilege;
pub mod providers;
pub mod recipe;
pub mod saved_plan;
pub mod version;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderId {
    Apt,
    Pacman,
    Dnf,
    Flatpak,
    Brew,
    Script,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageId {
    pub provider: ProviderId,
    pub name: String,
    pub architecture_or_scope: Option<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallReason {
    Explicit,
    Dependency,
    Unknown,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstalledPackage {
    pub id: PackageId,
    pub version: String,
    pub install_reason: InstallReason,
    pub dependencies: Vec<PackageId>,
    pub metadata: std::collections::BTreeMap<String, String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Inventory,
    Search,
    Install,
    Remove,
    Dependencies,
    Orphans,
    DryRun,
    ConfigInspection,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Native,
    RequiresReview,
    Unsupported,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    Install,
    Remove,
    Configure,
    MarkExplicit,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Operation {
    pub kind: OperationKind,
    pub package: PackageId,
    pub risk: RiskLevel,
    pub native_detail: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub operations: Vec<Operation>,
    pub observed_state_fingerprint: String,
    #[serde(default)]
    pub observed_packages: Vec<InstalledPackage>,
    pub uncertain: bool,
    pub warnings: Vec<String>,
}
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot run {program}: {source}")]
    Io {
        program: String,
        source: std::io::Error,
    },
    #[error("{0} timed out")]
    Timeout(String),
    #[error("{0} canceled")]
    Canceled(String),
    #[error("{program} exited with status {status:?}")]
    CommandFailed {
        program: String,
        status: Option<i32>,
    },
    #[error("invalid provider output: {0}")]
    Parse(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("operation refused: {0}")]
    Safety(String),
    #[error("unsupported capability: {0}")]
    Unsupported(String),
    #[error("provider unavailable: {0}")]
    Unavailable(String),
}

pub use execution::ExecutionRecord;
pub use manifest::{DesiredState, Diff};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    NativeMetadata,
    DeclaredOnly,
    Unknown,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceOwnership {
    Unmanaged,
    DeclaredOnly,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedResource {
    pub resource: recipe::Resource,
    pub ownership: ResourceOwnership,
    pub confidence: Confidence,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservedState {
    pub platform: String,
    pub packages: Vec<InstalledPackage>,
}
