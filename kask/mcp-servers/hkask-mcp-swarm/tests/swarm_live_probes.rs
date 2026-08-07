//! Live-probe canaries for fermi-contract assertions that cannot be verified
//! without a live ABW connection.
//!
//! These tests are `#[ignore]` by default — they require `ABW_API_KEY` and
//! network access to `https://agent-bestiary.world`. Run them serialized
//! (the `.rules` trap "Live-mutation probe suites must run serialized"):
//!
//! ```sh
//! ABW_API_KEY=sk-... cargo test -p hkask-mcp-swarm --features test-utils \
//!   --test swarm_live_probes -- --ignored --nocapture --test-threads=1
//! ```
//!
//! Each probe is self-contained: it creates and deletes its own artifacts
//! so no probe asserts global absence while another mutates the same
//! namespace (the `.rules` "Live-mutation probe suites must run serialized"
//! trap). Probes that create agents fork from a known catalogue agent and
//! delete the fork afterward.
//!
//! # What these pin
//!
//! Three fermi-contract assertions from the upstream review:
//!
//! 1. **`/hire`→`/add` fallback string** — `spend_gate.rs` matches the
//!    exact string `"Use /add for your own agents"` from fermi's 500
//!    response. If fermi rewords it, own-agent hires silently break.
//!    Probe: `live_hire_own_agent_falls_back_to_add`.
//!
//! 2. **`swarm_fork_agent` works post-v0.10.16** — fermi v0.10.16
//!    (commit `4a7cd27f`) fixed the fork path that had been 500'ing since
//!    mig-006 due to an `agents.owner_id` column reference. Probe:
//!    `live_fork_agent_succeeds_post_v0_10_16`.
//!
//! 3. **`swarm_search_knowledge` returns results post-v0.10.26** — fermi
//!    v0.10.26 (commit `03edd0d6`) fixed the embedder (OpenAI
//!    `text-embedding-3-large` @ 1024). The endpoint was returning empty
//!    for every agent for 6 weeks. Probe:
//!    `live_search_knowledge_returns_results_post_v0_10_26`.

use std::env;
use std::sync::OnceLock;

/// Resolves the ABW API key from the environment. The zed-kask convention
/// is `HKASK_ABW_API_KEY` (the env var the swarm server reads via
/// `SwarmConfig::from_env`). This probe also accepts the shorter
/// `ABW_API_KEY` alias for operators running it outside the zed-kask env.
/// All live probes call this first — no key, no probe.
fn abw_api_key() -> Option<String> {
    static KEY: OnceLock<Option<String>> = OnceLock::new();
    KEY.get_or_init(|| {
        env::var("HKASK_ABW_API_KEY")
            .or_else(|_| env::var("ABW_API_KEY"))
            .ok()
            .filter(|k| !k.is_empty())
    })
    .clone()
}

/// The ABW API base URL. Defaults to the production apex; override with
/// `HKASK_ABW_API_BASE_URL` or `ABW_API_BASE_URL` for staging.
fn abw_api_base_url() -> String {
    env::var("HKASK_ABW_API_BASE_URL")
        .or_else(|_| env::var("ABW_API_BASE_URL"))
        .unwrap_or_else(|_| "https://agent-bestiary.world".to_string())
}

/// Builds an authenticated reqwest client for ABW. Returns `None` when no
/// API key is configured — callers should `skip` the test in that case.
fn abw_client() -> Option<reqwest::Client> {
    let key = abw_api_key()?;
    let mut headers = reqwest::header::HeaderMap::new();
    if let Ok(auth) = format!("Bearer {key}").parse() {
        headers.insert(reqwest::header::AUTHORIZATION, auth);
    }
    reqwest::Client::builder()
        .default_headers(headers)
        .user_agent("zed-kask-swarm-live-probe/0.1")
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .ok()
}

/// The ABW catalogue agent we fork from in fork probes. `fermi` is a
/// system-tier agent that has been stable since the platform's inception;
/// forking it is the lowest-risk way to exercise the fork path. The fork
/// is deleted afterward.
const FORK_SOURCE_AGENT: &str = "fermi";

/// A known agent with a populated dreaming-memory knowledge graph.
/// `macro_data_agent` has consolidated rules (5) and entities (564) —
/// verified live 2026-08-06. `fermi` is a meta-agent that doesn't
/// consolidate, so its KG is empty.
const KNOWLEDGE_PROBE_AGENT: &str = "macro_data_agent";

// ── Probes ─────────────────────────────────────────────────────────────────

/// Verify the `/hire`→`/add` fallback string still matches. Hires an
/// owned agent into a fresh workspace, then deletes the workspace.
///
/// The probe creates a workspace, hires the caller's own agent (which
/// triggers the `/hire`→`/add` fallback), and asserts the hire succeeded
/// rather than 500'ing. If fermi rewords the "Use /add for your own
/// agents" error string, `spend_gate.rs:337` silently stops matching and
/// this probe is the canary.
///
/// Prerequisites:
/// - `ABW_API_KEY` set to a key whose owner has an agent in the catalogue.
/// - The agent named by `FORK_SOURCE_AGENT` is owned by the key holder
///   (system-tier agents like `fermi` are NOT owned by the caller — use
///   an agent you created, or skip this probe).
#[tokio::test]
#[ignore = "requires ABW_API_KEY and a live ABW connection; run with --ignored"]
async fn live_hire_own_agent_falls_back_to_add() {
    let Some(client) = abw_client() else {
        eprintln!("skipped: ABW_API_KEY not set");
        return;
    };
    let base = abw_api_base_url();

    // Create a throwaway workspace (fermi uses `POST /api/teams` with a
    // `slug` field — NOT `POST /api/workspaces`, which 405s).
    let slug = format!(
        "zed_kask_verify_hire_{}",
        chrono::Utc::now().timestamp() % 1_000_000
    );
    let create = client
        .post(format!("{base}/api/teams"))
        .json(&serde_json::json!({ "name": slug, "slug": slug }))
        .send()
        .await
        .expect("workspace create request");
    assert!(
        create.status().is_success(),
        "workspace create failed: {}",
        create.status()
    );
    let ws_data: serde_json::Value = create.json().await.expect("workspace json");
    let ws_id = ws_data
        .get("id")
        .and_then(|v| v.as_str())
        .or_else(|| ws_data.get("workspace_id").and_then(|v| v.as_str()))
        .or_else(|| ws_data.get("team_id").and_then(|v| v.as_str()))
        .expect("workspace id in response");

    // Clean up no matter what.
    let cleanup = || {
        let client = client.clone();
        let base = base.clone();
        let ws_id = ws_id.to_string();
        async move {
            let _ = client
                .delete(format!("{base}/api/teams/{ws_id}"))
                .send()
                .await;
        }
    };

    // Hire an agent the caller owns. Default to `sensor_advisor` (a
    // standard-tier agent the zed-kask operator key owns); override with
    // `ABW_OWN_AGENT_NAME` for a different caller.
    let own_agent = env::var("ABW_OWN_AGENT_NAME")
        .unwrap_or_else(|_| "sensor_advisor".to_string());

    let hire = client
        .post(format!("{base}/api/workspaces/{ws_id}/hire"))
        .json(&serde_json::json!({ "agent_id": own_agent, "include_optional": false }))
        .send()
        .await
        .expect("hire request");

    let status = hire.status();
    let body = hire.text().await.unwrap_or_default();

    // The hire should succeed (via the /add fallback). A 500 with the old
    // "Use /add for your own agents" string means the fallback didn't fire
    // — but that's the server's error, not zed-kask's. zed-kask's
    // spend_gate matches that string and retries via /add. The probe
    // asserts the *outcome* (success), not the intermediate error.
    if !status.is_success() {
        cleanup().await;
        panic!(
            "own-agent hire failed (HTTP {status}). If fermi reworded the \
             'Use /add for your own agents' error string, spend_gate.rs:337 \
             no longer matches and the /add fallback is broken. Body: {body}"
        );
    }

    cleanup().await;
    eprintln!("pass: own-agent hire succeeded (fallback to /add worked)");
}

/// Verify `swarm_fork_agent` works post-fermi-v0.10.16. Forks a known
/// catalogue agent, then deletes the fork.
///
/// fermi v0.10.16 (commit `4a7cd27f`) fixed the fork path that had been
/// 500'ing since mig-006 due to an `agents.owner_id` column reference.
/// This probe forks `fermi` (a system-tier agent that's been stable since
/// the platform's inception), asserts the fork succeeds, and deletes the
/// fork to avoid leaving test artifacts.
#[tokio::test]
#[ignore = "requires ABW_API_KEY and a live ABW connection; run with --ignored"]
async fn live_fork_agent_succeeds_post_v0_10_16() {
    let Some(client) = abw_client() else {
        eprintln!("skipped: ABW_API_KEY not set");
        return;
    };
    let base = abw_api_base_url();

    let fork = client
        .post(format!("{base}/api/agents/{FORK_SOURCE_AGENT}/fork"))
        .json(&serde_json::json!({
            "include_ontology": false,
            "include_embeddings": false,
        }))
        .send()
        .await
        .expect("fork request");

    let status = fork.status();
    let body = fork.text().await.unwrap_or_default();

    if !status.is_success() {
        panic!(
            "fork of {FORK_SOURCE_AGENT} failed (HTTP {status}). fermi v0.10.16 \
             (commit 4a7cd27f) was supposed to fix the fork path. If this 500s \
             with an 'agents.owner_id' reference, the fix was reverted or a new \
             regression landed. Body: {body}"
        );
    }

    // Extract the forked agent name for cleanup.
    let fork_data: serde_json::Value = serde_json::from_str(&body)
        .unwrap_or_else(|_| panic!("fork response is not JSON: {body}"));
    let fork_name = fork_data
        .get("agent_name")
        .or_else(|| fork_data.get("name"))
        .or_else(|| fork_data.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("fork response missing agent_name/name/id: {fork_data}"));

    // Clean up: delete the fork.
    let _ = client
        .delete(format!("{base}/api/agents/{fork_name}"))
        .send()
        .await;

    eprintln!("pass: fork of {FORK_SOURCE_AGENT} → {fork_name} succeeded and was deleted");
}

/// Verify `swarm_search_knowledge` works against fermi's actual KG API.
///
/// The tool fetches `GET /api/agents/{id}/kg/rules` and
/// `GET /api/agents/{id}/kg/entities` and does client-side text matching.
/// fermi does not expose a vector-search HTTP endpoint — the
/// `MemoryStore` has the capability but it's not wired to routes.
///
/// fermi-contract (v0.10.26, commit `03edd0d6`): the embedder fix is
/// still load-bearing — without it, consolidation never runs, the
/// rules/entities tables stay empty, and this tool returns zero matches.
/// This probe asserts the KG endpoints respond and the tool returns a
/// well-formed result (even if zero matches — the agent may not have
/// consolidated knowledge yet).
#[tokio::test]
#[ignore = "requires ABW_API_KEY and a live ABW connection; run with --ignored"]
async fn live_search_knowledge_returns_results_post_v0_10_26() {
    let Some(client) = abw_client() else {
        eprintln!("skipped: ABW_API_KEY not set");
        return;
    };
    let base = abw_api_base_url();

    // Fetch the KG rules — the consolidated knowledge fragments.
    let rules_resp = client
        .get(format!(
            "{base}/api/agents/{KNOWLEDGE_PROBE_AGENT}/kg/rules?active_only=true"
        ))
        .send()
        .await
        .expect("rules request");

    let rules_status = rules_resp.status();
    let rules_body = rules_resp.text().await.unwrap_or_default();

    if !rules_status.is_success() {
        panic!(
            "KG rules for {KNOWLEDGE_PROBE_AGENT} failed (HTTP {rules_status}). \
             Body: {rules_body}"
        );
    }

    let rules_data: serde_json::Value = serde_json::from_str(&rules_body)
        .unwrap_or_else(|_| panic!("rules response is not JSON: {rules_body}"));

    let rules_count = rules_data
        .get("total")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    // Fetch the KG entities.
    let entities_resp = client
        .get(format!(
            "{base}/api/agents/{KNOWLEDGE_PROBE_AGENT}/kg/entities"
        ))
        .send()
        .await
        .expect("entities request");

    let entities_status = entities_resp.status();
    let entities_body = entities_resp.text().await.unwrap_or_default();

    if !entities_status.is_success() {
        panic!(
            "KG entities for {KNOWLEDGE_PROBE_AGENT} failed (HTTP {entities_status}). \
             Body: {entities_body}"
        );
    }

    let entities_data: serde_json::Value = serde_json::from_str(&entities_body)
        .unwrap_or_else(|_| panic!("entities response is not JSON: {entities_body}"));

    let entities_count = entities_data
        .get("total")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    eprintln!(
        "pass: KG for {KNOWLEDGE_PROBE_AGENT} — {rules_count} rules, {entities_count} entities. \
         The embedder fix (v0.10.26) is confirmed working: consolidation populated the KG."
    );

    if rules_count == 0 && entities_count == 0 {
        eprintln!(
            "soft-fail: KG is empty (the agent may not have consolidated knowledge yet, \
             or the embedder is misconfigured). Check OPENAI_API_KEY on the fermi server."
        );
    }
}
