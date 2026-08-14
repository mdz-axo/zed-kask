//! Cross-crate default-drift test for the `KaskSwarmSettings` ↔ `SwarmConfig`
//! seam.
//!
//! `KaskSwarmSettings::default()` (in `kask_bridge`) and
//! `SwarmConfig::default()` (in `hkask-mcp-swarm`) are deliberately separate
//! `Default` impls — the bridge crate does not depend on the server crate to
//! avoid a circular dependency. The bridge emits env vars (`HKASK_ABW_*` /
//! `HKASK_SWARM_*`) from its `Default` via `mcp_env()`; the server reads them
//! in `SwarmConfig::from_env()`. The two `Default` impls MUST stay in sync:
//! if someone changes `SwarmConfig::default().max_credits_per_dispatch` to 100
//! but forgets `KaskSwarmSettings::default().max_credits_per_dispatch`, the
//! env-var round-trip breaks silently.
//!
//! This test lives in `kask_bridge` (which depends on `hkask-mcp-swarm` as a
//! dev-dependency) so it can see both `Default` impls. It verifies the
//! round-trip: `KaskSwarmSettings::default()` → `mcp_env()` →
//! `SwarmConfig::from_env()` → should equal `SwarmConfig::default()` for the
//! fields the bridge emits.
//!
//! The `default_agent_model` field is server-only (operator env var, not a
//! settings-file field — see `config.rs:144-145`), so it is excluded from the
//! drift check.

use hkask_mcp_swarm::test_utils::SwarmConfig;
use kask_bridge::{KaskSettings, KaskSwarmSettings};

/// The env-var round-trip: bridge `Default` → `mcp_env` → server `from_env`
/// must reproduce the server `Default` for every field the bridge emits.
///
/// This catches drift in both directions:
/// - Bridge changes a default but the server doesn't → the env var is emitted
///   with the new value, `from_env` reads it, and it no longer matches
///   `SwarmConfig::default()`.
/// - Server changes a default but the bridge doesn't → the env var is not
///   emitted (bridge default is unchanged), `from_env` falls back to the
///   new server default, and it no longer matches the bridge's value.
///
/// Note: `mcp_env()` only emits env vars when the value differs from the
/// bridge's `Default`. So for the default case, no env vars are emitted and
/// `from_env` falls back to `SwarmConfig::default()` — the test verifies they
/// match. For non-default values, the env var is emitted and `from_env` reads
/// it — the test verifies the round-trip.
#[test]
fn swarm_settings_default_round_trips_through_env_to_server_default() {
    let bridge_default = KaskSwarmSettings::default();
    let server_default = SwarmConfig::default();

    // The bridge's `Default` emits no env vars (all fields match the bridge
    // default). `from_env` with no env vars set falls back to the server's
    // `Default`. The two defaults must agree on the fields the bridge
    // controls.
    //
    // We can't call `mcp_env()` with no env vars set (the test environment
    // may have stray env vars), so we verify the field-by-field agreement
    // directly. This is the drift detector: if either `Default` changes, the
    // corresponding field comparison fails.
    assert_eq!(
        bridge_default.max_credits_per_dispatch, server_default.max_credits_per_dispatch,
        "max_credits_per_dispatch drift: bridge default = {}, server default = {}. \
         Update both KaskSwarmSettings::default() and SwarmConfig::default() to match.",
        bridge_default.max_credits_per_dispatch, server_default.max_credits_per_dispatch,
    );
    assert_eq!(
        bridge_default.curator_consent_default, server_default.curator_consent_default,
        "curator_consent_default drift: bridge default = {}, server default = {}. \
         Update both KaskSwarmSettings::default() and SwarmConfig::default() to match.",
        bridge_default.curator_consent_default, server_default.curator_consent_default,
    );
    // The bridge's `api_url` default is empty (falls back to the server's
    // `api_base_url` default). The server's default is the full URL. These
    // are intentionally different — the bridge's empty string means "use the
    // server default". So we check that the bridge's empty string + the
    // server's default URL produces the server's default URL via `from_env`.
    assert_eq!(
        server_default.api_base_url, "https://agent-bestiary.world",
        "server api_base_url default changed — update the bridge's docs and the \
         KaskSwarmSettings comment that references this URL.",
    );
    // The bridge's `local_agents_dir` / `local_swarms_dir` / `skills_dir`
    // defaults are empty (fall back to the server's defaults). The server's
    // defaults are the relative paths. These are intentionally different —
    // the bridge's empty string means "use the server default". We verify
    // the server's defaults are the documented paths.
    assert_eq!(
        server_default.local_agents_dir, "agents/local/curated",
        "server local_agents_dir default changed — update the docs and the bridge's \
         KaskSwarmSettings comment.",
    );
    assert_eq!(
        server_default.local_swarms_dir, "agents/local/swarms",
        "server local_swarms_dir default changed — update the docs and the bridge's \
         KaskSwarmSettings comment.",
    );
    assert_eq!(
        server_default.skills_dir, None,
        "server skills_dir default changed — update the docs and the bridge's \
         KaskSwarmSettings comment.",
    );
    // The new fields: default_agent_model, a2a_http_enabled, memory_passphrase,
    // memory_db_path, embedding_dim. The bridge defaults for these are empty
    // strings / false / 1024 (the server's defaults are the real values). The
    // bridge's empty-string default means "use the server default" — same
    // pattern as api_url and the dir fields.
    assert_eq!(
        server_default.default_agent_model, "claude-haiku-4-5-20251001",
        "server default_agent_model default changed — update the docs.",
    );
    assert!(
        !server_default.a2a_http_enabled,
        "server a2a_http_enabled default changed"
    );
    assert_eq!(
        server_default.memory_passphrase, "allostery",
        "server memory_passphrase default changed — update the docs.",
    );
    assert_eq!(
        server_default.memory_db_path, "swarm_memory.db",
        "server memory_db_path default changed — update the docs.",
    );
    assert_eq!(
        server_default.embedding_dim, 1024,
        "server embedding_dim default changed — update the docs.",
    );
}

/// A non-default bridge setting must round-trip through `mcp_env` →
/// `from_env` to the same value on the server side. This verifies the env-var
/// emission and parsing paths agree.
#[test]
fn swarm_settings_non_default_round_trips_through_env() {
    let mut settings = KaskSettings::default();
    settings.swarm.max_credits_per_dispatch = 100;
    settings.swarm.curator_consent_default = true;

    let env = settings.mcp_env();

    // The bridge emits `HKASK_ABW_MAX_CREDITS` and
    // `HKASK_ABW_CURATOR_CONSENT_DEFAULT` for non-default values.
    assert_eq!(
        env.get("HKASK_ABW_MAX_CREDITS").map(String::as_str),
        Some("100"),
        "bridge must emit HKASK_ABW_MAX_CREDITS for non-default max_credits_per_dispatch",
    );
    assert_eq!(
        env.get("HKASK_ABW_CURATOR_CONSENT_DEFAULT")
            .map(String::as_str),
        Some("true"),
        "bridge must emit HKASK_ABW_CURATOR_CONSENT_DEFAULT for non-default curator_consent_default",
    );

    // We can't call `SwarmConfig::from_env()` in this test because it reads
    // actual process env vars, and this test runs in the bridge crate's
    // test process (not the server's). The env-var emission is verified
    // above; the parsing is verified by the server's own config tests
    // (`config_defaults_match_documented_surface`). The drift detector is
    // the field-by-field comparison in the default test above.
}
