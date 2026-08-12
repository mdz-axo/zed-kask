//! Connection healing against a **real child process**.
//!
//! The unit tests in `runtime.rs` cover the bookkeeping around healing (launch
//! specs recorded, cooldown enforced, stop clears the reconnect path). They
//! cannot prove the thing that actually broke in production: that a server whose
//! process *dies* is detected, reconnected, and serves the next call. That needs
//! a genuine transport death, which needs a genuine child process.
//!
//! These tests drive `mcp-test-fixture` (see `src/bin/mcp_test_fixture.rs`), a
//! minimal MCP server whose failure behavior is scriptable via env vars.
//!
//! # Why this suite is `--test-threads=1`-safe without the flag
//!
//! Each test constructs its own `McpRuntime` and its own temp files, and the
//! fixture is addressed via `HKASK_MCP_<ID>_BIN` with a per-test server id. No
//! process-global state is touched, so these run in parallel safely \u2014 unlike the
//! live-mutation probe suites the repo `.rules` calls out.
//!
//! # Oracle
//! - A killed server is reconnected on the next call, and the call succeeds.
//! - The reconnected server is a *different* process (healing, not a stale peer).
//! - A server that dies mid-call reports `Interrupted`, never `Unavailable` \u2014
//!   the outcome is unknown, so it must not be presented as retry-safe.
//! - `stop_server` is not undone by the reconnect path.

use hkask_capability::{ToolPort, ToolPortError};
use hkask_mcp::McpRuntime;
use std::collections::HashMap;
use std::time::Duration;

/// Absolute path to the fixture binary, provided by cargo for `[[bin]]` targets.
const FIXTURE_BIN: &str = env!("CARGO_BIN_EXE_mcp-test-fixture");

fn agent() -> hkask_types::WebID {
    hkask_types::WebID::from_persona(b"reconnect-integration-test")
}

/// Point `resolve_mcp_binary` at the fixture for `server_id`.
///
/// `McpRuntime::start_server_with_env` resolves `HKASK_MCP_{ID}_BIN` before
/// falling back to PATH, which is the documented override hook \u2014 so this needs no
/// test-only seam in the runtime. Server ids are unique per test, so the env var
/// names are too, and parallel tests do not collide.
fn point_at_fixture(server_id: &str) {
    let var = format!("HKASK_MCP_{}_BIN", server_id.to_uppercase());
    // SAFETY: `set_var` is not thread-safe in general. Each test uses a unique
    // server id, hence a unique var name, so no two tests race the same key.
    unsafe { std::env::set_var(var, FIXTURE_BIN) };
}

/// Wait for a predicate to hold, polling briefly.
///
/// The keeper task that reaps a dead connection runs on the tokio scheduler, so
/// its effect is not synchronous with the child's death. Polling with a deadline
/// is the honest way to observe it \u2014 a fixed sleep would either be flaky or
/// needlessly slow.
async fn wait_until<F>(label: &str, mut predicate: F)
where
    F: AsyncPredicate,
{
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        if predicate.check().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("timed out waiting for: {label}");
}

/// Minimal async-predicate trait so `wait_until` can take an async closure.
trait AsyncPredicate {
    fn check(&mut self) -> impl std::future::Future<Output = bool>;
}

impl<F, Fut> AsyncPredicate for F
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    fn check(&mut self) -> impl std::future::Future<Output = bool> {
        self()
    }
}

/// Read the marker a `ping` result carries, identifying which fixture process
/// answered.
fn marker_of(value: &serde_json::Value) -> String {
    value
        .get("marker")
        .and_then(|m| m.as_str())
        .unwrap_or_default()
        .to_string()
}

/// Whether a pid is still a live process.
///
/// Reads `/proc`, which is authoritative on Linux (zed-kask is Linux-only) and
/// avoids trusting the exit status of an external `kill`.
fn is_alive(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}

/// Send SIGKILL to a pid and block until `/proc` shows it gone.
///
/// Uses `libc::kill` rather than spawning `/bin/kill`: the repo lint bans the
/// blocking `Command::status`, and a direct syscall is both cheaper and lets us
/// read `errno` instead of guessing from an exit code.
///
/// Asserts rather than ignoring failure. A kill that silently did nothing would
/// make every test in this file vacuous — they would "pass" by never actually
/// testing a dead server. That exact failure mode bit during development: an
/// earlier version swallowed the result with `let _ =`, and the healing test
/// passed even with all three healing mechanisms disabled.
fn kill_and_wait(pid: u32) {
    // SAFETY: `kill` is a plain syscall wrapper; `pid` came from the fixture's
    // own `std::process::id()`, and a stale pid yields ESRCH rather than UB.
    let outcome = unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
    assert_eq!(
        outcome,
        0,
        "kill(SIGKILL) on pid {pid} failed (errno {}); the test would otherwise pass \
         vacuously against a still-running server",
        std::io::Error::last_os_error()
    );

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if !is_alive(pid) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("process {pid} was still alive 5s after SIGKILL");
}

/// The headline regression: a server whose process is killed is reconnected on
/// the next tool call, and that call succeeds.
///
/// Before the healing work, this was permanently broken: the keeper task exited
/// without removing the connection, `get_peer` kept handing out the dead peer,
/// and `start_server_with_env`'s presence-based idempotency check refused to
/// replace it. Every subsequent call returned `Transport closed` until an
/// operator changed a setting.
#[tokio::test(flavor = "multi_thread")]
async fn killed_server_is_reconnected_on_the_next_call() {
    let server_id = "reconnect-heals";
    point_at_fixture(server_id);

    let pid_file = std::env::temp_dir().join(format!("hkask-fixture-{server_id}.pid"));
    let _ = std::fs::remove_file(&pid_file);

    let runtime = McpRuntime::new();
    let mut env = HashMap::new();
    env.insert("FIXTURE_MARKER".to_string(), "first".to_string());
    env.insert(
        "FIXTURE_PID_FILE".to_string(),
        pid_file.to_string_lossy().to_string(),
    );

    runtime
        .start_server_with_env(server_id, "unused-command", env)
        .await
        .expect("fixture server starts");

    // Baseline: the original process answers.
    let first = runtime
        .invoke(server_id, "ping", serde_json::json!({}), agent())
        .await
        .expect("the first call reaches the live fixture");
    assert_eq!(
        marker_of(&first),
        "first",
        "the original process must answer the first call"
    );

    let pid: u32 = std::fs::read_to_string(&pid_file)
        .expect("fixture writes its pid")
        .trim()
        .parse()
        .expect("pid is numeric");

    // Kill the server out from under the runtime, exactly as a crash or an
    // external restart would.
    kill_and_wait(pid);

    // The next call must heal rather than fail. The reconnect re-reads the
    // recorded launch spec, so the replacement inherits the same env \u2014 including
    // the marker, which is why identity is checked by pid below rather than by
    // marker here.
    let after_kill = runtime
        .invoke(server_id, "ping", serde_json::json!({}), agent())
        .await;

    let healed = match after_kill {
        Ok(value) => value,
        // A killed server can also surface as one `Interrupted`/`Unavailable`
        // before the reconnect lands, depending on scheduling. Retry once: the
        // claim under test is that the runtime heals, not that it heals on the
        // very first attempt after SIGKILL.
        Err(_) => runtime
            .invoke(server_id, "ping", serde_json::json!({}), agent())
            .await
            .expect(
                "after a server is killed, the runtime must reconnect and serve the call \
                 rather than failing until an operator changes a setting",
            ),
    };
    assert_eq!(
        marker_of(&healed),
        "first",
        "the reconnected process is launched from the recorded spec, so it carries \
         the same marker"
    );

    let new_pid: u32 = std::fs::read_to_string(&pid_file)
        .expect("the reconnected fixture rewrites the pid file")
        .trim()
        .parse()
        .expect("pid is numeric");
    assert_ne!(
        new_pid, pid,
        "healing must spawn a NEW process - an unchanged pid would mean the runtime \
         somehow reused the dead peer"
    );

    runtime.shutdown_all().await;
    let _ = std::fs::remove_file(&pid_file);
}

/// A server that dies *after* accepting a request reports `Interrupted`, not
/// `Unavailable`.
///
/// This is the duplicate-side-effect guard. `rmcp` reports both a failed send and
/// a dropped response channel as `ServiceError::TransportClosed`, so once a
/// request has reached a live peer the runtime cannot claim it never ran. If this
/// regressed to `Unavailable`, panels would read it as retry-safe and could
/// re-issue a state-changing tool \u2014 two tasks created, a hire charged twice.
#[tokio::test(flavor = "multi_thread")]
async fn server_dying_mid_call_reports_unknown_outcome_not_safe_retry() {
    let server_id = "reconnect-midcall";
    point_at_fixture(server_id);

    let runtime = McpRuntime::new();
    let mut env = HashMap::new();
    // Die before answering the very first call.
    env.insert("FIXTURE_EXIT_AFTER_CALLS".to_string(), "1".to_string());

    runtime
        .start_server_with_env(server_id, "unused-command", env)
        .await
        .expect("fixture server starts");

    let result = runtime
        .invoke(server_id, "ping", serde_json::json!({}), agent())
        .await;

    match result {
        Err(ToolPortError::Interrupted(_)) => {}
        Err(ToolPortError::Unavailable(detail)) => panic!(
            "a server that died after accepting the request must NOT report Unavailable \
             (which callers treat as safe to retry) - the effect may have been applied. \
             Got: {detail}"
        ),
        other => panic!("expected Interrupted for a mid-call death, got: {other:?}"),
    }

    runtime.shutdown_all().await;
}

/// `Interrupted` is not retryable, so the classification a panel branches on
/// matches what the transport can actually prove.
#[tokio::test(flavor = "multi_thread")]
async fn mid_call_death_is_not_advertised_as_retryable() {
    let server_id = "reconnect-midcall-retryable";
    point_at_fixture(server_id);

    let runtime = McpRuntime::new();
    let mut env = HashMap::new();
    env.insert("FIXTURE_EXIT_AFTER_CALLS".to_string(), "1".to_string());
    runtime
        .start_server_with_env(server_id, "unused-command", env)
        .await
        .expect("fixture server starts");

    let error = runtime
        .invoke(server_id, "ping", serde_json::json!({}), agent())
        .await
        .expect_err("the fixture exits before responding");

    assert!(
        !error.is_retryable(),
        "a call whose outcome is unknown must not be advertised as retryable: {error:?}"
    );

    runtime.shutdown_all().await;
}

/// The dead connection is actually removed from the runtime, not merely skipped.
///
/// Pins mechanism (1) of the healing design \u2014 reap-on-death. Observing the reap
/// directly, rather than only its downstream effect, keeps the test honest if the
/// reconnect path were ever to mask a missing reap.
#[tokio::test(flavor = "multi_thread")]
async fn dead_connection_is_reaped_from_the_runtime() {
    let server_id = "reconnect-reap";
    point_at_fixture(server_id);

    let pid_file = std::env::temp_dir().join(format!("hkask-fixture-{server_id}.pid"));
    let _ = std::fs::remove_file(&pid_file);

    let runtime = McpRuntime::new();
    let mut env = HashMap::new();
    env.insert(
        "FIXTURE_PID_FILE".to_string(),
        pid_file.to_string_lossy().to_string(),
    );
    runtime
        .start_server_with_env(server_id, "unused-command", env)
        .await
        .expect("fixture server starts");

    // Force the handshake to complete so the pid file exists.
    runtime
        .invoke(server_id, "ping", serde_json::json!({}), agent())
        .await
        .expect("the live fixture answers");

    let pid: u32 = std::fs::read_to_string(&pid_file)
        .expect("fixture writes its pid")
        .trim()
        .parse()
        .expect("pid is numeric");
    kill_and_wait(pid);

    wait_until("the dead connection to be reaped", || async {
        !runtime.is_connected(server_id).await
    })
    .await;

    runtime.shutdown_all().await;
    let _ = std::fs::remove_file(&pid_file);
}

/// A deliberate `stop_server` is not undone by the reconnect path.
///
/// The reconnect machinery exists to heal *unintended* death. If it also
/// resurrected servers an operator stopped, a settings-driven shutdown would
/// silently come back on the next tool call.
#[tokio::test(flavor = "multi_thread")]
async fn stopped_server_is_not_resurrected_by_a_tool_call() {
    let server_id = "reconnect-stopped";
    point_at_fixture(server_id);

    let runtime = McpRuntime::new();
    runtime
        .start_server_with_env(server_id, "unused-command", HashMap::new())
        .await
        .expect("fixture server starts");
    runtime
        .invoke(server_id, "ping", serde_json::json!({}), agent())
        .await
        .expect("the live fixture answers");

    runtime.stop_server(server_id).await;

    let after_stop = runtime
        .invoke(server_id, "ping", serde_json::json!({}), agent())
        .await;
    assert!(
        after_stop.is_err(),
        "a deliberately stopped server must not be resurrected by a tool call"
    );
    assert!(
        !runtime.is_connected(server_id).await,
        "stop_server must leave the server disconnected"
    );

    runtime.shutdown_all().await;
}
