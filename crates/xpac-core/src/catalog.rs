use crate::{
    backend::{Backend, SearchResult},
    Error, InstalledPackage,
};

fn description(package: &InstalledPackage) -> &str {
    ["Description", "Summary", "description"]
        .into_iter()
        .find_map(|key| package.metadata.get(key).map(String::as_str))
        .unwrap_or("")
}
/// Local literal filtering over one inventory snapshot. Never refreshes provider
/// caches or requires a repository-search capability.
pub fn filter_installed<'a>(
    packages: &'a [InstalledPackage],
    query: &str,
) -> Vec<&'a InstalledPackage> {
    let query = query.to_lowercase();
    packages
        .iter()
        .filter(|package| {
            format!(
                "{} {} {:?} {}",
                package.id,
                package.version,
                package.install_reason,
                description(package)
            )
            .to_lowercase()
            .contains(&query)
        })
        .collect()
}
pub fn search_installed(backend: &dyn Backend, query: &str) -> Result<Vec<SearchResult>, Error> {
    if query.is_empty() || query.len() > 256 || query.chars().any(char::is_control) {
        return Err(Error::InvalidInput(
            "installed search requires 1–256 printable characters".into(),
        ));
    }
    let packages = backend.inventory()?;
    Ok(filter_installed(&packages, query)
        .into_iter()
        .map(|package| SearchResult {
            id: package.id.clone(),
            summary: format!(
                "{} [{:?}] {}",
                package.version,
                package.install_reason,
                description(package)
            )
            .trim_end()
            .to_owned(),
        })
        .collect())
}
