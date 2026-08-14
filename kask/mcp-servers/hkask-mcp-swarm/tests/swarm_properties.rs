//! Property tests for the public surface of `hkask-mcp-swarm`.
//!
//! These replace the deleted stub-based dispatch tests (StubInferencePort,
//! StubToolDispatch, StubSkillExec, ToolCallingInferencePort, OllamaInferencePort,
//! MockAbw). The stubs existed to drive *async tool-dispatch* paths; this file
//! instead property-tests the *pure* behavior reachable through the crate's
//! public API using proptest + the shared oracle taxonomy.
//!
//! ## Reachable surface
//! Only a small part of the crate is `pub`: `LocalAgentCard` (and its
//! sub-structs), `LocalAgentRegistry`, and `SwarmError`. The sanitizers
//! (`sanitize_*`), ABW utilities (`detect_embedded_error`,
//! `url_encode_segment`, `make_swarm_slug`, `validate_agent_name`, …), config
//! parsing (`SwarmMode::from_str`, `resolve_local_agents_dir`), and consent
//! (`ConsentStore`, `fnv1a`, `mint_token`) are `pub(crate)` and are NOT
//! re-exported, so they cannot be reached from an integration test without a
//! source change (see the "Gaps" section in the PR description). They remain
//! covered by their inline `#[cfg(test)] mod tests` blocks.
//!
//! Notably, `LocalAgentRegistry::write_card` calls `sanitize_agent_id`
//! internally, so the path-containment property below exercises the sanitizer
//! — the security-relevant behavior — through the public seam.

use hkask_mcp_swarm::test_utils::{LocalAgentDependencies, SwarmError};
use hkask_mcp_swarm::{LocalAgentCapabilities, LocalAgentCard, LocalAgentRegistry};
use hkask_test_harness::{OracleVerdict, oracle_invariant, oracle_reference};
use proptest::prelude::*;
use serde_json::Value as JsonValue;

// ── Strategies ─────────────────────────────────────────────────────────────

/// Short arbitrary strings (any UTF-8, bounded length) — exercises unicode,
/// path separators, control chars, and injection-shaped payloads.
fn arb_short_string(max: usize) -> impl Strategy<Value = String> {
    prop::collection::vec(any::<char>(), 0..max).prop_map(|cs| cs.into_iter().collect())
}

/// A vector of short identifier-ish strings.
fn arb_string_vec(max_fields: usize, max_len: usize) -> impl Strategy<Value = Vec<String>> {
    prop::collection::vec(arb_short_string(max_len), 0..max_fields)
}

/// Arbitrary `LocalAgentDependencies` (both fields `#[serde(default)]`).
fn arb_dependencies() -> BoxedStrategy<LocalAgentDependencies> {
    (arb_string_vec(6, 24), arb_string_vec(6, 24))
        .prop_map(|(required, optional)| LocalAgentDependencies { required, optional })
        .boxed()
}

/// Arbitrary `LocalAgentCapabilities` (all fields `#[serde(default)]`).
fn arb_capabilities() -> BoxedStrategy<LocalAgentCapabilities> {
    (
        arb_short_string(32),
        arb_short_string(16),
        prop::option::of(arb_short_string(128)),
        arb_string_vec(8, 32),
        arb_string_vec(8, 32),
    )
        .prop_map(
            |(model, min_provider_class, system_prompt, mcp_tools, skills)| {
                LocalAgentCapabilities {
                    model,
                    min_provider_class,
                    system_prompt,
                    mcp_tools,
                    skills,
                }
            },
        )
        .boxed()
}

/// An arbitrary `LocalAgentCard`. `agent_id`/`agent_type` are required (no
/// serde default); the rest mirror the card's `#[serde(default)]` fields.
/// Sub-structs are generated via their own strategies so the outer tuple stays
/// within proptest's 12-element `Strategy` arity.
fn arb_local_agent_card() -> BoxedStrategy<LocalAgentCard> {
    let agent_id = arb_short_string(24);
    let agent_type = arb_short_string(24);
    let description = arb_short_string(64);
    let accepts = arb_string_vec(6, 24);
    let produces = arb_string_vec(6, 24);
    let deps = arb_dependencies();
    let caps = arb_capabilities();
    let cloud_id = prop::option::of(arb_short_string(24));
    let tags = arb_string_vec(6, 24);
    let visibility = arb_short_string(16);

    (
        agent_id,
        agent_type,
        description,
        accepts,
        produces,
        deps,
        caps,
        cloud_id,
        tags,
        visibility,
    )
        .prop_map(
            |(
                agent_id,
                agent_type,
                description,
                accepts,
                produces,
                dependencies,
                capabilities,
                cloud_id,
                tags,
                visibility,
            )| {
                LocalAgentCard {
                    agent_id,
                    agent_type,
                    description,
                    accepts,
                    produces,
                    dependencies,
                    capabilities,
                    cloud_id,
                    tags,
                    visibility,
                    ..Default::default()
                }
            },
        )
        .boxed()
}

// ── 1. LocalAgentCard serde round-trip ────────────────────────────────────

proptest! {
    /// For any `LocalAgentCard`, serializing → deserializing → re-serializing
    /// must reproduce the original JSON exactly. This pins the serde impl's
    /// totality (never panics on any field contents, including unicode,
    /// injection text, and empty strings) and round-trip fidelity.
    ///
    /// Oracle: reference (identity on the serialized JSON value).
    #[test]
    fn prop_local_agent_card_serde_round_trips(card in arb_local_agent_card()) {
        let original = serde_json::to_value(&card).expect("serialize must not fail");
        let json = serde_json::to_string(&card).expect("serialize to string must not fail");
        let back: LocalAgentCard = serde_json::from_str(&json).expect("deserialize must not fail");
        let round_tripped = serde_json::to_value(&back).expect("re-serialize must not fail");

        let oracle = oracle_reference(|input: &JsonValue| input.clone());
        prop_assert_eq!(oracle.verify(&original, &round_tripped), OracleVerdict::Pass);
    }
}

// ── 2. LocalAgentRegistry path-containment (exercises sanitize_agent_id) ───

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// For any `agent_id` string, `LocalAgentRegistry::write_card` must either
    /// return an `Err` (when the id has no safe characters) or write the card
    /// under a path contained within the registry root — never outside it, and
    /// never panic. The path-containment invariant is the security property
    /// `sanitize_agent_id` exists to enforce (it strips `/`, rejects `.`/`..`),
    /// tested here through the only public seam that calls it.
    ///
    /// Oracle: invariant (Ok ⇒ path under root; always total).
    #[test]
    fn prop_write_card_path_is_contained_under_root(
        agent_id in arb_short_string(24),
        agent_type in arb_short_string(16),
    ) {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let root = tmp.path().canonicalize().expect("canonicalize temp root");
        let registry = LocalAgentRegistry::new(root.to_string_lossy().to_string());

        let card = LocalAgentCard {
            agent_id,
            agent_type,
            description: String::new(),
            accepts: Vec::new(),
            produces: Vec::new(),
            dependencies: LocalAgentDependencies::default(),
            capabilities: LocalAgentCapabilities::default(),
            cloud_id: None,
            ..Default::default()
        };

        let input = serde_json::json!({
            "agent_id": card.agent_id,
            "root": root.to_string_lossy(),
        });
        let result = registry.write_card(&card);
        let output = match &result {
            Ok(path) => serde_json::json!({ "ok": true, "path": path }),
            Err(msg) => serde_json::json!({ "ok": false, "error": msg.to_string() }),
        };

        let oracle = oracle_invariant(|input: &JsonValue, output: &JsonValue| {
            let ok = output.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
            if !ok {
                // An Err is always acceptable (e.g. an all-symbol agent_id that
                // sanitizes to empty). The totality (no panic) is enforced by
                // proptest itself — a panic fails the case.
                return Ok(());
            }
            let path = output
                .get("path")
                .and_then(|v| v.as_str())
                .expect("ok result must carry a path");
            let root = input
                .get("root")
                .and_then(|v| v.as_str())
                .expect("input must carry the registry root");
            let canonical = std::path::Path::new(path)
                .canonicalize()
                .map_err(|e| format!("written path failed to canonicalize: {e}"))?;
            if !canonical.starts_with(root) {
                return Err(format!(
                    "path-containment violated: written path {canonical:?} escapes registry root {root:?}"
                ));
            }
            Ok(())
        });
        prop_assert_eq!(oracle.verify(&input, &output), OracleVerdict::Pass);
    }
}

// ── 3. write_card ⇒ list/get consistency ───────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    /// After a successful `write_card`, `get(agent_id)` must return a card
    /// whose `agent_id` matches, and `list()` must include a card with that
    /// `agent_id`. The registry re-reads from disk on every call, so this pins
    /// persistence + serde together, not just in-memory identity.
    ///
    /// Oracle: invariant (Ok ⇒ get/list consistent with the written agent_id).
    #[test]
    fn prop_write_card_then_get_list_consistent(card in arb_local_agent_card()) {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let registry = LocalAgentRegistry::new(tmp.path().to_string_lossy().to_string());

        let written = registry.write_card(&card);
        let input = serde_json::json!({ "agent_id": card.agent_id });
        let output = match &written {
            Ok(_) => {
                let got = registry.get(&card.agent_id);
                let listed = registry.list();
                serde_json::json!({
                    "ok": true,
                    "got_agent_id": got.as_ref().map(|c| c.agent_id.clone()),
                    "list_contains": listed.iter().any(|c| c.agent_id == card.agent_id),
                })
            }
            Err(msg) => serde_json::json!({ "ok": false, "error": msg.to_string() }),
        };

        let oracle = oracle_invariant(|input: &JsonValue, output: &JsonValue| {
            let ok = output.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
            if !ok {
                return Ok(());
            }
            let expected_id = input
                .get("agent_id")
                .and_then(|v| v.as_str())
                .expect("input carries agent_id");
            let got_id = output.get("got_agent_id").and_then(|v| v.as_str());
            let listed = output
                .get("list_contains")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            match got_id {
                Some(id) if id == expected_id => {}
                other => {
                    return Err(format!(
                        "get(agent_id) returned {other:?}, expected {expected_id:?}"
                    ));
                }
            }
            if !listed {
                return Err("list() did not contain the written agent_id".to_string());
            }
            Ok(())
        });
        prop_assert_eq!(oracle.verify(&input, &output), OracleVerdict::Pass);
    }
}

// ── 4. Missing-dir load is total and empty ─────────────────────────────────

proptest! {
    /// `LocalAgentRegistry::load` on a non-existent directory must return
    /// `Ok(0)` and yield an empty `list()` / `None` from `get` — never an
    /// error and never a panic. The registry's subdir path is derived from an
    /// arbitrary suffix under a temp dir (never created), so each case targets
    /// a fresh missing path.
    #[test]
    fn prop_load_missing_dir_is_ok_zero_and_empty(suffix in arb_short_string(16)) {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        // Reduce the suffix to a path-safe segment so it doesn't accidentally
        // exist or escape the temp dir; the point is "a path that doesn't exist".
        let safe: String = suffix
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        prop_assume!(!safe.is_empty());
        let missing = tmp.path().join(&safe);
        prop_assume!(!missing.exists());

        let registry = LocalAgentRegistry::new(missing.to_string_lossy().to_string());
        let count = registry.load().expect("load on missing dir must be Ok");
        prop_assert_eq!(count, 0, "missing dir must load zero cards");
        prop_assert!(registry.list().is_empty(), "list() must be empty");
        prop_assert!(
            registry.get("anything").is_none(),
            "get() must be None on a missing dir"
        );
    }
}

// ── 5. SwarmError Display + into_tool_error totality ───────────────────────

proptest! {
    /// For any string payload, constructing each `SwarmError` variant and
    /// calling both `.to_string()` (thiserror Display) and `.into_tool_error()`
    /// must never panic, and the Display output must be non-empty. This pins
    /// the error surface's totality — the panel and tools rely on these never
    /// failing on arbitrary upstream/ABW text.
    ///
    /// Oracle: invariant (non-empty Display, no panic — the latter enforced by
    /// proptest itself).
    #[test]
    fn prop_swarm_error_display_is_total_and_nonempty(
        m in arb_short_string(64),
        agent in arb_short_string(32),
        provider in arb_short_string(32),
    ) {
        let variants = vec![
            SwarmError::Auth(m.clone()),
            SwarmError::PaymentRequired(m.clone()),
            SwarmError::AgentNotFunded { agent, message: m.clone() },
            SwarmError::UpstreamModelError { provider, message: m.clone() },
            SwarmError::RateLimited(m.clone()),
            SwarmError::CuratorUnavailable(m.clone()),
            SwarmError::ApiVersionMismatch(m.clone()),
            SwarmError::ConsentDenied(m.clone()),
            SwarmError::Unavailable(m),
        ];

        let input = serde_json::json!({});
        let displays: Vec<String> = variants.iter().map(|e| e.to_string()).collect();
        let kinds: Vec<String> = variants
            .into_iter()
            .map(|e| e.into_tool_error().to_string())
            .collect();
        let output = serde_json::json!({
            "displays": displays,
            "kinds": kinds,
        });

        let oracle = oracle_invariant(|_input: &JsonValue, output: &JsonValue| {
            let displays = output.get("displays").and_then(|v| v.as_array()).unwrap();
            let kinds = output.get("kinds").and_then(|v| v.as_array()).unwrap();
            if displays.len() != 9 || kinds.len() != 9 {
                return Err(format!(
                    "expected 9 of each, got {} displays / {} kinds",
                    displays.len(),
                    kinds.len()
                ));
            }
            for (i, d) in displays.iter().enumerate() {
                let s = d.as_str().unwrap_or("");
                if s.is_empty() {
                    return Err(format!("Display for variant {i} is empty"));
                }
            }
            for (i, k) in kinds.iter().enumerate() {
                if k.as_str().unwrap_or("").is_empty() {
                    return Err(format!("into_tool_error Display for variant {i} is empty"));
                }
            }
            Ok(())
        });
        prop_assert_eq!(oracle.verify(&input, &output), OracleVerdict::Pass);
    }
}
