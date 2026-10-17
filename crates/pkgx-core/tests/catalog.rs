use pkgx_core::{
    backend::Backend, catalog, providers, Capability, Error, InstalledPackage, ProviderId,
};
struct InstalledOnly;
impl Backend for InstalledOnly {
    fn provider(&self) -> ProviderId {
        ProviderId::Pacman
    }
    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::Inventory]
    }
    fn inventory(&self) -> Result<Vec<InstalledPackage>, Error> {
        providers::parse_pacman(include_str!("../../../tests/fixtures/pacman-info.txt"))
    }
}
#[test]
fn installed_search_needs_no_repository_search_or_solver() {
    let by_description = catalog::search_installed(&InstalledOnly, "TRANSFER").unwrap();
    assert_eq!(by_description.len(), 1);
    assert_eq!(by_description[0].id.name, "curl");
    assert_eq!(
        catalog::search_installed(&InstalledOnly, "x86_64")
            .unwrap()
            .len(),
        2
    );
    assert!(catalog::search_installed(&InstalledOnly, "curl*")
        .unwrap()
        .is_empty());
    assert!(catalog::search_installed(&InstalledOnly, "\n").is_err());
}
