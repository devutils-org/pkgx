use xpac_core::{apt, manifest::DesiredState};
const INVENTORY: &str = include_str!("../../../tests/fixtures/dpkg.tsv");
#[test]
fn export_round_trips_and_repeated_diff_is_noop() {
    let observed = apt::parse_inventory(INVENTORY).unwrap();
    let exported = DesiredState::export(&observed).unwrap();
    let state = DesiredState::parse(&exported).unwrap();
    let diff = state.diff(&observed).unwrap();
    assert!(diff.missing.is_empty());
    assert!(diff.undeclared.is_empty());
    assert_eq!(diff.present.len(), 3);
    assert_eq!(exported, DesiredState::export(&observed).unwrap());
}
#[test]
fn additive_diff_never_removes_undeclared_packages() {
    let state =
        DesiredState::parse("version = 1\n[packages.apt]\ninstall = ['git', 'curl']").unwrap();
    let diff = state
        .diff(&apt::parse_inventory(INVENTORY).unwrap())
        .unwrap();
    assert_eq!(diff.missing[0].name, "git");
    assert_eq!(diff.present[0].name, "curl");
    assert_eq!(diff.undeclared.len(), 2);
}
#[test]
fn architecture_is_part_of_matching() {
    let state =
        DesiredState::parse("version = 1\n[packages.apt]\ninstall = ['curl:arm64']").unwrap();
    assert_eq!(
        state
            .diff(&apt::parse_inventory(INVENTORY).unwrap())
            .unwrap()
            .missing
            .len(),
        1
    );
}
#[test]
fn invalid_and_unsafe_manifests_are_rejected() {
    for input in [
        "version = 2",
        "version = 1\nunknown = true",
        "version = 1\n[packages.apt]\ninstall = ['curl']\nremove = ['bash']",
        "version = 1\n[packages.brew]\ninstall = ['curl']",
        "version = 1\n[packages.apt]\ninstall = ['curl', 'curl']",
        "version = 1\n[packages.apt]\ninstall = ['$(id)']",
        "version = 1\n[packages.apt]\ninstall = ['apt:curl']",
        "version = 1\n[[scripts]]\nname = 'untrusted'",
    ] {
        assert!(DesiredState::parse(input).is_err(), "accepted {input}");
    }
}

struct InventoryOnlyBackend;
impl xpac_core::backend::Backend for InventoryOnlyBackend {
    fn provider(&self) -> xpac_core::ProviderId {
        xpac_core::ProviderId::Apt
    }
    fn capabilities(&self) -> Vec<xpac_core::Capability> {
        vec![xpac_core::Capability::Inventory]
    }
    fn inventory(&self) -> Result<Vec<xpac_core::InstalledPackage>, xpac_core::Error> {
        apt::parse_inventory(INVENTORY)
    }
}
#[test]
fn generic_planner_produces_stable_noop_without_native_solver() {
    let observed = apt::parse_inventory(INVENTORY).unwrap();
    let state = DesiredState::parse(&DesiredState::export(&observed).unwrap()).unwrap();
    let (_, first) = xpac_core::planner::plan_manifest(&InventoryOnlyBackend, &state).unwrap();
    let (_, second) = xpac_core::planner::plan_manifest(&InventoryOnlyBackend, &state).unwrap();
    assert_eq!(first, second);
    assert!(first.operations.is_empty());
    assert!(!first.uncertain);
}
#[test]
fn generic_planner_preserves_unsupported_backend_capabilities() {
    let state = DesiredState::parse("version = 1\n[packages.apt]\ninstall = ['missing']").unwrap();
    assert!(matches!(
        xpac_core::planner::plan_manifest(&InventoryOnlyBackend, &state),
        Err(xpac_core::Error::Unsupported(_))
    ));
}

#[test]
fn profiles_compose_deterministically_and_platforms_are_explicit() {
    let input = "version = 1\n[packages.apt]\ninstall = ['curl']\n[platforms.macos.packages.brew]\ninstall = ['ripgrep:formula']\n[platforms.linux.packages.pacman]\ninstall = ['git']\n[profiles.base.packages.apt]\ninstall = ['git', 'curl']\n[profiles.dev]\nextends = ['base']\n[profiles.dev.packages.apt]\ninstall = ['build-essential']\n";
    let state = DesiredState::parse(input).unwrap();
    assert!(state.identities().is_err());
    let resolved = state
        .resolve("linux", &["dev".into(), "base".into()])
        .unwrap();
    assert_eq!(
        resolved.packages["apt"].install,
        ["build-essential", "curl", "git"]
    );
    assert!(!resolved.packages.contains_key("brew"));
    assert_eq!(
        resolved
            .resolution_notes
            .iter()
            .filter(|s| s.contains("Included profile: base"))
            .count(),
        1
    );
    assert!(resolved
        .resolution_notes
        .iter()
        .any(|s| s.contains("Skipped platform section: macos")));
    assert_eq!(resolved.identities().unwrap().len(), 4);
}
#[test]
fn cycles_unknown_profiles_and_inactive_invalid_fields_fail_closed() {
    assert!(DesiredState::parse(
        "version = 1\n[profiles.one]\nextends = ['two']\n[profiles.two]\nextends = ['one']"
    )
    .is_err());
    assert!(DesiredState::parse("version = 1\n[profiles.one]\nextends = ['missing']").is_err());
    assert!(DesiredState::parse("version = 1\n[platforms.macos.packages.brew]\ninstall = ['curl:formula']\ncommand = 'oops'").is_err());
    assert!(DesiredState::parse("version = 1")
        .unwrap()
        .resolve("linux", &["missing".into()])
        .is_err());
}
#[test]
fn mixed_provider_diff_never_matches_only_by_name() {
    let state = DesiredState::parse("version = 1\n[packages.pacman]\ninstall = ['curl']").unwrap();
    let diff = state
        .diff(&apt::parse_inventory(INVENTORY).unwrap())
        .unwrap();
    assert_eq!(diff.missing.len(), 1);
    assert!(diff.present.is_empty());
}

#[cfg(unix)]
#[test]
fn export_does_not_follow_a_destination_symlink() {
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("keep");
    let link = temp.path().join("link");
    std::fs::write(&target, b"original").unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();
    assert!(xpac_core::manifest::write_export(&link, "replacement").is_err());
    assert_eq!(std::fs::read(target).unwrap(), b"original");
}

#[test]
fn multi_provider_preflight_rejects_missing_or_duplicate_backend_before_query() {
    let state = DesiredState::parse(
        "version = 1\n[packages.apt]\ninstall = ['curl']\n[packages.pacman]\ninstall = ['curl']",
    )
    .unwrap();
    // Mock backend inventory has no native calls, and missing provider is caught
    // before querying it or constructing an executable operation.
    assert!(matches!(
        xpac_core::planner::plan_manifest_multi(&[&InventoryOnlyBackend], &state),
        Err(xpac_core::Error::Unavailable(_))
    ));
    let apt_only = DesiredState::parse("version = 1\n[packages.apt]\ninstall = ['curl']").unwrap();
    assert!(matches!(
        xpac_core::planner::plan_manifest_multi(
            &[&InventoryOnlyBackend, &InventoryOnlyBackend],
            &apt_only
        ),
        Err(xpac_core::Error::Unavailable(_))
    ));
}
#[test]
fn packages_named_like_their_provider_round_trip_through_export() {
    // Found by the disposable Arch root: the `pacman` package itself was
    // mistaken for a redundant provider prefix and broke export -> diff.
    let mut observed = apt::parse_inventory("apt\tamd64\t1\tinstalled\n").unwrap();
    observed.extend(
        xpac_core::providers::parse_pacman(
            "Name            : pacman\nVersion         : 7.1.0-1\nArchitecture    : x86_64\nInstall Reason  : Explicitly installed\n",
        )
        .unwrap(),
    );
    let exported = DesiredState::export(&observed).unwrap();
    let state = DesiredState::parse(&exported).unwrap();
    let diff = state.diff(&observed).unwrap();
    assert!(diff.missing.is_empty());
    assert_eq!(diff.present.len(), 2);
    let prefixed =
        DesiredState::parse("version = 1\n[packages.pacman]\ninstall = ['pacman:curl:x86_64']");
    assert!(prefixed.is_err_and(|e| e.to_string().contains("provider prefix")));
}
