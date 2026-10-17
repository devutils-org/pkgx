use pkgx_core::{
    apt, audit, backend::Backend, Capability, Error, InstalledPackage, PackageId, ProviderId,
};
struct Mock {
    fail: bool,
    supports: bool,
}
impl Backend for Mock {
    fn provider(&self) -> ProviderId {
        ProviderId::Apt
    }
    fn capabilities(&self) -> Vec<Capability> {
        if self.supports {
            vec![Capability::Inventory, Capability::Orphans]
        } else {
            vec![Capability::Inventory]
        }
    }
    fn inventory(&self) -> Result<Vec<InstalledPackage>, Error> {
        apt::parse_inventory(include_str!("../../../tests/fixtures/dpkg.tsv"))
    }
    fn orphan_candidates(&self) -> Result<Vec<PackageId>, Error> {
        if self.fail {
            Err(Error::CommandFailed {
                program: "mock".into(),
                status: Some(100),
            })
        } else {
            Ok(vec![apt::package_id("curl:amd64")?])
        }
    }
}
#[test]
fn audit_proposes_review_and_never_claims_cleanup_authority() {
    let report = audit::inspect(&Mock {
        fail: false,
        supports: true,
    })
    .unwrap();
    assert_eq!(report.installed_count, 3);
    assert_eq!(report.findings.len(), 1);
    assert!(!report.findings[0].automatic_removal);
    assert!(report.findings[0].estimated_reclaimable_bytes.is_none());
    assert!(report
        .limitations
        .iter()
        .any(|s| s.contains("Unmanaged files")));
}
#[test]
fn query_failure_is_not_reported_as_a_clean_system() {
    assert!(audit::inspect(&Mock {
        fail: true,
        supports: true
    })
    .is_err());
}
#[test]
fn absent_capability_is_explicit_and_does_not_invoke_orphan_query() {
    let report = audit::inspect(&Mock {
        fail: true,
        supports: false,
    })
    .unwrap();
    assert!(report.findings.is_empty());
    assert!(report
        .limitations
        .iter()
        .any(|s| s.contains("No orphan-query")));
}

#[test]
fn residual_records_carry_no_file_removal_authority() {
    struct ResidualBackend;
    impl Backend for ResidualBackend {
        fn provider(&self) -> ProviderId {
            ProviderId::Apt
        }
        fn capabilities(&self) -> Vec<Capability> {
            vec![Capability::Inventory, Capability::ConfigInspection]
        }
        fn inventory(&self) -> Result<Vec<InstalledPackage>, Error> {
            Ok(vec![])
        }
        fn residual_configurations(&self) -> Result<Vec<PackageId>, Error> {
            Ok(vec![apt::package_id("old-tool:amd64")?])
        }
    }
    let report = audit::inspect(&ResidualBackend).unwrap();
    assert_eq!(
        report.findings[0].kind,
        "native_residual_configuration_record"
    );
    assert!(!report.findings[0].automatic_removal);
    assert!(report.findings[0].estimated_reclaimable_bytes.is_none());
}
