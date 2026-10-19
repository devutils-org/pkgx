//! Native dependency expressions are not a fabricated, cross-provider graph.
use crate::{Error, InstalledPackage, PackageId};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct DependencyExpression {
    pub relation: String,
    pub expression: String,
}
#[derive(Debug, Serialize)]
pub struct DependencyReport {
    pub package: PackageId,
    pub installed_version: String,
    pub source: String,
    pub expressions: Vec<DependencyExpression>,
    /// False even when the expressions happen to look like concrete names.
    pub resolved_graph: bool,
    pub limitations: Vec<String>,
}
impl DependencyReport {
    pub fn native(package: InstalledPackage, source: &str) -> Self {
        Self {
            package: package.id,
            installed_version: package.version,
            source: source.into(),
            expressions: vec![],
            resolved_graph: false,
            limitations: vec!["Native expressions preserve alternatives, constraints and virtual capabilities; they do not identify resolved installed edges or historical installation causes.".into()],
        }
    }
    pub fn push(&mut self, relation: &str, expression: &str) {
        if !expression.trim().is_empty() {
            self.expressions.push(DependencyExpression {
                relation: relation.into(),
                expression: expression.into(),
            });
        }
    }
}
/// Scope-free lookup is permitted only when exactly one installed version matches.
pub fn installed_match(
    packages: Vec<InstalledPackage>,
    id: &PackageId,
) -> Result<InstalledPackage, Error> {
    crate::identity::validate(id)?;
    let mut matches = packages.into_iter().filter(|p| {
        p.id.provider == id.provider
            && p.id.name == id.name
            && id
                .architecture_or_scope
                .as_ref()
                .is_none_or(|scope| p.id.architecture_or_scope.as_ref() == Some(scope))
    });
    let package = matches
        .next()
        .ok_or_else(|| Error::Unavailable("package is not installed".into()))?;
    if matches.next().is_some() {
        return Err(Error::InvalidInput("installed identity is ambiguous; specify architecture/scope; multiple installed versions cannot be attributed individually".into()));
    }
    Ok(package)
}
