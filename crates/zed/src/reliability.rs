use anyhow::{Context as _, Result};
use client::{Client, telemetry::MINIDUMP_ENDPOINT};
use feature_flags::FeatureFlagAppExt;
use futures::{AsyncReadExt, TryStreamExt};
use gpui::{App, AppContext, Entity, TaskExt};
use http_client::{AsyncBody, HttpClient, Request};
use project::{Project, worktree_store::WorktreeStoreDiagnostics};
use proto::{CrashReport, GetCrashFilesResponse};
use reqwest::{
    Method,
    multipart::{Form, Part},
};
use serde::Deserialize;
use smol::stream::StreamExt;
use std::{
    collections::HashSet,
    ffi::OsStr,
    fs,
    sync::Arc,
    time::{Duration, Instant},
};
use sysinfo::{MemoryRefreshKind, ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System};
use util::ResultExt;
use workspace::WorkspaceStore;

mod hang_detection;

pub fn init(
    client: Arc<Client>,
    workspace_store: Entity<WorkspaceStore>,
    context_server_health_source: Arc<kask_bridge::BridgeContextServerHealthSource>,
    mcp_runtime: Arc<hkask_mcp::McpRuntime>,
    cx: &mut App,
) {
    hang_detection::start(client.clone(), cx);
    start_memory_usage_logging(workspace_store.clone(), cx);
    start_context_server_health_polling(
        workspace_store,
        context_server_health_source,
        client.clone(),
        mcp_runtime,
        cx,
    );

    cx.on_flags_ready({
        let client = client.clone();
        move |flags_ready, cx| {
            if flags_ready.is_staff {
                let client = client.clone();
                cx.background_spawn(async move {
                    upload_build_timings(client).await.warn_on_err();
                })
                .detach();
            }
        }
    })
    .detach();

    if client.telemetry().diagnostics_enabled() {
        let client = client.clone();
        cx.background_spawn(async move {
            upload_previous_minidumps(client).await.warn_on_err();
        })
        .detach()
    }

    cx.observe_new(move |project: &mut Project, _, cx| {
        let client = client.clone();

        let Some(remote_client) = project.remote_client() else {
            return;
        };
        remote_client.update(cx, |remote_client, cx| {
            if !client.telemetry().diagnostics_enabled() {
                return;
            }
            let request = remote_client
                .proto_client()
                .request(proto::GetCrashFiles {});
            cx.background_spawn(async move {
                let GetCrashFilesResponse { crashes } = request.await?;

                let Some(endpoint) = MINIDUMP_ENDPOINT.as_ref() else {
                    return Ok(());
                };
                for CrashReport {
                    metadata,
                    minidump_contents,
                } in crashes
                {
                    if let Some(metadata) = serde_json::from_str(&metadata).log_err() {
                        upload_minidump(client.clone(), endpoint, minidump_contents, &metadata)
                            .await
                            .log_err();
                    }
                }

                anyhow::Ok(())
            })
            .detach_and_log_err(cx);
        })
    })
    .detach();
}

const MEMORY_USAGE_POLL_INTERVAL: Duration = Duration::from_secs(30);
const MEMORY_USAGE_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10 * 60);
const MEMORY_USAGE_MINIMUM_LOGGED_DELTA: u64 = 64 * 1024 * 1024;

/// Periodically logs this process' memory usage, so that gradual memory growth can be
///
/// Logs on a fixed heartbeat, and additionally whenever resident memory changed
/// significantly since the last logged value, so that bursts of growth are timestamped
/// against the surrounding log entries.
fn start_memory_usage_logging(workspace_store: Entity<WorkspaceStore>, cx: &App) {
    let (diagnostics_sender, mut diagnostics_receiver) = futures::channel::mpsc::unbounded();
    cx.spawn(async move |cx| {
        while diagnostics_receiver.next().await.is_some() {
            cx.update(|cx| log_worktree_diagnostics(&workspace_store, cx));
        }
    })
    .detach();

    let executor = cx.background_executor().clone();
    cx.background_spawn(async move {
        let Some(pid) = sysinfo::get_current_pid().log_err() else {
            return;
        };
        let refresh_kind = ProcessRefreshKind::nothing().with_memory();
        let mut system = System::new();
        let mut last_logged_resident: Option<u64> = None;
        let mut last_logged_at = Instant::now();
        loop {
            let refreshed = system.refresh_processes_specifics(
                ProcessesToUpdate::Some(&[pid]),
                false,
                refresh_kind,
            );
            if refreshed == 1
                && let Some(process) = system.process(pid)
            {
                let resident = process.memory();
                let significant_change = last_logged_resident.is_none_or(|last| {
                    resident.abs_diff(last) >= (last / 10).max(MEMORY_USAGE_MINIMUM_LOGGED_DELTA)
                });
                if significant_change || last_logged_at.elapsed() >= MEMORY_USAGE_HEARTBEAT_INTERVAL
                {
                    const MIB: u64 = 1024 * 1024;
                    let delta = match last_logged_resident {
                        Some(last) => {
                            format!(" ({:+} MiB)", (resident as i64 - last as i64) / MIB as i64)
                        }
                        None => String::new(),
                    };
                    log::info!(
                        "memory usage: resident {} MiB{delta}, virtual {} MiB",
                        resident / MIB,
                        process.virtual_memory() / MIB,
                    );
                    if diagnostics_sender.unbounded_send(()).is_err() {
                        return;
                    }
                    last_logged_resident = Some(resident);
                    last_logged_at = Instant::now();
                }
            }
            executor.timer(MEMORY_USAGE_POLL_INTERVAL).await;
        }
    })
    .detach();
}

/// Polls all open projects' `ContextServerStore`s on a timer and updates
/// the shared health snapshot. The cybernetics loop's
/// `ContextServerHealthSensor` reads this snapshot each tick to detect MCP
/// servers stuck in `Starting` or `Error` — the signature of the
/// foreground-executor starvation bug where `initialize` never completes.
///
/// Runs on the foreground thread (reads GPUI entities) driven by a
/// background timer, mirroring `start_memory_usage_logging`'s pattern.
fn start_context_server_health_polling(
    workspace_store: Entity<WorkspaceStore>,
    health_source: Arc<kask_bridge::BridgeContextServerHealthSource>,
    client: Arc<Client>,
    mcp_runtime: Arc<hkask_mcp::McpRuntime>,
    cx: &App,
) {
    let (update_sender, mut update_receiver) = futures::channel::mpsc::unbounded();
    cx.spawn({
        async move |cx| {
            while update_receiver.next().await.is_some() {
                // Read ContextServerStore statuses synchronously on the foreground.
                let css_statuses = cx.update(|cx| {
                    read_context_server_statuses(
                        &workspace_store,
                        context_server_fleet_observable(client.user_id()),
                        cx,
                    )
                });
                // Query McpRuntime running servers asynchronously.
                let mrt_running = mcp_runtime.running_server_ids().await;
                // Merge and update.
                let (healthy, total) = merge_fleet_health(css_statuses, mrt_running);
                health_source.update(healthy, total);
            }
        }
    })
    .detach();

    let executor = cx.background_executor().clone();
    cx.background_spawn(async move {
        loop {
            if update_sender.unbounded_send(()).is_err() {
                return;
            }
            executor.timer(CONTEXT_SERVER_HEALTH_POLL_INTERVAL).await;
        }
    })
    .detach();
}

/// How often to poll context-server health. Matches the cybernetics loop's
/// tick cadence (10s) so the sensor sees a stuck server within one tick.
const CONTEXT_SERVER_HEALTH_POLL_INTERVAL: Duration = Duration::from_secs(10);

/// Whether the context-server fleet snapshot can be meaningfully measured.
///
/// Before the Zed user resolves (no credentials), kask's deferred servers
/// have not launched yet and credential-gated servers are stuck in
/// `Starting` — measuring then produced a constant false-positive
/// `ContextServerFleetDegraded` alert storm every tick. Gate on
/// `Client::user_id` (the same predicate the deferred post-login task
/// waits on via `UserStore::watch_current_user`).
fn context_server_fleet_observable(user_id: Option<u64>) -> bool {
    user_id.is_some()
}

/// Compute `(healthy_count, total_count)` across all open projects'
/// `ContextServerStore`s. Delegates the status-classification logic to
/// Read ContextServerStore statuses (per-project) as a map of server_id → is_healthy.
///
/// Only servers expected to be running count toward the map: `Starting`,
/// `Running`, and `Error`. Servers in `Stopped`, `AuthRequired`, etc. are
/// excluded — they are legitimate non-running states, not fleet degradation.
fn read_context_server_statuses(
    workspace_store: &Entity<WorkspaceStore>,
    observable: bool,
    cx: &App,
) -> std::collections::HashMap<String, bool> {
    use project::context_server_store::ContextServerStatus;
    use std::collections::HashMap;

    if !observable {
        return HashMap::new();
    }
    let workspaces = workspace_store
        .read(cx)
        .workspaces()
        .filter_map(|workspace| workspace.upgrade())
        .collect::<Vec<_>>();
    let mut seen_stores = HashSet::new();
    let mut statuses: HashMap<String, bool> = HashMap::new();
    for workspace in workspaces {
        let project = workspace.read(cx).project().clone();
        let context_server_store = project.read(cx).context_server_store();
        if !seen_stores.insert(context_server_store.entity_id()) {
            continue;
        }
        let store = context_server_store.read(cx);
        for server_id in store.server_ids() {
            if let Some(status) = store.status_for_server(server_id) {
                let is_counted = matches!(
                    status,
                    ContextServerStatus::Starting
                        | ContextServerStatus::Running
                        | ContextServerStatus::Error(_)
                );
                if is_counted {
                    let is_healthy = matches!(status, ContextServerStatus::Running);
                    statuses.insert(server_id.0.to_string(), is_healthy);
                }
            }
        }
    }
    statuses
}

/// Merge ContextServerStore and McpRuntime fleet health into (healthy, total).
///
/// A server is healthy if it is `Running` in the ContextServerStore OR has a
/// live connection in the McpRuntime. The total is the union of all server IDs
/// from both paths — no double-counting.
fn merge_fleet_health(
    css_statuses: std::collections::HashMap<String, bool>,
    mrt_running: Vec<String>,
) -> (usize, usize) {
    use std::collections::HashSet;

    let mut all_servers: HashSet<String> = HashSet::new();
    all_servers.extend(css_statuses.keys().cloned());
    all_servers.extend(mrt_running.iter().cloned());

    let total = all_servers.len();
    let healthy = all_servers
        .iter()
        .filter(|id| {
            *css_statuses.get(*id).unwrap_or(&false)
                || mrt_running.iter().any(|mrt_id| mrt_id == *id)
        })
        .count();

    (healthy, total)
}

/// Classify context-server fleet health from a slice of server statuses.
///
/// Only servers expected to be running count toward `total`: `Starting`,
/// `Running`, and `Error`. Servers in `Stopped` (deliberately stopped or
/// disabled), `AuthRequired`, `ClientSecretRequired`, and `Authenticating`
/// (OAuth-waiting) are excluded — they are legitimate non-running states,
/// not fleet degradation. Counting them produced constant false-positive
/// `ContextServerFleetDegraded` alerts every tick.
///
/// Returns `(healthy_count, total_count)` where `healthy_count` is the
/// number of `Running` servers and `total_count` is the number of servers
/// expected to be running (`Starting` + `Running` + `Error`).
#[cfg(test)]
fn classify_fleet_health(
    statuses: &[project::context_server_store::ContextServerStatus],
) -> (usize, usize) {
    use project::context_server_store::ContextServerStatus;

    let mut healthy = 0;
    let mut total = 0;
    for status in statuses {
        match status {
            ContextServerStatus::Starting
            | ContextServerStatus::Running
            | ContextServerStatus::Error(_) => {
                total += 1;
                if let ContextServerStatus::Running = status {
                    healthy += 1;
                }
            }
            // Deliberately not running — not fleet degradation.
            ContextServerStatus::Stopped
            | ContextServerStatus::AuthRequired
            | ContextServerStatus::ClientSecretRequired { .. }
            | ContextServerStatus::Authenticating => {}
        }
    }
    (healthy, total)
}

fn log_worktree_diagnostics(workspace_store: &Entity<WorkspaceStore>, cx: &App) {
    let workspaces = workspace_store
        .read(cx)
        .workspaces()
        .filter_map(|workspace| workspace.upgrade())
        .collect::<Vec<_>>();
    let mut worktree_store_ids = HashSet::new();
    let mut store_count = 0;
    let mut aggregate = WorktreeStoreDiagnostics::default();

    for workspace in workspaces {
        let project = workspace.read(cx).project().clone();
        let worktree_store = project.read(cx).worktree_store();
        if !worktree_store_ids.insert(worktree_store.entity_id()) {
            continue;
        }
        store_count += 1;

        let WorktreeStoreDiagnostics {
            worktree_slots,
            live_worktrees,
            visible_worktrees,
            strong_handles,
            dead_weak_handles,
            loading_worktrees,
            total_entries,
            visible_entries,
            largest_worktree,
        } = worktree_store.read(cx).diagnostics(cx);
        aggregate.worktree_slots += worktree_slots;
        aggregate.live_worktrees += live_worktrees;
        aggregate.visible_worktrees += visible_worktrees;
        aggregate.strong_handles += strong_handles;
        aggregate.dead_weak_handles += dead_weak_handles;
        aggregate.loading_worktrees += loading_worktrees;
        aggregate.total_entries += total_entries;
        aggregate.visible_entries += visible_entries;

        if let Some(largest_worktree) = largest_worktree
            && aggregate
                .largest_worktree
                .as_ref()
                .is_none_or(|largest| largest_worktree.entries > largest.entries)
        {
            aggregate.largest_worktree = Some(largest_worktree);
        }
    }

    let WorktreeStoreDiagnostics {
        worktree_slots,
        live_worktrees,
        visible_worktrees,
        strong_handles,
        dead_weak_handles,
        loading_worktrees,
        total_entries,
        visible_entries,
        largest_worktree,
    } = aggregate;
    match largest_worktree {
        Some(largest_worktree) => log::info!(
            "worktree diagnostics: stores {store_count}, slots {worktree_slots}, live {live_worktrees}, visible {visible_worktrees}, strong {strong_handles}, dead weak {dead_weak_handles}, loading {loading_worktrees}, entries {total_entries}, visible entries {visible_entries}, largest {} ({} entries, {} visible)",
            largest_worktree.path.display(),
            largest_worktree.entries,
            largest_worktree.visible_entries,
        ),
        None => log::info!(
            "worktree diagnostics: stores {store_count}, slots {worktree_slots}, live {live_worktrees}, visible {visible_worktrees}, strong {strong_handles}, dead weak {dead_weak_handles}, loading {loading_worktrees}, entries {total_entries}, visible entries {visible_entries}, largest none",
        ),
    }
}

pub async fn upload_previous_minidumps(client: Arc<Client>) -> anyhow::Result<()> {
    let Some(minidump_endpoint) = MINIDUMP_ENDPOINT.as_ref() else {
        log::warn!("Minidump endpoint not set");
        return Ok(());
    };

    let mut children = smol::fs::read_dir(paths::logs_dir()).await?;
    while let Some(child) = children.next().await {
        let child = child?;
        let child_path = child.path();
        if child_path.extension() != Some(OsStr::new("dmp")) {
            continue;
        }
        let mut json_path = child_path.clone();
        json_path.set_extension("json");
        let Ok(metadata) = smol::fs::read(&json_path)
            .await
            .map_err(|e| anyhow::anyhow!(e))
            .and_then(|data| serde_json::from_slice(&data).map_err(|e| anyhow::anyhow!(e)))
        else {
            continue;
        };
        if upload_minidump(
            client.clone(),
            minidump_endpoint,
            smol::fs::read(&child_path)
                .await
                .context("Failed to read minidump")?,
            &metadata,
        )
        .await
        .log_err()
        .is_some()
        {
            fs::remove_file(child_path).ok();
            fs::remove_file(json_path).ok();
        }
    }
    Ok(())
}

async fn upload_minidump(
    client: Arc<Client>,
    endpoint: &str,
    minidump: Vec<u8>,
    metadata: &crashes::CrashInfo,
) -> Result<()> {
    if metadata.init.commit_sha == "no sha" {
        log::warn!("No commit sha set, skipping minidump upload");
        return Ok(());
    }
    let mut form = Form::new()
        .part(
            "upload_file_minidump",
            Part::bytes(minidump)
                .file_name("minidump.dmp")
                .mime_str("application/octet-stream")?,
        )
        .text(
            "sentry[tags][channel]",
            metadata.init.release_channel.clone(),
        )
        .text("sentry[tags][version]", metadata.init.zed_version.clone())
        .text("sentry[tags][binary]", metadata.init.binary.clone())
        .text("sentry[release]", metadata.init.commit_sha.clone())
        .text("platform", "rust");
    let mut panic_message = "".to_owned();
    if let Some(panic_info) = metadata.panic.as_ref() {
        panic_message = panic_info.message.clone();
        form = form
            .text("sentry[logentry][formatted]", panic_info.message.clone())
            .text("span", panic_info.span.clone());
    }
    if let Some(minidump_error) = metadata.minidump_error.clone() {
        form = form.text("minidump_error", minidump_error);
    }
    if let Some(abort_message) = metadata.abort_message.as_ref() {
        // Sentry tag values are limited to 200 characters on a single line, so
        // put a searchable prefix in the tag (which grouping rules also match
        // on) and the full message in a context.
        let tag: String = abort_message
            .lines()
            .next()
            .unwrap_or_default()
            .chars()
            .take(200)
            .collect();
        form = form
            .text("sentry[tags][abort_message]", tag)
            .text("sentry[contexts][abort][message]", abort_message.clone());
    }

    if let Some(is_staff) = &metadata
        .user_info
        .as_ref()
        .and_then(|user_info| user_info.is_staff)
    {
        form = form.text(
            "sentry[user][is_staff]",
            if *is_staff { "true" } else { "false" },
        );
    }

    if let Some(metrics_id) = metadata
        .user_info
        .as_ref()
        .and_then(|user_info| user_info.metrics_id.as_ref())
    {
        form = form.text("sentry[user][id]", metrics_id.clone());
    } else if let Some(id) = client.telemetry().installation_id() {
        form = form.text("sentry[user][id]", format!("installation-{}", id))
    }

    ::telemetry::event!(
        "Minidump Uploaded",
        panic_message = panic_message,
        crashed_version = metadata.init.zed_version.clone(),
        commit_sha = metadata.init.commit_sha.clone(),
    );

    let gpu_count = metadata.gpus.len();
    for (index, gpu) in metadata.gpus.iter().cloned().enumerate() {
        let system_specs::GpuInfo {
            device_name,
            device_pci_id,
            vendor_name,
            vendor_pci_id,
            driver_version,
            driver_name,
        } = gpu;
        let num = if gpu_count == 1 && metadata.active_gpu.is_none() {
            String::new()
        } else {
            index.to_string()
        };
        let name = format!("gpu{num}");
        let root = format!("sentry[contexts][{name}]");
        form = form
            .text(
                format!("{root}[Description]"),
                "A GPU found on the users system. May or may not be the GPU Zed is running on",
            )
            .text(format!("{root}[type]"), "gpu")
            .text(format!("{root}[name]"), device_name.unwrap_or(name))
            .text(format!("{root}[id]"), format!("{:#06x}", device_pci_id))
            .text(
                format!("{root}[vendor_id]"),
                format!("{:#06x}", vendor_pci_id),
            )
            .text_if_some(format!("{root}[vendor_name]"), vendor_name)
            .text_if_some(format!("{root}[driver_version]"), driver_version)
            .text_if_some(format!("{root}[driver_name]"), driver_name);
    }
    if let Some(active_gpu) = metadata.active_gpu.clone() {
        form = form
            .text(
                "sentry[contexts][Active_GPU][Description]",
                "The GPU Zed is running on",
            )
            .text("sentry[contexts][Active_GPU][type]", "gpu")
            .text("sentry[contexts][Active_GPU][name]", active_gpu.device_name)
            .text(
                "sentry[contexts][Active_GPU][driver_version]",
                active_gpu.driver_info,
            )
            .text(
                "sentry[contexts][Active_GPU][driver_name]",
                active_gpu.driver_name,
            )
            .text(
                "sentry[contexts][Active_GPU][is_software_emulated]",
                active_gpu.is_software_emulated.to_string(),
            );
    }

    // TODO: feature-flag-context, and more of device-context like screen resolution, available ram, device model, etc

    let content_type = format!("multipart/form-data; boundary={}", form.boundary());
    let mut body_bytes = Vec::new();
    let mut stream = form
        .into_stream()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
        .into_async_read();
    stream.read_to_end(&mut body_bytes).await?;
    let req = Request::builder()
        .method(Method::POST)
        .uri(endpoint)
        .header("Content-Type", content_type)
        .body(AsyncBody::from(body_bytes))?;
    let mut response_text = String::new();
    let mut response = client.http_client().send(req).await?;
    response
        .body_mut()
        .read_to_string(&mut response_text)
        .await?;
    if !response.status().is_success() {
        anyhow::bail!("failed to upload minidump: {response_text}");
    }
    log::info!("Uploaded minidump. event id: {response_text}");
    Ok(())
}

#[derive(Debug, Deserialize)]
struct BuildTiming {
    started_at: chrono::DateTime<chrono::Utc>,
    duration_ms: f32,
    first_crate: String,
    target: String,
    blocked_ms: f32,
    command: String,
}

// NOTE: this is a bit of a hack. We want to be able to have internal
// metrics around build times, but we don't have an easy way to authenticate
// users - except - we know internal users use Zed.
// So, we have it upload the timings on their behalf, it'd be better to do
// this more directly in ./script/cargo-timing-info.js.
async fn upload_build_timings(_client: Arc<Client>) -> Result<()> {
    let build_timings_dir = paths::data_dir().join("build_timings");

    if !build_timings_dir.exists() {
        return Ok(());
    }

    let cpu_count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let system = System::new_with_specifics(
        RefreshKind::nothing().with_memory(MemoryRefreshKind::everything()),
    );
    let ram_size_gb = (system.total_memory() as f64) / (1024.0 * 1024.0 * 1024.0);

    let mut entries = smol::fs::read_dir(&build_timings_dir).await?;
    while let Some(entry) = entries.next().await {
        let entry = entry?;
        let path = entry.path();

        if path.extension() != Some(OsStr::new("json")) {
            continue;
        }

        let contents = match smol::fs::read_to_string(&path).await {
            Ok(contents) => contents,
            Err(err) => {
                log::warn!("Failed to read build timing file {:?}: {}", path, err);
                continue;
            }
        };

        let timing: BuildTiming = match serde_json::from_str(&contents) {
            Ok(timing) => timing,
            Err(err) => {
                log::warn!("Failed to parse build timing file {:?}: {}", path, err);
                continue;
            }
        };

        telemetry::event!(
            "Build Timing: Cargo Build",
            started_at = timing.started_at.to_rfc3339(),
            duration_ms = timing.duration_ms,
            first_crate = timing.first_crate,
            target = timing.target,
            blocked_ms = timing.blocked_ms,
            command = timing.command,
            cpu_count = cpu_count,
            ram_size_gb = ram_size_gb
        );

        if let Err(err) = smol::fs::remove_file(&path).await {
            log::warn!("Failed to delete build timing file {:?}: {}", path, err);
        }
    }

    Ok(())
}

trait FormExt {
    fn text_if_some(
        self,
        label: impl Into<std::borrow::Cow<'static, str>>,
        value: Option<impl Into<std::borrow::Cow<'static, str>>>,
    ) -> Self;
}

impl FormExt for Form {
    fn text_if_some(
        self,
        label: impl Into<std::borrow::Cow<'static, str>>,
        value: Option<impl Into<std::borrow::Cow<'static, str>>>,
    ) -> Self {
        match value {
            Some(value) => self.text(label.into(), value.into()),
            None => self,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{classify_fleet_health, context_server_fleet_observable};
    use project::context_server_store::ContextServerStatus;
    use std::sync::Arc;

    #[test]
    fn all_running_is_fully_healthy() {
        let statuses = vec![
            ContextServerStatus::Running,
            ContextServerStatus::Running,
            ContextServerStatus::Running,
        ];
        let (healthy, total) = classify_fleet_health(&statuses);
        assert_eq!(healthy, 3);
        assert_eq!(total, 3);
    }

    #[test]
    fn starting_counts_toward_total_but_not_healthy() {
        // A server stuck in Starting is the real degradation signal — it must
        // count toward total so the ratio drops below 1.0 and the alert fires.
        let statuses = vec![ContextServerStatus::Running, ContextServerStatus::Starting];
        let (healthy, total) = classify_fleet_health(&statuses);
        assert_eq!(healthy, 1);
        assert_eq!(total, 2);
    }

    #[test]
    fn error_counts_toward_total_but_not_healthy() {
        let statuses = vec![
            ContextServerStatus::Running,
            ContextServerStatus::Error("spawn failed".into()),
        ];
        let (healthy, total) = classify_fleet_health(&statuses);
        assert_eq!(healthy, 1);
        assert_eq!(total, 2);
    }

    #[test]
    fn stopped_servers_are_excluded_from_total() {
        // Stopped is a deliberate state (disabled, AI off) — not fleet
        // degradation. It must NOT count toward total, or the alert fires
        // every tick as a false positive.
        let statuses = vec![
            ContextServerStatus::Running,
            ContextServerStatus::Stopped,
            ContextServerStatus::Stopped,
        ];
        let (healthy, total) = classify_fleet_health(&statuses);
        assert_eq!(healthy, 1);
        assert_eq!(total, 1);
    }

    #[test]
    fn oauth_waiting_states_are_excluded_from_total() {
        // AuthRequired, ClientSecretRequired, and Authenticating are
        // OAuth-waiting states, not broken servers.
        let statuses = vec![
            ContextServerStatus::Running,
            ContextServerStatus::AuthRequired,
            ContextServerStatus::ClientSecretRequired { error: None },
            ContextServerStatus::Authenticating,
        ];
        let (healthy, total) = classify_fleet_health(&statuses);
        assert_eq!(healthy, 1);
        assert_eq!(total, 1);
    }

    #[test]
    fn all_stopped_produces_zero_total_no_alert() {
        // When every server is deliberately stopped, total is 0 and the
        // sensor returns None (no signal) — no false-positive alert.
        let statuses = vec![ContextServerStatus::Stopped, ContextServerStatus::Stopped];
        let (healthy, total) = classify_fleet_health(&statuses);
        assert_eq!(healthy, 0);
        assert_eq!(total, 0);
    }

    #[test]
    fn all_starting_signals_degradation() {
        // The original storm scenario: every server stuck in Starting.
        let statuses = vec![
            ContextServerStatus::Starting,
            ContextServerStatus::Starting,
            ContextServerStatus::Starting,
        ];
        let (healthy, total) = classify_fleet_health(&statuses);
        assert_eq!(healthy, 0);
        assert_eq!(total, 3);
    }

    #[test]
    fn mixed_real_and_excluded_states() {
        // Real-world mix: some running, some stopped, one stuck starting,
        // one in error, one waiting for OAuth.
        let error: Arc<str> = "connection refused".into();
        let statuses = vec![
            ContextServerStatus::Running,
            ContextServerStatus::Running,
            ContextServerStatus::Stopped,
            ContextServerStatus::Starting,
            ContextServerStatus::Error(error),
            ContextServerStatus::AuthRequired,
        ];
        let (healthy, total) = classify_fleet_health(&statuses);
        // 2 Running (healthy), 1 Starting + 1 Error count toward total.
        // Stopped and AuthRequired excluded.
        assert_eq!(healthy, 2);
        assert_eq!(total, 4);
    }

    #[test]
    fn empty_slice_is_zero_zero() {
        let (healthy, total) = classify_fleet_health(&[]);
        assert_eq!(healthy, 0);
        assert_eq!(total, 0);
    }

    #[test]
    fn pre_login_fleet_is_unobservable() {
        // No credentials → gate is closed → snapshot reports (0, 0) → the
        // sensor emits no signal. This pins the false-positive-storm fix.
        assert!(!context_server_fleet_observable(None));
    }

    #[test]
    fn resolved_user_opens_the_gate() {
        assert!(context_server_fleet_observable(Some(7)));
    }

    // ── merge_fleet_health tests ──────────────────────────────────────

    #[test]
    fn merge_both_paths_running_is_fully_healthy() {
        // Both ContextServerStore and McpRuntime have the same servers running.
        let css = std::collections::HashMap::from([
            ("curator".to_string(), true),
            ("corpus".to_string(), true),
        ]);
        let mrt = vec!["curator".to_string(), "corpus".to_string()];
        let (healthy, total) = super::merge_fleet_health(css, mrt);
        assert_eq!(healthy, 2);
        assert_eq!(total, 2);
    }

    #[test]
    fn merge_mrt_only_server_is_healthy() {
        // A server only in McpRuntime (not in ContextServerStore) is still healthy.
        let css = std::collections::HashMap::from([
            ("curator".to_string(), true),
        ]);
        let mrt = vec!["curator".to_string(), "companies".to_string()];
        let (healthy, total) = super::merge_fleet_health(css, mrt);
        assert_eq!(healthy, 2);
        assert_eq!(total, 2);
    }

    #[test]
    fn merge_css_error_mrt_running_is_healthy() {
        // A server in Error state in ContextServerStore but running in McpRuntime
        // is healthy — the McpRuntime path compensates.
        let css = std::collections::HashMap::from([
            ("media".to_string(), false), // Error in CSS
        ]);
        let mrt = vec!["media".to_string()]; // Running in MRT
        let (healthy, total) = super::merge_fleet_health(css, mrt);
        assert_eq!(healthy, 1);
        assert_eq!(total, 1);
    }

    #[test]
    fn merge_both_empty_is_zero() {
        let css = std::collections::HashMap::new();
        let mrt = vec![];
        let (healthy, total) = super::merge_fleet_health(css, mrt);
        assert_eq!(healthy, 0);
        assert_eq!(total, 0);
    }

    #[test]
    fn merge_no_double_counting() {
        // Same server in both paths — counted once.
        let css = std::collections::HashMap::from([
            ("curator".to_string(), true),
        ]);
        let mrt = vec!["curator".to_string()];
        let (healthy, total) = super::merge_fleet_health(css, mrt);
        assert_eq!(healthy, 1);
        assert_eq!(total, 1);
    }
}
