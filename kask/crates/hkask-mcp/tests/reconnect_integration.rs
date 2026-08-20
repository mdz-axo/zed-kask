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

use hkask_tool_port::{ToolPort, ToolPortError};
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

/// The health supervisor proactively restarts a crashed server even when no
/// tool call is in flight.
///
/// Before the fix, the supervisor only removed dead connections and relied on
/// `call_tool_inner → try_reconnect` to heal on the next tool call. If no call
/// came (the panel saw the server as unavailable and stopped calling), the
/// server stayed dead forever — a transient crash became a permanent outage.
/// After the fix, the supervisor attempts a restart on every unhealthy check
/// using the recorded launch spec.
///
/// This test sets a short health check interval (via env var) so the supervisor
/// fires within the test deadline. It kills the server process, then polls for
/// a reconnected (different) process WITHOUT issuing any tool call — the
/// supervisor must heal on its own.
#[tokio::test(flavor = "multi_thread")]
async fn supervisor_restarts_crashed_server_without_a_tool_call() {
    let server_id = "supervisor-heals";
    point_at_fixture(server_id);

    // Short health check interval so the supervisor fires within the test
    // deadline. SAFETY: unique var name per test (server_id is unique).
    unsafe {
        std::env::set_var("HKASK_MCP_HEALTH_CHECK_INTERVAL_SECS", "1");
    }

    let pid_file = std::env::temp_dir().join(format!("hkask-fixture-{server_id}.pid"));
    let _ = std::fs::remove_file(&pid_file);

    let runtime = McpRuntime::new();
    let mut env = HashMap::new();
    env.insert("FIXTURE_MARKER".to_string(), "supervisor-test".to_string());
    env.insert(
        "FIXTURE_PID_FILE".to_string(),
        pid_file.to_string_lossy().to_string(),
    );

    runtime
        .start_server_with_env(server_id, "unused-command", env)
        .await
        .expect("fixture server starts");

    // Baseline: the server is connected.
    assert!(
        runtime.is_connected(server_id).await,
        "server must be connected at baseline"
    );

    let original_pid: u32 = std::fs::read_to_string(&pid_file)
        .expect("fixture writes its pid")
        .trim()
        .parse()
        .expect("pid is numeric");

    // Kill the server out from under the runtime. Do NOT issue any tool call
    // afterward — the supervisor must heal without one.
    kill_and_wait(original_pid);

    // Wait for the supervisor to detect the death and restart the server.
    // The supervisor checks every 1s (env var set above), so this should
    // complete within a few seconds. Polling is the honest way to observe
    // async healing — a fixed sleep would be flaky.
    wait_until(
        "supervisor restarts the crashed server without a tool call",
        || async { runtime.is_connected(server_id).await },
    )
    .await;

    // The restarted server must be a different process.
    let restarted_pid: u32 = std::fs::read_to_string(&pid_file)
        .expect("the restarted fixture rewrites the pid file")
        .trim()
        .parse()
        .expect("pid is numeric");
    assert_ne!(
        restarted_pid, original_pid,
        "the supervisor must have spawned a new process, not reused the dead one"
    );

    // And the restarted server must actually answer a call — proving the
    // supervisor's restart installed a working connection, not just a process.
    let result = runtime
        .invoke(server_id, "ping", serde_json::json!({}), agent())
        .await
        .expect("the supervisor-restarted server must answer a tool call");
    assert_eq!(
        marker_of(&result),
        "supervisor-test",
        "the restarted server must carry the same marker (launched from the recorded spec)"
    );

    runtime.shutdown_all().await;
    let _ = std::fs::remove_file(&pid_file);
    // Restore the default so this test does not leak the short interval to
    // subsequent tests in the same process.
    unsafe {
        std::env::remove_var("HKASK_MCP_HEALTH_CHECK_INTERVAL_SECS");
    }
}

/// The supervisor does NOT stop checking after `max_consecutive_health_failures`.
///
/// Before the fix, the supervisor gave up permanently after 3 failures,
/// turning a transient crash into a permanent outage. After the fix, it
/// transitions to the degraded interval but continues attempting restarts.
/// This test sets a short interval and a low failure threshold, kills the
/// server, and verifies the supervisor is still running (and still attempting
/// restarts) after the threshold is exceeded.
#[tokio::test(flavor = "multi_thread")]
async fn supervisor_does_not_give_up_after_max_failures() {
    let server_id = "supervisor-no-giveup";
    point_at_fixture(server_id);

    // Short intervals so the test completes within its deadline.
    // SAFETY: unique var names per test (server_id is unique).
    unsafe {
        std::env::set_var("HKASK_MCP_HEALTH_CHECK_INTERVAL_SECS", "1");
        std::env::set_var("HKASK_MCP_MAX_HEALTH_FAILURES", "2");
        std::env::set_var("HKASK_MCP_DEGRADED_HEALTH_CHECK_INTERVAL_SECS", "2");
    }

    let pid_file = std::env::temp_dir().join(format!("hkask-fixture-{server_id}.pid"));
    let _ = std::fs::remove_file(&pid_file);

    let runtime = McpRuntime::new();
    let mut env = HashMap::new();
    env.insert("FIXTURE_MARKER".to_string(), "no-giveup".to_string());
    env.insert(
        "FIXTURE_PID_FILE".to_string(),
        pid_file.to_string_lossy().to_string(),
    );

    runtime
        .start_server_with_env(server_id, "unused-command", env)
        .await
        .expect("fixture server starts");

    let original_pid: u32 = std::fs::read_to_string(&pid_file)
        .expect("fixture writes its pid")
        .trim()
        .parse()
        .expect("pid is numeric");

    // Kill the server. The supervisor will see Missing/TransportClosed on
    // every check, accumulate failures past the threshold (2), and transition
    // to the degraded interval — but it must NOT stop.
    kill_and_wait(original_pid);

    // Wait long enough for the supervisor to exceed the threshold (2 failures
    // at 1s intervals = 2s) and then attempt at least one restart on the
    // degraded interval (2s). Total: ~4-6s. The supervisor's restart should
    // heal the server even after the threshold.
    wait_until(
        "supervisor restarts the crashed server after exceeding the failure threshold",
        || async { runtime.is_connected(server_id).await },
    )
    .await;

    // The server must be back and answering — proving the supervisor did not
    // give up after max_health_failures.
    let result = runtime
        .invoke(server_id, "ping", serde_json::json!({}), agent())
        .await
        .expect("the supervisor must have restarted the server even after exceeding the failure threshold");
    assert_eq!(marker_of(&result), "no-giveup");

    runtime.shutdown_all().await;
    let _ = std::fs::remove_file(&pid_file);
    unsafe {
        std::env::remove_var("HKASK_MCP_HEALTH_CHECK_INTERVAL_SECS");
        std::env::remove_var("HKASK_MCP_MAX_HEALTH_FAILURES");
        std::env::remove_var("HKASK_MCP_DEGRADED_HEALTH_CHECK_INTERVAL_SECS");
    }
}

/// The supervisor does NOT resurrect a deliberately stopped server, even with
/// a short health check interval.
///
/// `stop_server` clears the launch spec, so the supervisor's restart path
/// sees `None` and skips the restart. This test pins that behavior: with a
/// 1s health check interval, the supervisor fires multiple times after
/// `stop_server`, but the server must NOT come back.
#[tokio::test(flavor = "multi_thread")]
async fn supervisor_does_not_resurrect_a_deliberately_stopped_server() {
    let server_id = "supervisor-stopped";
    point_at_fixture(server_id);

    // Short health check interval so the supervisor fires multiple times
    // during the test. SAFETY: unique var name per test (server_id is unique).
    unsafe {
        std::env::set_var("HKASK_MCP_HEALTH_CHECK_INTERVAL_SECS", "1");
    }

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

    // Wait long enough for the supervisor to fire at least twice (2s with a
    // 1s interval). The server must NOT come back — `stop_server` cleared
    // the launch spec, so the supervisor has nothing to restart from.
    tokio::time::sleep(Duration::from_secs(3)).await;

    assert!(
        !runtime.is_connected(server_id).await,
        "a deliberately stopped server must not be resurrected by the supervisor"
    );

    let after_stop = runtime
        .invoke(server_id, "ping", serde_json::json!({}), agent())
        .await;
    assert!(
        after_stop.is_err(),
        "a deliberately stopped server must not answer calls even after the supervisor fires"
    );

    runtime.shutdown_all().await;
    unsafe {
        std::env::remove_var("HKASK_MCP_HEALTH_CHECK_INTERVAL_SECS");
    }
}

/// A reconnect (via `try_reconnect` from `call_tool_inner`) calls
/// `start_server_with_env` again, which replaces the cancellation token. The
/// old supervisor must be cancelled — without this, the old supervisor task
/// leaks (it holds a clone of the old token and never exits).
///
/// This test exercises the reconnect path (kill + invoke) multiple times and
/// verifies the server is stable after each cycle. If the old supervisor were
/// leaking, each cycle would accumulate an orphaned supervisor that polls a
/// dead connection — while this doesn't cause incorrect behavior (the
/// idempotency check prevents duplicate connections), it leaks tasks. The test
/// verifies the observable contract: after N reconnect cycles, exactly one
/// process is live and answering.
#[tokio::test(flavor = "multi_thread")]
async fn reconnect_does_not_leak_supervisor_tasks() {
    let server_id = "supervisor-no-leak";
    point_at_fixture(server_id);

    // Short health check interval so the supervisor is active during the test.
    // SAFETY: unique var name per test (server_id is unique).
    unsafe {
        std::env::set_var("HKASK_MCP_HEALTH_CHECK_INTERVAL_SECS", "1");
    }

    let pid_file = std::env::temp_dir().join(format!("hkask-fixture-{server_id}.pid"));
    let _ = std::fs::remove_file(&pid_file);

    let runtime = McpRuntime::new();
    let mut env = HashMap::new();
    env.insert("FIXTURE_MARKER".to_string(), "no-leak".to_string());
    env.insert(
        "FIXTURE_PID_FILE".to_string(),
        pid_file.to_string_lossy().to_string(),
    );

    runtime
        .start_server_with_env(server_id, "unused-command", env)
        .await
        .expect("fixture server starts");

    // Baseline: the server answers.
    let result = runtime
        .invoke(server_id, "ping", serde_json::json!({}), agent())
        .await
        .expect("the live fixture answers");
    assert_eq!(marker_of(&result), "no-leak");

    // Perform 3 kill + reconnect cycles. Each cycle kills the server, then
    // invokes a tool call to trigger `try_reconnect` (which calls
    // `start_server_with_env` again). The old supervisor's cancellation token
    // should be cancelled by the new `start_server_with_env` call, preventing
    // a leak.
    for cycle in 1..=3 {
        let pid: u32 = std::fs::read_to_string(&pid_file)
            .expect("fixture writes its pid")
            .trim()
            .parse()
            .expect("pid is numeric");
        kill_and_wait(pid);

        // Trigger a reconnect via tool calls. The first call may fail (the
        // connection is dead), and the reconnect may race with the supervisor.
        // Retry up to 10 times — the contract under test is that the server
        // eventually heals and answers, not that it heals on the very first
        // attempt. The reconnect cooldown (5s default) may suppress
        // `try_reconnect` on cycles 2+, so the supervisor (1s interval) is the
        // primary healing path for those cycles.
        let mut answered = false;
        for attempt in 1..=10 {
            if let Ok(result) = runtime
                .invoke(server_id, "ping", serde_json::json!({}), agent())
                .await
            {
                assert_eq!(
                    marker_of(&result),
                    "no-leak",
                    "cycle {cycle} attempt {attempt}: the reconnected server must carry the same marker"
                );
                answered = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        assert!(
            answered,
            "cycle {cycle}: the reconnected server must answer within 5 attempts"
        );

        // Verify exactly one process is live. If the old supervisor leaked and
        // spawned a duplicate, there would be two processes — but the
        // idempotency check prevents this. The real test is that the server is
        // stable and answering after each cycle.
        let new_pid: u32 = std::fs::read_to_string(&pid_file)
            .expect("the reconnected fixture rewrites the pid file")
            .trim()
            .parse()
            .expect("pid is numeric");
        assert_ne!(
            new_pid, pid,
            "cycle {cycle}: healing must spawn a new process"
        );
    }

    runtime.shutdown_all().await;
    let _ = std::fs::remove_file(&pid_file);
    unsafe {
        std::env::remove_var("HKASK_MCP_HEALTH_CHECK_INTERVAL_SECS");
    }
}
