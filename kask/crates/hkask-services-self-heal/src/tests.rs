use super::healer::SelfHealer;
use super::registry::HealRegistry;
use super::types::{DebugLogAction, HealAction, HealContext, HealOutcome, MiniDebugLog};
use std::path::PathBuf;

#[test]
fn registry_has_default_strategies() {
    assert!(HealRegistry::with_defaults().strategies.len() >= 4);
}

#[test]
fn find_api_key_strategy() {
    let r = HealRegistry::with_defaults();
    assert_eq!(
        r.find_strategy("No API key for classifier").unwrap().name,
        "missing-api-key"
    );
}

#[test]
fn find_permission_strategy() {
    assert_eq!(
        HealRegistry::with_defaults()
            .find_strategy("Permission denied (os error 13)")
            .unwrap()
            .name,
        "permission-denied"
    );
}

#[test]
fn find_network_strategy() {
    assert_eq!(
        HealRegistry::with_defaults()
            .find_strategy("connection refused")
            .unwrap()
            .name,
        "network-error"
    );
}

#[test]
fn find_transient_strategy() {
    let r = HealRegistry::with_defaults();
    assert!(
        r.find_strategy("request timed out after 30 seconds")
            .is_some()
    );
    assert!(r.find_strategy("HTTP 502 Bad Gateway").is_some());
}

#[test]
fn no_match_returns_none() {
    assert!(
        HealRegistry::with_defaults()
            .find_strategy("unknown XYZ")
            .is_none()
    );
}

#[test]
fn unmatched_returns_unhealable() {
    let h = SelfHealer::new();
    let o = h.attempt("unknown error", &HealContext::default());
    assert!(matches!(o, HealOutcome::Unhealable { .. }));
}

#[test]
fn api_key_strategy_loads_dotenv() {
    let h = SelfHealer::new();
    assert!(!matches!(
        h.attempt("No API key for classifier", &HealContext::default()),
        HealOutcome::Unhealable { .. }
    ));
}

#[test]
fn healable_retries_with_backoff() {
    use std::time::Instant;
    let h = SelfHealer::new();
    let mut calls = 0u32;
    let start = Instant::now();
    let r: Result<u32, &str> = h.healable(
        || {
            calls += 1;
            if calls < 3 { Err("timeout") } else { Ok(42) }
        },
        HealContext {
            operation: "test".into(),
            error_message: "timeout".into(),
            ..Default::default()
        },
    );
    assert!(r.is_ok());
    assert_eq!(calls, 3);
    assert!(start.elapsed().as_millis() >= 2900);
}

#[test]
fn healable_exhausted_returns_error() {
    assert!(
        SelfHealer::new()
            .healable(
                || Err::<u32, _>("connection refused"),
                HealContext::default()
            )
            .is_err()
    );
}

#[test]
fn debug_log_serializes() {
    let log = MiniDebugLog {
        attempt_count: 3,
        reg_spans: vec!["reg.heal.attempt".into()],
        modifications: vec!["Loaded .env".into()],
        actions_taken: vec![DebugLogAction {
            name: "x".into(),
            output: "ok".into(),
            success: true,
        }],
        suggestion: "fix".into(),
    };
    assert!(
        serde_json::to_string(&log)
            .unwrap()
            .contains("attempt_count")
    );
}

#[test]
fn llm_assisted_without_inference_errors() {
    let h = SelfHealer::new();
    assert!(
        h.execute_action(
            &HealAction::LlmAssisted {
                template_path: PathBuf::from("nonexistent.j2")
            },
            &HealContext::default()
        )
        .is_err()
    );
}

#[test]
fn heal_templates_exist_on_disk() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    for tpl in &[
        "missing_api_key.j2",
        "permission_denied.j2",
        "command_not_found.j2",
        "config_not_found.j2",
        "network_error.j2",
        "transient_retry.j2",
        "classify-error.j2",
    ] {
        assert!(
            root.join("registry/templates/heal").join(tpl).exists(),
            "Missing: {}",
            tpl
        );
    }
}
