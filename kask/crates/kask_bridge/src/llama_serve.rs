//! Managed lifecycle for a local `llama serve` process paired with `zed-kask`.
//!
//! Peers with `McpRuntime` in `crates/zed/src/main.rs`:
//! - Auto-launches `llama serve` on session startup when `kask.llama_serve.enabled`
//!   is true, a local `llama` binary is found, and
//!   `language_models["llama.cpp"].api_url` points at loopback.
//! - Adopts an already-running server on that endpoint without taking ownership
//!   (never kills an externally started `llama serve` on exit).
//! - Spawns owned instances in their own Unix process group (`process_group(0)`)
//!   and tears down the full group (`SIGTERM` → bounded wait → `SIGKILL` + reap)
//!   inside `cx.on_app_quit` alongside `McpRuntime::shutdown_all()`.
//! - Serializes all lifecycle transitions under one async lock with a terminal
//!   `shutdown` latch so late settings/startup calls cannot resurrect a child
//!   after quit.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::time::Instant;

const DEFAULT_HEALTH_PROBE_TIMEOUT: Duration = Duration::from_millis(250);
const DEFAULT_READINESS_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_READINESS_POLL_INTERVAL: Duration = Duration::from_millis(100);
const DEFAULT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

/// Effective specification for the managed `llama serve` process, derived from
/// `kask.llama_serve` + `language_models["llama.cpp"].api_url`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LlamaServeSpec {
    pub enabled: bool,
    pub api_url: String,
    pub binary_path: Option<String>,
    pub extra_args: Vec<String>,
}

/// Result of reconciling `LlamaServeSupervisor` against a [`LlamaServeSpec`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnsureOutcome {
    /// Spawned a managed `llama serve` and verified `/health` returned HTTP 200.
    Started,
    /// An existing server was already answering `/health`; adopted without ownership.
    Adopted,
    /// Already managing a live process with the identical spec.
    AlreadyRunning,
    /// Disabled by settings or `api_url` is not a loopback HTTP(S) URL.
    Skipped,
    /// No local `llama` binary was found (clean opt-out).
    BinaryNotFound,
    /// Supervisor has already shut down (terminal latch).
    ShutDown,
    /// Spawn failed, child exited early, or `/health` timed out.
    Failed,
}

impl EnsureOutcome {
    /// True when the `llama.cpp` provider should refresh `/v1/models` and arm
    /// its `/models/sse` event stream.
    #[must_use]
    pub fn should_refresh_provider(self) -> bool {
        matches!(self, Self::Started | Self::Adopted)
    }
}

enum ActiveServer {
    Stopped,
    Adopted {
        api_url: String,
    },
    Owned {
        child: Child,
        pgid: Option<i32>,
        spec: LlamaServeSpec,
    },
}

struct SupervisorState {
    shutdown: bool,
    active: ActiveServer,
}

/// Session-scoped supervisor for a local `llama serve` process.
pub struct LlamaServeSupervisor {
    state: Mutex<SupervisorState>,
    readiness_timeout: Duration,
    shutdown_timeout: Duration,
}

impl Default for LlamaServeSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

impl LlamaServeSupervisor {
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Mutex::new(SupervisorState {
                shutdown: false,
                active: ActiveServer::Stopped,
            }),
            readiness_timeout: DEFAULT_READINESS_TIMEOUT,
            shutdown_timeout: DEFAULT_SHUTDOWN_TIMEOUT,
        }
    }

    #[cfg(test)]
    fn with_timeouts(readiness_timeout: Duration, shutdown_timeout: Duration) -> Self {
        Self {
            state: Mutex::new(SupervisorState {
                shutdown: false,
                active: ActiveServer::Stopped,
            }),
            readiness_timeout,
            shutdown_timeout,
        }
    }

    /// Reconcile the running state against `spec`.
    ///
    /// Safe to call at startup and on every `SettingsStore` change.
    pub async fn ensure_running(&self, spec: LlamaServeSpec) -> EnsureOutcome {
        let mut state = self.state.lock().await;
        if state.shutdown {
            return EnsureOutcome::ShutDown;
        }

        if !spec.enabled {
            self.stop_active_locked(&mut state).await;
            return EnsureOutcome::Skipped;
        }

        let Some((host, port)) = parse_loopback_endpoint(&spec.api_url) else {
            self.stop_active_locked(&mut state).await;
            tracing::debug!(
                target: "hkask.llama_serve",
                api_url = %spec.api_url,
                "llama.cpp api_url is not a local loopback endpoint; skipping managed llama serve"
            );
            return EnsureOutcome::Skipped;
        };

        // Check if we already have an active server matching this spec.
        match &mut state.active {
            ActiveServer::Owned {
                child,
                spec: current_spec,
                ..
            } if *current_spec == spec => match child.try_wait() {
                Ok(None) => return EnsureOutcome::AlreadyRunning,
                Ok(Some(status)) => {
                    tracing::warn!(
                        target: "hkask.llama_serve",
                        %status,
                        "managed llama serve exited unexpectedly; restarting"
                    );
                    state.active = ActiveServer::Stopped;
                }
                Err(error) => {
                    tracing::warn!(
                        target: "hkask.llama_serve",
                        %error,
                        "failed to poll managed llama serve status; restarting"
                    );
                    state.active = ActiveServer::Stopped;
                }
            },
            ActiveServer::Adopted { api_url } if *api_url == spec.api_url => {
                if probe_health(&host, port, DEFAULT_HEALTH_PROBE_TIMEOUT).await {
                    return EnsureOutcome::AlreadyRunning;
                }
                // Previously adopted server went away; fall through to re-check/spawn.
                state.active = ActiveServer::Stopped;
            }
            ActiveServer::Owned { .. } | ActiveServer::Adopted { .. } => {
                // Spec changed (e.g. port/args/binary); tear down any owned instance first.
                self.stop_active_locked(&mut state).await;
            }
            ActiveServer::Stopped => {}
        }

        // Adopt if an external server is already answering on this endpoint.
        if probe_health(&host, port, DEFAULT_HEALTH_PROBE_TIMEOUT).await {
            tracing::info!(
                target: "hkask.llama_serve",
                api_url = %spec.api_url,
                "llama serve already listening on configured loopback endpoint; adopting without ownership"
            );
            state.active = ActiveServer::Adopted {
                api_url: spec.api_url,
            };
            return EnsureOutcome::Adopted;
        }

        let Some(binary) = resolve_llama_binary(spec.binary_path.as_deref()) else {
            tracing::info!(
                target: "hkask.llama_serve",
                "no local llama binary found; skipping managed llama serve launch"
            );
            return EnsureOutcome::BinaryNotFound;
        };

        let args = build_serve_args(&host, port, &spec.extra_args);
        tracing::info!(
            target: "hkask.llama_serve",
            binary = %binary.display(),
            ?args,
            api_url = %spec.api_url,
            "launching managed llama serve"
        );

        let mut cmd = Command::new(&binary);
        cmd.args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        cmd.process_group(0);

        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(error) => {
                tracing::warn!(
                    target: "hkask.llama_serve",
                    binary = %binary.display(),
                    %error,
                    "failed to spawn llama serve"
                );
                return EnsureOutcome::Failed;
            }
        };

        let pgid = child.id().and_then(|pid| i32::try_from(pid).ok());
        spawn_pipe_loggers(&mut child);

        let ready = wait_until_ready(&mut child, &host, port, self.readiness_timeout).await;
        if !ready {
            terminate_child(child, pgid, self.shutdown_timeout).await;
            return EnsureOutcome::Failed;
        }

        tracing::info!(
            target: "hkask.llama_serve",
            pid = ?pgid,
            api_url = %spec.api_url,
            "managed llama serve is ready"
        );
        state.active = ActiveServer::Owned { child, pgid, spec };
        EnsureOutcome::Started
    }

    /// Permanently shut down the supervisor and terminate any owned `llama serve`
    /// process group. Adopted (external) servers are left untouched.
    ///
    /// Idempotent: repeated calls are harmless.
    pub async fn shutdown(&self) {
        let mut state = self.state.lock().await;
        state.shutdown = true;
        self.stop_active_locked(&mut state).await;
    }

    async fn stop_active_locked(&self, state: &mut SupervisorState) {
        match std::mem::replace(&mut state.active, ActiveServer::Stopped) {
            ActiveServer::Stopped => {}
            ActiveServer::Adopted { api_url } => {
                tracing::info!(
                    target: "hkask.llama_serve",
                    %api_url,
                    "leaving externally managed llama serve running"
                );
            }
            ActiveServer::Owned { child, pgid, .. } => {
                tracing::info!(
                    target: "hkask.llama_serve",
                    pid = ?pgid,
                    "stopping managed llama serve process group"
                );
                terminate_child(child, pgid, self.shutdown_timeout).await;
            }
        }
    }
}

/// Extract `(bind_host, port)` from a local loopback HTTP(S) `api_url`.
/// Returns `None` for remote hosts or unparseable URLs.
pub fn parse_loopback_endpoint(api_url: &str) -> Option<(String, u16)> {
    let trimmed = api_url.trim();
    if trimmed.is_empty() || !settings_content::is_local_provider_api_url(trimmed) {
        return None;
    }
    let parsed = url::Url::parse(trimmed).ok()?;
    let host_str = parsed.host_str()?;
    let bind_host = if host_str.eq_ignore_ascii_case("localhost") {
        "127.0.0.1".to_string()
    } else {
        host_str
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_string()
    };
    let port = parsed.port_or_known_default()?;
    Some((bind_host, port))
}

/// Resolve the local `llama` binary:
/// 1. Explicit `configured` path (expanding leading `~/` via `HOME`).
/// 2. `llama` on `PATH`.
/// 3. `$HOME/.local/bin/llama`.
pub fn resolve_llama_binary(configured: Option<&str>) -> Option<PathBuf> {
    if let Some(raw) = configured.map(str::trim).filter(|s| !s.is_empty()) {
        let candidate = expand_tilde(raw);
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
        tracing::warn!(
            target: "hkask.llama_serve",
            path = %candidate.display(),
            "configured kask.llama_serve.binary_path does not exist or is not executable"
        );
        return None;
    }

    if let Some(path_var) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join("llama");
            if is_executable_file(&candidate) {
                return Some(candidate);
            }
        }
    }

    if let Some(home) = std::env::var_os("HOME") {
        let candidate = PathBuf::from(home).join(".local/bin/llama");
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }

    None
}

fn expand_tilde(raw: &str) -> PathBuf {
    if let Some(rest) = raw.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(raw)
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn build_serve_args(host: &str, port: u16, extra_args: &[String]) -> Vec<String> {
    let has_host = extra_args
        .iter()
        .any(|arg| arg == "--host" || arg.starts_with("--host="));
    let has_port = extra_args
        .iter()
        .any(|arg| arg == "--port" || arg.starts_with("--port="));

    let mut args = vec!["serve".to_string()];
    if !has_host {
        args.push("--host".to_string());
        args.push(host.to_string());
    }
    if !has_port {
        args.push("--port".to_string());
        args.push(port.to_string());
    }
    args.extend(extra_args.iter().cloned());
    args
}

async fn probe_health(host: &str, port: u16, timeout: Duration) -> bool {
    let fut = async {
        let mut stream = TcpStream::connect((host, port)).await.ok()?;
        let host_header = if host.contains(':') {
            format!("[{host}]:{port}")
        } else {
            format!("{host}:{port}")
        };
        let req =
            format!("GET /health HTTP/1.1\r\nHost: {host_header}\r\nConnection: close\r\n\r\n");
        stream.write_all(req.as_bytes()).await.ok()?;
        let mut reader = BufReader::new(stream);
        let mut status_line = String::new();
        reader.read_line(&mut status_line).await.ok()?;
        let code = status_line.split_whitespace().nth(1)?;
        Some(code == "200")
    };
    tokio::time::timeout(timeout, fut)
        .await
        .ok()
        .flatten()
        .unwrap_or(false)
}

async fn wait_until_ready(child: &mut Child, host: &str, port: u16, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if probe_health(host, port, DEFAULT_HEALTH_PROBE_TIMEOUT).await {
            return true;
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                tracing::warn!(
                    target: "hkask.llama_serve",
                    %status,
                    "llama serve exited before becoming healthy"
                );
                return false;
            }
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(
                    target: "hkask.llama_serve",
                    %error,
                    "error polling llama serve during startup"
                );
                return false;
            }
        }
        if Instant::now() >= deadline {
            tracing::warn!(
                target: "hkask.llama_serve",
                ?timeout,
                host,
                port,
                "timed out waiting for llama serve /health"
            );
            return false;
        }
        tokio::time::sleep(DEFAULT_READINESS_POLL_INTERVAL).await;
    }
}

fn spawn_pipe_loggers(child: &mut Child) {
    if let Some(stdout) = child.stdout.take() {
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if !line.trim().is_empty() {
                    tracing::debug!(target: "hkask.llama_serve", stream = "stdout", "{line}");
                }
            }
        });
    }
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if !line.trim().is_empty() {
                    tracing::debug!(target: "hkask.llama_serve", stream = "stderr", "{line}");
                }
            }
        });
    }
}

async fn terminate_child(mut child: Child, pgid: Option<i32>, grace: Duration) {
    #[cfg(unix)]
    if let Some(raw_pgid) = pgid
        && let Some(pid) = rustix::process::Pid::from_raw(raw_pgid)
    {
        let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::TERM);
    }

    #[cfg(not(unix))]
    {
        let _ = pgid;
        let _ = child.start_kill();
    }

    match tokio::time::timeout(grace, child.wait()).await {
        Ok(Ok(_status)) => {}
        Ok(Err(error)) => {
            tracing::warn!(target: "hkask.llama_serve", %error, "error waiting for llama serve exit");
        }
        Err(_) => {
            tracing::warn!(
                target: "hkask.llama_serve",
                pid = ?pgid,
                "llama serve did not exit within grace period; sending SIGKILL to process group"
            );
            #[cfg(unix)]
            if let Some(raw_pgid) = pgid
                && let Some(pid) = rustix::process::Pid::from_raw(raw_pgid)
            {
                let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
            }
            let _ = child.start_kill();
            let _ = child.wait().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    async fn spawn_mock_health_server() -> (u16, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut buf = [0u8; 512];
                    let _ = tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await;
                    let _ = stream
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 15\r\nConnection: close\r\n\r\n{\"status\":\"ok\"}")
                        .await;
                });
            }
        });
        (port, handle)
    }

    #[test]
    fn parse_loopback_endpoint_accepts_loopback_and_rejects_remote() {
        assert_eq!(
            parse_loopback_endpoint("http://localhost:8080"),
            Some(("127.0.0.1".to_string(), 8080))
        );
        assert_eq!(
            parse_loopback_endpoint("http://127.0.0.1:9931"),
            Some(("127.0.0.1".to_string(), 9931))
        );
        assert_eq!(parse_loopback_endpoint("http://10.0.0.5:9931"), None);
        assert_eq!(parse_loopback_endpoint("https://example.com:9931"), None);
        assert_eq!(parse_loopback_endpoint(""), None);
    }

    #[test]
    fn build_serve_args_derives_host_and_port_unless_overridden() {
        assert_eq!(
            build_serve_args("127.0.0.1", 9931, &[]),
            vec!["serve", "--host", "127.0.0.1", "--port", "9931"]
        );
        assert_eq!(
            build_serve_args(
                "127.0.0.1",
                9931,
                &[
                    "--port".into(),
                    "8888".into(),
                    "--models-max".into(),
                    "2".into()
                ]
            ),
            vec![
                "serve",
                "--host",
                "127.0.0.1",
                "--port",
                "8888",
                "--models-max",
                "2"
            ]
        );
    }

    #[tokio::test]
    async fn adopts_existing_server_and_does_not_kill_it_on_shutdown() {
        let (port, handle) = spawn_mock_health_server().await;
        let supervisor =
            LlamaServeSupervisor::with_timeouts(Duration::from_secs(2), Duration::from_secs(1));
        let spec = LlamaServeSpec {
            enabled: true,
            api_url: format!("http://127.0.0.1:{port}"),
            binary_path: Some("/nonexistent/llama".to_string()),
            extra_args: Vec::new(),
        };

        let outcome = supervisor.ensure_running(spec.clone()).await;
        assert_eq!(outcome, EnsureOutcome::Adopted);
        assert!(outcome.should_refresh_provider());

        // Second call with identical spec is a no-op while server stays up.
        assert_eq!(
            supervisor.ensure_running(spec).await,
            EnsureOutcome::AlreadyRunning
        );

        supervisor.shutdown().await;
        // Adopted server must still be alive after supervisor shutdown.
        assert!(probe_health("127.0.0.1", port, Duration::from_millis(500)).await);
        handle.abort();
    }

    #[tokio::test]
    async fn skips_non_loopback_and_disabled_specs() {
        let supervisor =
            LlamaServeSupervisor::with_timeouts(Duration::from_secs(1), Duration::from_secs(1));

        let remote = LlamaServeSpec {
            enabled: true,
            api_url: "http://192.168.1.50:9931".to_string(),
            binary_path: None,
            extra_args: Vec::new(),
        };
        assert_eq!(
            supervisor.ensure_running(remote).await,
            EnsureOutcome::Skipped
        );

        let disabled = LlamaServeSpec {
            enabled: false,
            api_url: "http://127.0.0.1:9931".to_string(),
            binary_path: None,
            extra_args: Vec::new(),
        };
        assert_eq!(
            supervisor.ensure_running(disabled).await,
            EnsureOutcome::Skipped
        );
    }

    #[tokio::test]
    async fn missing_binary_is_clean_noop() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let unused_port = listener.local_addr().unwrap().port();
        drop(listener);

        let supervisor =
            LlamaServeSupervisor::with_timeouts(Duration::from_secs(1), Duration::from_secs(1));
        let spec = LlamaServeSpec {
            enabled: true,
            api_url: format!("http://127.0.0.1:{unused_port}"),
            binary_path: Some("/definitely/not/a/real/llama/binary".to_string()),
            extra_args: Vec::new(),
        };
        assert_eq!(
            supervisor.ensure_running(spec).await,
            EnsureOutcome::BinaryNotFound
        );
    }

    #[tokio::test]
    async fn rejects_late_start_after_shutdown() {
        let supervisor =
            LlamaServeSupervisor::with_timeouts(Duration::from_secs(1), Duration::from_secs(1));
        supervisor.shutdown().await;

        let spec = LlamaServeSpec {
            enabled: true,
            api_url: "http://127.0.0.1:9931".to_string(),
            binary_path: None,
            extra_args: Vec::new(),
        };
        assert_eq!(
            supervisor.ensure_running(spec).await,
            EnsureOutcome::ShutDown
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn spawns_and_reaps_process_group_on_shutdown() {
        use std::os::unix::fs::PermissionsExt;

        // Allocate an unused port for the stub server.
        let probe_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = probe_listener.local_addr().unwrap().port();
        drop(probe_listener);

        let dir = tempfile::tempdir().unwrap();
        let child_pid_file = dir.path().join("grandchild.pid");
        let stub_path = dir.path().join("fake-llama.sh");

        // Pure bash + nc/socat-free health stub using bash's own process + a
        // background sleep grandchild to verify full process-group teardown.
        // We check if `nc` is available; if not, skip gracefully? Better: use
        // `busybox`/`nc` or Rust's own mock? Even better: use `nc -lk` if
        // present, else we can test process-group termination directly via
        // `terminate_child`! Let's use `terminate_child` directly AND test
        // `ensure_running` with a real health server when `nc` or `python` isn't
        // used (remember .rules: NO Python!).
        let script = format!(
            "#!/usr/bin/env bash\n\
             sleep 60 &\n\
             echo $! > {}\n\
             wait\n",
            child_pid_file.display()
        );
        std::fs::write(&stub_path, script).unwrap();
        std::fs::set_permissions(&stub_path, std::fs::Permissions::from_mode(0o755)).unwrap();

        let mut cmd = Command::new(&stub_path);
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0);
        let child = cmd.spawn().unwrap();
        let pgid = child.id().and_then(|pid| i32::try_from(pid).ok());

        // Wait for the grandchild PID file to appear.
        let mut grandchild_pid = None;
        for _ in 0..40 {
            if let Ok(content) = std::fs::read_to_string(&child_pid_file)
                && let Ok(pid) = content.trim().parse::<i32>()
            {
                grandchild_pid = Some(pid);
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        let grandchild_pid = grandchild_pid.expect("grandchild pid should be recorded");

        // Verify grandchild is alive before shutdown.
        let gc = rustix::process::Pid::from_raw(grandchild_pid).unwrap();
        assert!(
            rustix::process::test_kill_process(gc).is_ok(),
            "grandchild should be alive before terminate_child"
        );

        terminate_child(child, pgid, Duration::from_secs(2)).await;

        // Give the OS a brief moment to reap the signaled grandchild.
        let mut gone = false;
        for _ in 0..40 {
            if rustix::process::test_kill_process(gc).is_err() {
                gone = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert!(
            gone,
            "grandchild in process group must be terminated on shutdown"
        );
        let _ = port;
    }
}
