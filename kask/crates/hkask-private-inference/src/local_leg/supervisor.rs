//! L4 — the supervisor: spawn / health-poll / stop / drain over a
//! guard-verified launch plan, with typed refusals everywhere and NO
//! silent fallback (a dead server is surfaced, never restarted or
//! substituted — the restart policy is operator-owned, plan §10).
//!
//! Process placement: the supervisor runs inside the host's tokio
//! runtime (the bridge's `gpui_tokio::Tokio::spawn`); the
//! llama-server child OUTLIVES the editor process by design (a
//! machine-local service — adoption recovers it next session, saving
//! the measured 20-minute load bound; the launch record is verified
//! liveness + digest, never trusted as a file, slice plan D-S3).
//!
//! One serving instance at a time on the fixed policy port (S2, plan
//! §10 delta 9): launching a different model while one serves is a
//! typed refusal (`ServingOtherModel`) — the PI stops first; the
//! supervisor never swaps models silently.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::local_leg::fleet::{self, FleetError};
use crate::local_leg::gguf::{self, GgufError};
use crate::local_leg::launch::{LaunchPlan, endpoint_url};
use crate::local_leg::manifest::{self, EngineSurfaceManifest, ManifestError};
use crate::local_leg::placement::{self, PlacementError, PlacementPolicy};
use crate::local_leg::probe::{self, ProbeError};

/// The health-poll interval (the launch gate's wait AND the running
/// supervision cadence). The death bound: a dead process surfaces
/// within one interval plus one child-reap check.
pub const POLL_INTERVAL_SECS: u64 = 2;

/// The bound to first healthy after spawn — the measured load bound of
/// the P0 matrix (baseline record §4: the harness polls `/health` with
/// a 20-minute bound per leg).
pub const HEALTH_TIMEOUT_SECS: u64 = 20 * 60;

/// How long a SIGTERM'd server gets to drain before SIGKILL.
pub const DRAIN_TIMEOUT_SECS: u64 = 30;

/// How many consecutive failed health probes mark a live process
/// unhealthy (typed failure — the streak distinguishes death from a
/// transient blip; a REAPED process is death immediately).
const UNHEALTHY_STREAK: u32 = 3;

const RECORD_FILE: &str = "launch-record.json";
const KEY_FILE: &str = "llama-server.key";

/// A launch request — everything the supervisor needs, resolved by the
/// bridge: the backend binary (engine probe or the test fixture), the
/// endpoint token (the keychain slot read — the crate never reads
/// credentials itself, the acquisition-module discipline), the
/// machine-state dir (hidden root) and the models root (artifacts
/// root), and the placement policy (settings host/port).
#[derive(Debug, Clone)]
pub struct LaunchConfig {
    pub model_id: String,
    pub binary: PathBuf,
    /// The endpoint token; `None`/empty is a typed refusal — the token
    /// slot at the provider `api_url` is the toggle.
    pub api_key: Option<String>,
    pub state_dir: PathBuf,
    pub models_root: PathBuf,
    pub policy: PlacementPolicy,
}

/// Everything `launch` computes before touching the process world.
/// Production builds it via [`prepare_launch`]; the stub integration
/// tests build it directly with a hand-built `GgufMeta` and a
/// constructed hardware inventory — the parse is the one step that
/// needs real weight bytes (pinned by its own live-gated test); the
/// guards, the probe and the whole spawn path run for real on both.
#[derive(Debug, Clone)]
pub struct PreparedLaunch {
    pub plan: LaunchPlan,
    pub manifest: EngineSurfaceManifest,
}

/// Resolve the fleet model, parse its shards, probe the hardware and
/// compile the guard-verified plan — the full production prepare path.
pub fn prepare_launch(config: &LaunchConfig) -> Result<PreparedLaunch, SupervisionError> {
    let files = fleet::resolve_model_files(&config.models_root, &config.model_id)?;
    let meta = gguf::parse(&files)?;
    let brief = fleet::shipped_model(&config.model_id)?.brief;
    let hardware = placement::probe_hardware()?;
    let manifest = manifest::shipped_manifest()?;
    let plan = placement::compile_placement(
        files[0].clone(),
        &meta,
        &hardware,
        &brief,
        &config.policy,
        &manifest,
    )?;
    Ok(PreparedLaunch { plan, manifest })
}

/// The launch record — machine state under the hidden root. Written
/// after first healthy; the adopt-or-refuse gate VERIFIES it (live pid,
/// matching digest and endpoint, healthy probe) — the file alone is
/// never evidence.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LaunchRecord {
    pub pid: u32,
    pub model_id: String,
    pub plan_digest: String,
    pub endpoint: String,
    pub started_at_unix: u64,
    pub binary: String,
}

/// What the page renders: the supervisor's live state.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum StatusSnapshot {
    #[default]
    Stopped,
    Serving {
        model_id: String,
        endpoint: String,
        adopted: bool,
    },
    Failed {
        reason: String,
    },
}

/// The serving state returned by a successful launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServingSnapshot {
    pub model_id: String,
    pub endpoint: String,
    pub adopted: bool,
}

/// Supervisor failures — per-variant, never a blanket string; every
/// refusal is surfaced typed to the PI, never silently absorbed.
#[derive(Debug, thiserror::Error)]
pub enum SupervisionError {
    #[error(
        "the endpoint token is missing — the launch is refused (the token at the provider api_url keychain slot is the toggle; set it on the Private Inference page)"
    )]
    TokenMissing,
    #[error("fleet: {0}")]
    Fleet(#[from] FleetError),
    #[error("the model's GGUF headers failed to parse: {0}")]
    Gguf(#[from] GgufError),
    #[error("the hardware probe failed: {0}")]
    Probe(#[from] ProbeError),
    #[error("the engine manifest failed to load: {0}")]
    Manifest(#[from] ManifestError),
    #[error("the placement compiler refused the launch: {0}")]
    Placement(#[from] PlacementError),
    /// Single-flight port arbitration: a launch is already in progress.
    #[error(
        "a launch is already in progress — one launch or swap at a time \
         (the port arbitration is single-flight)"
    )]
    AlreadyLaunching,
    /// The policy port is held by something the supervisor cannot
    /// verify as its own — adoption failed, the holder is named.
    #[error(
        "the policy endpoint is occupied by a process the supervisor \
         cannot verify as its own launch (holder: {holder}) — the launch \
         record is verified liveness + digest, never trusted as a file"
    )]
    PortOccupied { holder: String },
    #[error(
        "the server did not reach healthy within {HEALTH_TIMEOUT_SECS}s — killed, no server left behind"
    )]
    HealthTimeout,
    #[error("spawning the server failed: {source}")]
    Spawn { source: std::io::Error },
    #[error("supervisor state io: {context}: {source}")]
    Io {
        context: String,
        source: std::io::Error,
    },
    #[error("the server did not stop within {DRAIN_TIMEOUT_SECS}s of SIGTERM — SIGKILLed")]
    StopTimeout,
}

/// The supervisor handle — cheap to clone; the watch task and the
/// interior state are shared.
#[derive(Clone, Default)]
pub struct Supervisor {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Default)]
struct Inner {
    status: StatusSnapshot,
    watch: Option<WatchHandle>,
    /// The single-flight launch guard's flag.
    launching: bool,
}

struct WatchHandle {
    stop: Arc<tokio::sync::Notify>,
    join: tokio::task::JoinHandle<()>,
}

/// The occupied-port decision: adopt a verified matching server, or
/// clear OUR verified other-model server for the explicit swap.
enum OccupiedOutcome {
    Adopted(ServingSnapshot),
    ClearedForSpawn,
}

/// Clears the single-flight launch flag on every exit path (success,
/// refusal, panic — the guard's drop is the mechanism).
struct LaunchGuard {
    inner: Arc<Mutex<Inner>>,
}
impl Drop for LaunchGuard {
    fn drop(&mut self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.launching = false;
        }
    }
}

impl Supervisor {
    /// The full production launch: prepare (resolve, parse, probe,
    /// compile) then spawn. Takes the config BY VALUE — the returned
    /// future owns its data (the bridge hands it to `Tokio::spawn`).
    pub async fn launch(&self, config: LaunchConfig) -> Result<ServingSnapshot, SupervisionError> {
        let prepared = prepare_launch(&config)?;
        self.launch_prepared(config, &prepared).await
    }

    /// Spawn over a prepared launch — the seam the stub tests use.
    pub async fn launch_prepared(
        &self,
        config: LaunchConfig,
        prepared: &PreparedLaunch,
    ) -> Result<ServingSnapshot, SupervisionError> {
        // Single-flight: a concurrent second launch would race the port
        // arbitration — typed refusal, cleared on every exit path.
        let _launch_guard = self.begin_launch()?;
        let token = config
            .api_key
            .as_deref()
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .ok_or(SupervisionError::TokenMissing)?;

        // The startup probe — never a wrong-flag launch (Good
        // Regulator: the manifest is never trusted alone). It runs BEFORE
        // any swap: a refused probe leaves the serving server untouched.
        probe::probe_binary(&config.binary, &prepared.manifest).await?;

        let endpoint = endpoint_url(&prepared.plan.host, prepared.plan.port);
        let record_path = config.state_dir.join(RECORD_FILE);
        let digest = plan_digest(&prepared.plan, &config.model_id);

        std::fs::create_dir_all(&config.state_dir).map_err(|source| SupervisionError::Io {
            context: format!("create state dir {}", config.state_dir.display()),
            source,
        })?;

        // Launch is explicit: while THIS supervisor serves another model,
        // launching a different model is a SWAP — stop and drain the
        // current server (the operator's click is the intent; the status
        // line shows the swap), never a silent substitution. A same-model
        // relaunch falls through to the adopt path below.
        if let StatusSnapshot::Serving { model_id, .. } = self.status() {
            if model_id != config.model_id {
                self.stop().await?;
            }
        }

        // Occupied port (a fresh session over a previous editor's
        // server): adopt only a VERIFIED record (D-S3). A live record of
        // ANOTHER model that is provably ours is swapped by pid — the
        // process identity is verified against the recorded binary,
        // never by pid alone (the pid-recycling kill hazard); anything
        // less verifiable is a typed PortOccupied naming the holder.
        if tcp_connectable(&prepared.plan.host, prepared.plan.port).await {
            match self
                .adopt_or_refuse(&config, prepared, &record_path, &endpoint, &digest)
                .await?
            {
                OccupiedOutcome::Adopted(snapshot) => return Ok(snapshot),
                OccupiedOutcome::ClearedForSpawn => {
                    // Our verified other-model server stopped — spawn the
                    // requested model on the freed port below.
                }
            }
        }

        // Write the token file (0600, hidden root) and spawn.
        let key_path = config.state_dir.join(KEY_FILE);
        write_secret_file(&key_path, token.as_bytes()).map_err(|source| SupervisionError::Io {
            context: format!("write {}", key_path.display()),
            source,
        })?;

        let mut command = tokio::process::Command::new(&config.binary);
        command.args(&prepared.plan.args);
        command
            .arg(&prepared.manifest.flags.alias)
            .arg(&config.model_id)
            .arg(&prepared.manifest.flags.api_key_file)
            .arg(&key_path);
        command
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let mut child = command
            .spawn()
            .map_err(|source| SupervisionError::Spawn { source })?;

        // Health-wait, bounded by the measured load bound.
        let client = health_probe_client()?;
        let health_url = format!("http://{}:{}", prepared.plan.host, prepared.plan.port)
            + &prepared.manifest.probe.health_path;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(HEALTH_TIMEOUT_SECS);
        loop {
            if health_ok(&client, &health_url).await {
                break;
            }
            if tokio::time::Instant::now() >= deadline {
                // Fail-closed: no zombie server left behind.
                let _ = child.start_kill();
                let _ = std::fs::remove_file(&key_path);
                return Err(SupervisionError::HealthTimeout);
            }
            // The child dying mid-wait surfaces on the next poll as a
            // connection error — but check the process directly: a
            // spawn that dies instantly should not wait the deadline.
            if let Ok(Some(status)) = child.try_wait() {
                let _ = std::fs::remove_file(&key_path);
                return Err(SupervisionError::Spawn {
                    source: std::io::Error::other(format!(
                        "the server exited during startup: {status}"
                    )),
                });
            }
            tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;
        }

        let record = LaunchRecord {
            pid: child.id().unwrap_or(0),
            model_id: config.model_id.clone(),
            plan_digest: digest,
            endpoint: endpoint.clone(),
            started_at_unix: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            binary: config.binary.to_string_lossy().to_string(),
        };
        write_record(&record_path, &record).map_err(|source| SupervisionError::Io {
            context: format!("write {}", record_path.display()),
            source,
        })?;

        let snapshot = ServingSnapshot {
            model_id: config.model_id.clone(),
            endpoint: endpoint.clone(),
            adopted: false,
        };
        let (notify, join) = self.spawn_watch(
            client,
            health_url,
            record_path.clone(),
            key_path.clone(),
            record,
            Some(child),
        );

        {
            let mut inner = self.inner.lock().expect("supervisor lock");
            inner.status = StatusSnapshot::Serving {
                model_id: snapshot.model_id.clone(),
                endpoint: snapshot.endpoint.clone(),
                adopted: false,
            };
            inner.watch = Some(WatchHandle { stop: notify, join });
        }
        Ok(snapshot)
    }

    /// The occupied-port gate (D-S3): verify the record, then adopt a
    /// matching live server or clear OUR verified other-model server
    /// for the swap — anything less verifiable is a typed refusal.
    async fn adopt_or_refuse(
        &self,
        config: &LaunchConfig,
        prepared: &PreparedLaunch,
        record_path: &Path,
        endpoint: &str,
        digest: &str,
    ) -> Result<OccupiedOutcome, SupervisionError> {
        let record = read_record(record_path);
        let Some(record) = record else {
            return Err(SupervisionError::PortOccupied {
                holder: "unknown (no launch record)".into(),
            });
        };
        if !pid_alive(record.pid) {
            // A stale record over an occupied port: the holder is not
            // ours — refuse naming what the record claimed.
            return Err(SupervisionError::PortOccupied {
                holder: format!("pid {} (dead, stale record)", record.pid),
            });
        }
        if record.model_id != config.model_id {
            // A live record of ANOTHER model. Only a process the record
            // provably names (argv[0] == the recorded binary — the
            // pid-recycling kill guard) may be swapped by pid: the
            // explicit launch is the intent, the identity check is the
            // boundary. A foreign process at the slot is NEVER killed.
            if recorded_process_is_ours(&record) {
                stop_by_pid(record.pid, record_path, &config.state_dir.join(KEY_FILE)).await;
                return Ok(OccupiedOutcome::ClearedForSpawn);
            }
            return Err(SupervisionError::PortOccupied {
                holder: format!(
                    "pid {} (a live server of `{}` the supervisor cannot verify as its own)",
                    record.pid, record.model_id
                ),
            });
        }
        if record.plan_digest != digest || record.endpoint != endpoint {
            return Err(SupervisionError::PortOccupied {
                holder: format!(
                    "pid {} (record does not match this launch: digest/endpoint differ)",
                    record.pid
                ),
            });
        }
        let client = health_probe_client()?;
        let health_url = format!("http://{}:{}", prepared.plan.host, prepared.plan.port)
            + &prepared.manifest.probe.health_path;
        if !health_ok(&client, &health_url).await {
            return Err(SupervisionError::PortOccupied {
                holder: format!(
                    "pid {} (record matches but health probe failed)",
                    record.pid
                ),
            });
        }
        // Verified adoption: a live, healthy, matching server.
        let snapshot = ServingSnapshot {
            model_id: record.model_id.clone(),
            endpoint: endpoint.to_string(),
            adopted: true,
        };
        let key_path = config.state_dir.join(KEY_FILE);
        let (notify, join) = self.spawn_watch(
            client,
            health_url,
            record_path.to_path_buf(),
            key_path,
            record,
            None,
        );
        {
            let mut inner = self.inner.lock().expect("supervisor lock");
            inner.status = StatusSnapshot::Serving {
                model_id: snapshot.model_id.clone(),
                endpoint: snapshot.endpoint.clone(),
                adopted: true,
            };
            inner.watch = Some(WatchHandle { stop: notify, join });
        }
        Ok(OccupiedOutcome::Adopted(snapshot))
    }

    /// The single-flight launch guard — the drop guard clears the flag
    /// on every exit path (success, refusal, panic).
    fn begin_launch(&self) -> Result<LaunchGuard, SupervisionError> {
        {
            let mut inner = self.inner.lock().expect("supervisor lock");
            if inner.launching {
                return Err(SupervisionError::AlreadyLaunching);
            }
            inner.launching = true;
        }
        Ok(LaunchGuard {
            inner: Arc::clone(&self.inner),
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn_watch(
        &self,
        client: reqwest::Client,
        health_url: String,
        record_path: PathBuf,
        key_path: PathBuf,
        record: LaunchRecord,
        child: Option<tokio::process::Child>,
    ) -> (Arc<tokio::sync::Notify>, tokio::task::JoinHandle<()>) {
        let supervisor = self.clone();
        let notify = Arc::new(tokio::sync::Notify::new());
        let stop = notify.clone();
        let join = tokio::spawn(async move {
            let mut child = child;
            let mut streak = 0u32;
            loop {
                tokio::select! {
                    _ = stop.notified() => {
                        stop_and_drain(&mut child, &record, &record_path, &key_path).await;
                        supervisor.set_status(StatusSnapshot::Stopped);
                        break;
                    }
                    _ = tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)) => {}
                }

                // Death first (cheap, no HTTP): a reaped process is
                // death immediately; a live-but-unhealthy process
                // needs the streak.
                if let Some(child) = child.as_mut() {
                    if let Ok(Some(status)) = child.try_wait() {
                        cleanup_state(&record_path, &key_path);
                        supervisor.set_status(StatusSnapshot::Failed {
                            reason: format!("the server exited: {status}"),
                        });
                        break;
                    }
                }
                if health_ok(&client, &health_url).await {
                    streak = 0;
                } else {
                    streak += 1;
                    if streak >= UNHEALTHY_STREAK {
                        cleanup_state(&record_path, &key_path);
                        supervisor.set_status(StatusSnapshot::Failed {
                            reason: format!(
                                "the health probe failed {UNHEALTHY_STREAK} consecutive times"
                            ),
                        });
                        break;
                    }
                }
            }
        });
        (notify, join)
    }

    /// Stop the serving server: SIGTERM, bounded drain, SIGKILL;
    /// remove the record and the token file. Typed on overrun.
    pub async fn stop(&self) -> Result<(), SupervisionError> {
        let handle = {
            let mut inner = self.inner.lock().expect("supervisor lock");
            inner.status = StatusSnapshot::Stopped;
            inner.watch.take()
        };
        let Some(handle) = handle else {
            return Ok(()); // Already stopped — idempotent.
        };
        handle.stop.notify_one();
        match tokio::time::timeout(
            Duration::from_secs(DRAIN_TIMEOUT_SECS + POLL_INTERVAL_SECS + 5),
            handle.join,
        )
        .await
        {
            Ok(_) => Ok(()),
            Err(_) => Err(SupervisionError::StopTimeout),
        }
    }

    /// The page's live state — a cheap snapshot read, never HTTP.
    pub fn status(&self) -> StatusSnapshot {
        self.inner.lock().expect("supervisor lock").status.clone()
    }

    fn set_status(&self, status: StatusSnapshot) {
        self.inner.lock().expect("supervisor lock").status = status;
    }
}

/// The serving-identity digest: the plan's command line PLUS the
/// endpoint fields and the alias — the same model on the same policy
/// digests equal; any policy or model change breaks adoption (the
/// plan's `Display` alone misses host/port, so they are hashed
/// explicitly — found by the digest's own falsifier test).
fn plan_digest(plan: &LaunchPlan, model_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(plan.to_string());
    hasher.update(plan.host.as_bytes());
    hasher.update(plan.port.to_be_bytes());
    hasher.update(b" --alias ");
    hasher.update(model_id);
    format!("{:x}", hasher.finalize())
}

async fn tcp_connectable(host: &str, port: u16) -> bool {
    tokio::net::TcpStream::connect((host, port)).await.is_ok()
}

/// Health-probe client: a probe slower than the poll cadence reads as a
/// failed poll. Without a bound, an accept-but-never-answer server (the
/// wedged-portal class) freezes the probe itself — the 20-minute launch
/// deadline and the unhealthy-streak watchdog only advance BETWEEN probes,
/// so an unbounded probe defeats both and status stays `Serving` forever.
fn health_probe_client() -> Result<reqwest::Client, SupervisionError> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(POLL_INTERVAL_SECS))
        .build()
        .map_err(|source| SupervisionError::Io {
            context: "health probe client construction".to_string(),
            source: std::io::Error::other(source.to_string()),
        })
}

async fn health_ok(client: &reqwest::Client, url: &str) -> bool {
    client
        .get(url)
        .send()
        .await
        .is_ok_and(|r| r.status().is_success())
}

fn pid_alive(pid: u32) -> bool {
    std::fs::metadata(format!("/proc/{pid}")).is_ok()
}

/// Write the endpoint token to a 0600 file — machine state, hidden
/// root; the server reads it via `--api-key-file` (no `/proc/cmdline`
/// leak, no env var).
fn write_secret_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)
}

fn write_record(path: &Path, record: &LaunchRecord) -> std::io::Result<()> {
    let json = serde_json::to_string(record).expect("the launch record serializes");
    std::fs::write(path, json)
}

fn read_record(path: &Path) -> Option<LaunchRecord> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn cleanup_state(record_path: &Path, key_path: &Path) {
    let _ = std::fs::remove_file(record_path);
    let _ = std::fs::remove_file(key_path);
}

/// SIGTERM → bounded drain → SIGKILL, then state cleanup. For an owned
/// child the handle reaps it; for an adopted pid this kills by the
/// recorded pid — but ONLY after verifying the recorded process is
/// still the one the record names (the pid-recycling kill guard); a
/// recycled or foreign pid is never killed.
async fn stop_and_drain(
    child: &mut Option<tokio::process::Child>,
    record: &LaunchRecord,
    record_path: &Path,
    key_path: &Path,
) {
    match child.as_mut() {
        Some(child) => {
            // SIGTERM via the pid (child.kill() is SIGKILL-only);
            // nix's safe wrapper — no unsafe blocks in this crate.
            kill(Pid::from_raw(record.pid as i32), Signal::SIGTERM).ok();
            let drained =
                tokio::time::timeout(Duration::from_secs(DRAIN_TIMEOUT_SECS), child.wait())
                    .await
                    .is_ok_and(|status| status.is_ok());
            if !drained {
                let _ = child.start_kill();
                let _ = child.wait().await;
            }
        }
        None => {
            if !recorded_process_is_ours(record) {
                // The recorded process died and the pid was recycled, or
                // something else took the slot: clean OUR state only —
                // never kill an unverified pid.
                cleanup_state(record_path, key_path);
                return;
            }
            stop_by_pid(record.pid, record_path, key_path).await;
        }
    }
    cleanup_state(record_path, key_path);
}

/// Whether the recorded pid is still the process the record names:
/// `/proc/<pid>/cmdline`'s argv[0] must equal the recorded binary path.
/// A recycled pid (the kill hazard) or a foreign process at the slot
/// fails this — the supervisor never kills an unverified pid.
fn recorded_process_is_ours(record: &LaunchRecord) -> bool {
    if !pid_alive(record.pid) {
        return false;
    }
    let Ok(cmdline) = std::fs::read(format!("/proc/{}/cmdline", record.pid)) else {
        return false;
    };
    // argv[0] — the NUL-separated command line's first field.
    let argv0 = cmdline.split(|byte| *byte == 0).next().unwrap_or_default();
    String::from_utf8_lossy(argv0) == record.binary
}

/// Stop a process by recorded pid: SIGTERM → bounded drain → SIGKILL,
/// then state cleanup. For a swapped or adopted server the supervisor
/// does not own the process handle — the recorded pid is the only
/// handle, and the caller has VERIFIED its identity first
/// (`recorded_process_is_ours`).
async fn stop_by_pid(pid: u32, record_path: &Path, key_path: &Path) {
    kill(Pid::from_raw(pid as i32), Signal::SIGTERM).ok();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(DRAIN_TIMEOUT_SECS);
    while pid_alive(pid) && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    if pid_alive(pid) {
        kill(Pid::from_raw(pid as i32), Signal::SIGKILL).ok();
    }
    cleanup_state(record_path, key_path);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_leg::manifest::shipped_manifest;

    /// Source pin (the D31 register style): the probe deadline lives inside
    /// reqwest and is unobservable from a unit test — pin the construction
    /// instead, over the production half of this file (the tests mod is
    /// excluded, as in the D31 pins): both probe sites route through
    /// `health_probe_client` (bounded at the poll interval) and no
    /// production site constructs an unbounded client (an unbounded probe
    /// freezes the health deadline and the streak watchdog — both only
    /// advance between probes).
    #[test]
    fn health_probes_are_deadline_bounded() {
        let source = include_str!("supervisor.rs");
        let production = source.split("mod tests {").next().unwrap_or(source);
        assert_eq!(
            production.matches("reqwest::Client::new()").count(),
            0,
            "an unbounded client reintroduces the wedged-server freeze"
        );
        assert_eq!(
            production
                .matches("let client = health_probe_client()?;")
                .count(),
            2,
            "both probe sites (launch health-wait, adopt probe) must be bounded"
        );
    }

    #[test]
    fn plan_digest_is_stable_and_policy_sensitive() {
        let manifest = shipped_manifest().expect("manifest");
        let plan = LaunchPlan {
            surface_id: "s".into(),
            binary: "llama-server".into(),
            host: "127.0.0.1".into(),
            port: 8010,
            args: vec!["--ctx-size".into(), "65536".into()],
        };
        let _ = manifest;
        let a = plan_digest(&plan, "qwen3-8-flash-next");
        assert_eq!(a, plan_digest(&plan, "qwen3-8-flash-next"), "stable");
        // A different model never adopts the same record.
        assert_ne!(a, plan_digest(&plan, "gemma-4-31b"));
        // A different policy (port) never adopts either (the digest
        // hashes the endpoint fields explicitly — the plan's Display
        // alone misses host/port).
        let mut other = plan;
        other.port = 8011;
        assert_ne!(a, plan_digest(&other, "qwen3-8-flash-next"));
    }

    #[test]
    fn launch_record_round_trips_through_disk() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("record.json");
        let record = LaunchRecord {
            pid: 4242,
            model_id: "qwen3-8-flash-next".into(),
            plan_digest: "d".repeat(64),
            endpoint: "http://127.0.0.1:8010/v1".into(),
            started_at_unix: 1_735_000_000,
            binary: "/bin/llama-server".into(),
        };
        write_record(&path, &record).expect("write");
        assert_eq!(read_record(&path).as_ref(), Some(&record));
        assert!(pid_alive(std::process::id()), "this process is alive");
        assert!(!pid_alive(u32::MAX - 3), "a bogus pid is not alive");
    }

    #[test]
    fn secret_file_is_written_0600() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("llama-server.key");
        write_secret_file(&path, b"the-token").expect("write");
        assert_eq!(std::fs::read(&path).expect("read"), b"the-token");
        let mode = std::fs::metadata(&path).expect("stat").permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "the token file is owner-only");
    }
}
