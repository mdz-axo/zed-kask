use super::catalog::*;
use super::env::*;
use crate::test_support::MockKeychain;
use credentials_provider::test_support::FakeCredentialsProvider;

/// The experimentation server's registration entry is pinned: id, binary, the
/// SQLCipher passphrase credential, and the config allowlist aligned with
/// the crate's actual env reads (HKASK_DATA_DIR for the D28 default DB
/// path, HKASK_EXPERIMENTATION_DB for the override). A drift here breaks launch
/// under governed env before any tool runs — the allowlist is the only
/// delivery path for the passphrase (no std::env fallback tier in the
/// child).
#[test]
fn experimentation_registration_entry_is_pinned() {
    let server = BUILT_IN_MCP_SERVERS
        .iter()
        .find(|s| s.id == "experimentation")
        .expect("experimentation server registered");
    assert_eq!(server.binary, "hkask-mcp-experimentation");
    let expected_credentials: &[&str] = &["HKASK_DB_PASSPHRASE"];
    let expected_config: &[&str] = &["HKASK_DATA_DIR", "HKASK_EXPERIMENTATION_DB"];
    assert_eq!(server.credentials, expected_credentials);
    assert_eq!(server.config_env, expected_config);
}

/// P12 attribution: every built-in MCP server except `curator` (which uses
/// `HKASK_CURATOR_WEBID` via `mcp_env()`) and `kata-kanban` (which shares
/// `WebID::anonymous()` with `GoalHost`, pinned by D58) gets a distinct,
/// deterministic, non-anonymous `WebID` that round-trips through `FromStr`.
#[test]
fn non_curator_non_kanban_servers_receive_distinct_deterministic_webids() {
    use std::collections::HashSet;
    use std::str::FromStr;

    assert!(
        default_server_webid("curator").is_none(),
        "curator identity comes from HKASK_CURATOR_WEBID, not default_server_webid"
    );
    assert!(
        default_server_webid("kata-kanban").is_none(),
        "kata-kanban must stay excluded from HKASK_WEBID (D58)"
    );
    assert!(
        default_server_webid("unknown-server").is_none(),
        "unregistered server ids must not synthesize a WebID"
    );

    let mut seen = HashSet::new();
    for server in BUILT_IN_MCP_SERVERS {
        if server.id == "curator" || server.id == "kata-kanban" {
            continue;
        }
        let webid = default_server_webid(server.id)
            .unwrap_or_else(|| panic!("expected deterministic WebID for {}", server.id));
        assert_ne!(
            webid,
            hkask_types::WebID::anonymous(),
            "{} must not resolve to anonymous",
            server.id
        );
        let parsed = hkask_types::WebID::from_str(&webid.to_string())
            .expect("serialized WebID must parse back cleanly");
        assert_eq!(parsed, webid);
        assert!(
            seen.insert(webid),
            "server {} collided with another server's WebID",
            server.id
        );
    }
    assert!(
        seen.len() >= 10,
        "expected distinct WebIDs for all non-curator, non-kanban built-in servers"
    );
}

/// The relay holds no secret: a credential added to its entry is a
/// design change (the relay would then hold a key or a passphrase) and
/// must fail here first. Its config allowlist is pinned to the three
/// policy variables `Config::from_env` reads plus the data dir.
#[test]
fn nostr_relay_registration_entry_is_pinned() {
    let server = BUILT_IN_MCP_SERVERS
        .iter()
        .find(|s| s.id == "nostr-relay")
        .expect("nostr-relay server registered");
    assert_eq!(server.binary, "hkask-mcp-nostr-relay");
    assert!(
        server.credentials.is_empty(),
        "the relay verifies signatures and never signs; it receives no credential"
    );
    let expected_config: &[&str] = &[
        "HKASK_DATA_DIR",
        "HKASK_NOSTR_RELAY_PORT",
        "HKASK_NOSTR_RELAY_AUTHORS",
        "HKASK_NOSTR_RELAY_SYNC_RELAYS",
    ];
    assert_eq!(server.config_env, expected_config);
}

/// Operator ruling 2026-11-12: the relay is blocked on private threads.
/// Its sync tool contacts public relays, so its tools are refused there
/// by default, like the media and corpus servers.
#[test]
fn nostr_relay_is_disabled_on_private_threads_by_default() {
    assert!(
        settings_content::DEFAULT_PRIVATE_THREAD_DISABLED_SERVERS.contains(&"nostr-relay"),
        "nostr-relay must be in the default private-thread disabled set"
    );
    assert!(
        BUILT_IN_MCP_SERVERS.iter().any(|s| s.id == "nostr-relay"),
        "the disabled-set entry must name a registered server"
    );
}

/// The Nostr client parses untrusted relay text, and a Nostr key cannot
/// be rotated or recovered. So the client receives no credential at all:
/// a credential added to its entry is a design change and must fail here
/// first. Its config allowlist is pinned to what `Config::from_env`
/// reads: the data dir (where the relay server announces its listener)
/// and the two public values.
#[test]
fn nostr_client_registration_entry_is_pinned() {
    let server = BUILT_IN_MCP_SERVERS
        .iter()
        .find(|s| s.id == "nostr-client")
        .expect("nostr-client server registered");
    assert_eq!(server.binary, "hkask-mcp-nostr-client");
    assert!(
        server.credentials.is_empty(),
        "the Nostr client holds no key and cannot sign; it receives no credential"
    );
    let expected_config: &[&str] = &[
        "HKASK_DATA_DIR",
        "HKASK_NOSTR_CLIENT_NPUB",
        "HKASK_NOSTR_CLIENT_SEED_RELAYS",
        "HKASK_NOSTR_CLIENT_BLOSSOM_SERVERS",
        "HKASK_NOSTR_BLOSSOM_PUBLIC_URL",
    ];
    assert_eq!(server.config_env, expected_config);
}

#[test]
fn nostr_blossom_registration_entry_is_pinned() {
    let server = BUILT_IN_MCP_SERVERS
        .iter()
        .find(|s| s.id == "nostr-blossom")
        .expect("nostr-blossom server registered");
    assert_eq!(server.binary, "hkask-mcp-nostr-blossom");
    assert!(
        server.credentials.is_empty(),
        "the Blossom sidecar verifies signatures and never signs; it receives no credential"
    );
    let expected_config: &[&str] = &[
        "HKASK_DATA_DIR",
        "HKASK_NOSTR_BLOSSOM_PORT",
        "HKASK_NOSTR_BLOSSOM_MAX_BYTES",
        "HKASK_NOSTR_BLOSSOM_PUBLIC_URL",
        "HKASK_NOSTR_BLOSSOM_AUTHORS",
    ];
    assert_eq!(server.config_env, expected_config);
}

#[test]
fn nostr_blossom_is_disabled_on_private_threads_by_default() {
    assert!(
        settings_content::DEFAULT_PRIVATE_THREAD_DISABLED_SERVERS.contains(&"nostr-blossom"),
        "nostr-blossom must be in the default private-thread disabled set"
    );
    assert!(
        BUILT_IN_MCP_SERVERS.iter().any(|s| s.id == "nostr-blossom"),
        "the disabled-set entry must name a registered server"
    );
}

#[test]
fn wallet_registration_entry_is_pinned() {
    let server = BUILT_IN_MCP_SERVERS
        .iter()
        .find(|s| s.id == "wallet")
        .expect("wallet server registered");
    assert_eq!(server.binary, "hkask-mcp-wallet");
    assert!(
        server.credentials.is_empty(),
        "wallet receives zero credentials via launch env; DB unlock uses editor IPC"
    );
    let expected_config: &[&str] = &[
        "HKASK_DATA_DIR",
        "HKASK_WALLET_NETWORK",
        "HKASK_WALLET_MAINNET_ENABLED",
        "HKASK_WALLET_PER_TX_CAP_SATS",
        "HKASK_WALLET_DAILY_CAP_SATS",
        "HKASK_WALLET_MAX_FEE_SATS",
        "HKASK_WALLET_ALLOWED_MINTS",
        "HKASK_WALLET_DEFAULT_MINT",
        "HKASK_WALLET_NWC_LABEL",
    ];
    assert_eq!(server.config_env, expected_config);
}

#[test]
fn wallet_is_disabled_on_private_threads_by_default() {
    assert!(
        settings_content::DEFAULT_PRIVATE_THREAD_DISABLED_SERVERS.contains(&"wallet"),
        "wallet must be in the default private-thread disabled set (D100/D110)"
    );
    assert!(
        BUILT_IN_MCP_SERVERS.iter().any(|s| s.id == "wallet"),
        "the disabled-set entry must name a registered server"
    );
}

/// The client finds the local relay through a file the relay server
/// writes under the data dir. Both must be handed the same data dir, or
/// an operator override sends them to different directories.
#[test]
fn nostr_client_and_relay_are_given_the_same_data_dir() {
    for id in ["nostr-relay", "nostr-client", "nostr-blossom"] {
        let server = BUILT_IN_MCP_SERVERS
            .iter()
            .find(|server| server.id == id)
            .unwrap_or_else(|| panic!("{id} is registered"));
        assert!(
            server.config_env.contains(&"HKASK_DATA_DIR"),
            "{id} must be given HKASK_DATA_DIR"
        );
    }
}

/// Operator ruling 2026-11-11: "nostr is not enabled or is disabled on
/// private threads - you shouldn't be sharing private threads on a public
/// network". The whole client server is refused there by default; no
/// per-tool table, so a tool added later is covered.
#[test]
fn nostr_client_is_disabled_on_private_threads_by_default() {
    assert!(
        settings_content::DEFAULT_PRIVATE_THREAD_DISABLED_SERVERS.contains(&"nostr-client"),
        "nostr-client must be in the default private-thread disabled set"
    );
    assert!(
        BUILT_IN_MCP_SERVERS.iter().any(|s| s.id == "nostr-client"),
        "the disabled-set entry must name a registered server"
    );
}

/// Whatever credentials exist, none reaches the Nostr client: the
/// per-server filter gives it nothing, including the database passphrase
/// every other stateful server receives.
#[test]
fn nostr_client_receives_no_credential_from_the_filter() {
    let offered: Vec<(String, String)> = BUILT_IN_MCP_SERVERS
        .iter()
        .flat_map(|server| server.credentials.iter())
        .chain(["NOSTR_NSEC", "HKASK_NOSTR_NSEC"].iter())
        .map(|env| (env.to_string(), "url".to_string()))
        .collect();
    assert!(
        offered.iter().any(|(env, _)| env == "HKASK_DB_PASSPHRASE"),
        "the control: real credentials are on offer"
    );
    assert_eq!(
        filter_credentials_for_server("nostr-client", &offered),
        Vec::<(String, String)>::new()
    );
}

#[test]
fn qa_generation_setting_round_trips_and_reaches_only_its_consumer() {
    use crate::KaskSettings;
    use hkask_inference::model_constants::QA_GENERATION_MODEL_ENV;
    let mut content = settings::KaskSettingsContent::default();
    let models = content.models.get_or_insert_default();
    models.default_model = Some("OpenRouter/z-ai/glm-5.3-flash".into());
    models.qa_generation_model = Some("OpenRouter/~openai/gpt-sol-latest".into());
    let saved = serde_json::to_string(&content).expect("serialize settings content");
    let reloaded = serde_json::from_str::<settings::KaskSettingsContent>(&saved)
        .expect("reload settings content");
    let settings: KaskSettings = reloaded.into();
    assert_eq!(
        settings.models.qa_generation_model,
        "OpenRouter/~openai/gpt-sol-latest"
    );
    let env = settings.mcp_env();
    for server in BUILT_IN_MCP_SERVERS {
        let filtered = filter_config_env_for_server(server.id, &env);
        assert_eq!(
            filtered.get(QA_GENERATION_MODEL_ENV).map(String::as_str),
            (server.id == "corpus").then_some("OpenRouter/~openai/gpt-sol-latest"),
            "{}",
            server.id
        );
    }
    // Clearing the setting removes it from the restart-diff environment.
    let cleared: settings::KaskSettingsContent =
        serde_json::from_str(r#"{"models":{"qa_generation_model":""}}"#).expect("cleared setting");
    let cleared: KaskSettings = cleared.into();
    assert!(!cleared.mcp_env().contains_key(QA_GENERATION_MODEL_ENV));
    assert!(
        !KaskSettings::default()
            .mcp_env()
            .contains_key(QA_GENERATION_MODEL_ENV)
    );
    assert!(
        KaskSettings::default()
            .models
            .qa_generation_model
            .is_empty()
    );
}

/// The decision default (operator adoption 2026-11-06: TypeSafe's Jev
/// 1.14, `DEFAULT_DECISION_MODEL` — a System One decision model served
/// over the Decisions API) must reach the prediction-markets server's
/// filtered env, and only it — the eqm scoring's consumer-scoping
/// discipline (the rerank pin's shape).
#[test]
fn decision_default_reaches_only_the_prediction_markets_server() {
    use hkask_inference::model_constants::DEFAULT_DECISION_MODEL;
    let settings: crate::KaskSettings = settings::KaskSettingsContent::default().into();
    assert_eq!(settings.models.decision_model, DEFAULT_DECISION_MODEL);
    let env = settings.mcp_env();
    assert_eq!(
        env.get("HKASK_DECISION_MODEL").map(String::as_str),
        Some(DEFAULT_DECISION_MODEL)
    );
    for server in BUILT_IN_MCP_SERVERS {
        let filtered = filter_config_env_for_server(server.id, &env);
        assert_eq!(
            filtered.get("HKASK_DECISION_MODEL").map(String::as_str),
            (server.id == "prediction-markets").then_some(DEFAULT_DECISION_MODEL),
            "{}",
            server.id
        );
    }
}

/// The rerank default (operator ruling 2026-09-11) must reach the research
/// server's filtered env, and only the research server — the same
/// consumer-scoping discipline the QA generator follows.
#[test]
fn rerank_default_reaches_only_the_research_server() {
    use hkask_inference::model_constants::DEFAULT_RERANK_MODEL;
    let settings: crate::KaskSettings = settings::KaskSettingsContent::default().into();
    assert_eq!(settings.models.rerank_model, DEFAULT_RERANK_MODEL);
    let env = settings.mcp_env();
    assert_eq!(
        env.get("HKASK_RERANK_MODEL").map(String::as_str),
        Some(DEFAULT_RERANK_MODEL)
    );
    for server in BUILT_IN_MCP_SERVERS {
        let filtered = filter_config_env_for_server(server.id, &env);
        assert_eq!(
            filtered.get("HKASK_RERANK_MODEL").map(String::as_str),
            (server.id == "research").then_some(DEFAULT_RERANK_MODEL),
            "{}",
            server.id
        );
    }
}

fn server_by_id(id: &str) -> &'static BuiltinMcpServer {
    BUILT_IN_MCP_SERVERS
        .iter()
        .find(|s| s.id == id)
        .unwrap_or_else(|| panic!("server '{id}' not in BUILT_IN_MCP_SERVERS"))
}

/// The startup-default gate: both passphrase env vars are in
/// `DEFAULT_PASSPHRASE_ENV_VARS`, so `provide_default_passphrase`
/// resolves it (env → keychain → first-run "allostery") at MCP
/// launch time. This is what makes "stores never start down" true at
/// startup — remove it from the list and the gate regresses.
#[test]
fn startup_default_passphrase_gate_includes_db_passphrase() {
    assert!(DEFAULT_PASSPHRASE_ENV_VARS.contains(&"HKASK_DB_PASSPHRASE"));
}

/// The derived fns must match the main registry — this pins the single-source
// invariant so a future edit to the fns can't silently drift.
#[test]
fn builtin_mcp_server_ids_match_main_registry() {
    let ids: Vec<_> = BUILT_IN_MCP_SERVERS.iter().map(|s| s.id).collect();
    assert_eq!(builtin_mcp_server_ids(), ids);
}

#[test]
fn builtin_mcp_server_pairs_match_main_registry() {
    let pairs: Vec<_> = BUILT_IN_MCP_SERVERS
        .iter()
        .map(|s| (s.id, s.description))
        .collect();
    assert_eq!(builtin_mcp_server_pairs(), pairs);
}

#[test]
fn prediction_markets_allowlist_matches_actual_reads() {
    let s = server_by_id("prediction-markets");
    // Read site: ctx.credentials.get("HKASK_FRED_API_KEY") in `run()` for
    // live reference-level fetches (FRED API). Optional — curated static
    // defaults used when absent.
    assert_eq!(
        s.credentials.to_vec(),
        vec!["HKASK_FRED_API_KEY"],
        "prediction-markets credentials allowlist drifted — HKASK_FRED_API_KEY \
         is read in run() for live reference-level fetches; add a credential \
         only with a read site in hkask-mcp-prediction-markets"
    );
    // Read site: std::env::var("HKASK_PREDICTION_MARKETS_CACHE_TTL_SECS")
    // in `run()` (with a malformed-value warn, not silent fallback).
    // HKASK_DATA_DIR is read transitively via `resolve_under_data_dir` →
    // `resolve_data_dir` in hkask-types (the server's fallback when
    // HKASK_PREDICTION_MARKETS_DATA is unset). Allowlisted so an
    // operator `HKASK_DATA_DIR` override is not silently dropped by
    // `filter_config_env_for_server`.
    // HKASK_DECISION_MODEL is read at the eqm scoring site via
    // `hkask_inference::model_constants::resolve_decision_model`
    // (eqm.rs `score_rationale` — the decision model for the 12 typed
    // EQM score questions; unset refuses typed, never the chat model).
    assert_eq!(
        s.config_env.to_vec(),
        vec![
            "HKASK_DATA_DIR",
            "HKASK_PREDICTION_MARKETS_CACHE_TTL_SECS",
            "HKASK_PREDICTION_MARKETS_DATA",
            "HKASK_DECISION_MODEL",
        ],
        "prediction-markets config_env allowlist drifted — every entry must \
         have a read site in hkask-mcp-prediction-markets (HKASK_DATA_DIR is \
         read transitively via resolve_under_data_dir)"
    );
}

/// The media config_env allowlist must cover every env var the media
/// server reads — the reads→allowlist direction. The composed-path
/// filter silently drops an unlisted var while `mcp_env()` still emits
/// it, which is how HKASK_MEDIA_CROSS_CHECK_STT_MODELS shipped broken:
/// emitted by the settings layer, stripped before the child process,
/// with both half-tests green (the emission pin tests the pre-filter
/// map; the round-trip test asserted only the older keys).
#[test]
fn media_config_env_allowlist_matches_actual_reads() {
    let s = server_by_id("media");
    // Read sites: the media server's `models` module resolves STT_ENV,
    // CROSS_CHECK_STT_ENV, VISION_ENV, SOUND_ANNOTATION_ENV, TAGGING_ENV
    // and PASS_ENV (hkask-mcp-media/src/hkask_mcp_media.rs); HKASK_MEDIA_DB
    // resolves the gallery DB; HKASK_INFERENCE_SOCKET, HKASK_DATA_DIR,
    // HKASK_ARTIFACTS_DIR and HKASK_EMBEDDING_MODEL are read transitively
    // (IPC bridge, path resolution, embed_text).
    assert_eq!(
        s.config_env.to_vec(),
        vec![
            "HKASK_INFERENCE_SOCKET",
            "HKASK_DATA_DIR",
            "HKASK_ARTIFACTS_DIR",
            "HKASK_MEDIA_DB",
            "HKASK_MEDIA_STT_MODEL",
            "HKASK_MEDIA_CROSS_CHECK_STT_MODELS",
            "HKASK_MEDIA_VISION_MODEL",
            "HKASK_MEDIA_SOUND_ANNOTATION_MODEL",
            "HKASK_MEDIA_TAGGING_MODEL",
            "HKASK_MEDIA_PASS_MODEL",
            "HKASK_EMBEDDING_MODEL",
        ],
        "media config_env allowlist drifted — every entry must have a \
         read site in hkask-mcp-media, and every HKASK_MEDIA_* read must \
         be listed (filter_config_env_for_server drops unlisted vars)"
    );
}

// The curator server should receive only the DB passphrase, no data
// service API keys. This pins the blast-radius reduction.
#[test]
fn curator_credentials_do_not_include_data_service_keys() {
    let all_credentials: Vec<(String, String)> = ["OPENROUTER_API_KEY"]
        .iter()
        .map(|env| (env.to_string(), "url".to_string()))
        .collect();
    let filtered = filter_credentials_for_server("curator", &all_credentials);
    let env_vars: Vec<&str> = filtered.iter().map(|(k, _)| k.as_str()).collect();
    assert!(
        !env_vars.contains(&"OPENROUTER_API_KEY"),
        "curator should NOT receive OPENROUTER_API_KEY"
    );
}

/// expect: [P7] Research model choices reach only media; creative model
/// variables cannot re-enable retired pipelines through child injection.
#[test]
fn media_models_are_research_only_and_round_trip_to_child() {
    let content: settings_content::KaskMediaSettingsContent =
        serde_json::from_value(serde_json::json!({
            "stt_model":"OpenRouter/research-stt",
            "vision_model":"OpenRouter/research-vision",
            "sound_annotation_model":"OpenRouter/research-sound",
            "tagging_model":"ollama/research-tagging"
        }))
        .expect("research media content");
    let mut settings = crate::KaskSettings::default();
    settings.media = content.into();
    let env = settings.mcp_env();
    let media_env = filter_config_env_for_server("media", &env);
    for key in [
        "HKASK_MEDIA_STT_MODEL",
        "HKASK_MEDIA_CROSS_CHECK_STT_MODELS",
        "HKASK_MEDIA_VISION_MODEL",
        "HKASK_MEDIA_SOUND_ANNOTATION_MODEL",
        "HKASK_MEDIA_TAGGING_MODEL",
    ] {
        assert_eq!(media_env.get(key), env.get(key));
        assert!(media_env.contains_key(key));
        for server in BUILT_IN_MCP_SERVERS
            .iter()
            .filter(|server| server.id != "media")
        {
            assert!(!filter_config_env_for_server(server.id, &env).contains_key(key));
        }
    }
    let cleared: settings_content::KaskMediaSettingsContent =
        serde_json::from_value(serde_json::json!({
            "stt_model": "",
            "cross_check_stt_models": [],
            "vision_model": "",
            "sound_annotation_model": "",
            "tagging_model": ""
        }))
        .expect("explicitly cleared models");
    settings.media = cleared.into();
    let cleared_env = settings.mcp_env();
    for key in [
        "HKASK_MEDIA_STT_MODEL",
        "HKASK_MEDIA_CROSS_CHECK_STT_MODELS",
        "HKASK_MEDIA_VISION_MODEL",
        "HKASK_MEDIA_SOUND_ANNOTATION_MODEL",
        "HKASK_MEDIA_TAGGING_MODEL",
    ] {
        assert!(!cleared_env.contains_key(key));
    }
    let retired: std::collections::HashMap<String, String> = [
        "HKASK_MEDIA_TTS_MODEL",
        "HKASK_MEDIA_IMAGE_GEN_MODEL",
        "HKASK_MEDIA_VIDEO_MODEL",
    ]
    .into_iter()
    .map(|key| (key.to_string(), "retired".to_string()))
    .collect();
    assert!(filter_config_env_for_server("media", &retired).is_empty());
}

// The Reduct.cloud integration was removed (2026-10-22, operator-authorized;
// DIVERGENCE.md D9/D5): no Data Services descriptor and no child allowlist
// may carry its key — a dormant third-party secret with no read path is
// worse than a logged one-time delete (the ABW precedent).
#[test]
fn reduct_key_reaches_no_child() {
    assert!(
        crate::DATA_SERVICES
            .iter()
            .all(|entry| entry.credential_key != "reduct_api_key"),
        "the Reduct.cloud removal is complete — no Data Services descriptor may carry its key"
    );
    for server in BUILT_IN_MCP_SERVERS.iter() {
        assert!(
            !server.credentials.contains(&"REDUCT_API_KEY"),
            "the Reduct.cloud integration is removed — its API key must not reach the {} server",
            server.id
        );
    }
}

// Unknown server IDs fail closed: no credentials are injected.
#[test]
fn unknown_server_gets_no_credentials() {
    let credentials = vec![
        ("KEY_A".to_string(), "url_a".to_string()),
        ("KEY_B".to_string(), "url_b".to_string()),
    ];
    let filtered = filter_credentials_for_server("nonexistent", &credentials);
    assert!(filtered.is_empty());
}

// Unknown server IDs fail closed: no config env is injected.
#[test]
fn unknown_server_gets_no_config_env() {
    let mut config_env = std::collections::HashMap::new();
    config_env.insert("KEY_A".to_string(), "val_a".to_string());
    config_env.insert("KEY_B".to_string(), "val_b".to_string());
    let filtered = filter_config_env_for_server("nonexistent", &config_env);
    assert!(filtered.is_empty());
}

/// fal.ai is deprecated for the corpus server (the OCR docres path was
/// removed). The corpus
/// server no longer reads `FALAI_API_KEY` or `HKASK_USE_FAL_DOCRES`. Pin
/// the removal so a future re-add is caught (`.rules` "tests must pin
/// deliberate deviations").
#[test]
fn corpus_allowlist_excludes_deprecated_fal_ai() {
    let corpus = server_by_id("corpus");
    let creds = corpus.credentials;
    assert!(
        !creds.contains(&"FALAI_API_KEY"),
        "FALAI_API_KEY must not be granted to the corpus server — fal.ai docres is removed"
    );
    let cfg = corpus.config_env;
    assert!(
        !cfg.contains(&"HKASK_USE_FAL_DOCRES"),
        "HKASK_USE_FAL_DOCRES must not be granted to the corpus server — fal.ai docres is removed"
    );
}

// ── read-alignment history and current per-server pins ─────────
//
// This block began when only 5 of 13 then-registered servers had a
// read-alignment test. The retired training server's large secret grant
// helped expose both drift directions: under-grants silently drop operator
// overrides; over-grants hand secrets to processes that never read them.
//
// Each expectation below was derived by grepping the server's own source for
// `std::env::var` / `ctx.credentials.get` sites, so the assertion is against
// observed reads rather than intent.

#[test]
fn curator_allowlist_matches_actual_reads() {
    let s = server_by_id("curator");
    // Secret reads: ctx.credentials.get("HKASK_DB_PASSPHRASE")
    // (SQLCipher curator.db) — the only credential the server reads.
    let creds = s.credentials;
    assert!(
        creds.contains(&"HKASK_DB_PASSPHRASE"),
        "curator must receive HKASK_DB_PASSPHRASE to open its SQLCipher curator.db \
         under governed launch — the run() reads it from ctx.credentials \
         with no std::env::var fallback"
    );
    assert!(
        !s.config_env.is_empty(),
        "curator config_env should not be empty — the server reads the curator \
         DB path, memory-life and distillation model settings from it"
    );
    // HKASK_MEMORY_LIFE_DAYS: read by `memory_life_days_from_env` at
    // `open_curator_stores` (hkask_mcp_curator.rs) — the decay constant
    // from `kask.memory.memory_life_days`, emitted by
    // `emit_curator_distillation_env`. Without this allowlist entry the
    // per-server filter silently drops the operator's override and decay
    // reverts to the default 180 while the regulation sensor keeps
    // reporting the configured value — the exact misreport this check exists to end.
    assert!(
        s.config_env.contains(&"HKASK_MEMORY_LIFE_DAYS"),
        "curator must receive HKASK_MEMORY_LIFE_DAYS so the configured \
         decay constant reaches the store"
    );
    // The embedding model must be allowlisted: `embed_for_semantic_recall`
    // and the semantic-recall tools read HKASK_EMBEDDING_MODEL via the env
    // accessor with no fallback — an unallowlisted entry is silently
    // dropped by the per-server filter and every semantic recall degrades
    // (the live gap observed 2026-09-04).
    let s = server_by_id("curator");
    assert!(
        s.config_env.contains(&"HKASK_EMBEDDING_MODEL"),
        "curator must receive HKASK_EMBEDDING_MODEL for semantic recall"
    );
    // HKASK_MEMORY_CONFIDENCE_FLOOR: read by the write tools' floor check
    // (CU-31 b) — emitted by emit_curator_distillation_env from
    // kask.memory.confidence_floor. Without the allowlist entry the
    // per-server filter silently drops it and floor-crossing writes stop
    // warning that the row is a deferred delete.
    // HKASK_PRIVATE_CLASSIFIER_MODEL: read by DistillationConfig::from_env
    // for the distillation pass's private-batch routing (CU-19 b).
    assert!(
        s.config_env
            .contains(&hkask_inference::model_constants::PRIVATE_CLASSIFIER_MODEL_ENV),
        "curator must receive HKASK_PRIVATE_CLASSIFIER_MODEL so private batches never \
         generate through the configured cloud classifier (CU-19 b)"
    );
}

#[test]
fn research_allowlist_matches_actual_reads() {
    let s = server_by_id("research");
    // ctx.credentials.get sites: EXA, TAVILY, BRAVE, SERPAPI, FIRECRAWL,
    // and the optional SEMANTIC_SCHOLAR key. HKASK_DB_PASSPHRASE is read
    // for the RSS store.
    let creds = s.credentials;
    for key in [
        "HKASK_EXA_API_KEY",
        "HKASK_TAVILY_API_KEY",
        "HKASK_BRAVE_API_KEY",
        "HKASK_SERPAPI_API_KEY",
        "HKASK_FIRECRAWL_API_KEY",
        "HKASK_SEMANTIC_SCHOLAR_API_KEY",
    ] {
        assert!(
            creds.contains(&key),
            "research reads {key} via ctx.credentials.get but it is not \
             allowlisted — the provider would be silently unavailable"
        );
    }
    assert!(
        s.config_env.contains(&"HKASK_RESEARCH_DB"),
        "research reads HKASK_RESEARCH_DB via std::env::var but it is not allowlisted"
    );
    assert!(
        s.config_env.contains(&"HKASK_EMBEDDING_MODEL"),
        "research reads HKASK_EMBEDDING_MODEL via model_constants::embedding_model() \
         but it is not allowlisted — the semantic duplication tier would silently \
         degrade to the shingle floor despite an operator override"
    );
    assert!(
        s.config_env.contains(&"HKASK_DATA_DIR"),
        "research resolves its default research DB via resolve_under_data_dir \
         but HKASK_DATA_DIR is not allowlisted — an operator override \
         would be silently dropped"
    );
    assert!(
        !s.config_env.contains(&"HKASK_ARTIFACTS_DIR"),
        "research no longer resolves the artifacts dir — the InlineTable \
         presentation was removed with the spreadsheet capability \
         (2026-10-15); re-add the entry only with a read site in \
         hkask-mcp-research"
    );
    assert!(
        s.config_env.contains(&"HKASK_RERANK_MODEL"),
        "research reads HKASK_RERANK_MODEL via std::env::var (rerank model \
         resolution) but it is not allowlisted — an operator override \
         would be silently stripped under governed launch"
    );
    assert!(
        s.config_env.contains(&"HKASK_PRIVATE_RERANK_MODEL"),
        "research reads HKASK_PRIVATE_RERANK_MODEL via \
         model_constants::private_rerank_model() but it is not \
         allowlisted — a private call's rerank stage would surface the \
         heuristic skip instead of routing locally"
    );
}

#[test]
fn scenarios_allowlist_matches_actual_reads() {
    let s = server_by_id("scenarios");
    // No secret read sites at all — the server is storage-only.
    assert!(
        s.credentials.is_empty(),
        "scenarios has no credential read site; granting one would be an \
         unjustified secret grant"
    );
    // HKASK_DATA_DIR is read transitively via `resolve_under_data_dir`
    // → `resolve_data_dir` in hkask-types (the server's fallback when
    // HKASK_SCENARIOS_DATA is unset). Allowlisted so an operator
    // `HKASK_DATA_DIR` override is not silently dropped by
    // `filter_config_env_for_server`.
    assert_eq!(
        s.config_env.to_vec(),
        vec!["HKASK_DATA_DIR", "HKASK_SCENARIOS_DATA"],
        "scenarios config_env allowlist drifted — HKASK_SCENARIOS_DATA is \
         its direct std::env::var read; HKASK_DATA_DIR is read transitively \
         via resolve_under_data_dir"
    );
}

/// The training server was removed (training is outside the co-researcher
/// mandate). Pin its absence so a reintroduction is a deliberate decision
/// with its own registry entry, not an accidental revival.
#[test]
fn training_server_is_not_registered() {
    assert!(
        !BUILT_IN_MCP_SERVERS.iter().any(|s| s.id == "training"),
        "the training server was removed — reintroducing it needs a new \
         registry entry, build-list line and settings surface"
    );
    // The training-only child grants must not reappear. RunPod itself is
    // intentionally absent from this list: its API key remains live in the
    // native D29 OCR provider, but no MCP child consumes it now.
    // HF_TOKEN has no reader at all: its zed-side reinstatement
    // (2026-11-04) was retired with the kask model-weights fetcher
    // (2026-11-11), and no MCP child receives it either.
    const RETIRED_TRAINING_CREDENTIALS: &[&str] = &["HF_TOKEN", "RUNPOD_API_KEY"];
    const RETIRED_TRAINING_CONFIG: &[&str] = &[
        "HKASK_TRAINING_HOST",
        "HKASK_TRAINING_CACHE_DIR",
        "HKASK_TRAINING_DB",
        "HKASK_HF_ARTIFACT_OWNER",
        "HKASK_HF_DATASET_REPO",
        "HKASK_HF_MODEL_REPO",
        "RUNPOD_TEMPLATE_ID",
        "RUNPOD_GPU_TYPE_ID",
        "RUNPOD_CONTAINER_DISK_GB",
        "RUNPOD_DOCKER_IMAGE",
        "RUNPOD_DOCKER_ARGS",
        "HKASK_PODS_FILE",
        "NEBIUS_PROJECT_ID",
        "NEBIUS_SUBNET_ID",
        "NEBIUS_GPU_PLATFORM",
        "NEBIUS_GPU_PRESET",
        "NEBIUS_IMAGE_FAMILY",
        "NEBIUS_CLI_PATH",
    ];
    for server in BUILT_IN_MCP_SERVERS {
        for key in RETIRED_TRAINING_CREDENTIALS {
            assert!(
                !server.credentials.contains(key),
                "{} still grants retired training credential {key}",
                server.id
            );
        }
        for key in RETIRED_TRAINING_CONFIG {
            assert!(
                !server.config_env.contains(key),
                "{} still grants retired training config {key}",
                server.id
            );
        }
    }
    assert!(
        !crate::credential_urls_for_mcp()
            .iter()
            .any(|(env_var, _)| env_var == "RUNPOD_API_KEY"),
        "RunPod OCR is native-only after training removal; its key must not \
         be injected into or restart MCP children"
    );
}

/// Every secret-shaped credential grant must be justified by a read site.
/// This is the generic backstop for the 8 servers without a bespoke test:
/// it cannot verify each name, but it catches the case where a credential
/// allowlist grows without the registry comment that documents why.
#[test]
fn every_credential_grant_is_secret_shaped_or_documented() {
    for server in BUILT_IN_MCP_SERVERS {
        for key in server.credentials {
            let upper = key.to_uppercase();
            assert!(
                upper.contains("KEY")
                    || upper.contains("TOKEN")
                    || upper.contains("PASSWORD")
                    || upper.contains("PASSPHRASE")
                    || upper.contains("SECRET")
                    || upper.contains("PROJECT_ID")
                    || upper.contains("SUBNET_ID")
                    || upper.contains("TEMPLATE_ID"),
                "{} grants '{key}' as a CREDENTIAL but the name is not \
                 secret-shaped — non-secret configuration belongs in \
                 config_env, which is not treated as sensitive",
                server.id
            );
        }
    }
}

/// The composition invariant `build_mcp_server_env` encodes: after the
/// filter sequence, the only env vars present are those in the server's
/// `config_env` allowlist, its `credentials` allowlist, or the inference
/// socket. This is the test the two old paths lacked — Path A leaked the
/// full unfiltered `mcp_env()` map (the `extend` only overwrote allowed
/// keys, never removed disallowed ones), and Path B dropped every
/// credential (the config filter ran on the credential map too). Both
/// bugs were invisible because the allowlist-alignment tests above
/// exercise the filter helpers in isolation, never the composed path.
///
/// This is a synchronous stand-in: it simulates the filter sequence with
/// a populated `mcp_env()`-style map (including config vars no server
/// allowlists) and asserts no key outside
/// `config_env ∪ credentials ∪ {INFERENCE_SOCKET_ENV}` survives. The
/// async keychain read is orthogonal — the invariant is the filter order.
#[test]
fn build_mcp_server_env_composition_respects_allowlists() {
    // A `mcp_env()`-shaped map: config vars no server allowlists plus a
    // few vars other servers emit. In a real launch `mcp_env()` produces this.
    let mut full_config = std::collections::HashMap::new();
    full_config.insert("HKASK_DATA_DIR".to_string(), "/data".to_string());
    full_config.insert(
        "HKASK_CORPUS_EMBEDDING_MODEL".to_string(),
        "OpenRouter/research-embedding".to_string(),
    );
    full_config.insert(
        "HKASK_MEDIA_STT_MODEL".to_string(),
        "OpenRouter/research-stt".to_string(),
    );
    full_config.insert("HKASK_RESEARCH_DB".to_string(), "/research.db".to_string());
    // Credential-shaped keys that live in `credentials`, not `config_env`.
    // `OPENROUTER_API_KEY` is allowlisted by the media server; `FALAI_API_KEY`
    // is not allowlisted by any server (fal.ai docres was removed).
    let credential_keys = ["OPENROUTER_API_KEY", "FALAI_API_KEY"];

    for server in BUILT_IN_MCP_SERVERS {
        // Simulate `build_mcp_server_env`'s filter sequence:
        //   1. config filtered by `config_env` allowlist
        //   2. credentials merged (filtered by `credentials` allowlist)
        //   3. inference socket added
        //   4. inference timeout added
        let mut env = filter_config_env_for_server(server.id, &full_config);
        for key in server.credentials {
            if credential_keys.contains(&key) {
                env.insert(key.to_string(), "secret-value".to_string());
            }
        }
        env.insert(
            hkask_types::inference_ipc::INFERENCE_SOCKET_ENV.to_string(),
            "/tmp/sock".to_string(),
        );
        env.insert(
            hkask_types::inference_ipc::INFERENCE_TIMEOUT_ENV.to_string(),
            "300".to_string(),
        );

        let allowed: std::collections::HashSet<&str> = std::collections::HashSet::from_iter(
            server
                .config_env
                .iter()
                .copied()
                .chain(server.credentials.iter().copied())
                .chain(std::iter::once(
                    hkask_types::inference_ipc::INFERENCE_SOCKET_ENV,
                ))
                .chain(std::iter::once(
                    hkask_types::inference_ipc::INFERENCE_TIMEOUT_ENV,
                )),
        );

        for key in env.keys() {
            assert!(
                allowed.contains(key.as_str()),
                "{}: env var '{key}' survived the filter sequence but is not in \
                 config_env or credentials — the composition leaked it. \
                 This is the Path A regression (full mcp_env() map leaked \
                 because `extend` only overwrote allowed keys).",
                server.id
            );
        }
    }
}

/// Pin the shell-override-survives-env_clear fix. The governed McpRuntime
/// path (`start_server_with_env`) calls `cmd.env_clear()` before injecting
/// `extra_env`, so a shell-set credential that is merely `continue`-d in
/// `build_mcp_server_env` is LOST — the child sees neither the shell value
/// nor the keychain value. The fix inserts the parent env value into the
/// map so it survives the clear. This test mirrors the credential loop
/// logic synchronously (the async keychain read is orthogonal) and asserts
/// a shell-set credential lands in the output map.
///
/// SAFETY: Setting/removing test environment variables in test code is
/// safe in a single-threaded test context (Rust runs tests serially by
/// default). The `OPENROUTER_API_KEY` env var is set and removed within
/// this test only.
#[test]
fn shell_set_credential_survives_env_clear() {
    // SAFETY: Single-threaded test context. Set a shell value for the
    // OpenRouter key, simulating an operator who exports it in their
    // shell.
    unsafe { std::env::set_var("OPENROUTER_API_KEY", "shell-secret-value") };

    // Mirror the credential loop from `build_mcp_server_env`:
    //   if parent env has a non-empty value → insert into env, continue
    //   else → read from keychain (simulated as None here)
    let mut env = std::collections::HashMap::<String, String>::new();
    let cred_urls = filter_credentials_for_server("media", &crate::credential_urls_for_mcp());
    for (env_var, _url) in cred_urls {
        if let Ok(value) = std::env::var(&env_var)
            && !value.is_empty()
        {
            env.insert(env_var, value);
            continue;
        }
        // Keychain read would go here; simulated as None for this test.
    }

    // The shell-set credential must be in the output map. If the `continue`
    // were a skip (the pre-fix behavior), this would fail — the value would
    // be lost when the governed path clears the child's env.
    assert_eq!(
        env.get("OPENROUTER_API_KEY").map(|v| v.as_str()),
        Some("shell-secret-value"),
        "shell-set OPENROUTER_API_KEY must be inserted into the env map so it \
         survives the governed path's cmd.env_clear() — the prior `continue` \
         skipped it, and the child received neither the shell value nor the \
         keychain value"
    );

    // SAFETY: Clean up the test env var.
    unsafe { std::env::remove_var("OPENROUTER_API_KEY") };
}

// ── build_mcp_server_env: composed-path filter-order tests ──────────
//
// P18: the existing `build_mcp_server_env_composition_respects_allowlists`
// test simulates the filter sequence synchronously — it never calls the
// real `build_mcp_server_env`. The comment at L386-394 warns that both
// prior bugs were invisible because the filter helpers were exercised in
// isolation, never the composed path. These tests call the real composed
// function with a mock `CredentialsProvider` and pin the load-bearing
// filter order: config filtered first, then credentials merged into the
// already-filtered map (the two filters apply to disjoint key sets).

/// expect: "Settings unload revokes delegated authority before stopping the child" [P1]
#[test]
fn main_unload_revokes_before_stop() {
    let main = include_str!("../../../../../crates/zed/src/main.rs");
    assert!(main.contains("for server_id in to_stop {\n                kask_bridge::revoke_delegation_grant(server_id);\n                runtime.stop_server(server_id).await;"));
}

static ENV_TEST_LOCK: std::sync::LazyLock<tokio::sync::Mutex<()>> =
    std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));

/// expect: "Only loaded children receive stable parent-held grants, and unload cannot remint authority" [P1]
#[tokio::test]
async fn delegated_grants_follow_settings_and_launch_lifecycle() {
    let _guard = ENV_TEST_LOCK.lock().await;
    let provider = FakeCredentialsProvider::default();
    // Keychain reads go through the typed seam, not the provider — seed
    // the mock keychain with every credential URL so each
    // `build_mcp_server_env` call reads Loaded. In particular the DB
    // passphrase must read Loaded: an absent passphrase would arm the
    // REAL startup-default provisioning tier, and no test may reach it.
    let _mock_keychain = MockKeychain::from_secrets(
        crate::credential_urls_for_mcp()
            .into_iter()
            .map(|(_, url)| (url, b"disposable-test-secret".to_vec()))
            .collect(),
    )
    .install();
    let context = gpui::TestAppContext::single();
    let cx = context.to_async();
    let content: settings_content::KaskSettingsContent = serde_json::from_value(
        serde_json::json!({"mcp":{"delegated_tools":{"kata-kanban":["host/create_worktree_thread","research/rss_search"]}}}),
    )
    .expect("settings");
    let mut settings = crate::KaskSettings::from(content);
    assert!(
        crate::KaskSettings::default()
            .mcp
            .delegated_tools
            .is_empty()
    );
    let schema = serde_json::to_value(schemars::schema_for!(
        settings_content::KaskMcpSettingsContent
    ))
    .expect("schema");
    assert!(schema["properties"]["delegated_tools"].is_object());
    let first = build_mcp_server_env("kata-kanban", &settings, &provider, None, None, &cx).await;
    let token = first
        .get(hkask_types::inference_ipc::TOOL_GRANT_ENV)
        .expect("token")
        .to_string();
    // The granted ceiling: a granted tool survives the narrowing
    // intersection; a non-granted tool drops out, never widens.
    assert_eq!(
        crate::delegation_grants::worktree_tools(
            Some(&token),
            Some(&["research/rss_search".to_string()])
        ),
        Some(vec!["research/rss_search".to_string()])
    );
    assert_eq!(
        crate::delegation_grants::worktree_tools(
            Some(&token),
            Some(&["research/rss_fetch".to_string()])
        ),
        Some(Vec::new())
    );
    let again = build_mcp_server_env("kata-kanban", &settings, &provider, None, None, &cx).await;
    assert_eq!(
        again.get(hkask_types::inference_ipc::TOOL_GRANT_ENV),
        Some(token.as_str())
    );
    let other = build_mcp_server_env("portfolio", &settings, &provider, None, None, &cx).await;
    assert!(
        other
            .get(hkask_types::inference_ipc::TOOL_GRANT_ENV)
            .is_none()
    );
    settings.mcp.overrides.insert("kata-kanban".into(), false);
    crate::revoke_delegation_grant("kata-kanban");
    let unloaded = build_mcp_server_env("kata-kanban", &settings, &provider, None, None, &cx).await;
    assert!(
        unloaded
            .get(hkask_types::inference_ipc::TOOL_GRANT_ENV)
            .is_none(),
        "observer env diff must not remint an unloaded grant"
    );
    assert_eq!(
        crate::delegation_grants::worktree_tools(
            Some(&token),
            Some(&["research/rss_search".to_string()])
        ),
        None,
        "the revoked grant resolves to no ceiling at all"
    );
    settings.mcp.overrides.insert("kata-kanban".into(), true);
    let reloaded = build_mcp_server_env("kata-kanban", &settings, &provider, None, None, &cx).await;
    assert_ne!(
        reloaded.get(hkask_types::inference_ipc::TOOL_GRANT_ENV),
        Some(token.as_str())
    );
    settings
        .mcp
        .delegated_tools
        .insert("kata-kanban".into(), vec!["research/*".into()]);
    let invalid = build_mcp_server_env("kata-kanban", &settings, &provider, None, None, &cx).await;
    assert!(
        invalid
            .get(hkask_types::inference_ipc::TOOL_GRANT_ENV)
            .is_none()
    );
    crate::revoke_delegation_grant("kata-kanban");
}

#[tokio::test]
async fn build_mcp_server_env_forwards_allowlisted_operator_overrides() {
    let _guard = ENV_TEST_LOCK.lock().await;
    let settings = crate::KaskSettings::default();
    let provider = FakeCredentialsProvider::default();
    // Keychain reads go through the typed seam — seed the mock keychain
    // with every credential URL (the DB passphrase especially: an
    // absent passphrase would arm the REAL provisioning tier).
    let _mock_keychain = MockKeychain::from_secrets(
        crate::credential_urls_for_mcp()
            .into_iter()
            .map(|(_, url)| (url, b"disposable-test-secret".to_vec()))
            .collect(),
    )
    .install();
    unsafe {
        std::env::set_var("HKASK_CURATOR_DB", "/tmp/test-curator-override.db");
    }
    let cx = gpui::TestAppContext::single().to_async();
    let curator_env = build_mcp_server_env("curator", &settings, &provider, None, None, &cx).await;
    unsafe {
        std::env::remove_var("HKASK_CURATOR_DB");
    }
    assert_eq!(
        curator_env.get("HKASK_CURATOR_DB"),
        Some("/tmp/test-curator-override.db"),
        "HKASK_CURATOR_DB set by main.rs must survive build_mcp_server_env for curator"
    );
}

/// A-4: the keychain-read failure states are distinguishable —
/// Loaded / Missing / Empty / InvalidUtf8 / ProviderError / Transient
/// each classify distinctly over the keystore's TYPED error taxonomy
/// (the `.ok()??` collapse this replaces made them read as "not set"),
/// and a whitespace-only value classifies as Empty rather than
/// injecting an empty env value. The mock injects at the typed
/// keychain-read seam (`MockKeychain` in test_support.rs) — the layer
/// BELOW the provider trait — because the states now come from the
/// keystore's `KeychainError` variants, which the provider trait
/// collapses to `Ok(None)`. The failure classes are the typed errors
/// themselves: `Platform` for a locked/D-Bus-failing keychain,
/// `Timeout` for a read that never answered — which is what made the
/// transient state testable at all.
#[tokio::test]
async fn keychain_credential_failure_states_are_distinguishable() {
    // Serialize against the other mock-installing tests: the mock is
    // process-global, and parallel installs would trip the leak guard.
    let _guard = ENV_TEST_LOCK.lock().await;
    let (_, passphrase_url) = crate::credential_urls_for_mcp()
        .into_iter()
        .find(|(env_var, _)| env_var == "HKASK_DB_PASSPHRASE")
        .expect("HKASK_DB_PASSPHRASE has a credential URL");

    // Missing: the keychain answered and holds no entry at the URL —
    // typed `NotFound`. An empty mock serves `NotFound` for every URL,
    // like a keychain with no entry where one was requested.
    let mock = MockKeychain::new().install();
    let read = read_keychain_credential("curator", "HKASK_DB_PASSPHRASE", &passphrase_url).await;
    assert_eq!(read, KeychainCredentialRead::Missing, "{read:?}");
    drop(mock);

    // Loaded: a real non-empty value.
    let mock = MockKeychain::new()
        .with_secret(&passphrase_url, b"keychain-secret-passphrase")
        .install();
    let read = read_keychain_credential("curator", "HKASK_DB_PASSPHRASE", &passphrase_url).await;
    assert_eq!(
        read,
        KeychainCredentialRead::Loaded("keychain-secret-passphrase".to_string()),
        "{read:?}"
    );
    drop(mock);

    // Empty: an entry whose value is the empty string.
    let mock = MockKeychain::new()
        .with_secret(&passphrase_url, Vec::<u8>::new())
        .install();
    let read = read_keychain_credential("curator", "HKASK_DB_PASSPHRASE", &passphrase_url).await;
    assert_eq!(read, KeychainCredentialRead::Empty, "{read:?}");
    drop(mock);

    // Empty (whitespace-only): a value that survives the emptiness filter
    // but trims to nothing — previously injected as an EMPTY env value.
    let mock = MockKeychain::new()
        .with_secret(&passphrase_url, b"  \t \n")
        .install();
    let read = read_keychain_credential("curator", "HKASK_DB_PASSPHRASE", &passphrase_url).await;
    assert_eq!(read, KeychainCredentialRead::Empty, "{read:?}");
    drop(mock);

    // InvalidUtf8: an entry with corrupt bytes. The passphrase is exempt
    // from trimming but not from UTF-8 validation — a non-UTF-8 value can
    // never be injected as a credential. The seam is byte-typed so this
    // state stays classifiable even though today's keystore returns
    // (lossily decoded) strings.
    let mock = MockKeychain::new()
        .with_secret(&passphrase_url, vec![0xff_u8, 0xfe, 0xfd])
        .install();
    let read = read_keychain_credential("curator", "HKASK_DB_PASSPHRASE", &passphrase_url).await;
    assert_eq!(read, KeychainCredentialRead::InvalidUtf8, "{read:?}");
    drop(mock);

    // ProviderError: the keychain failed at the platform layer (locked,
    // D-Bus) — typed `Platform`. The key may exist; it is unreachable.
    let mock = MockKeychain::new()
        .with_platform_error(&passphrase_url, "keychain locked")
        .install();
    let read = read_keychain_credential("curator", "HKASK_DB_PASSPHRASE", &passphrase_url).await;
    assert_eq!(
        read,
        KeychainCredentialRead::ProviderError("keychain locked".to_string()),
        "{read:?}"
    );
    drop(mock);

    // Transient: the read did not answer within its deadline — typed
    // `Timeout`, the wedged-portal class (observed live 2026-10-25/26).
    // The key may exist; this must NEVER classify as Missing.
    let mock = MockKeychain::new()
        .with_timeout(&passphrase_url, 5)
        .install();
    let read = read_keychain_credential("curator", "HKASK_DB_PASSPHRASE", &passphrase_url).await;
    assert_eq!(
        read,
        KeychainCredentialRead::Transient { timeout_secs: 5 },
        "{read:?}"
    );
    drop(mock);
}

/// The wedged-portal defect pin (card db0d8e88): a timed-out read of
/// an EXISTING key must fire the honest transient message, never
/// "has no keychain entry". The provider trait's `Ok(None)` collapse
/// classified transient reads as `Missing` and told the operator to
/// set keys that exist (observed live 2026-10-25 21:36 and
/// 2026-10-26 11:07: HKASK_DB_PASSPHRASE under a wedged secret portal,
/// TAVILY/EXA/BRAVE keys for the research server).
#[tokio::test]
async fn timed_out_read_of_an_existing_key_warns_transient_never_missing() {
    // Serialize against the other mock-installing tests: the mock is
    // process-global, and parallel installs would trip the leak guard.
    let _guard = ENV_TEST_LOCK.lock().await;
    let (_, passphrase_url) = crate::credential_urls_for_mcp()
        .into_iter()
        .find(|(env_var, _)| env_var == "HKASK_DB_PASSPHRASE")
        .expect("HKASK_DB_PASSPHRASE has a credential URL");

    // The timed-out read classifies Transient, carrying the expired
    // deadline — not Missing.
    let mock = MockKeychain::new()
        .with_timeout(&passphrase_url, 5)
        .install();
    let read = read_keychain_credential("curator", "HKASK_DB_PASSPHRASE", &passphrase_url).await;
    assert_eq!(
        read,
        KeychainCredentialRead::Transient { timeout_secs: 5 },
        "a timed-out read is a wedged portal, not a missing entry — the key may exist"
    );
    drop(mock);

    // The Transient warn message names the deadline and the wedge, and
    // never tells the operator the key "has no keychain entry" (the
    // dishonest remediation the defect shipped).
    let message = keychain_read_warning_message(
        "curator",
        "HKASK_DB_PASSPHRASE",
        &passphrase_url,
        &KeychainCredentialRead::Transient { timeout_secs: 5 },
    );
    assert!(
        message.contains("timed out after 5s"),
        "the transient message must name the expired deadline: {message}"
    );
    assert!(
        message.contains("may be wedged"),
        "the transient message must name the wedged portal: {message}"
    );
    assert!(
        !message.contains("has no keychain entry"),
        "a timed-out read of an existing key must never be told \
         'has no keychain entry': {message}"
    );

    // The dedup state key must differ from Missing's — a wedge that
    // later flips to a genuinely absent key re-warns as a state
    // change, and a wedge never dedups into Missing's quiet slot.
    assert_ne!(
        warn_dedup_state_key(&KeychainCredentialRead::Transient { timeout_secs: 5 }),
        warn_dedup_state_key(&KeychainCredentialRead::Missing),
        "transient and missing must dedup under distinct state keys"
    );

    // Known-positive for the assertion instrument (a no-match is not
    // evidence the pattern discriminates): the Missing message DOES
    // carry "has no keychain entry".
    let missing_message = keychain_read_warning_message(
        "curator",
        "HKASK_DB_PASSPHRASE",
        &passphrase_url,
        &KeychainCredentialRead::Missing,
    );
    assert!(
        missing_message.contains("has no keychain entry"),
        "the Missing message is the known-positive for the contrast above: {missing_message}"
    );
}

/// The composed-path pin for the wedged-portal defect (card db0d8e88):
/// a timed-out credential read must leave that credential OUT of the
/// composed env — the server fails with permission_denied, honestly
/// degraded — WITHOUT taking the other credentials down with it. The
/// research server is the live incident's server (2026-10-26 11:07:
/// TAVILY/EXA/BRAVE timed out together under a wedged portal).
#[tokio::test]
async fn build_mcp_server_env_times_out_one_credential_without_dropping_the_rest() {
    let _guard = ENV_TEST_LOCK.lock().await;
    let settings = crate::KaskSettings::default();
    let research_urls =
        filter_credentials_for_server("research", &crate::credential_urls_for_mcp());
    let url_for = |env_var: &str| {
        research_urls
            .iter()
            .find(|(var, _)| var == env_var)
            .map(|(_, url)| url.clone())
            .unwrap_or_else(|| panic!("{env_var} must be in the research allowlist"))
    };
    let tavily_url = url_for("HKASK_TAVILY_API_KEY");
    let exa_url = url_for("HKASK_EXA_API_KEY");
    let passphrase_url = url_for("HKASK_DB_PASSPHRASE");

    // TAVILY wedged (typed Timeout); EXA and the DB passphrase read
    // Loaded. The passphrase MUST read Loaded — an absent passphrase
    // would arm the startup-default provisioning tier, which writes
    // the REAL keychain; no test may reach it.
    let _mock_keychain = MockKeychain::new()
        .with_timeout(&tavily_url, 5)
        .with_secret(&exa_url, b"exa-keychain-secret")
        .with_secret(&passphrase_url, b"keychain-secret-passphrase")
        .install();

    // SAFETY: clear the shell tier so TAVILY's read exercises the
    // keychain branch. Single-threaded test under ENV_TEST_LOCK.
    unsafe { std::env::remove_var("HKASK_TAVILY_API_KEY") };

    let cx = gpui::TestAppContext::single().to_async();
    let env = build_mcp_server_env(
        "research",
        &settings,
        &FakeCredentialsProvider::default(),
        None,
        None,
        &cx,
    )
    .await;

    assert_eq!(
        env.get("HKASK_TAVILY_API_KEY"),
        None,
        "the timed-out credential must not be injected — the server fails \
         with permission_denied when it tries to use it"
    );
    assert_eq!(
        env.get("HKASK_EXA_API_KEY"),
        Some("exa-keychain-secret"),
        "a wedged read of ONE credential must not take the OTHERS down"
    );
    assert_eq!(
        env.get("HKASK_DB_PASSPHRASE"),
        Some("keychain-secret-passphrase"),
        "the loaded passphrase must survive the sibling timeout"
    );
}

#[test]
fn all_mcp_servers_allowlists_match_source_env_reads() {
    fn collect_rs_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let entries = std::fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", dir.display()));
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_rs_files(&path, out);
            } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
                if !name.ends_with("_tests.rs") && name != "tests.rs" && name != "test_support.rs" {
                    out.push(path);
                }
            }
        }
    }

    let mcp_servers_root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mcp-servers");
    let prefixes = [
        "std::env::var(\"",
        "env::var(\"",
        "std::env::var_os(\"",
        "env::var_os(\"",
        "parse_env_warn(\"",
        "parse_env_u64(\"",
        "required_env(\"",
        "configured(\"",
        "resolve_model(\"",
        "resolve_credential(\"",
        "credentials.get(\"",
    ];

    for server in BUILT_IN_MCP_SERVERS {
        let src_dir = mcp_servers_root.join(server.binary).join("src");
        let mut files = Vec::new();
        collect_rs_files(&src_dir, &mut files);

        let mut read_vars = std::collections::BTreeSet::new();
        for file in files {
            let content = std::fs::read_to_string(&file)
                .unwrap_or_else(|e| panic!("failed to read {}: {e}", file.display()));
            // Stop at the first #[cfg(test)] marker so test-only env vars are ignored.
            let prod_content = content.split("#[cfg(test)]").next().unwrap_or(&content);
            for prefix in &prefixes {
                let mut rest = prod_content;
                while let Some(idx) = rest.find(prefix) {
                    // Word boundary: a prefix must not match inside a
                    // longer identifier. `model_not_configured("…")` in
                    // hkask-mcp-curator tripped the gate as a phantom env
                    // var named after the error message (observed
                    // 2026-11-13) — the character before a real read
                    // (`.` `(` `:` or whitespace) is never an identifier
                    // character.
                    let preceded_by_ident = idx > 0
                        && rest[..idx]
                            .chars()
                            .next_back()
                            .is_some_and(|c| c.is_alphanumeric() || c == '_');
                    rest = &rest[idx + prefix.len()..];
                    if preceded_by_ident {
                        continue;
                    }
                    if let Some(end) = rest.find('"') {
                        let var_name = &rest[..end];
                        if var_name != "HOME" && var_name != "PATH" {
                            read_vars.insert(var_name.to_string());
                        }
                        rest = &rest[end + 1..];
                    }
                }
            }
            // Media model env constants in `hkask_mcp_media::models`.
            for (const_ref, env_name) in [
                ("configured(STT_ENV)", "HKASK_MEDIA_STT_MODEL"),
                ("configured(VISION_ENV)", "HKASK_MEDIA_VISION_MODEL"),
                ("configured(PASS_ENV)", "HKASK_MEDIA_PASS_MODEL"),
                (
                    "configured(SOUND_ANNOTATION_ENV)",
                    "HKASK_MEDIA_SOUND_ANNOTATION_MODEL",
                ),
                ("configured(TAGGING_ENV)", "HKASK_MEDIA_TAGGING_MODEL"),
            ] {
                if prod_content.contains(const_ref) {
                    read_vars.insert(env_name.to_string());
                }
            }
        }

        let allowed: std::collections::BTreeSet<&str> = server
            .credentials
            .iter()
            .copied()
            .chain(server.config_env.iter().copied())
            .collect();

        for var in &read_vars {
            assert!(
                allowed.contains(var.as_str()),
                "MCP server '{}' ({}) reads env var '{}' in production src/, \
                 but it is missing from BuiltinMcpServer::{{credentials, config_env}}",
                server.id,
                server.binary,
                var
            );
        }
    }
}

/// Pin the load-bearing filter order in `build_mcp_server_env`: config is
/// filtered first, then credentials are resolved and merged into the
/// already-filtered map. The two filters apply to disjoint key sets — a
/// credential key must survive the config filter even if a config var of
/// the same name existed in the unfiltered `mcp_env()` map. The prior Path
/// B bug dropped every credential because the config filter ran on the
/// credential map too.
///
/// This test constructs a `KaskSettings` whose `mcp_env()` emits a config
/// var that collides with a credential key for the `curator` server, then
/// asserts the credential value (from the mock keychain) survives in the
/// composed output — not the config value, and not absent.
#[tokio::test]
async fn build_mcp_server_env_composed_path_credential_survives_config_filter() {
    let _guard = ENV_TEST_LOCK.lock().await;
    // The curator server's `credentials` allowlist includes
    // `HKASK_DB_PASSPHRASE`. We inject a mock keychain secret
    // for its credential URL and assert it lands in the composed env.
    let settings = crate::KaskSettings::default();
    // Collect the (env_var, url) pairs the curator server would receive.
    let cred_urls = filter_credentials_for_server("curator", &crate::credential_urls_for_mcp());
    // Find the DB passphrase URL; if absent, the test setup is
    // stale relative to the registry and we fail loudly.
    let passphrase_url = cred_urls
        .iter()
        .find(|(env_var, _)| env_var == "HKASK_DB_PASSPHRASE")
        .map(|(_, url)| url.clone())
        .expect("HKASK_DB_PASSPHRASE must be in the curator credentials allowlist");

    let provider = FakeCredentialsProvider::default();
    let _mock_keychain = MockKeychain::new()
        .with_secret(&passphrase_url, b"keychain-secret-passphrase")
        .install();

    // SAFETY: Ensure no shell env var leaks into the test — the keychain
    // branch must be the one that injects the value. Single-threaded test.
    unsafe { std::env::remove_var("HKASK_DB_PASSPHRASE") };

    let cx = gpui::TestAppContext::single().to_async();
    let env = build_mcp_server_env(
        "curator",
        &settings,
        &provider,
        Some("/tmp/sock"),
        Some(300),
        &cx,
    )
    .await;

    // The credential must survive the config filter and land in the
    // composed env with the keychain value — not be dropped by the config
    // filter running on the credential map (the Path B regression).
    assert_eq!(
        env.get("HKASK_DB_PASSPHRASE"),
        Some("keychain-secret-passphrase"),
        "credential must survive the config filter in the composed path — \
         the prior Path B bug dropped every credential because the config \
         filter ran on the credential map too"
    );

    // The inference socket must also be injected (it is added last,
    // outside both filters).
    assert_eq!(
        env.get(hkask_types::inference_ipc::INFERENCE_SOCKET_ENV),
        Some("/tmp/sock"),
        "inference socket must be injected into the composed env"
    );

    // The inference timeout must also be injected — it is published
    // alongside the socket so child IPC clients can align their read
    // deadline with the server's `inference_timeout`. Without this, a
    // slow-but-alive provider produces a `BrokenPipe` warn storm (client
    // times out first, server's later write hits EPIPE).
    assert_eq!(
        env.get(hkask_types::inference_ipc::INFERENCE_TIMEOUT_ENV),
        Some("300"),
        "inference timeout must be injected into the composed env so IPC \
         clients can align their read deadline with the server's"
    );
}

/// Pin the other half of the filter order: a config var that is NOT in
/// the server's `config_env` allowlist must NOT survive the composed path,
/// even if it collides with a credential key. This is the Path A
/// regression — the full unfiltered `mcp_env()` map leaked because
/// `extend` only overwrote allowed keys.
///
/// We use the `scenarios` server, whose `config_env` allowlist is
/// `["HKASK_DATA_DIR", "HKASK_SCENARIOS_DATA"]`
/// and `credentials` allowlist is `[]` (empty). A config var outside the
/// allowlist (e.g. `HKASK_RESEARCH_DB`,
/// which only the research server may receive) must be filtered out by the
/// composed path.
#[tokio::test]
async fn build_mcp_server_env_composed_path_filters_unallowed_config() {
    let _guard = ENV_TEST_LOCK.lock().await;
    // Build settings whose `mcp_env()` emits a research-only config var;
    // the composed path must filter it out for the scenarios server. If
    // the var survives, the config filter was bypassed (Path A regression).
    let mut settings = crate::KaskSettings::default();
    settings.research = crate::KaskResearchSettings {
        research_db: "/research.db".to_string(),
    };
    let provider = FakeCredentialsProvider::default();
    // Hygiene: the scenarios server's credentials allowlist is empty,
    // so no keychain read should occur — but an installed empty mock
    // guarantees none can reach the REAL keychain if the allowlist
    // ever drifts.
    let _mock_keychain = MockKeychain::new().install();

    let cx = gpui::TestAppContext::single().to_async();
    let env = build_mcp_server_env("scenarios", &settings, &provider, None, None, &cx).await;

    // The scenarios server's allowlist is
    // `[HKASK_DATA_DIR, HKASK_SCENARIOS_DATA]`.
    // `HKASK_RESEARCH_DB` (research-only config) must not survive —
    // if it does, the config filter was bypassed (Path A regression).
    assert_eq!(
        env.get("HKASK_RESEARCH_DB"),
        None,
        "unallowlisted config var must not survive the composed path — \
         the prior Path A bug leaked the full mcp_env() map"
    );
}

/// Hang-audit Pr3: one keychain read per distinct credential URL per
/// sweep. Two servers sharing `HKASK_DB_PASSPHRASE` over ONE sweep
/// consult the keychain once — the second server is served the
/// memoized outcome even after the keychain changed underneath — and a
/// FRESH sweep re-reads, preserving `nudge_mcp_servers`'s freshness
/// contract (a rotated key reaches the next restart pass).
#[tokio::test]
async fn credential_sweep_reads_each_url_once_per_sweep() {
    let _guard = ENV_TEST_LOCK.lock().await;
    let (_, passphrase_url) = crate::credential_urls_for_mcp()
        .into_iter()
        .find(|(env_var, _)| env_var == "HKASK_DB_PASSPHRASE")
        .expect("HKASK_DB_PASSPHRASE has a credential URL");
    let settings = crate::KaskSettings::default();
    let provider = FakeCredentialsProvider::default();

    let mock = MockKeychain::new()
        .with_secret(&passphrase_url, b"keychain-secret-passphrase")
        .install();
    let cx = gpui::TestAppContext::single().to_async();
    let sweep = CredentialSweep::new();
    let curator =
        build_mcp_server_env_with_sweep("curator", &settings, &provider, None, None, &sweep, &cx)
            .await;
    assert_eq!(
        curator.get("HKASK_DB_PASSPHRASE"),
        Some("keychain-secret-passphrase")
    );
    drop(mock);

    // The keychain now holds a rotated value; the shared sweep must
    // serve the memoized outcome (one read per URL per sweep).
    let mock = MockKeychain::new()
        .with_secret(&passphrase_url, b"rotated-secret-passphrase")
        .install();
    let kanban = build_mcp_server_env_with_sweep(
        "kata-kanban",
        &settings,
        &provider,
        None,
        None,
        &sweep,
        &cx,
    )
    .await;
    assert_eq!(
        kanban.get("HKASK_DB_PASSPHRASE"),
        Some("keychain-secret-passphrase"),
        "the shared sweep memoizes the URL's one read — the second \n             server is served the outcome the first server read"
    );
    drop(mock);

    // A fresh sweep re-reads: the rotated key reaches the next pass.
    let mock = MockKeychain::new()
        .with_secret(&passphrase_url, b"rotated-secret-passphrase")
        .install();
    let fresh = CredentialSweep::new();
    let curator_rotated =
        build_mcp_server_env_with_sweep("curator", &settings, &provider, None, None, &fresh, &cx)
            .await;
    assert_eq!(
        curator_rotated.get("HKASK_DB_PASSPHRASE"),
        Some("rotated-secret-passphrase"),
        "a fresh sweep re-reads the keychain — a rotated key reaches \n             the next restart pass (the nudge freshness contract)"
    );
    drop(mock);
}

// ── startup-default passphrase tests ───────────────────────────────
//
// The "allostery" default is a stated first-run requirement: DB-backed
// servers must open their SQLCipher databases without login or manual
// keychain configuration. These tests pin the wiring that guarantees it
// at the canonical env path — the vars that have a default, and that
// non-default credentials never silently gain one.

/// Pin the requirement surface: exactly the two passphrase vars carry a
/// startup default, and both are reachable through the credential loop
/// for the servers that need them (so the provisioning tier actually
/// fires — a var with a default but no credential URL would never reach
/// `provision_default_passphrase`).
#[test]
fn default_passphrase_env_vars_pin_the_requirement() {
    assert_eq!(
        DEFAULT_PASSPHRASE_ENV_VARS,
        ["HKASK_DB_PASSPHRASE"],
        "the startup-default passphrase set is a stated requirement — \
         the ONE DB passphrase defaults to 'allostery' on first run and \
         every SQLCipher DB (curator, kanban, research, \
         experimentation) opens with it; changing this set is a requirements \
         change, not a refactor"
    );

    // Both vars must be in the credential URL list so the loop reaches
    // the provisioning tier for servers that allowlist them.
    let urls = crate::credential_urls_for_mcp();
    for env_var in DEFAULT_PASSPHRASE_ENV_VARS {
        assert!(
            urls.iter().any(|(var, _)| var == env_var),
            "{env_var} must have a credential URL — otherwise the credential \
             loop never reaches provision_default_passphrase for it"
        );
    }

    // And at least one DB-backed server must allowlist the DB passphrase —
    // pins that the default actually reaches a consumer.
    let corpus_urls = filter_credentials_for_server("corpus", &urls);
    assert!(
        corpus_urls
            .iter()
            .any(|(var, _)| var == "HKASK_DB_PASSPHRASE"),
        "the corpus server must allowlist HKASK_DB_PASSPHRASE so the \
         startup default is provisioned for its launch env"
    );
}

/// Pin that non-default credentials never silently gain a default: a
/// missing API key must return `None` (falling through to the
/// missing-credential warning) rather than provisioning a value.
/// "Not configured" must stay visible for credentials that have no
/// required default.
#[tokio::test]
async fn provision_default_passphrase_returns_none_for_non_default_vars() {
    let cx = gpui::TestAppContext::single().to_async();
    assert!(
        provision_default_passphrase("HKASK_ABW_API_KEY", &cx)
            .await
            .is_none(),
        "HKASK_ABW_API_KEY has no startup default — a miss must fall through \
         to the missing-credential warning, not be silently provisioned"
    );
}

fn stdio_entry() -> settings_content::ContextServerSettingsContent {
    use settings_content::ContextServerCommand;
    settings_content::ContextServerSettingsContent::Stdio {
        enabled: true,
        remote: false,
        command: ContextServerCommand {
            path: "/bin/true".into(),
            args: Vec::new(),
            env: None,
            timeout: None,
        },
    }
}

/// The shadowing fix: a raw `context_servers` entry whose ID matches a
/// kask built-in server is removed (and reported), because it silently
/// overrides the managed registration that injects credentials and the
/// inference socket. This is the exact pattern that shipped the
/// keyless research server: a stale `env: {}` entry shadowed the
/// keyed registration with no warning anywhere.
#[test]
fn remove_shadowing_context_server_entries_removes_builtins_and_keeps_others() {
    let mut content = settings_content::SettingsContent::default();
    content
        .project
        .context_servers
        .insert("research".into(), stdio_entry());
    content
        .project
        .context_servers
        .insert("some-other-server".into(), stdio_entry());

    let removed = remove_shadowing_context_server_entries(&mut content);

    assert_eq!(
        removed.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
        vec!["research"],
        "only the kask built-in entry is removed — the caller warns per ID"
    );
    assert!(
        !content.project.context_servers.contains_key("research"),
        "the shadowing entry must be gone so the managed registration wins"
    );
    assert!(
        content
            .project
            .context_servers
            .contains_key("some-other-server"),
        "non-kask entries are the user's own servers — never touched"
    );
}

/// No shadowing entries → nothing removed, nothing to warn about. Pins
/// that the removal is a no-op on a clean configuration (the common
/// case on every settings change).
#[test]
fn remove_shadowing_context_server_entries_is_noop_without_builtins() {
    let mut content = settings_content::SettingsContent::default();
    content
        .project
        .context_servers
        .insert("some-other-server".into(), stdio_entry());

    let removed = remove_shadowing_context_server_entries(&mut content);

    assert!(
        removed.is_empty(),
        "a clean configuration must not produce removals or warnings"
    );
    assert_eq!(content.project.context_servers.len(), 1);
}

/// Detection must work on content parsed from real settings-file text,
/// not just on hand-constructed `SettingsContent`s — the live path is
/// `raw_user_settings()` → `UserSettingsContent::parse_json` output. If
/// the parse routes `context_servers` somewhere other than
/// `content.project.context_servers`, the removal silently no-ops.
#[test]
fn shadowed_detection_works_on_parsed_file_content() {
    use settings_content::RootUserSettings;

    let text = r#"{
        "context_servers": {
            "research": {"command": "/bin/true", "args": [], "env": {}},
            "my-own-server": {"command": "/bin/true", "args": [], "env": {}}
        }
    }"#;
    let (parsed, _status) = settings_content::UserSettingsContent::parse_json(text);
    let user = parsed.expect("settings text must parse");
    let shadowed = shadowed_context_server_entry_ids(&user.content);
    assert_eq!(
        shadowed.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
        vec!["research"],
        "detection must find the shadowing entry in parsed file content"
    );
}

/// The removal write goes through `edits_for_update`, which diffs
/// `serde_json::to_value(old)` against `to_value(new)`. If the
/// serialization of `UserSettingsContent` drops `context_servers`, the
/// diff is empty, `new_text == old_text`, and D32's no-op-write skip
/// silently swallows the removal — the file keeps the shadowing entry
/// forever with no error anywhere. This pins that the round-trip
/// preserves the entry so the removal produces a real edit.
#[test]
fn user_settings_serialization_preserves_context_servers_for_edit_diff() {
    use settings_content::RootUserSettings;

    let text = r#"{
        "context_servers": {
            "research": {"command": "/bin/true", "args": [], "env": {}}
        }
    }"#;
    let (parsed, _status) = settings_content::UserSettingsContent::parse_json(text);
    let user = parsed.expect("settings text must parse");

    let old_value = serde_json::to_value(&user).expect("serialize");
    assert!(
        old_value
            .get("context_servers")
            .and_then(|v| v.get("research"))
            .is_some(),
        "serialization must preserve context_servers — otherwise the edit diff \
         is empty and the removal write is silently skipped"
    );

    let mut new_user = user;
    let removed = remove_shadowing_context_server_entries(&mut new_user.content);
    assert_eq!(removed.len(), 1);
    let new_value = serde_json::to_value(&new_user).expect("serialize");
    assert!(
        new_value.get("context_servers").is_none()
            || new_value["context_servers"].get("research").is_none(),
        "after removal the serialized entry must be gone — the diff drives the file edit"
    );
}

/// A pasted trailing newline must not survive keychain→env injection
/// (the live Firecrawl failure), and the DB passphrase must never be
/// trimmed — its exact bytes key the SQLCipher databases.
#[test]
fn credential_trim_strips_whitespace_but_never_the_passphrase() {
    let dirty_key = trim_credential_for_injection(
        "research",
        "HKASK_FIRECRAWL_API_KEY",
        "fc-0123456789abcdef0123456789abcdef\n".to_string(),
    );
    assert_eq!(dirty_key, "fc-0123456789abcdef0123456789abcdef");

    let clean_key = trim_credential_for_injection(
        "research",
        "HKASK_FIRECRAWL_API_KEY",
        "fc-0123456789abcdef0123456789abcdef".to_string(),
    );
    assert_eq!(clean_key, "fc-0123456789abcdef0123456789abcdef");

    let passphrase = trim_credential_for_injection(
        "curator",
        "HKASK_DB_PASSPHRASE",
        "pass phrase \n".to_string(),
    );
    assert_eq!(passphrase, "pass phrase \n");
}
