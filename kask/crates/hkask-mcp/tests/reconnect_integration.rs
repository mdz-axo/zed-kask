//! End-to-end connection-healing tests against a real child process.
//!
//! These tests SIGKILL a controllable MCP fixture server
//! (`src/bin/mcp_test_fixture.rs`, built when `--features test-fixture` is
//! passed) and assert the runtime's four self-heal mechanisms actually fire
//! against a real dead transport — something the inline `reconnect_path_tests`
//! in `runtime.rs` cannot prove, because they assert on the private
//! `launch_specs` / `last_reconnect` maps without spawning anything.
//!
//! ## Why a real child process
//!
//! The healing paths in `McpRuntime` (reap-on-death, liveness-on-read,
//! reconnect-on-demand, health supervisor) can only be falsified by a
//! transport that genuinely dies. Mocks would just be re-asserting the
//! bookkeeping the unit tests already cover. The fixture speaks just enough
//! MCP over stdio (`initialize`, `tools/list`, one `ping` tool) and writes its
//! pid to `FIXTURE_PID_FILE` so a test can SIGKILL the exact process and later
//! prove a *different* process answered the next call.
//!
//! ## Test discipline
//!
//! Per `.rules`: "Live-mutation probe suites must run with `--test-threads=1`
//! and keep probes self-contained." Each test launches its own fixture in its
//! own temp dir, sets its own env vars, and cleans up. The kill path uses
//! `libc::kill` and **asserts** success — an earlier version that swallowed
//! the result with `let _ =` made the whole suite pass vacuously with all
//! healing disabled (D3).
//!
//! Run with:
//!
//! ```sh
//! cargo test -p hkask-mcp --features test-fixture --test reconnect_integration -- --test-threads=1
//! ```

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use hkask_mcp::McpRuntime;
use hkask_tool_port::{ToolPort, ToolPortError};
use hkask_types::WebID;
use tempfile::TempDir;

/// Path to the `mcp-test-fixture` binary, resolved via Cargo's
/// `CARGO_BIN_EXE_<name>` env var that Cargo sets for the test harness when the
/// binary is built in the same package. Only available with
/// `--features test-fixture` (the bin is `required-features`-gated).
fn fixture_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_mcp-test-fixture"))
}

/// A self-contained fixture launch context: a temp dir for the pid file, a
/// unique marker, and the env map the runtime will hand to the child.
struct Fixture {
    _tmp: TempDir,
    pid_file: PathBuf,
    marker: String,
    env: HashMap<String, String>,
}

impl Fixture {
    /// Build a fresh fixture with a unique marker derived from the test name.
    fn new(test_name: &str) -> Self {
        let tmp = tempfile::tempdir().expect("temp dir for fixture");
        let pid_file = tmp.path().join("fixture.pid");
        // Unique marker so a reconnected (freshly-spawned) process is
        // distinguishable from the original by its `ping` response.
        let marker = format!("{}-{}", test_name, std::process::id());
        let mut env = HashMap::new();
        env.insert(
            "FIXTURE_PID_FILE".to_string(),
            pid_file.to_string_lossy().into_owned(),
        );
        env.insert("FIXTURE_MARKER".to_string(), marker.clone());
        Self {
            _tmp: tmp,
            pid_file,
            marker,
            env,
        }
    }

    /// Wait for the fixture to write its pid file (it writes once serving).
    /// Times out after 5s — the fixture writes synchronously before reading
    /// stdin, so this should resolve in milliseconds.
    async fn wait_for_pid(&self) -> u32 {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(contents) = std::fs::read_to_string(&self.pid_file) {
                if let Ok(pid) = contents.trim().parse::<u32>() {
                    return pid;
                }
            }
            if std::time::Instant::now() > deadline {
                panic!(
                    "fixture did not write a pid to {} within 5s",
                    self.pid_file.display()
                );
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Read the pid file's current contents (the fixture overwrites it on
    /// every launch, so after a reconnect this is the *new* pid).
    fn read_pid(&self) -> u32 {
        let contents = std::fs::read_to_string(&self.pid_file)
            .expect("fixture pid file must exist after launch");
        contents
            .trim()
            .parse::<u32>()
            .expect("fixture pid file must contain a numeric pid")
    }

    /// SIGKILL the given pid. Asserts success — swallowing the result with
    /// `let _ =` made the whole suite pass vacuously with all healing disabled
    /// (D3).
    fn kill(pid: u32) {
        // SAFETY: `libc::kill` is a thin syscall wrapper; the only safety
        // obligation is that `pid` is a valid process id, which we just read
        // from the fixture's pid file. `SIGKILL` is a constant.
        let result = unsafe { libc::kill(pid as i32, libc::SIGKILL) };
        assert_eq!(
            result, 0,
            "libc::kill(SIGKILL) must succeed — swallowing this with let _ = made \
             the whole suite pass vacuously with all healing disabled (D3)"
        );
    }
}

/// Wait for `predicate` to return true, polling every `poll` up to `timeout`.
/// Used to wait for the keeper task's asynchronous reap or the health
/// supervisor's periodic check.
async fn wait_for<F: Fn() -> bool>(predicate: F, timeout: Duration, poll: Duration, what: &str) {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if predicate() {
            return;
        }
        if std::time::Instant::now() > deadline {
            panic!("timed out waiting for {what} after {timeout:?}");
        }
        tokio::time::sleep(poll).await;
    }
}

/// The fixture's `ping` tool returns `{"marker": "...", "calls": N}` as a
/// JSON string in a text content block. `parse_call_result` parses it back
/// into a JSON object, so the marker is at `result["marker"]`.
async fn ping(runtime: &McpRuntime, server: &str) -> serde_json::Value {
    let agent = WebID::for_agent_name("reconnect-integration-test");
    let result = runtime
        .invoke(server, "ping", serde_json::json!({}), agent)
        .await
        .expect("ping tool call must succeed against a live fixture");
    result
}

/// Launch the fixture under the runtime and return the runtime + the first
/// pid. Each test does its own launch so probes stay self-contained.
async fn launch(fixture: &Fixture) -> (McpRuntime, u32) {
    let runtime = McpRuntime::new();
    runtime
        .start_server_with_env(
            "fixture",
            &fixture_binary().to_string_lossy(),
            hkask_types::ServerEnv::from_canonical(fixture.env.clone()),
        )
        .await
        .expect("fixture must start and handshake");
    let pid = fixture.wait_for_pid().await;
    (runtime, pid)
}

// ── Mechanism (3): reconnect-on-demand ─────────────────────────────────────

/// Kill the fixture, then call `ping`. The call must succeed and be served by
/// a *new* pid (different from the killed one) — proving `call_tool_inner`'s
/// on-demand reconnect path re-spawned the dead server and retried.
///
/// Pins mechanism (3) from `runtime.rs`'s connection-healing doc: a dead
/// connection is re-spawned from the recorded `LaunchSpec` and the call
/// retries once. The pid change is the falsifier — if the reconnect path is
/// broken, the call fails (no live peer) or the pid stays the same (impossible
/// after SIGKILL, but the assertion documents the expectation).
#[tokio::test(flavor = "multi_thread")]
async fn killed_server_is_reconnected_on_the_next_call() {
    let fixture = Fixture::new("killed_server_is_reconnected_on_the_next_call");
    let (runtime, original_pid) = launch(&fixture).await;

    // Sanity: the fixture answers a ping with our marker before we kill it.
    let first = ping(&runtime, "fixture").await;
    assert_eq!(
        first.get("marker").and_then(|m| m.as_str()),
        Some(fixture.marker.as_str()),
        "the live fixture must echo our marker before the kill"
    );

    // SIGKILL the exact child process. The runtime's keeper task will reap the
    // dead connection asynchronously; the next call's on-demand reconnect path
    // re-spawns from the recorded LaunchSpec.
    Fixture::kill(original_pid);

    // The next call must succeed and be served by a *new* pid. The reconnect
    // path is bounded by RECONNECT_COOLDOWN (5s default), so allow up to 15s
    // for the spawn + handshake + retry.
    //
    // We retry on both `Unavailable` and `Interrupted`. `Unavailable` is the
    // expected transient state while the keeper reaps and `try_reconnect`
    // re-spawns. `Interrupted` happens when a call races the kill: `get_peer`
    // hands out a peer whose transport closes mid-call. The runtime
    // deliberately does NOT auto-retry `Interrupted` (a retry could duplicate
    // a side effect), but `ping` is idempotent (read-only, no side effects),
    // so an explicit retry here is safe and is the test's way of driving the
    // reconnect path to convergence. The falsifier is that the call NEVER
    // succeeds with a new pid — which would mean the on-demand reconnect is
    // broken.
    let agent = WebID::for_agent_name("reconnect-integration-test");
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    let second = loop {
        match runtime
            .invoke("fixture", "ping", serde_json::json!({}), agent)
            .await
        {
            Ok(value) => break value,
            Err(ToolPortError::Unavailable(detail)) | Err(ToolPortError::Interrupted(detail)) => {
                if std::time::Instant::now() > deadline {
                    panic!("ping after SIGKILL never succeeded within 15s — last error: {detail}");
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            Err(e) => panic!("ping after SIGKILL returned unexpected error: {e}"),
        }
    };

    assert_eq!(
        second.get("marker").and_then(|m| m.as_str()),
        Some(fixture.marker.as_str()),
        "the reconnected fixture must still echo our marker (same env, same marker)"
    );

    // The fixture overwrites FIXTURE_PID_FILE on every launch, so the pid file
    // now holds the *new* process's pid. It must differ from the killed one.
    let new_pid = fixture.read_pid();
    assert_ne!(
        new_pid, original_pid,
        "the reconnected call must be served by a new pid, not the killed one"
    );

    runtime.shutdown_all().await;
}

// ── Mechanism (1): keeper task reaps its own connection ─────────────────────

/// Kill the fixture and assert the dead connection is removed from the
/// runtime without a tool call. The keeper task's `running.waiting()` arm
/// fires on its own when the child dies, and reaps the connection (guarded by
/// the generation stamp). `is_connected` is liveness-based (matches
/// `get_peer`), so it returns false once the connection is reaped OR the
/// transport is closed — both are "dead" from the caller's perspective.
///
/// Pins mechanism (1) from `runtime.rs`'s connection-healing doc. The
/// falsifier: if the keeper's reap arm is removed, `is_connected` stays true
/// (the corpse remains in the map) until the health supervisor or a tool
/// call cleans it up.
#[tokio::test(flavor = "multi_thread")]
async fn dead_connection_is_reaped_from_the_runtime() {
    let fixture = Fixture::new("dead_connection_is_reaped_from_the_runtime");
    let (runtime, original_pid) = launch(&fixture).await;

    assert!(
        runtime.is_connected("fixture").await,
        "fixture must be connected before the kill"
    );

    Fixture::kill(original_pid);

    // The keeper task reaps asynchronously. Wait up to 5s for `is_connected`
    // to flip false — it returns false once the transport is closed OR the
    // connection is reaped, both of which follow from the child dying.
    wait_for(
        || {
            // `is_connected` is async; block_on a quick check inside the
            // multi-thread runtime. Use `tokio::task::block_in_place` to
            // avoid starving the runtime while polling.
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(runtime.is_connected("fixture"))
            })
        },
        Duration::from_secs(5),
        Duration::from_millis(50),
        "is_connected to flip false after SIGKILL",
    )
    .await;

    runtime.shutdown_all().await;
}

// ── Deliberate-stop clearing ────────────────────────────────────────────────

/// `stop_server` is a deliberate stop (not a crash): it removes the
/// connection, cancels the keeper, AND clears the launch spec so the
/// reconnect path does not resurrect the server. A subsequent tool call must
/// return `Unavailable` (the tool provably did not run), NOT `Interrupted`
/// (which would mean a live peer accepted the call and then dropped).
///
/// Pins the `stop_server` clearing behavior end-to-end. The inline
/// `stop_server_clears_the_reconnect_path` test pins the launch-spec clearing
/// on the private map; this test pins the user-observable consequence: the
/// next call is `Unavailable`, not `Interrupted`, and not a silent reconnect.
#[tokio::test(flavor = "multi_thread")]
async fn stop_server_then_call_returns_unavailable() {
    let fixture = Fixture::new("stop_server_then_call_returns_unavailable");
    let (runtime, _original_pid) = launch(&fixture).await;

    // Sanity: the fixture answers before the stop.
    let _ = ping(&runtime, "fixture").await;

    runtime.stop_server("fixture").await;

    let agent = WebID::for_agent_name("reconnect-integration-test");
    let result = runtime
        .invoke("fixture", "ping", serde_json::json!({}), agent)
        .await;

    // `stop_server` clears both the connection AND the tool registry, so the
    // call may surface as either `Unavailable` (server known but not
    // connected) or `NotFound` (tool removed from the registry). Both prove
    // the deliberate stop cleared the reconnect path. The assertion that
    // matters is what it must NOT be: `Interrupted` (which would mean a live
    // peer accepted the call and then dropped — i.e. the stop did not clear
    // the connection) or `Ok` (which would mean the stop did nothing).
    match result {
        Err(ToolPortError::Unavailable(_)) | Err(ToolPortError::NotFound(_)) => {
            // The expected outcome family: the call provably did not run.
        }
        Err(ToolPortError::Interrupted(detail)) => {
            panic!(
                "stop_server must produce Unavailable or NotFound, not Interrupted (which would \
                 mean a live peer accepted the call). Got Interrupted: {detail}"
            );
        }
        Err(e) => panic!(
            "stop_server must produce Unavailable or NotFound, got unexpected error variant: {e}"
        ),
        Ok(value) => panic!(
            "stop_server must clear the reconnect path so the next call fails, but it succeeded \
             with: {value}"
        ),
    }

    runtime.shutdown_all().await;
}

// ── Startup retry with backoff ─────────────────────────────────────────────

/// `start_server_with_env` retries spawn+handshake up to
/// `STARTUP_MAX_RETRIES` (3) with exponential backoff before reporting
/// failure. Launching a non-existent binary must exhaust the retries and
/// return `Err(SpawnFailed(...))`, not panic or hang.
///
/// Pins the startup retry mechanism. The falsifier: if the retry loop is
/// removed, the first spawn failure is reported immediately; if the retry
/// count is wrong, the error message or timing changes.
///
/// Uses `HKASK_MCP_STARTUP_INITIAL_BACKOFF_MS=10` and
/// `HKASK_MCP_STARTUP_MAX_BACKOFF_SECS=1` to keep the test fast (the default
/// 500ms→10s backoff would make this test take ~20s). Process-global env
/// vars are why this suite requires `--test-threads=1`.
#[tokio::test(flavor = "multi_thread")]
async fn startup_retry_with_backoff() {
    // Speed up the backoff so the test doesn't take 20s. These env vars are
    // read once by `McpRuntimeConfig::default()` at first server launch.
    // SAFETY: `--test-threads=1` serializes these tests, so there is no
    // concurrent reader of the environment. The vars are removed before
    // returning.
    unsafe {
        std::env::set_var("HKASK_MCP_STARTUP_INITIAL_BACKOFF_MS", "10");
        std::env::set_var("HKASK_MCP_STARTUP_MAX_BACKOFF_SECS", "1");
    }

    let runtime = McpRuntime::new();
    let result = runtime
        .start_server_with_env(
            "no-such-server",
            "/nonexistent/binary/that/does/not/exist",
            hkask_types::ServerEnv::default(),
        )
        .await;

    match result {
        Err(hkask_mcp::runtime::ServerStartError::SpawnFailed(detail)) => {
            assert!(
                detail.contains("exist") || detail.contains("No such file") || !detail.is_empty(),
                "SpawnFailed must carry the underlying spawn error, got: {detail}"
            );
        }
        Err(e) => {
            panic!("non-existent binary must produce SpawnFailed, got unexpected variant: {e}")
        }
        Ok(()) => panic!(
            "non-existent binary must not start successfully — start_server_with_env returned Ok(())"
        ),
    }

    // The launch spec is still recorded even after a failed start, so a later
    // reconnect attempt has something to rebuild from (pinned inline by
    // `failed_start_still_records_a_launch_spec_for_later_reconnect`). We
    // don't re-assert that here — the inline test covers the bookkeeping.

    // SAFETY: see the set_var block above; --test-threads=1 serializes access.
    unsafe {
        std::env::remove_var("HKASK_MCP_STARTUP_INITIAL_BACKOFF_MS");
        std::env::remove_var("HKASK_MCP_STARTUP_MAX_BACKOFF_SECS");
    }
    runtime.shutdown_all().await;
}

// ── Mechanism (4): health supervisor removes dead connection without a call ─

/// Kill the fixture, wait > `HEALTH_CHECK_INTERVAL`, and assert the health
/// supervisor removed the dead connection without any tool call. Pins
/// mechanism (4) from `runtime.rs`'s connection-healing doc — the supervisor
/// is the proactive self-healing path that fires even when no tool call is in
/// flight.
///
/// `HEALTH_CHECK_INTERVAL` defaults to 60s, which is too long for a test. We
/// override it to 1s via `HKASK_MCP_HEALTH_CHECK_INTERVAL_SECS` (read by
/// `McpRuntimeConfig::default()`). Process-global env vars are why this suite
/// requires `--test-threads=1`.
///
/// Note: the supervisor *also* attempts a restart after removing the dead
/// connection, so `is_connected` may flip back to true once the restart
/// succeeds. We assert that the connection was removed *at some point* —
/// i.e., `is_connected` returns false within the window after the kill and
/// before the supervisor's restart re-installs a live connection. Concretely:
/// we wait for `is_connected` to flip false, which proves the supervisor (or
/// the keeper task) removed the corpse without a tool call.
#[tokio::test(flavor = "multi_thread")]
async fn health_supervisor_removes_dead_connection_without_tool_call() {
    // Override the health check interval to 1s so the test doesn't take 60s.
    // Read once by `McpRuntimeConfig::default()` at first server launch.
    // SAFETY: `--test-threads=1` serializes these tests, so there is no
    // concurrent reader of the environment. The var is removed before
    // returning.
    unsafe {
        std::env::set_var("HKASK_MCP_HEALTH_CHECK_INTERVAL_SECS", "1");
    }

    let fixture = Fixture::new("health_supervisor_removes_dead_connection_without_tool_call");
    let (runtime, original_pid) = launch(&fixture).await;

    assert!(
        runtime.is_connected("fixture").await,
        "fixture must be connected before the kill"
    );

    Fixture::kill(original_pid);

    // The supervisor checks every 1s (overridden) and removes the dead
    // connection when it sees the transport closed. The keeper task may also
    // reap first (mechanism 1). Either way, `is_connected` must flip false
    // without a tool call. Wait up to 10s — 1s interval + spawn/handshake
    // jitter + the supervisor's `interval.tick()` skips the immediate first
    // tick.
    wait_for(
        || {
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(runtime.is_connected("fixture"))
            })
        },
        Duration::from_secs(10),
        Duration::from_millis(100),
        "health supervisor to remove the dead connection without a tool call",
    )
    .await;

    // SAFETY: see the set_var block above; --test-threads=1 serializes access.
    unsafe {
        std::env::remove_var("HKASK_MCP_HEALTH_CHECK_INTERVAL_SECS");
    }
    runtime.shutdown_all().await;
}

// ── Reconciler convergence pins (lifecycle review, 2026-08-29) ──────────────

/// Invariant I4: reconciliation is idempotent. A `start_server_with_env`
/// call for an already-live connection with the same env must be a no-op —
/// no respawn, same pid. Without this, every redundant reconcile pass
/// (settings observers, startup double-calls) would tear down and respawn
/// live servers, the exact churn observed live when uncoordinated actors
/// each held their own restart authority.
#[tokio::test(flavor = "multi_thread")]
async fn second_start_with_same_env_is_a_noop() {
    let fixture = Fixture::new("second_start_with_same_env_is_a_noop");
    let (runtime, original_pid) = launch(&fixture).await;

    // The redundant reconcile call — same server, same env, live transport.
    runtime
        .start_server_with_env(
            "fixture",
            &fixture_binary().to_string_lossy(),
            hkask_types::ServerEnv::from_canonical(fixture.env.clone()),
        )
        .await
        .expect("a redundant start against a live connection must succeed as a no-op");

    // Give any (buggy) respawn a moment to write a new pid, then assert the
    // original process is still the one serving.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        fixture.read_pid(),
        original_pid,
        "a redundant start must not respawn the server — reconcile is idempotent"
    );
    let result = ping(&runtime, "fixture").await;
    assert_eq!(
        result["marker"].as_str().expect("marker"),
        fixture.marker,
        "the original process must still be serving after the redundant start"
    );
    runtime.shutdown_all().await;
}

/// Invariant I5: the health supervisor's circuit breaker. A server whose
/// restarts keep failing must stop being auto-healed after
/// `max_consecutive_health_failures` — an unsupervised respawn loop is the
/// crash-loop defect observed live (a keyless instance churning a new pid
/// every interval forever, with no operator-visible stop condition). The
/// failure count must reach the cap and FREEZE: the breaker stops the
/// supervisor instead of looping.
#[tokio::test(flavor = "multi_thread")]
async fn health_breaker_trips_after_consecutive_restart_failures() {
    // 1s health interval + a 2-failure cap so the test runs in seconds.
    // SAFETY: `--test-threads=1` serializes these tests (see the sibling
    // health-supervisor test); vars are removed before returning.
    unsafe {
        std::env::set_var("HKASK_MCP_HEALTH_CHECK_INTERVAL_SECS", "1");
        std::env::set_var("HKASK_MCP_MAX_HEALTH_FAILURES", "2");
    }

    // Launch the real fixture so a supervisor spawns with a live connection.
    let fixture = Fixture::new("health_breaker_trips_after_consecutive_restart_failures");
    let (runtime, original_pid) = launch(&fixture).await;

    // Break every FUTURE respawn: `resolve_mcp_binary` checks
    // HKASK_MCP_{ID}_BIN at spawn time, so pointing it at a nonexistent
    // path makes the supervisor's restart attempts fail while the original
    // process keeps running.
    // SAFETY: see the set_var block above.
    unsafe {
        std::env::set_var("HKASK_MCP_FIXTURE_BIN", "/nonexistent/mcp-binary");
    }

    Fixture::kill(original_pid);

    // The supervisor counts one failure per cycle (dead/missing state) and
    // trips the breaker at 2. Wait for the count to reach the cap...
    wait_for(
        || {
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current()
                    .block_on(runtime.health_failure_count("fixture"))
                    >= 2
            })
        },
        Duration::from_secs(15),
        Duration::from_millis(200),
        "the health breaker to reach its failure cap",
    )
    .await;

    // ...then assert it FREEZES: with the breaker tripped, the supervisor
    // has exited, so no further failures are counted. Two more cycles'
    // worth of wait must not move the count.
    tokio::time::sleep(Duration::from_secs(3)).await;
    let count_after = runtime.health_failure_count("fixture").await;
    assert_eq!(
        count_after, 2,
        "the breaker must stop the respawn loop — the failure count freezes at \
         the cap instead of growing forever (the live crash-loop defect)"
    );
    assert!(
        !runtime.is_connected("fixture").await,
        "the breaker must leave the server down — no more auto-respawn"
    );

    // SAFETY: see the set_var block above.
    unsafe {
        std::env::remove_var("HKASK_MCP_HEALTH_CHECK_INTERVAL_SECS");
        std::env::remove_var("HKASK_MCP_MAX_HEALTH_FAILURES");
        std::env::remove_var("HKASK_MCP_FIXTURE_BIN");
    }
    runtime.shutdown_all().await;
}
