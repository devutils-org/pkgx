use pkgx_core::{apt, planner::inventory_token, saved_plan::SavedPlan, Plan, ProviderId};
fn bundle() -> SavedPlan {
    let observed = apt::parse_inventory("curl\tamd64\t1\tinstalled\n").unwrap();
    SavedPlan::new(
        ProviderId::Apt,
        vec![apt::package_id("curl:amd64").unwrap()],
        vec![],
        Plan {
            operations: vec![],
            observed_state_fingerprint: inventory_token(&observed),
            observed_packages: observed,
            uncertain: false,
            warnings: vec![],
        },
        100,
        300,
    )
    .unwrap()
}
#[test]
fn saved_preview_round_trips_and_has_a_bounded_review_window() {
    let saved = bundle();
    let json = saved.to_json().unwrap();
    let loaded = SavedPlan::parse(&json, 101).unwrap();
    assert_eq!(saved.plan, loaded.plan);
    assert!(SavedPlan::parse(&json, 99).is_err());
    assert!(SavedPlan::parse(&json, 400).is_err());
    let mut saved = saved;
    saved.expires_unix_secs = 100 + 3601;
    assert!(saved.validate_at(100).is_err());
}
#[test]
fn malformed_foreign_and_executable_fields_are_rejected() {
    let mut value = serde_json::to_value(bundle()).unwrap();
    value["command"] = serde_json::json!("rm -rf /");
    assert!(SavedPlan::parse(&value.to_string(), 100).is_err());
    let mut value = serde_json::to_value(bundle()).unwrap();
    value["plan"]["shell"] = serde_json::json!("sh");
    assert!(SavedPlan::parse(&value.to_string(), 100).is_err());
    let mut value = serde_json::to_value(bundle()).unwrap();
    value["install"][0]["provider"] = serde_json::json!("pacman");
    assert!(SavedPlan::parse(&value.to_string(), 100).is_err());
    let mut value = serde_json::to_value(bundle()).unwrap();
    value["plan"]["observed_state_fingerprint"] = serde_json::json!("altered");
    assert!(SavedPlan::parse(&value.to_string(), 100).is_err());
}
#[test]
fn edited_saved_plans_are_not_authorized_by_serialization() {
    struct Never;
    impl pkgx_core::command::CommandRunner for Never {
        fn run(
            &self,
            _: &pkgx_core::command::CommandSpec,
            _: std::time::Duration,
            _: &std::sync::atomic::AtomicBool,
        ) -> Result<pkgx_core::command::CommandOutput, pkgx_core::Error> {
            panic!("expired plans must fail before any native command");
        }
    }
    assert!(pkgx_core::execution::apply_saved_apt(
        &Never,
        &bundle(),
        true,
        &std::sync::atomic::AtomicBool::new(false),
        None
    )
    .is_err());
}
