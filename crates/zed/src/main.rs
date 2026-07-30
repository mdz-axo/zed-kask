// Disable command line from opening on release mode
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod reliability;
mod zed;

// Ensure the binary name stays in sync with APP_NAME so that the paths used
// at runtime (data dir, config dir, etc.) match what the binary is called.
const _: () = assert!(
    paths::APP_NAME_LOWERCASE
        .as_bytes()
        .eq_ignore_ascii_case(env!("CARGO_BIN_NAME").as_bytes()),
    "paths::APP_NAME_LOWERCASE must match the binary name. \
     Forks: update APP_NAME in crates/paths/src/paths.rs when renaming the binary.",
);

use agent_ui::AgentPanel;
use anyhow::{Context as _, Result};
use clap::Parser;
use cli::FORCE_CLI_MODE_ENV_VAR_NAME;
use client::{
    Client, ProxySettings, RefreshLlmTokenListener, UserStore, ZED_URL_SCHEME, parse_zed_link,
};
use collab_ui::channel_view::ChannelView;
use collections::HashMap;
use crashes::InitCrashHandler;
use db::kvp::{GlobalKeyValueStore, KeyValueStore};
use editor::Editor;
use extension::ExtensionHostProxy;
use fs::{Fs, RealFs};
use futures::{FutureExt, StreamExt, channel::oneshot, future};
use git::GitHostingProviderRegistry;
use git_ui::clone::clone_and_open;
use gpui::{
    App, AppContext, Application, AsyncApp, QuitMode, Task, TaskExt, UpdateGlobal as _, block_on,
};
use gpui_platform;

use gpui_tokio::Tokio;
use language::LanguageRegistry;
use onboarding::{FIRST_OPEN, show_onboarding_view};
use project_panel::ProjectPanel;
use prompt_store::PromptBuilder;
use remote::RemoteConnectionOptions;
use reqwest_client::ReqwestClient;

use assets::Assets;
use node_runtime::{NodeBinaryOptions, NodeRuntime};
use parking_lot::Mutex;
use project::{project_settings::ProjectSettings, trusted_worktrees};
use recent_projects::{RemoteSettings, open_remote_project};
use release_channel::{AppCommitSha, AppVersion, ReleaseChannel};
use session::{AppSession, Session};
use settings::{BaseKeymap, Settings, SettingsStore, watch_config_file};
use smol::future::poll_once;
use std::{
    cell::RefCell,
    env,
    io::{self, IsTerminal},
    path::{Path, PathBuf},
    process,
    rc::Rc,
    sync::{Arc, LazyLock, OnceLock},
    time::Instant,
};
use theme::{ActiveTheme, GlobalTheme, ThemeRegistry};
use theme_settings::load_user_theme;
use util::{ResultExt, maybe};
use uuid::Uuid;
use workspace::{
    AppState, MultiWorkspace, SerializedWorkspaceLocation, SessionWorkspace, Toast,
    WorkspaceSettings, WorkspaceStore,
    notifications::{NotificationId, NotifyResultExt},
    restore_multiworkspace,
};
use zed::{
    OpenListener, OpenRequest, RawOpenRequest, app_menus, build_window_options,
    derive_paths_with_position, edit_prediction_registry, handle_cli_connection,
    handle_keymap_file_changes, initialize_workspace, open_paths_with_positions,
};

use crate::zed::{CrashHandler, OpenRequestKind, eager_load_active_theme_and_icon_theme};

#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn build_application() -> Application {
    let platform = gpui_platform::current_platform(false);
    if std::env::var("ZED_EXPERIMENTAL_A11Y").as_deref() == Ok("1") {
        Application::with_platform(platform)
    } else {
        Application::new_inaccessible(platform)
    }
}

fn files_not_created_on_launch(errors: HashMap<io::ErrorKind, Vec<&Path>>) {
    let message = "Zed failed to launch";
    let error_details = errors
        .into_iter()
        .flat_map(|(kind, paths)| {
            #[allow(unused_mut)] // for non-unix platforms
            let mut error_kind_details = match paths.len() {
                0 => return None,
                1 => format!(
                    "{kind} when creating directory {:?}",
                    paths.first().expect("match arm checks for a single entry")
                ),
                _many => format!("{kind} when creating directories {paths:?}"),
            };

            #[cfg(unix)]
            {
                if kind == io::ErrorKind::PermissionDenied {
                    error_kind_details.push_str("\n\nConsider using chown and chmod tools for altering the directories permissions if your user has corresponding rights.\
                        \nFor example, `sudo chown $(whoami):staff ~/.config` and `chmod +uwrx ~/.config`");
                }
            }

            Some(error_kind_details)
        })
        .collect::<Vec<_>>().join("\n\n");

    eprintln!("{message}: {error_details}");
    build_application()
        .with_quit_mode(QuitMode::Explicit)
        .run(move |cx| {
            if let Ok(window) = cx.open_window(gpui::WindowOptions::default(), |_, cx| {
                cx.new(|_| gpui::Empty)
            }) {
                window
                    .update(cx, |_, window, cx| {
                        let response = window.prompt(
                            gpui::PromptLevel::Critical,
                            message,
                            Some(&error_details),
                            &["Exit"],
                            cx,
                        );

                        cx.spawn_in(window, async move |_, cx| {
                            response.await?;
                            cx.update(|_, cx| cx.quit())
                        })
                        .detach_and_log_err(cx);
                    })
                    .log_err();
            } else {
                fail_to_open_window(anyhow::anyhow!("{message}: {error_details}"), cx)
            }
        })
}

fn fail_to_open_window_async(e: anyhow::Error, cx: &mut AsyncApp) {
    cx.update(|cx| fail_to_open_window(e, cx));
}

fn fail_to_open_window(e: anyhow::Error, _cx: &mut App) {
    eprintln!(
        "Zed failed to open a window: {e:?}. See https://zed.dev/docs/linux for troubleshooting steps."
    );
    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    {
        process::exit(1);
    }

    // Maybe unify this with gpui::platform::linux::platform::ResultExt::notify_err(..)?
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    {
        use ashpd::desktop::notification::{Notification, NotificationProxy, Priority};
        _cx.spawn(async move |_cx| {
            let Ok(proxy) = NotificationProxy::new().await else {
                process::exit(1);
            };

            let notification_id = "dev.zed.Oops";
            proxy
                .add_notification(
                    notification_id,
                    Notification::new("Zed failed to launch")
                        .body(Some(
                            format!(
                                "{e:?}. See https://zed.dev/docs/linux for troubleshooting steps."
                            )
                            .as_str(),
                        ))
                        .priority(Priority::High)
                        .icon(ashpd::desktop::Icon::with_names(&[
                            "dialog-question-symbolic",
                        ])),
                )
                .await
                .ok();

            process::exit(1);
        })
        .detach();
    }
}
static STARTUP_TIME: OnceLock<Instant> = OnceLock::new();

/// The Unix socket path for the inference IPC bridge.
///
/// Set by the model-dependent kask wiring block after the `GuardedInferencePort`
/// is constructed. Read by the deferred MCP server launch task to pass the
/// socket path to MCP server child processes via the `HKASK_INFERENCE_SOCKET`
/// env var.
static INFERENCE_SOCKET_PATH: OnceLock<String> = OnceLock::new();

fn main() {
    STARTUP_TIME.get_or_init(|| Instant::now());

    // If this process was re-executed as a Linux sandbox helper, run that mode
    // without returning. Must run before argument parsing: the wrapped command's
    // args are appended verbatim and would otherwise be misinterpreted as Zed's
    // own arguments.
    sandbox::run_sandbox_launcher_if_invoked();

    #[cfg(unix)]
    util::prevent_root_execution();

    let args = Args::parse();

    // `zed --askpass` Makes zed operate in nc/netcat mode for use with askpass
    #[cfg(not(target_os = "windows"))]
    if let Some(socket) = &args.askpass {
        askpass::main(socket);
        return;
    }

    // `zed --crash-handler` Makes zed operate in minidump crash handler mode
    if let Some(socket) = &args.crash_handler {
        crashes::crash_server(socket.as_path(), paths::logs_dir().clone());
        return;
    }

    #[cfg(target_os = "windows")]
    if args.record_etw_trace {
        let zed_pid = args
            .etw_zed_pid
            .and_then(|pid| if pid >= 0 { Some(pid as u32) } else { None });
        let Some(output_path) = args.etw_output else {
            eprintln!("--etw-output is required for --record-etw-trace");
            process::exit(1);
        };

        let Some(etw_socket) = args.etw_socket else {
            eprintln!("--etw-socket is required for --record-etw-trace");
            process::exit(1);
        };

        if let Err(error) =
            etw_tracing::record_etw_trace(zed_pid, &output_path, etw_socket.as_str())
        {
            eprintln!("ETW trace recording failed: {error:#}");
            process::exit(1);
        }
        return;
    }

    #[cfg(all(not(debug_assertions), target_os = "windows"))]
    unsafe {
        use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};

        if args.foreground {
            let _ = AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }

    // `zed --printenv` Outputs environment variables as JSON to stdout
    if args.printenv {
        util::shell_env::print_env();
        return;
    }

    if args.dump_all_actions {
        dump_all_gpui_actions();
        return;
    }

    // Load kask `.env` file if present. The file contains API keys and
    // configuration for kask inference providers (DEEPINFRA_API_KEY, FALAI_API_KEY,
    // TOGETHERAI_API_KEY, OPENROUTER_API_KEY, etc.) and kask runtime settings (HKASK_*).
    // Without this, the keys are invisible to the process even though they're
    // in the file.
    //
    // Search order:
    // 1. `<config_dir>/.env` — the installed-binary location, alongside
    //    `settings.json`. This is where users put their `.env` after install:
    //    `~/.config/zed-kask/.env` on Linux,
    //    `~/Library/Application Support/Zed-Kask/.env` on macOS,
    //    `%APPDATA%/Zed-Kask/.env` on Windows.
    // 2. `.env` in CWD — dev convenience (running from the repo root).
    // 3. `kask/.env` — legacy dev layout (running from a kask project dir).
    //
    // dotenvy does NOT override existing env vars — if a key is already in
    // the process environment (e.g. from the shell), the file value is
    // ignored. This is the correct behavior: shell env > .env file.
    let config_env = paths::config_dir().join(".env");
    for env_path in [
        config_env.as_path(),
        std::path::Path::new(".env"),
        std::path::Path::new("kask/.env"),
    ] {
        if env_path.is_file() {
            if let Err(e) = dotenvy::from_path(env_path) {
                log::warn!("Failed to load {}: {e}", env_path.display());
            } else {
                log::info!("Loaded environment from {}", env_path.display());
            }
            break;
        }
    }

    // Set custom data directory.
    if let Some(dir) = &args.user_data_dir {
        paths::set_custom_data_dir(dir);
    }

    #[cfg(target_os = "windows")]
    match util::get_zed_cli_path() {
        Ok(path) => askpass::set_askpass_program(path),
        Err(err) => {
            eprintln!("Error: {}", err);
            if std::option_env!("ZED_BUNDLE").is_some() {
                process::exit(1);
            }
        }
    }

    let file_errors = init_paths();
    if !file_errors.is_empty() {
        files_not_created_on_launch(file_errors);
        return;
    }

    zlog::init();

    if stdout_is_a_pty() {
        zlog::init_output_stdout();
    } else {
        let result = zlog::init_output_file(paths::log_file(), Some(paths::old_log_file()));
        if let Err(err) = result {
            eprintln!("Could not open log file: {}... Defaulting to stdout", err);
            zlog::init_output_stdout();
        };
    }
    ztracing::init();

    let version = option_env!("ZED_BUILD_ID");
    let app_commit_sha =
        option_env!("ZED_COMMIT_SHA").map(|commit_sha| AppCommitSha::new(commit_sha.to_string()));
    let app_version = AppVersion::load(env!("CARGO_PKG_VERSION"), version, app_commit_sha.clone());

    if args.system_specs {
        let system_specs = system_specs::SystemSpecs::new_stateless(
            app_version,
            app_commit_sha,
            *release_channel::RELEASE_CHANNEL,
            client::telemetry::os_name(),
            client::telemetry::os_version(),
        );
        println!("Zed System Specs (from CLI):\n{}", system_specs);
        return;
    }

    rayon::ThreadPoolBuilder::new()
        .num_threads(std::thread::available_parallelism().map_or(1, |n| n.get().div_ceil(2)))
        .stack_size(10 * 1024 * 1024)
        .thread_name(|ix| format!("RayonWorker{}", ix))
        .build_global()
        .unwrap();

    log::info!(
        "========== starting zed version {}, sha {} ==========",
        app_version,
        app_commit_sha
            .as_ref()
            .map(|sha| sha.short())
            .as_deref()
            .unwrap_or("unknown"),
    );

    #[cfg(windows)]
    check_for_conpty_dll();

    let app = build_application().with_assets(Assets);

    let app_db = db::AppDatabase::new();
    let system_id = app.background_executor().spawn(system_id());
    let installation_id = app
        .background_executor()
        .spawn(installation_id(KeyValueStore::from_app_db(&app_db)));
    let session_id = Uuid::new_v4().to_string();
    let session = app.background_executor().spawn(Session::new(
        session_id.clone(),
        KeyValueStore::from_app_db(&app_db),
    ));
    let background_executor = app.background_executor();

    let (open_listener, mut open_rx) = OpenListener::new();

    let failed_single_instance_check = if *zed_env_vars::ZED_STATELESS
        || *release_channel::RELEASE_CHANNEL == ReleaseChannel::Dev
    {
        false
    } else {
        #[cfg(any(target_os = "linux", target_os = "freebsd"))]
        {
            crate::zed::listen_for_cli_connections(open_listener.clone()).is_err()
        }

        #[cfg(target_os = "windows")]
        {
            !crate::zed::windows_only_instance::handle_single_instance(open_listener.clone(), &args)
        }

        #[cfg(target_os = "macos")]
        {
            use zed::mac_only_instance::*;
            ensure_only_instance() != IsOnlyInstance::Yes
        }
    };
    if failed_single_instance_check {
        println!("zed is already running");
        return;
    }

    let should_install_crash_handler =
        client::telemetry::should_install_crash_handler(*release_channel::RELEASE_CHANNEL);

    let crash_handler = if should_install_crash_handler {
        Some(
            app.background_executor().spawn(crashes::init(
                InitCrashHandler {
                    session_id,
                    // strip the build and channel information from the version string, we send them separately
                    zed_version: semver::Version::new(
                        app_version.major,
                        app_version.minor,
                        app_version.patch,
                    )
                    .to_string(),
                    binary: "zed".to_string(),
                    release_channel: release_channel::RELEASE_CHANNEL_NAME.clone(),
                    commit_sha: app_commit_sha
                        .as_ref()
                        .map(|sha| sha.full())
                        .unwrap_or_else(|| "no sha".to_owned()),
                },
                {
                    let background_executor1 = app.background_executor();
                    move |task| {
                        background_executor1.spawn(task).detach();
                    }
                },
                |pid| paths::temp_dir().join(format!("zed-crash-handler-{pid}")),
                move |duration| background_executor.timer(duration),
            )),
        )
    } else {
        crashes::force_backtrace();
        None
    };

    let git_hosting_provider_registry = Arc::new(GitHostingProviderRegistry::new());
    let git_binary_path =
        if cfg!(target_os = "macos") && option_env!("ZED_BUNDLE").as_deref() == Some("true") {
            app.path_for_auxiliary_executable("git")
                .context("could not find git binary path")
                .log_err()
        } else {
            None
        };
    if let Some(git_binary_path) = &git_binary_path {
        log::info!("Using git binary path: {:?}", git_binary_path);
    }

    let fs = Arc::new(RealFs::new(git_binary_path, app.background_executor()));
    let (user_keymap_file_rx, user_keymap_watcher) = watch_config_file(
        &app.background_executor(),
        fs.clone(),
        paths::keymap_file().clone(),
    );

    let (shell_env_loaded_tx, shell_env_loaded_rx) = oneshot::channel();
    if !stdout_is_a_pty() {
        app.background_executor()
            .spawn(async {
                #[cfg(unix)]
                util::load_login_shell_environment().await.log_err();
                shell_env_loaded_tx.send(()).ok();
            })
            .detach();
    } else {
        drop(shell_env_loaded_tx)
    }

    app.on_open_urls({
        let open_listener = open_listener.clone();
        move |urls| {
            open_listener.open(RawOpenRequest {
                urls,
                diff_paths: Vec::new(),
                ..Default::default()
            })
        }
    });
    app.on_reopen(move |cx| {
        if let Some(app_state) = AppState::try_global(cx) {
            cx.spawn({
                async move |cx| {
                    if let Err(e) = restore_or_create_workspace(app_state, cx).await {
                        fail_to_open_window_async(e, cx)
                    }
                }
            })
            .detach();
        }
    });

    app.run(move |cx| {
        cx.set_global(app_db);
        let db_trusted_paths = match workspace::WorkspaceDb::global(cx).fetch_trusted_worktrees() {
            Ok(trusted_paths) => trusted_paths,
            Err(e) => {
                log::error!("Failed to do initial trusted worktrees fetch: {e:#}");
                HashMap::default()
            }
        };
        trusted_worktrees::init(db_trusted_paths, cx);
        menu::init();
        zed_actions::init();

        release_channel::init(app_version, cx);

        // Build the kask tokio runtime and register it as the GPUI-global tokio
        // runtime via gpui_tokio. This eliminates the split-brain between a
        // separate "kask_tokio_runtime" and gpui_tokio's own runtime — all
        // kask async code (CyberneticsLoop, MetacognitionLoop, MCP server
        // launches, embedding HTTP calls, inference IPC, skill manifest
        // execution) now routes through Tokio::spawn(cx, ...) /
        // Tokio::handle(cx), the same pattern zed's own code uses
        // (livekit_client, extension_host).
        //
        // The runtime is multi-threaded with more workers than gpui_tokio's
        // default 2, because kask drives MCP server I/O, embedding HTTP calls,
        // and regulation loops concurrently.
        let kask_tokio_runtime = tokio::runtime::Builder::new_multi_thread()
            .thread_name("kask-tokio")
            .worker_threads(4)
            .enable_all()
            .build()
            .expect("failed to build kask tokio runtime — cannot start regulation loops");
        let kask_runtime_handle = kask_tokio_runtime.handle().clone();
        // The runtime is owned by GlobalTokio (registered below) for the
        // lifetime of the app. Dropping GlobalTokio on app shutdown will
        // shutdown_background the runtime.
        gpui_tokio::init_from_handle(cx, kask_runtime_handle.clone());
        std::mem::forget(kask_tokio_runtime);

        // D1 composition root: wire the hKask manifest executor into the SkillTool.
        // After this call, skill activations run the hKask cascade (KnowAct/FlowDef/
        // RenderAct + PDCA + gas/rjoule + OCAP) instead of injecting the SKILL.md body.
        // The SKILL.md files in .agents/skills/ remain the discovery-only catalog entries.
        // The manifest YAMLs in kask/registry/manifests/ drive the cascade.
        //
        // This uses a OnceLock global hook so the agent crate doesn't depend on kask_bridge.
        //
        // The built-in kask MCP servers come from the canonical registry in
        // `kask_bridge::BUILT_IN_MCP_SERVERS` (single source of truth).

        // D3: Construct the McpRuntime (manages MCP server child processes).
        // The McpRuntime implements ToolPort — OCAP-gated tool invocation
        // with gas/rjoule tracking and reg.tool.* span emission.
        // MCP servers are started as child processes (stdio transport).
        //
        // Server auto-launch happens after settings::init() (below) so we
        // can read KaskSettings to determine which servers to load.
        //
        // The regulation system (CyberneticsLoop, RegulationLedger, gas budgets,
        // OCAP verification) is wired here so all tool invocations are
        // governed. The CyberneticsLoop runs sense→compare→compute→act
        // cycles on background tasks; the RegulationLedger tracks variety
        // and algedonic alerts; the FlatEnergyEstimator provides conservative
        // per-call gas costs; the LedgerSink forwards Regulation spans from
        // the MCP runtime and the CyberneticsLoop into the ledger's
        // subscriber bus so registered `LedgerObserver`s (e.g. the Curator
        // status surface) actually observe them. Replacing the previous
        // NoopEventSink is what closes the S3 monitoring path — without it,
        // every emitted span was silently discarded.
        let regulation_ledger = std::sync::Arc::new(tokio::sync::RwLock::new(
            hkask_regulation::RegulationLedger::default(),
        ));

        // Create the alert channel: CyberneticsLoop sends alerts →
        // MetacognitionLoop receives them. This closes the feedback loop.
        let (alert_tx, alert_rx) = tokio::sync::mpsc::unbounded_channel();

        // The event sink must be constructed before the CyberneticsLoop so
        // both the loop and the MCP runtime share one sink. `LedgerSink::new`
        // takes the tokio handle explicitly — the GPUI foreground thread is
        // NOT inside a tokio reactor context, so `Handle::current()` would
        // panic here. We pass the kask tokio handle captured above instead.
        let event_sink: std::sync::Arc<dyn hkask_types::RegulationSink> =
            std::sync::Arc::new(hkask_regulation::LedgerSink::new(
                regulation_ledger.clone(),
                kask_runtime_handle,
            ));

        // Alert email sink — outbound algedonic alert emails via MXroute.
        //
        // At startup, env vars aren't set yet (they come from kask settings,
        // loaded in the deferred task below), so `try_from_env()` returns
        // `None` and the sink stays unwired. The deferred task re-wires it
        // from `KaskSettings` after the user resolves.
        //
        // When email is never configured, the sink stays `None` for the
        // entire session — the cybernetics loop silently skips the email
        // path. This is the zero-config default: no error, no warning. The
        // "CRITICAL: Algedonic alert LOST" path in `cybernetics_loop.rs`
        // only fires when ALL alert paths (live channel, archive, email) are
        // unavailable, which is a genuine operator-visible error.
        let alert_email_sink: Option<std::sync::Arc<dyn hkask_regulation::AlertEmailSink>> =
            hkask_email::CuratorAlertEmailSink::try_from_env();

        let cybernetics_loop_inner =
            hkask_regulation::CyberneticsLoop::new(regulation_ledger.clone())
                .with_alerts_channel(alert_tx)
                .with_event_sink(event_sink.clone());
        let cybernetics_loop_inner = if let Some(sink) = alert_email_sink {
            cybernetics_loop_inner.with_alert_email_sink(sink)
        } else {
            cybernetics_loop_inner
        };
        let cybernetics_loop = std::sync::Arc::new(tokio::sync::RwLock::new(
            cybernetics_loop_inner,
        ));
        let cybernetics_loop_for_tick = cybernetics_loop.clone();
        let cybernetics_loop_for_panel = cybernetics_loop.clone();
        let energy_estimator: std::sync::Arc<dyn hkask_regulation::EnergyEstimator> =
            std::sync::Arc::new(hkask_mcp::FlatEnergyEstimator::new());
        let mcp_runtime = std::sync::Arc::new(
            hkask_mcp::McpRuntime::new()
                .with_governance(cybernetics_loop, event_sink, energy_estimator),
        );
        log::info!("hKask regulation system wired — tool invocations are governed, regulation spans forwarded to ledger subscribers");

        // Run the CyberneticsLoop's tick cycle and the MetacognitionLoop on
        // the GPUI-global tokio runtime (registered above via
        // gpui_tokio::init_from_handle). All kask async code that uses tokio
        // APIs (timers, I/O, locks, process spawns) must route through
        // Tokio::spawn(cx, ...) — GPUI's background_spawn uses its own
        // thread-pool executor, not a tokio reactor, so spawning tokio code
        // there panics with "there is no reactor running".
        //
        // CyberneticsLoop tick cycle (10s interval).
        // Without this, the RegulationLedger stays empty — no variety
        // counters, no regulation health, no algedonic alerts. The
        // metacognition loop would be sensing a dead system.
        gpui_tokio::Tokio::spawn(cx, async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
            interval.tick().await; // skip the first immediate tick
            loop {
                interval.tick().await;
                let loop_guard = cybernetics_loop_for_tick.read().await;
                use hkask_regulation::RegulationLoop;
                loop_guard.tick().await;
            }
        })
        .detach();
        log::info!("CyberneticsLoop tick cycle started (10s interval)");

        // Curator metacognition loop — runs sense→compare→compute→act cycles.
        // Reads from RegulationLedger (populated by the CyberneticsLoop tick
        // above) and receives alerts from the CyberneticsLoop via the alert
        // channel.
        //
        // This is a self-contained implementation in hkask-regulation that
        // doesn't need hkask-pods. It reads directly from RegulationLedger.
        //
        // A clone of the ledger is hoisted for the kask panel's regulation
        // status bar (wired later in the deferred task's model-dependent
        // wiring block).
        let panel_regulation_ledger = regulation_ledger.clone();
        // The alert sink forwards critical alerts to a GPUI foreground task
        // that dispatches them as toasts, so the user is notified even when
        // the Kask panel is closed. The channel bridges the background tokio
        // task (metacognition loop) to the single-threaded GPUI foreground —
        // `AsyncApp` is not `Send`, so the sink holds only the `Sender`.
        let (alert_sink_tx, alert_sink_rx) =
            tokio::sync::mpsc::unbounded_channel::<hkask_regulation::AlertEvent>();
        spawn_alert_toast_drainer(alert_sink_rx, cx);
        let alert_sink = std::sync::Arc::new(ToastAlertSink::new(alert_sink_tx));
        let metacognition_loop = std::sync::Arc::new(
            hkask_regulation::MetacognitionLoop::new(regulation_ledger)
                .with_alert_receiver(alert_rx)
                .with_alert_sink(alert_sink),
        );
        let metacog_loop = metacognition_loop.clone();
        gpui_tokio::Tokio::spawn(cx, async move {
            metacog_loop.run().await;
        })
        .detach();
        log::info!("Curator metacognition loop started (30s tick interval)");

        // Wire the metacognition provider so the CuratorStatusTool can read
        // health snapshots from the agent's tool surface.
        let provider = std::sync::Arc::new(
            kask_bridge::BridgeMetacognitionProvider::new(metacognition_loop),
        );
        agent::set_metacognition_provider(Some(provider));
        log::info!("Curator metacognition provider wired to CuratorStatusTool");
        let mcp_runtime_for_startup = mcp_runtime.clone();
        let tool_port = std::sync::Arc::new(kask_bridge::BridgeToolPort::new(
            mcp_runtime,
        ));
        // D5: a2a_secret for OCAP delegation token minting.
        // Resolved via the `keyring` crate (synchronous OS keychain I/O).
        //
        // Hoisted out of the deferred task's model-dependent wiring block so
        // it's in scope for both the early logging-memory wiring and the
        // deferred model-dependent wiring.
        //
        // When the secret cannot be resolved (no env var, no keychain entry),
        // `unwrap_or_default()` produces an empty Vec. Downstream OCAP token
        // minting then falls back to a zeroed Ed25519 key (warned per-invocation
        // in `PanelToolInvoker::invoke_tool`), which means every tool
        // invocation's signature is publicly predictable. Surface this at
        // startup so the operator can set `HKASK_A2A_SECRET` before relying on
        // governed tool invocation — the per-invocation warning is too late.
        let a2a_secret = hkask_keystore::keychain::resolve_a2a_secret()
            .map(|s| s.to_vec())
            .unwrap_or_else(|e| {
                log::warn!(
                    "hKask a2a_secret not resolved ({}). OCAP delegation tokens will be \
                     signed with a zeroed key — set HKASK_A2A_SECRET or store the secret \
                     in the OS keychain before relying on governed tool invocation.",
                    e
                );
                Vec::new()
            });

        {
            // D6 (early): Install a logging memory port now so the global hook
            // is set before any thread completes a turn. The port starts as a
            // no-op (BridgeMemoryPort wrapping LoggingMemoryPort) and is replaced
            // with a real port once the Zed user resolves (see the deferred task
            // after AppState::set_global). `set_memory_port` uses a Mutex (not
            // OnceLock), so the upgrade is a simple second call.
            //
            // This is hoisted out of the model-dependent wiring block (now in
            // the deferred task) so it is always installed, even when no default
            // LanguageModel is configured yet.
            let logging_memory: std::sync::Arc<dyn hkask_types::MemoryPort> =
                std::sync::Arc::new(kask_bridge::LoggingMemoryPort::new());
            let bridge_memory = std::sync::Arc::new(kask_bridge::BridgeMemoryPort::new(logging_memory));
            agent::set_memory_port(Some(bridge_memory));
            log::info!("hKask memory port wired (logging) — will upgrade when Zed user resolves");

            // D5: Keystore uses the `keyring` crate directly for all
            // sovereignty key reads/writes (synchronous OS keychain I/O).
            // API keys for inference providers are handled by zed's own
            // CredentialsProvider through the LanguageModelRegistry.

            // D3: McpRuntime is constructed above (before the block) so it's
            // available for both the manifest executor and the post-settings
            // auto-launch.
            //
            // The model-dependent wiring (manifest executor, guard, panel) is
            // deferred to after language_model::init() — see the block after
            // language_models::init() below. The a2a_secret and logging memory
            // port are wired here (early) because they don't depend on the
            // language model registry.
        }

        if let Some(app_commit_sha) = app_commit_sha {
            AppCommitSha::set_global(app_commit_sha, cx);
        }
        // Register the global Fs so `<dyn Fs>::global(cx)` works (used by
        // kask_bridge::ensure_openai_compatible_entries and other callers).
        // Upstream zed sets this in the same position; the kask fork was
        // missing the call, causing a panic on startup.
        <dyn fs::Fs>::set_global(fs.clone(), cx);
        settings::init(cx);
        zlog_settings::init(cx);
        zed::watch_settings_files(fs.clone(), cx);
        handle_keymap_file_changes(user_keymap_file_rx, user_keymap_watcher, cx);

        // Determine which kask MCP servers to auto-launch based on KaskSettings.
        // The actual launch is deferred until the Zed user resolves (see the
        // deferred task below) so MCP servers can route inference through zed's
        // LanguageModelRegistry via the IPC socket.
        let kask_settings_for_mcp = kask_bridge::KaskSettings::get_global(cx).clone();

        // Ensure `openai_compatible.<provider_id>` entries exist in settings.json
        // for every enabled inference provider. This makes the providers appear
        // in Settings → AI → LLM Providers and the agent model picker via the
        // existing `register_compatible_providers` machinery in `language_models`.
        // Must run before `language_models::init` so the providers are registered
        // on the first settings observation.
        kask_bridge::ensure_openai_compatible_entries(&kask_settings_for_mcp, cx);

        // Re-sync `openai_compatible` entries whenever kask settings change so
        // toggling a provider in the settings UI takes effect immediately
        // (without requiring a restart). The `language_models` crate's own
        // `SettingsStore` observer then registers/unregisters the provider.
        cx.observe_global::<SettingsStore>(move |cx| {
            let settings = kask_bridge::KaskSettings::get_global(cx).clone();
            kask_bridge::ensure_openai_compatible_entries(&settings, cx);
        })
        .detach();

        let servers_to_start: Vec<String> = if kask_settings_for_mcp.mcp.load_default {
            kask_bridge::BUILT_IN_MCP_SERVERS
                .iter()
                .filter(|s| {
                    *kask_settings_for_mcp.mcp.overrides.get(s.id).unwrap_or(&true)
                })
                .map(|s| s.id.to_string())
                .collect()
        } else {
            Vec::new()
        };

        let user_agent = format!(
            "Zed/{} ({}; {})",
            AppVersion::global(cx),
            std::env::consts::OS,
            std::env::consts::ARCH
        );
        let proxy_url = ProxySettings::get_global(cx).proxy_url();
        let http = {
            let _guard = Tokio::handle(cx).enter();

            ReqwestClient::proxy_and_user_agent(proxy_url, &user_agent)
                .expect("could not start HTTP client")
        };
        cx.set_http_client(Arc::new(http));

        <dyn Fs>::set_global(fs.clone(), cx);

        GitHostingProviderRegistry::set_global(git_hosting_provider_registry, cx);
        git_hosting_providers::init(cx);

        OpenListener::set_global(cx, open_listener.clone());

        extension::init(cx);
        let extension_host_proxy = ExtensionHostProxy::global(cx);

        let client = Client::production(cx);
        cx.set_http_client(client.http_client());
        let mut languages = LanguageRegistry::new(cx.background_executor().clone());
        languages.set_language_server_download_dir(paths::languages_dir().clone());
        let languages = Arc::new(languages);
        let (mut tx, rx) = watch::channel(None);
        cx.observe_global::<SettingsStore>(move |cx| {
            let settings = &ProjectSettings::get_global(cx).node;
            let options = NodeBinaryOptions {
                allow_path_lookup: !settings.ignore_system_version,
                // TODO: Expose this setting
                allow_binary_download: true,
                use_paths: settings.path.as_ref().map(|node_path| {
                    let node_path = PathBuf::from(shellexpand::tilde(node_path).as_ref());
                    let npm_path = settings
                        .npm_path
                        .as_ref()
                        .map(|path| PathBuf::from(shellexpand::tilde(&path).as_ref()));
                    (
                        node_path.clone(),
                        npm_path.unwrap_or_else(|| {
                            let base_path = PathBuf::new();
                            node_path.parent().unwrap_or(&base_path).join("npm")
                        }),
                    )
                }),
            };
            tx.send(Some(options)).log_err();
        })
        .detach();
        ui::on_new_scrollbars::<SettingsStore>(cx);

        let node_runtime = NodeRuntime::new(client.http_client(), Some(shell_env_loaded_rx), rx);

        debug_adapter_extension::init(extension_host_proxy.clone(), cx);
        languages::init(languages.clone(), fs.clone(), node_runtime.clone(), cx);
        let user_store = cx.new(|cx| UserStore::new(client.clone(), cx));
        let workspace_store = cx.new(|cx| WorkspaceStore::new(client.clone(), cx));

        language_extension::init(
            language_extension::LspAccess::ViaWorkspaces({
                let workspace_store = workspace_store.clone();
                Arc::new(move |cx: &mut App| {
                    workspace_store.update(cx, |workspace_store, cx| {
                        Ok(workspace_store
                            .workspaces()
                            .filter_map(|weak| weak.upgrade())
                            .map(|workspace: gpui::Entity<workspace::Workspace>| {
                                workspace.read(cx).project().read(cx).lsp_store()
                            })
                            .collect())
                    })
                })
            }),
            extension_host_proxy.clone(),
            languages.clone(),
        );

        Client::set_global(client.clone(), cx);

        zed::init(cx);
        #[cfg(target_os = "macos")]
        zed::move_to_applications::init(cx);
        project::Project::init(&client, cx);

        // Register the built-in kask MCP servers as zed context servers.
        // This makes kask MCP tools appear in the agent tool picker and
        // available to zed's agent thread. The servers are launched as stdio
        // child processes by zed's ContextServerStore, using the binary names
        // from kask_bridge::BUILT_IN_MCP_SERVERS. Configuration (which servers
        // to load) is managed from kask settings (kask.mcp.load_default + overrides).
        //
        // The registration is reactive: a SettingsStore observer re-syncs the
        // ContextServerDescriptorRegistry whenever kask settings change.
        sync_kask_mcp_servers(cx);
        cx.observe_global::<SettingsStore>(sync_kask_mcp_servers).detach();

        debugger_ui::init(cx);
        debugger_tools::init(cx);
        client::init(&client, cx);
        feature_flags::FeatureFlagStore::init(cx);

        let system_id = cx.foreground_executor().block_on(system_id).ok();
        let installation_id = cx.foreground_executor().block_on(installation_id).ok();
        let session = cx.foreground_executor().block_on(session);

        let telemetry = client.telemetry();
        telemetry.start(
            system_id.as_ref().map(|id| id.to_string()),
            installation_id.as_ref().map(|id| id.to_string()),
            session.id().to_owned(),
            cx,
        );
        cx.subscribe(&user_store, {
            let telemetry = telemetry.clone();
            move |_, evt: &client::user::Event, cx| match evt {
                client::user::Event::PrivateUserInfoUpdated => {
                    if let Some(crash_client) = cx.try_global::<CrashHandler>() {
                        crashes::set_user_info(
                            &crash_client.0,
                            crashes::UserInfo {
                                metrics_id: telemetry.metrics_id().map(|s| s.to_string()),
                                is_staff: telemetry.is_staff(),
                            },
                        );
                    }
                }
                _ => {}
            }
        })
        .detach();

        let is_new_install = matches!(&installation_id, Some(IdType::New(_)));

        // We should rename these in the future to `first app open`, `first app open for release channel`, and `app open`
        if let (Some(system_id), Some(installation_id)) = (&system_id, &installation_id) {
            match (&system_id, &installation_id) {
                (IdType::New(_), IdType::New(_)) => {
                    telemetry::event!("App First Opened");
                    telemetry::event!("App First Opened For Release Channel");
                }
                (IdType::Existing(_), IdType::New(_)) => {
                    telemetry::event!("App First Opened For Release Channel");
                }
                (_, IdType::Existing(_)) => {
                    telemetry::event!("App Opened");
                }
            }
        }
        let app_session = cx.new(|cx| AppSession::new(session, cx));

        let app_state = Arc::new(AppState {
            languages,
            client: client.clone(),
            user_store,
            fs: fs.clone(),
            build_window_options,
            workspace_store,
            node_runtime,
            session: app_session,
        });
        AppState::set_global(app_state.clone(), cx);

        // D6/D11/MCP-launch (deferred): Wait for the Zed user to resolve, then
        // replace the logging memory port with a real one, wire the context
        // injector, and launch MCP servers.
        //
        // The agent name is derived from User::username (the GitHub-style
        // login from the Zed account) via sanitize_name().
        //
        // If the user is already logged in (session restored), the upgrade
        // happens immediately on the first watch tick. If not, the task waits
        // until `authenticate()` (spawned below) completes.
        {
            let user_store = app_state.user_store.clone();
            let mcp_runtime_for_deferred = mcp_runtime_for_startup;
            let servers_to_start_clone = servers_to_start;
            // Captures for the model-dependent wiring block (moved here from
            // the synchronous startup so it runs after the user resolves and
            // the LanguageModelRegistry is populated). See the
            // "Process-global hooks set at runtime need a startup-failure
            // signal" trap in .rules — these OnceLock-based hooks must be
            // wired from the deferred task, not from startup.
            let tool_port_for_deferred = tool_port;
            let a2a_secret_for_deferred = a2a_secret;
            let cybernetics_loop_for_panel_deferred = cybernetics_loop_for_panel;
            let _panel_regulation_ledger_deferred = panel_regulation_ledger;
            let app_state_for_deferred = app_state.clone();
            cx.spawn(async move |cx| {
                let mut current_user = user_store.read_with(cx, |store, _| store.watch_current_user());

                // Wait for the user to resolve.
                while current_user.borrow().is_none() {
                    // postage::watch::Receiver implements Stream — `.next()` yields
                    // the latest value when it changes.
                    if current_user.next().await.is_none() {
                        // Stream ended (store dropped).
                        break;
                    }
                }

                let Some(user) = current_user.borrow().clone() else {
                    log::warn!("Zed user stream ended without resolving — kask memory stays in logging mode");
                    return;
                };

                let username = user.username.to_string();
                let Some(agent_name) = kask_bridge::agent_name_from_username(&username) else {
                    log::warn!("Zed username '{username}' sanitized to empty — kask memory stays in logging mode");
                    return;
                };

                log::info!("Zed user resolved — kask agent name: {agent_name}");

                // D6: Provision the agent's storage and replace the logging memory port
                // with a real one. `provision_agent` handles first-run setup
                // as lookups and directory creation — no interactive onboarding.
                //
                // The keystore uses the `keyring` crate directly
                // (synchronous OS keychain I/O).
                let kask_settings = cx.update(|cx| kask_bridge::KaskSettings::get_global(cx).clone());
                let embedding_model = std::env::var("HKASK_EMBEDDING_MODEL")
                    .unwrap_or_else(|_| kask_settings.corpus.embedding_model.clone());
                let embedding_dim = kask_settings.corpus.embedding_dim as usize;
                let username_for_provision = username.clone();

                let provision_result = cx.background_spawn(async move {
                    kask_bridge::provision_agent(&username_for_provision)
                }).await;

                // Hoisted to the outer scope so the IPC server (started later
                // in the `cx.update` block) can access it. Set inside the
                // `match provision_result` below.
                let mut embedding_port_for_ipc: Option<kask_bridge::LanguageModelEmbeddingPort> = None;

                match provision_result {
                    Ok(provisioned) => {
                        let kask_bridge::ProvisionedAgent { db_path, passphrase, webid: user_webid } = provisioned;

                        // Construct the embedding port. This resolves a zed
                        // LanguageModel from the embedding model's provider
                        // (e.g. `DeepInfra/...` → the `deepinfra` provider)
                        // and uses its `api_url()` + `api_key()` to make raw
                        // `/embeddings` calls through zed's HTTP client —
                        // replacing hKask's standalone `EmbeddingRouter` which
                        // resolved credentials from env vars and bypassed
                        // zed's `LanguageModelRegistry` + keychain.
                        //
                        // The port is cloned: one copy goes to
                        // `RealMemoryPort` (in-process embedding recall),
                        // the other goes to `InferenceIpcServer` (so MCP
                        // server child processes can route embeddings through
                        // zed too, via the IPC bridge).
                        //
                        // Per the .rules trap "Process-global hooks set at
                        // runtime need a startup-failure signal": if the
                        // provider isn't registered or has no model, we warn
                        // loudly and skip the real memory port (logging mode
                        // stays active) rather than silently degrading.
                        let embedding_port_result = cx.update(|cx| {
                            resolve_embedding_port(
                                &embedding_model,
                                &app_state_for_deferred,
                                cx,
                            )
                        });

                        let Some((embedding_port, embedding_task)) = embedding_port_result else {
                            // `resolve_embedding_port` already logged a warn
                            // with remediation guidance. Skip the real memory
                            // port — logging mode stays active.
                            return;
                        };
                        embedding_task.detach();

                        // Clone for the IPC server (the other copy goes to
                        // RealMemoryPort below).
                        embedding_port_for_ipc = Some(embedding_port.clone());

                        match kask_bridge::RealMemoryPort::new(
                            &db_path,
                            &passphrase,
                            user_webid,
                            embedding_model,
                            embedding_dim,
                            embedding_port,
                            kask_settings.memory.consolidation_cadence_secs,
                            kask_settings.memory.confidence_floor,
                            gpui_tokio::Tokio::handle_async(&*cx),
                        ) {
                            Ok(real) => {
                                // Start the background consolidation timer before
                                // moving the port into the Arc<dyn MemoryPort>.
                                // The timer runs on the tokio runtime and fires
                                // consolidation on the configured cadence,
                                // decoupled from the ingestion path.
                                //
                                // The returned JoinHandle is dropped — in tokio,
                                // dropping a JoinHandle detaches the task (it
                                // continues running). We don't need to await it.
                                if real.start_consolidation_timer().is_some() {
                                    log::info!(
                                        "hKask consolidation timer started \
                                         (cadence: {}s)",
                                        kask_settings.memory.consolidation_cadence_secs
                                    );
                                }
                                // Keep a typed handle for the curator context
                                // injector, which calls `recall_context_curator`
                                // (an inherent method on `RealMemoryPort`, not on
                                // the `MemoryPort` trait). The trait-object coercion
                                // below would lose the concrete type.
                                let real_memory_typed: std::sync::Arc<kask_bridge::RealMemoryPort> =
                                    std::sync::Arc::new(real);
                                let real_memory: std::sync::Arc<dyn hkask_types::MemoryPort> =
                                    real_memory_typed.clone();
                                let bridge = std::sync::Arc::new(
                                    kask_bridge::BridgeMemoryPort::new(real_memory.clone()),
                                );
                                agent::set_memory_port(Some(bridge));
                                log::info!(
                                    "hKask memory port upgraded to RealMemoryPort \
                                     (agent: {agent_name}, db: {db_path})"
                                );

                                // Set env vars for the curator MCP server so it
                                // reads from the same `agents/curator/pod.db` the
                                // agent writes curator copies to. These are read
                                // by `open_curator_stores` in the curator MCP
                                // server and by `open_curator_semantic` in
                                // `RealMemoryPort::new`.
                                //
                                // `HKASK_CURATOR_DB` — the curator's sovereign DB
                                // path. `HKASK_CURATOR_WEBID` — the curator's
                                // WebID, stashed in a non-global env var that
                                // `mcp_env()` maps to `HKASK_WEBID` only for the
                                // curator server (via the config_env allowlist).
                                // We do NOT set `HKASK_WEBID` here — it's
                                // process-global and would override the identity
                                // of all other MCP servers (codegraph, condenser,
                                // etc.), which resolve their identity from it in
                                // `transport.rs`.
                                let curator_db = hkask_types::agent_paths::resolve_under_data_dir(
                                    &hkask_types::agent_paths::agent_pod_db("curator"),
                                );
                                let curator_webid = hkask_types::WebID::from_persona(b"curator");
                                // SAFETY: Set during the deferred task (post-login,
                                // before MCP servers read these). The curator MCP
                                // server reads `HKASK_CURATOR_DB` at process start;
                                // `HKASK_CURATOR_WEBID` is consumed by `mcp_env()`.
                                // Neither var is read by other MCP servers.
                                unsafe {
                                    std::env::set_var(
                                        "HKASK_CURATOR_DB",
                                        curator_db.to_string_lossy().as_ref(),
                                    );
                                    std::env::set_var(
                                        "HKASK_CURATOR_WEBID",
                                        curator_webid.to_string().as_str(),
                                    );
                                }
                                log::info!(
                                    "Curator env injected — DB: {}, WebID: {}",
                                    curator_db.display(),
                                    curator_webid.redacted_display(),
                                );
                                // Re-sync MCP servers so the curator server picks
                                // up the new env vars. The ContextServerStore
                                // re-evaluates descriptors on notify.
                                cx.update(|cx| sync_kask_mcp_servers(cx));

                                // D11: Wire the context injector now that the real memory port exists.
                                // The injector shares the same memory port as the ingestion path.
                                //
                                // Note: set_context_injector uses OnceLock, so this is a one-shot.
                                // If the user logs out and back in as a different user, the
                                // injector is not re-wired.
                                if kask_settings.memory.auto_inject {
                                    let injector = std::sync::Arc::new(
                                        kask_bridge::BridgeContextInjector::new(
                                            real_memory,
                                            kask_settings.memory.recall_limit,
                                            kask_settings.memory.recall_min_confidence,
                                        ),
                                    );
                                    agent::set_context_injector(Some(injector));
                                    log::info!("hKask context injector wired (agent: {agent_name})");

                                    // D11 curator mirror: wire the curator context
                                    // injector so the Curator recalls its own
                                    // sovereign memory (episodic + semantic from
                                    // `agents/curator/pod.db`). Without this, the
                                    // Curator has no automatic recall — it must
                                    // call `curator_memory_recall` /
                                    // `curator_semantic_search` as tools, which is
                                    // the asymmetry this block fixes.
                                    let curator_injector = std::sync::Arc::new(
                                        kask_bridge::BridgeCuratorContextInjector::new(
                                            real_memory_typed.clone(),
                                            kask_settings.memory.recall_limit,
                                            kask_settings.memory.recall_min_confidence,
                                        ),
                                    );
                                    agent::set_curator_context_injector(Some(curator_injector));
                                    log::info!(
                                        "hKask curator context injector wired \
                                         (agent: {agent_name}) — curator will \
                                         recall from its own sovereign DB"
                                    );
                                } else {
                                    // auto_inject is off — log that the curator
                                    // injector is also unwired, per the .rules trap
                                    // "Process-global hooks set at runtime need a
                                    // startup-failure signal". The warn names both
                                    // hooks left unwired so the operator can
                                    // remediate correctly.
                                    log::warn!(
                                        "kask.memory.auto_inject is false — \
                                         both the user context injector and the \
                                         curator context injector are unwired. \
                                         Set kask.memory.auto_inject true to enable \
                                         memory recall for both agents."
                                    );
                                }

                                // Wire the lazy tool router. The router narrows
                                // the tool set on complex or tool-directed requests,
                                // reducing the tool list the model must reason about
                                // when hKask's MCP servers expose many tools.
                                agent::set_tool_router(Some(std::sync::Arc::new(
                                    agent::tool_router::LazyToolRouter::new(),
                                )));
                                log::info!("hKask lazy tool router wired");
                            }
                            Err(e) => {
                                log::warn!(
                                    "Failed to open memory DB at {db_path} for {agent_name}: {e} \
                                     — staying in logging mode"
                                );
                            }
                        }
                    }
                    Err(e) => {
                        log::warn!(
                            "Failed to provision agent storage for {agent_name}: {e} \
                             — staying in logging mode"
                        );
                    }
                }

                // D1/D3/D4/D10/D12: Model-dependent kask wiring.
                //
                // This block was originally in the synchronous startup, but
                // moved here because LanguageModelRegistry::default_model()
                // returns None until the user authenticates. Running it at
                // startup left all OnceLock-based hooks (manifest executor,
                // panel tool invoker, scoped inference, regulation status,
                // thread condenser) unwired when no model was configured at
                // startup — the "Process-global hooks set at runtime need a
                // startup-failure signal" trap from .rules.
                //
                // The fusion discovery is async (OpenRouter HTTP call with a
                // 5s timeout) and must run outside cx.update (which holds the
                // app borrow and can't pump the foreground executor). The
                // rest is synchronous GPUI mutation and runs inside cx.update.
                let kask_settings = cx.update(|cx| kask_bridge::KaskSettings::get_global(cx).clone());
                let fusion_config = kask_settings.fusion.to_fusion_config();
                let async_cx_for_fusion = cx.clone();
                // Capture whether fusion is configured before `fusion_config` is
                // consumed by the `if let Some(mut fc)` block below. Used later
                // to decide whether to write auto-favorites even when the fusion
                // model itself fails to construct.
                let fusion_configured = fusion_config.is_some();

                // Lazily wire the alert email sink now that kask settings have
                // loaded. The non-secret email fields are set as process env
                // vars so `send_email()` (called from `tokio::spawn` inside
                // `CuratorAlertEmailSink::send_alert_email`) can read them.
                // The SMTP password is read from the keychain by
                // `mcp_env_with_credentials` for MCP server child processes;
                // for the main-process alert sink we set `HKASK_SMTP_PASSWORD`
                // from the keychain here too.
                //
                // When email is not configured (no `smtp_username`), the sink
                // is `None` — the cybernetics loop silently skips the email
                // path. This is the zero-config default: no error, no warning.
                if !kask_settings.curator.email.smtp_username.is_empty() {
                    // Set non-secret env vars for the main process.
                    // `set_var` is unsafe in Rust 2024 (process-global mutation).
                    unsafe {
                        std::env::set_var(
                            "HKASK_MXROUTE_SERVER",
                            &kask_settings.curator.email.mxroute_server,
                        );
                        std::env::set_var(
                            "HKASK_SMTP_USERNAME",
                            &kask_settings.curator.email.smtp_username,
                        );
                        if !kask_settings.curator.email.curator_email.is_empty() {
                            std::env::set_var(
                                "HKASK_CURATOR_EMAIL",
                                &kask_settings.curator.email.curator_email,
                            );
                        }
                        if !kask_settings.curator.email.alert_email.is_empty() {
                            std::env::set_var(
                                "HKASK_ALERT_EMAIL",
                                &kask_settings.curator.email.alert_email,
                            );
                        }
                        if !kask_settings.curator.email.authorized_emails.is_empty() {
                            std::env::set_var(
                                "HKASK_AUTHORIZED_EMAILS",
                                kask_settings.curator.email.authorized_emails.join(","),
                            );
                        }
                    }

                    // Read the SMTP password from the keychain and set it as
                    // a process env var so `send_email()` can use it.
                    let smtp_password_url = format!(
                        "{}/hkask_smtp_password",
                        kask_bridge::KASK_CREDENTIAL_NAMESPACE
                    );
                    let credentials_provider =
                        cx.update(|cx| zed_credentials_provider::global(cx));
                    let password_result = credentials_provider
                        .read_credentials(&smtp_password_url, cx)
                        .await;
                    match password_result {
                        Ok(Some((_user, password_bytes))) => {
                            if let Ok(password_str) = std::str::from_utf8(&password_bytes) {
                                unsafe {
                                    std::env::set_var("HKASK_SMTP_PASSWORD", password_str);
                                }
                                log::info!("hKask SMTP password loaded from keychain");
                            } else {
                                log::warn!(
                                    "hKask SMTP password in keychain is not valid UTF-8 — \
                                     alert emails will fail to send"
                                );
                            }
                        }
                        Ok(None) => {
                            log::warn!(
                                "hKask SMTP password not found in keychain — alert emails \
                                 will fail to send until the password is configured in \
                                 Settings → Kask → Curator Email (smtp_username is set, \
                                 indicating email was intended to be configured)"
                            );
                        }
                        Err(e) => {
                            log::warn!(
                                "hKask SMTP password keychain read failed: {e} — alert \
                                 emails will fail to send until the password is configured"
                            );
                        }
                    }

                    // Now wire the sink. The sink is wired even if the
                    // password wasn't found — `send_alert_email` spawns the
                    // send in a background task and logs a warning on failure,
                    // so a missing password degrades gracefully (no panic, no
                    // error propagation to the cybernetics loop).
                    let sink = hkask_email::CuratorAlertEmailSink::try_from_settings(
                        &kask_settings.curator.email.smtp_username,
                        &kask_settings.curator.email.alert_email,
                    );
                    let cybernetics_loop_for_email = cybernetics_loop_for_panel_deferred.clone();
                    gpui_tokio::Tokio::spawn(cx, async move {
                        let mut loop_guard = cybernetics_loop_for_email.write().await;
                        loop_guard.set_alert_email_sink(sink);
                    })
                    .detach();
                    log::info!("hKask alert email sink wired from kask settings");
                } else {
                    log::info!(
                        "hKask alert email not configured — algedonic alerts rely \
                         on the live channel and archive (zero-config default)"
                    );
                }

                // Fusion auto-discovery (async, outside cx.update).
                let mut discovered_favorites: Vec<kask_bridge::FavoriteModel> = Vec::new();
                let fusion_model: Option<Arc<dyn language_model::LanguageModel>> =
                    if let Some(mut fc) = fusion_config {
                        if kask_bridge::should_auto_discover(&kask_settings.fusion.panel_models) {
                            log::info!(
                                "hKask fusion: auto-discovering panel models from OpenRouter \
                                 (max_price=${}/M, min_ia={})",
                                kask_settings.fusion.openrouter_max_price,
                                kask_settings.fusion.openrouter_min_intelligence
                            );
                            let or_api_key = std::env::var("OPENROUTER_API_KEY").unwrap_or_default();
                            let max_price = kask_settings.fusion.openrouter_max_price;
                            let min_ia = kask_settings.fusion.openrouter_min_intelligence;
                            let discovery_task = {
                                let _tokio_guard = gpui_tokio::Tokio::handle_async(&*cx).enter();
                                cx.background_spawn(async move {
                                    kask_bridge::discover_favorites(&or_api_key, max_price, min_ia).await
                                })
                            };
                            let timeout = cx.background_executor().timer(std::time::Duration::from_secs(5));
                            let result = futures::select_biased! {
                                favs = discovery_task.fuse() => favs,
                                _ = timeout.fuse() => {
                                    log::warn!(
                                        "hKask fusion: OpenRouter discovery timed out after 5s — \
                                         falling back to kask_default panel"
                                    );
                                    Vec::new()
                                }
                            };
                            if !result.is_empty() {
                                let panel_names: Vec<String> = result.iter()
                                    .map(|f| f.prefixed_id.clone())
                                    .collect();
                                log::info!(
                                    "hKask fusion: discovered {} favorites — {:?}",
                                    panel_names.len(),
                                    panel_names
                                );
                                if let Some(panel) = hkask_types::fusion::NonEmptyVec::from_vec(panel_names) {
                                    fc.panel = panel;
                                }
                                discovered_favorites = result;
                            } else {
                                log::info!(
                                    "hKask fusion: no favorites discovered — using kask_default panel"
                                );
                            }
                        }

                        // Resolve fusion panel/judge models and construct the
                        // FusionLanguageModel. This needs the model registry,
                        // so it runs inside cx.update.
                        let mut names = fc.panel.iter().cloned().collect::<Vec<_>>();
                        if fc.judge.to_lowercase() != "algo" {
                            names.push(fc.judge.clone());
                        }
                        let async_cx_for_closure = async_cx_for_fusion.clone();
                        cx.update(|cx| {
                            let model_registry = language_model::LanguageModelRegistry::read_global(cx);
                            let resolved = kask_bridge::resolve_fusion_models(
                                model_registry,
                                &names,
                                cx,
                            );
                            match kask_bridge::FusionLanguageModel::new(
                                fc.clone(),
                                resolved,
                                async_cx_for_closure.clone(),
                            ) {
                                Some(m) => {
                                    log::info!(
                                        "hKask fusion enabled — mode: {}, judge: {}, panel: {:?}",
                                        fc.mode.as_str(),
                                        fc.judge,
                                        fc.panel.iter().collect::<Vec<_>>()
                                    );
                                    Some(Arc::new(m) as Arc<dyn language_model::LanguageModel>)
                                }
                                None => {
                                    log::warn!(
                                        "hKask fusion config present but construction failed — falling back to single model"
                                    );
                                    None
                                }
                            }
                        })
                    } else {
                        log::info!("hKask fusion disabled (kask.fusion.enabled = false)");
                        None
                    };

                // Sync model-dependent wiring (inside cx.update).
                cx.update(|cx| {
                    let model_registry = language_model::LanguageModelRegistry::read_global(cx);
                    // ── Panel + curator wiring: unconditional ─────────────────
                    //
                    // The curator is always_on by default (KaskCuratorSettings::
                    // default().always_on == true). The panel tool invoker and
                    // regulation status don't need an inference model at all —
                    // they only need the tool_port and the cybernetics loop /
                    // ledger. The curator session factory uses the same
                    // LazyInferencePort as the manifest executor: when no model
                    // is resolved, curator turns return a clear error with
                    // remediation guidance; when the model resolves, the lazy
                    // port is swapped in and curator turns route through it.
                    let panel_tool_invoker = std::sync::Arc::new(PanelToolInvoker {
                        tool_port: tool_port_for_deferred.clone(),
                        a2a_secret: a2a_secret_for_deferred.clone(),
                        executor: cx.background_executor().clone(),
                    });
                    kask_panel::set_tool_invoker(Some(panel_tool_invoker));
                    log::info!(
                        "Kask panel tool invoker wired \
                         (curator turns now route through NativeAgent — \
                         the ConversationView handles streaming + tool dispatch)"
                    );

                    // ── Condenser wiring: unconditional ───────────────────────
                    //
                    // The condenser doesn't need a model at construction time —
                    // it uses the inference router lazily when compressing.
                    let condenser_settings = &kask_settings.condenser;
                    if condenser_settings.auto_compress_tool_results {
                        let condenser = std::sync::Arc::new(kask_bridge::BridgeThreadCondenser::new(
                            &condenser_settings.profile,
                            condenser_settings.auto_compress_tool_results,
                        ));
                        agent::set_thread_condenser(Some(condenser));
                        log::info!(
                            "hKask thread condenser wired — tool results will be compressed (profile: {})",
                            condenser_settings.profile
                        );
                    } else {
                        log::info!("hKask tool result compression disabled (kask.condenser.auto_compress_tool_results = false)");
                    }

                    if kask_settings.memory.auto_inject {
                        log::info!("hKask context injection enabled — injector will be wired after agent resolves");
                    } else {
                        log::info!("hKask context injection disabled (kask.memory.auto_inject = false)");
                    }

                    if let Some(configured) = model_registry.default_model() {
                        let async_cx = cx.to_async();
                        // Manifest registry paths. These are *fallbacks* — the
                        // authoritative manifests are embedded at build time
                        // via `hkask-templates/build.rs` and consulted via
                        // `process_manifest_yaml()`. The filesystem paths
                        // exist for dev workflows (edit a manifest, rebuild).
                        // For installed binaries, these may not resolve, and
                        // that's fine — the embedded registry handles it.
                        let registry_manifests_dir = std::path::PathBuf::from("kask/registry/manifests");
                        let registry_templates_dir = std::path::PathBuf::from("kask/registry/templates");
                        if !registry_manifests_dir.is_dir() {
                            log::debug!(
                                "hKask manifest dir '{}' not found — relying on embedded registry",
                                registry_manifests_dir.display()
                            );
                        }
                        if !registry_templates_dir.is_dir() {
                            log::debug!(
                                "hKask templates dir '{}' not found — relying on embedded templates",
                                registry_templates_dir.display()
                            );
                        }

                        let inference_model: Arc<dyn language_model::LanguageModel> =
                            fusion_model.clone().unwrap_or_else(|| {
                                let kask_default = kask_settings.models.effective_default_model();
                                if kask_default != kask_bridge::KaskModelsSettings::DEFAULT_INFERENCE_MODEL {
                                    if let Some(model) = kask_bridge::resolve_fusion_models(
                                        model_registry,
                                        &[kask_default.to_string()],
                                        cx,
                                    ).into_values().next() {
                                        log::info!(
                                            "hKask inference using kask.models.default_model: {}",
                                            kask_default
                                        );
                                        return model;
                                    }
                                    log::warn!(
                                        "kask.models.default_model '{}' could not be resolved \
                                         from LanguageModelRegistry — falling back to zed default",
                                        kask_default
                                    );
                                }
                                configured.model.clone()
                            });

                        // Register the fusion provider so it appears in the agent
                        // panel's model picker. When fusion is disabled, the provider
                        // returns no models and doesn't appear in the picker.
                        let fusion_provider =
                            kask_bridge::FusionLanguageModelProvider::new(cx);
                        language_model::LanguageModelRegistry::global(cx).update(cx, |registry, cx| {
                            registry.register_provider(Arc::new(fusion_provider), cx);
                        });
                        log::info!("Kask fusion language model provider registered");

                        // Auto-favorite: when fusion is configured (enabled in
                        // settings), add the fusion model and any discovered
                        // OpenRouter favorites to `agent.favorite_models` so
                        // they appear in the agent panel's model picker favorites
                        // cycle (CycleFavoriteModels action). This is decoupled
                        // from `fusion_model` construction success: discovered
                        // favorites should appear in the picker even if the
                        // fusion model itself failed to construct (e.g. because
                        // the OpenRouter provider hadn't fetched its model list
                        // yet at startup). The user can still cycle to the
                        // discovered models and select them manually.
                        // `add_favorite_model` is idempotent — entries already
                        // present are not duplicated. This is best-effort:
                        // settings write failures are logged but do not block
                        // startup.
                        if fusion_configured {
                            let fs = app_state_for_deferred.fs.clone();
                            let mut selections = kask_bridge::favorite_model_selections(&discovered_favorites);
                            selections.push(kask_bridge::fusion_model_selection());
                            let favorite_count = selections.len();
                            let discovered_count = discovered_favorites.len();
                            let fusion_constructed = fusion_model.is_some();
                            settings::update_settings_file(fs, cx, move |settings, _| {
                                let agent = settings.agent.get_or_insert_default();
                                for selection in &selections {
                                    agent.add_favorite_model(selection.clone());
                                }
                            });
                            log::info!(
                                "hKask fusion: auto-favorited {} model(s) (fusion + {} discovered); fusion model {}",
                                favorite_count,
                                discovered_count,
                                if fusion_constructed { "constructed" } else { "NOT constructed — favorites still written" },
                            );
                        }

                        let (inference_port, inference_task) =
                            kask_bridge::LanguageModelInferencePort::new(
                                inference_model.clone(),
                                async_cx,
                            );
                        inference_task.detach();

                        let guard_config = hkask_guard::GuardConfig::from_env();
                        let content_guard = hkask_guard::ContentGuard::mandatory(&guard_config);
                        let guarded_inference = std::sync::Arc::new(
                            hkask_guard::GuardedInferencePort::new(
                                std::sync::Arc::new(inference_port),
                                content_guard,
                            )
                        );

                        // Start the inference IPC server so MCP server child processes
                        // can route inference through zed's LanguageModelRegistry (with
                        // fusion, guard, and zed's configured API keys) instead of
                        // constructing their own InferenceRouter with separate keys.
                        //
                        // The media router is a hKask `InferenceRouter` used for
                        // media generation (image/video/speech/transcription via
                        // fal.ai/DeepInfra). These backends aren't part of zed's
                        // `LanguageModel` abstraction, so the media MCP server routes
                        // them through the IPC bridge to this router instead of
                        // constructing its own. Credentials come from env vars
                        // (FALAI_API_KEY, DEEPINFRA_API_KEY) resolved by the zed
                        // process — the same keys the media MCP server used to hold.
                        let media_router = std::sync::Arc::new(
                            kask_bridge::InferenceRouter::new(
                                kask_bridge::InferenceConfig::from_env(),
                            ),
                        );
                        match kask_bridge::InferenceIpcServer::start(
                            guarded_inference.clone(),
                            embedding_port_for_ipc.clone(),
                            Some(media_router),
                            cx,
                        ) {
                            Ok(ipc_server) => {
                                let socket_path = ipc_server.socket_path().to_string_lossy().to_string();
                                let _ = INFERENCE_SOCKET_PATH.set(socket_path.clone());
                                log::info!(
                                    "hKask inference IPC server started at {socket_path} — \
                                     MCP servers will route inference through zed"
                                );
                                // Keep the server alive for the lifetime of the process.
                                // It's stored in a detached task — the socket is cleaned
                                // up on drop, but we don't drop it until process exit.
                                std::mem::forget(ipc_server);
                                // Re-sync MCP servers so the inference socket path is
                                // included in the env passed to context server processes.
                                // The KaskMcpDescriptor::command() resolves env at call
                                // time, so this notification triggers maintain_servers
                                // to restart servers with the updated env.
                                sync_kask_mcp_servers(cx);
                            }
                            Err(e) => {
                                log::warn!(
                                    "Failed to start inference IPC server: {e} — \
                                     MCP servers will fall back to InferenceRouter with env-var keys"
                                );
                            }
                        }

                        let tool_port_as_dyn: std::sync::Arc<dyn hkask_capability::ToolPort> =
                            tool_port_for_deferred.clone();
                        let executor = std::sync::Arc::new(
                            kask_bridge::BridgeManifestExecutor::new(
                                guarded_inference,
                                tool_port_as_dyn,
                                a2a_secret_for_deferred.clone(),
                                registry_manifests_dir,
                                registry_templates_dir,
                                gpui_tokio::Tokio::handle(cx),
                            ),
                        );
                        agent::set_manifest_executor(Some(executor));
                        log::info!("hKask manifest executor wired with GuardedInferencePort — skills will run the guarded cascade");

                        if kask_settings.memory.auto_inject {
                            log::info!("hKask context injection enabled — injector will be wired after agent resolves");
                        } else {
                            log::info!("hKask context injection disabled (kask.memory.auto_inject = false)");
                        }

                        let condenser_settings = &kask_settings.condenser;
                        if condenser_settings.auto_compress_tool_results {
                            let condenser = std::sync::Arc::new(kask_bridge::BridgeThreadCondenser::new(
                                &condenser_settings.profile,
                                condenser_settings.auto_compress_tool_results,
                            ));
                            agent::set_thread_condenser(Some(condenser));
                            log::info!(
                                "hKask thread condenser wired — tool results will be compressed (profile: {})",
                                condenser_settings.profile
                            );
                        } else {
                            log::info!("hKask tool result compression disabled (kask.condenser.auto_compress_tool_results = false)");
                        }

                        let panel_tool_invoker = std::sync::Arc::new(PanelToolInvoker {
                            tool_port: tool_port_for_deferred.clone(),
                            a2a_secret: a2a_secret_for_deferred.clone(),
                            executor: cx.background_executor().clone(),
                        });
                        kask_panel::set_tool_invoker(Some(panel_tool_invoker));
                        log::info!("Kask panel tool invoker wired");
                    } else {
                        // Body injection is disabled in zed-kask: with no manifest
                        // executor wired, the `skill` tool returns the no-op envelope
                        // ("Skill manifest executor not configured..."). The log
                        // message must name ALL hooks left unwired by this branch, not
                        // just the manifest executor — operators reading the log need
                        // to know the full scope of the startup-failure. This is the
                        // "Process-global hooks set at runtime need a startup-failure
                        // signal" trap from .rules: the warn must cover the full
                        // ensemble of model-dependent hooks, not just one.
                        //
                        // The inference IPC server is still started (with a
                        // `NoModelInferencePort`) so MCP server child processes
                        // receive `HKASK_INFERENCE_SOCKET` and route inference
                        // through this bridge rather than falling back to
                        // `InferenceRouter::from_env()`. That fallback reads from
                        // the `hkask` keychain namespace, which is empty in zed-kask
                        // (inference keys live in zed's `CredentialsProvider` under
                        // `kask://credentials/<key>`). Without the IPC server, MCP
                        // servers silently failed with "API key not configured" —
                        // an error operators could not trace to the missing socket.
                        // The `NoModelInferencePort` returns a clear diagnostic so
                        // the failure mode is visible and actionable.
                        log::warn!(
                            "No default LanguageModel configured — hKask model-dependent hooks not wired: \
                             manifest executor (skill invocations return no-op envelope), \
                             thread condenser (tool results not compressed), \
                             panel tool invoker (panel cannot dispatch tools), \
                             curator session factory (panel cannot run per-tab curator conversations), \
                             regulation status (panel cannot emit regulation spans). \
                             The inference IPC server is started with a no-op port so MCP \
                             servers route through the bridge and get a diagnostic error \
                             instead of falling back to an empty keychain. \
                             Remediation: configure a default LanguageModel in Settings → \
                             or sign in to your model provider. The deferred task will re-run \
                             on next login and wire these hooks."
                        );

                        // Start the IPC server with a no-op inference port so
                        // `INFERENCE_SOCKET_PATH` is set and MCP servers connect
                        // to the bridge. The media router is still constructed
                        // from env-var keys so media generation (fal.ai/DeepInfra)
                        // works without a default chat model — media backends are
                        // not part of zed's `LanguageModel` abstraction.
                        let media_router = std::sync::Arc::new(
                            kask_bridge::InferenceRouter::new(
                                kask_bridge::InferenceConfig::from_env(),
                            ),
                        );
                        let no_model_port: std::sync::Arc<dyn hkask_types::InferencePort> =
                            std::sync::Arc::new(kask_bridge::NoModelInferencePort);
                        match kask_bridge::InferenceIpcServer::start(
                            no_model_port,
                            None,
                            Some(media_router),
                            cx,
                        ) {
                            Ok(ipc_server) => {
                                let socket_path =
                                    ipc_server.socket_path().to_string_lossy().to_string();
                                let _ = INFERENCE_SOCKET_PATH.set(socket_path.clone());
                                log::info!(
                                    "hKask inference IPC server started (no-op port) at {socket_path} — \
                                     MCP servers will route through the bridge and receive a \
                                     diagnostic error until a default model is configured"
                                );
                                std::mem::forget(ipc_server);
                                sync_kask_mcp_servers(cx);
                            }
                            Err(e) => {
                                log::warn!(
                                    "Failed to start inference IPC server (no-op port): {e} — \
                                     MCP servers will fall back to InferenceRouter with env-var keys"
                                );
                            }
                        }
                    }
                });

                // Launch MCP servers via McpRuntime for app-global governed
                // dispatch (OCAP/gas/regulation). These instances serve the
                // skill cascade (FlowDef) and kask panel.
                //
                // Zed's ContextServerStore (per-project) launches separate
                // instances for the agent tool picker — registered via
                // sync_kask_mcp_servers. The two systems serve different
                // consumers with different governance requirements; the
                // parallel instances are by design, not a bug.
                if !servers_to_start_clone.is_empty() {
                    let tokio_handle = gpui_tokio::Tokio::handle_async(&*cx);
                    let _tokio_guard = tokio_handle.enter();
                    let credential_urls = cx.update(|cx| {
                        let settings = kask_bridge::KaskSettings::get_global(cx);
                        kask_bridge::credential_urls_for_mcp(&settings)
                    });
                    let credentials_provider = cx.update(|cx| zed_credentials_provider::global(cx));
                    for server_id in &servers_to_start_clone {
                        let binary = format!("hkask-mcp-{server_id}");
                        // Filter credentials per-server (same allowlist as the
                        // ContextServerStore path via KaskMcpDescriptor).
                        let server_credential_urls =
                            kask_bridge::filter_credentials_for_server(
                                server_id,
                                &credential_urls,
                            );
                        let server_env = kask_settings
                            .mcp_env_with_credentials(
                                &server_credential_urls,
                                credentials_provider.as_ref(),
                                cx,
                            )
                            .await;
                        // Filter the base config env per-server — the curator's
                        // email config should not be injected into codegraph.
                        let server_env =
                            kask_bridge::filter_config_env_for_server(server_id, &server_env);
                        let mut mcp_env = server_env;
                        if let Some(socket_path) = INFERENCE_SOCKET_PATH.get() {
                            mcp_env.insert(
                                hkask_types::inference_ipc::INFERENCE_SOCKET_ENV.to_string(),
                                socket_path.clone(),
                            );
                        }
                        mcp_runtime_for_deferred
                            .register_server(hkask_mcp::McpServer {
                                id: server_id.to_string(),
                                name: server_id.to_string(),
                                tools: vec![],
                            })
                            .await;
                        match mcp_runtime_for_deferred
                            .start_server_with_env(server_id, &binary, mcp_env)
                            .await
                        {
                            Ok(()) => log::info!("Kask MCP server '{server_id}' started (McpRuntime)"),
                            Err(e) => log::warn!(
                                "Kask MCP server '{server_id}' failed to start: {e} \
                                 — set HKASK_MCP_{}_BIN to the binary path",
                                server_id.to_uppercase()
                            ),
                        }
                    }
                }
            }).detach();
        }

        auto_update::init(client.clone(), cx);
        dap_adapters::init(cx);
        auto_update_ui::init(cx);
        reliability::init(client.clone(), app_state.workspace_store.clone(), cx);
        extension_host::init(
            extension_host_proxy.clone(),
            app_state.fs.clone(),
            app_state.client.clone(),
            app_state.node_runtime.clone(),
            cx,
        );

        theme_settings::init(theme::LoadThemes::All(Box::new(Assets)), cx);
        eager_load_active_theme_and_icon_theme(fs.clone(), cx);
        theme_extension::init(
            extension_host_proxy,
            ThemeRegistry::global(cx),
            cx.background_executor().clone(),
        );
        command_palette::init(cx);
        let copilot_chat_configuration = copilot_chat::CopilotChatConfiguration {
            enterprise_uri: language::language_settings::all_language_settings(None, cx)
                .edit_predictions
                .copilot
                .enterprise_uri
                .clone(),
        };
        copilot_chat::init(
            app_state.fs.clone(),
            app_state.client.http_client(),
            copilot_chat_configuration,
            cx,
        );

        copilot_ui::init(&app_state, cx);
        language_model::init(cx);
        RefreshLlmTokenListener::register(
            app_state.client.clone(),
            app_state.user_store.clone(),
            cx,
        );
        language_models::init(app_state.user_store.clone(), app_state.client.clone(), cx);
        acp_tools::init(cx);
        zed::telemetry_log::init(cx);
        zed::remote_debug::init(cx);
        edit_prediction_ui::init(cx);
        web_search::init(cx);
        web_search_providers::init(app_state.client.clone(), app_state.user_store.clone(), cx);
        snippet_provider::init(cx);
        edit_prediction_registry::init(app_state.client.clone(), app_state.user_store.clone(), cx);
        let prompt_builder = PromptBuilder::load(app_state.fs.clone(), stdout_is_a_pty(), cx);
        project::AgentRegistryStore::init_global(
            cx,
            app_state.fs.clone(),
            app_state.client.http_client(),
        );
        agent_ui::init(
            app_state.fs.clone(),
            prompt_builder,
            app_state.languages.clone(),
            is_new_install,
            false,
            cx,
        );
        kask_panel::init(cx);
        kask_extensions_ui::init(cx);
        zed::watch_user_agents_md(app_state.fs.clone(), cx);

        // D1/D3/D4/D10/D12: Model-dependent kask wiring now runs in the
        // deferred task (after the Zed user resolves and the
        // LanguageModelRegistry is populated). See the deferred task above.
        // Running it here left OnceLock-based hooks unwired when no model was
        // configured at startup (the "Process-global hooks set at runtime
        // need a startup-failure signal" trap from .rules).

        repl::init(app_state.fs.clone(), cx);
        recent_projects::init(cx);
        dev_container::init(cx);

        load_embedded_fonts(cx);

        editor::init(cx);
        image_viewer::init(cx);
        repl::notebook::init(cx);
        diagnostics::init(cx);

        audio::init(cx);
        workspace::init(app_state.clone(), cx);
        ui_prompt::init(cx);

        go_to_line::init(cx);
        file_finder::init(cx);
        tab_switcher::init(cx);
        outline::init(cx);
        project_symbols::init(cx);
        project_panel::init(cx);
        outline_panel::init(cx);
        tasks_ui::init(cx);
        snippets_ui::init(cx);
        channel::init(&app_state.client.clone(), app_state.user_store.clone(), cx);
        search::init(cx);
        lsp_locations::init(cx);
        cx.set_global(workspace::PaneSearchBarCallbacks {
            setup_search_bar: |languages, toolbar, window, cx| {
                let search_bar = cx.new(|cx| search::BufferSearchBar::new(languages, window, cx));
                toolbar.update(cx, |toolbar, cx| {
                    toolbar.add_item(search_bar, window, cx);
                });
            },
            wrap_div_with_search_actions: search::buffer_search::register_pane_search_actions,
        });
        vim::init(cx);
        terminal_view::init(cx);
        journal::init(app_state.clone(), cx);
        encoding_selector::init(cx);
        language_selector::init(cx);
        line_ending_selector::init(cx);
        toolchain_selector::init(cx);
        theme_selector::init(cx);
        settings_profile_selector::init(cx);
        language_tools::init(cx);
        call::init(app_state.client.clone(), app_state.user_store.clone(), cx);
        notifications::init(app_state.client.clone(), app_state.user_store.clone(), cx);
        collab_ui::init(&app_state, cx);
        git_ui::init(cx);
        feedback::init(cx);
        markdown_preview::init(cx);
        csv_preview::init(cx);
        svg_preview::init(cx);
        onboarding::init(cx);
        settings_ui::init(cx);
        keymap_editor::init(cx);
        extensions_ui::init(cx);
        edit_prediction::init(cx);
        inspector_ui::init(app_state.clone(), cx);
        json_schema_store::init(cx);
        miniprofiler_ui::init(*STARTUP_TIME.get().unwrap(), cx);
        which_key::init(cx);
        #[cfg(target_os = "windows")]
        etw_tracing::init(cx);

        cx.observe_global::<SettingsStore>({
            let http = app_state.client.http_client();
            let client = app_state.client.clone();
            move |cx| {
                for &mut window in cx.windows().iter_mut() {
                    let background_appearance = cx.theme().window_background_appearance();
                    window
                        .update(cx, |_, window, _| {
                            window.set_background_appearance(background_appearance)
                        })
                        .ok();
                }

                cx.set_text_rendering_mode(
                    match WorkspaceSettings::get_global(cx).text_rendering_mode {
                        settings::TextRenderingMode::PlatformDefault => {
                            gpui::TextRenderingMode::PlatformDefault
                        }
                        settings::TextRenderingMode::Subpixel => gpui::TextRenderingMode::Subpixel,
                        settings::TextRenderingMode::Grayscale => {
                            gpui::TextRenderingMode::Grayscale
                        }
                    },
                );

                let new_host = &client::ClientSettings::get_global(cx).server_url;
                if &http.base_url() != new_host {
                    http.set_base_url(new_host);
                    if client.status().borrow().is_connected() {
                        client.reconnect(&cx.to_async());
                    }
                }
            }
        })
        .detach();
        app_state.languages.set_theme(cx.theme().clone());
        cx.observe_global::<GlobalTheme>({
            let languages = app_state.languages.clone();
            move |cx| {
                languages.set_theme(cx.theme().clone());
            }
        })
        .detach();
        telemetry::event!(
            "Settings Changed",
            setting = "theme",
            value = cx.theme().name.to_string()
        );
        telemetry::event!(
            "Settings Changed",
            setting = "keymap",
            value = BaseKeymap::get_global(cx).to_string()
        );
        telemetry.flush_events().detach();

        let fs = app_state.fs.clone();
        load_user_themes_in_background(fs.clone(), cx);
        watch_themes(fs.clone(), cx);
        #[cfg(debug_assertions)]
        watch_languages(fs.clone(), app_state.languages.clone(), cx);

        let menus = app_menus(cx);
        cx.set_menus(menus);

        if let Some(mut crash_handler) = crash_handler {
            let crash_handler2 = block_on(poll_once(&mut crash_handler));
            match crash_handler2 {
                Some(crash_handler) => {
                    cx.set_global(CrashHandler(crash_handler));
                }
                None => {
                    cx.spawn(async move |cx| {
                        let client1 = crash_handler.await;
                        cx.update(|cx| {
                            cx.set_global(CrashHandler(client1));
                        });
                    })
                    .detach();
                }
            }
        }

        initialize_workspace(app_state.clone(), cx);

        cx.activate(true);

        cx.spawn({
            let client = app_state.client.clone();
            async move |cx| authenticate(client, cx).await
        })
        .detach_and_log_err(cx);

        let urls: Vec<_> = args
            .paths_or_urls
            .iter()
            .map(|arg| parse_url_arg(arg, cx))
            .collect();

        // Check if any diff paths are directories to determine diff_all mode
        let diff_all_mode = args
            .diff
            .chunks(2)
            .any(|pair| Path::new(&pair[0]).is_dir() || Path::new(&pair[1]).is_dir());

        let diff_paths: Vec<[String; 2]> = args
            .diff
            .chunks(2)
            .map(|chunk| [chunk[0].clone(), chunk[1].clone()])
            .collect();

        #[cfg(target_os = "windows")]
        let wsl = args.wsl;
        #[cfg(not(target_os = "windows"))]
        let wsl = None;

        if !urls.is_empty() || !diff_paths.is_empty() {
            open_listener.open(RawOpenRequest {
                urls,
                diff_paths,
                wsl,
                diff_all: diff_all_mode,
                dev_container: args.dev_container,
                ..Default::default()
            })
        }

        let (current_session_id, last_session_id) = {
            let session = app_state.session.read(cx);
            (
                session.id().to_owned(),
                session.last_session_id().map(|id| id.to_owned()),
            )
        };

        let restore_task = match open_rx
            .try_recv()
            .ok()
            .and_then(|request| OpenRequest::parse(request, cx).log_err())
        {
            Some(request) if request.is_focus_app_only() => cx.spawn({
                let app_state = app_state.clone();
                async move |cx| {
                    if let Err(e) = restore_or_create_workspace(app_state, cx).await {
                        fail_to_open_window_async(e, cx)
                    }
                }
            }),
            Some(request) => {
                handle_open_request(request, app_state.clone(), cx);
                Task::ready(())
            }
            None => cx.spawn({
                let app_state = app_state.clone();
                async move |cx| {
                    if let Err(e) = restore_or_create_workspace(app_state, cx).await {
                        fail_to_open_window_async(e, cx)
                    }
                }
            }),
        };

        cx.spawn({
            let db = workspace::WorkspaceDb::global(cx);
            let fs = app_state.fs.clone();
            async move |_cx| {
                restore_task.await;
                db.garbage_collect_workspaces(
                    fs.as_ref(),
                    &current_session_id,
                    last_session_id.as_deref(),
                )
                .await
            }
        })
        .detach_and_log_err(cx);

        let app_state = app_state.clone();

        component_preview::init(app_state.clone(), cx);

        cx.spawn(async move |cx| {
            while let Some(urls) = open_rx.next().await {
                cx.update(|cx| {
                    if let Some(request) = OpenRequest::parse(urls, cx).log_err() {
                        handle_open_request(request, app_state.clone(), cx);
                    }
                });
            }
        })
        .detach();
    });
}

// ── Kask MCP server registration ───────────────────────────────────────────
//
// Registers the built-in kask MCP servers as zed context servers via the
// app-level ContextServerDescriptorRegistry. This makes kask MCP tools appear
// in the agent tool picker and available to zed's agent thread. The servers
// are launched as stdio child processes by zed's ContextServerStore.

/// A ContextServerDescriptor for a built-in kask MCP server.
///
/// Returns the binary path (`hkask-mcp-{id}`) and env vars (kask settings +
/// credentials + inference socket) when `command()` is called. The env is
/// resolved at call time so credentials are fresh.
///
/// Credentials are filtered per-server via `filter_credentials_for_server` —
/// only env vars in the server's `BuiltinMcpServer::credentials` allowlist are
/// injected. This limits the blast radius of a compromised MCP server.
struct KaskMcpDescriptor {
    id: &'static str,
    binary: &'static str,
}

impl project::context_server_store::registry::ContextServerDescriptor for KaskMcpDescriptor {
    fn command(
        &self,
        _worktree_store: gpui::Entity<project::worktree_store::WorktreeStore>,
        cx: &gpui::AsyncApp,
    ) -> gpui::Task<anyhow::Result<context_server::ContextServerCommand>> {
        let binary = self.binary.to_string();
        let server_id = self.id.to_string();
        cx.spawn(async move |cx| {
            // Resolve env vars from kask settings + credentials.
            let (settings, credential_urls, base_env) = cx.update(|cx| {
                let settings = kask_bridge::KaskSettings::get_global(cx).clone();
                let credential_urls = kask_bridge::credential_urls_for_mcp(&settings);
                let env = settings.mcp_env();
                (settings, credential_urls, env)
            });

            // Filter credentials to only those this server is allowed to
            // receive (per-server allowlist in BuiltinMcpServer::credentials).
            let credential_urls =
                kask_bridge::filter_credentials_for_server(&server_id, &credential_urls);

            let credentials_provider = cx.update(|cx| zed_credentials_provider::global(cx));
            let std_env_map = settings
                .mcp_env_with_credentials(&credential_urls, credentials_provider.as_ref(), cx)
                .await;
            let mut env_map: collections::HashMap<String, String> =
                std_env_map.into_iter().collect();
            // Filter the base config env (from mcp_env()) per-server too —
            // the curator's email config should not be injected into codegraph.
            let filtered_base_env =
                kask_bridge::filter_config_env_for_server(&server_id, &base_env);
            env_map.extend(filtered_base_env);

            // Pass the inference IPC socket path so MCP servers can route
            // inference through zed's LanguageModelRegistry.
            if let Some(socket_path) = INFERENCE_SOCKET_PATH.get() {
                env_map.insert(
                    hkask_types::inference_ipc::INFERENCE_SOCKET_ENV.to_string(),
                    socket_path.clone(),
                );
            }

            Ok(context_server::ContextServerCommand {
                path: binary.into(),
                args: vec![],
                env: Some(env_map),
                timeout: None,
            })
        })
    }

    fn configuration(
        &self,
        _worktree_store: gpui::Entity<project::worktree_store::WorktreeStore>,
        _cx: &gpui::AsyncApp,
    ) -> gpui::Task<anyhow::Result<Option<extension::ContextServerConfiguration>>> {
        gpui::Task::ready(Ok(None))
    }
}

/// Reconcile the app-level `ContextServerDescriptorRegistry` with the
/// current kask MCP settings.
///
/// Registers descriptors for all enabled servers and unregisters descriptors
/// for servers that are no longer enabled. Called once at startup, whenever
/// `SettingsStore` changes (via `cx.observe_global::<SettingsStore>`), and
/// after the inference IPC socket is set (so servers get the socket path).
///
/// The `ContextServerStore` (per-project) observes the registry and will
/// start/stop/restart the actual server processes to match.
fn sync_kask_mcp_servers(cx: &mut gpui::App) {
    let settings = kask_bridge::KaskSettings::get_global(cx).clone();
    let registry =
        project::context_server_store::registry::ContextServerDescriptorRegistry::default_global(
            cx,
        );
    registry.update(cx, |registry, cx| {
        for server in kask_bridge::BUILT_IN_MCP_SERVERS {
            let enabled = settings.mcp.load_default
                && *settings.mcp.overrides.get(server.id).unwrap_or(&true);
            let id: std::sync::Arc<str> = std::sync::Arc::from(server.id);
            let already_registered = registry.context_server_descriptor(server.id).is_some();
            if enabled && !already_registered {
                registry.register_context_server_descriptor(
                    id,
                    std::sync::Arc::new(KaskMcpDescriptor {
                        id: server.id,
                        binary: server.binary,
                    })
                        as std::sync::Arc<
                            dyn project::context_server_store::registry::ContextServerDescriptor,
                        >,
                    cx,
                );
                log::info!(
                    "Registered kask MCP server '{}' as zed context server",
                    server.id
                );
            } else if !enabled && already_registered {
                registry.unregister_context_server_descriptor_by_id(server.id, cx);
                log::info!(
                    "Unregistered kask MCP server '{}' from zed context servers",
                    server.id
                );
            }
        }
        // Always notify so the ContextServerStore re-runs maintain_servers.
        // This is needed because the KaskMcpDescriptor::command() resolves env
        // vars (credentials, inference socket) at call time — if the socket
        // wasn't available when maintain_servers last ran, the running server
        // processes have stale env. Notifying forces maintain_servers to
        // re-evaluate and restart servers whose configuration changed.
        cx.notify();
    });
}

// ── Embedding port resolution ─────────────────────────────────────────────
//
// Resolves a zed `LanguageModel` from the embedding model's provider prefix
// (e.g. `DeepInfra/Qwen/...` → the `deepinfra` provider) and constructs a
// `LanguageModelEmbeddingPort` that uses the provider's `api_url()` + `api_key()`
// to make raw `/embeddings` calls through zed's HTTP client. This replaces
// hKask's standalone `EmbeddingRouter` which resolved credentials from env vars.

/// Parse the zed provider id from an embedding model string.
///
/// `DeepInfra/Qwen/...` → `"deepinfra"`, `OpenRouter/...` → `"openrouter"`,
/// etc. Case-insensitive. Returns `None` if no recognized prefix is found.
fn embedding_provider_id(embedding_model: &str) -> Option<&str> {
    // Long-form prefixes (case-insensitive) → zed provider id.
    const LONG_FORM: &[(&str, &str)] = &[
        ("DeepInfra/", "deepinfra"),
        ("fal.ai/", "fal"),
        ("Together AI/", "together"),
        ("RunPod/", "runpod"),
        ("OpenRouter/", "openrouter"),
        ("KiloCode/", "kilocode"),
        ("ollama/", "ollama"),
        ("Cline/", "cline"),
    ];
    for (prefix, id) in LONG_FORM {
        if embedding_model.len() >= prefix.len()
            && embedding_model[..prefix.len()].eq_ignore_ascii_case(prefix)
        {
            return Some(id);
        }
    }
    None
}

/// Resolve a zed `LanguageModel` from the embedding model's provider and
/// construct a `LanguageModelEmbeddingPort`.
///
/// Returns `None` (after logging a warn with remediation guidance) if:
/// - The embedding model has no recognized provider prefix.
/// - The provider isn't registered in zed's `LanguageModelRegistry`.
/// - The provider has no models (not authenticated / no models discovered).
///
/// Per the `.rules` trap "Process-global hooks set at runtime need a
/// startup-failure signal": failure is surfaced, not silently degraded.
fn resolve_embedding_port(
    embedding_model: &str,
    app_state: &Arc<workspace::AppState>,
    cx: &mut gpui::App,
) -> Option<(kask_bridge::LanguageModelEmbeddingPort, gpui::Task<()>)> {
    let provider_id = embedding_provider_id(embedding_model).or_else(|| {
        // No prefix — try the default model's provider as a fallback.
        // This handles users who set a bare model id like `Qwen/Qwen3-Embedding-0.6B`.
        log::warn!(
            "Embedding model '{}' has no recognized provider prefix \
             (expected e.g. 'DeepInfra/...'). \
             Cannot resolve provider from zed's LanguageModelRegistry. \
             Set kask.corpus.embedding_model to a provider-prefixed name, \
             or set HKASK_EMBEDDING_MODEL.",
            embedding_model
        );
        None
    })?;

    let registry = language_model::LanguageModelRegistry::read_global(cx);
    let provider = registry
        .providers()
        .into_iter()
        .find(|p| p.id().0 == provider_id);

    let Some(provider) = provider else {
        log::warn!(
            "Embedding provider '{}' is not registered in zed's \
             LanguageModelRegistry. Add it in Settings → AI → LLM Providers, \
             or set the corresponding env var (e.g. DEEPINFRA_API_KEY). \
             Embedding-based recall will not work until the provider is configured.",
            provider_id
        );
        return None;
    };

    let Some(credential_model) = provider.default_model(cx) else {
        log::warn!(
            "Embedding provider '{}' has no default model — it may not be \
             authenticated or has not discovered models yet. Add the API key \
             in Settings → AI → LLM Providers. Embedding-based recall will \
             not work until the provider is authenticated.",
            provider_id
        );
        return None;
    };

    let http_client = app_state.client.http_client();
    let async_cx = cx.to_async();
    let (port, task) =
        kask_bridge::LanguageModelEmbeddingPort::new(credential_model, http_client, async_cx);
    log::info!(
        "hKask embedding port constructed — routing embeddings through \
         zed provider '{}' (model: {})",
        provider_id,
        embedding_model
    );
    Some((port, task))
}

// ── D10: Kask panel adapters ───────────────────────────────────────────────
//
// This adapter implements kask_panel's ToolInvoker trait by delegating to the
// bridge's BridgeToolPort. It's defined here (in the zed binary crate) because
// kask_bridge can't depend on kask_panel (circular dependency), and the
// composition root is the natural place for adapter construction.
//
// The chat panel itself no longer uses ToolInvoker — it routes through
// NativeAgent's ToolRouter via the ConversationView. This adapter remains for
// the per-server visualization views (KanbanBoardView, PortfolioDashboardView,
// ScenariosView), which fetch data via direct MCP tool calls.

/// Adapter implementing `kask_panel::ToolInvoker` via `BridgeToolPort`.
struct PanelToolInvoker {
    tool_port: std::sync::Arc<kask_bridge::BridgeToolPort>,
    a2a_secret: Vec<u8>,
    executor: gpui::BackgroundExecutor,
}

impl kask_panel::ToolInvoker for PanelToolInvoker {
    fn invoke_tool(
        &self,
        server: &str,
        tool: &str,
        args: serde_json::Value,
    ) -> gpui::Task<Result<String, String>> {
        use hkask_capability::{DelegationAction, DelegationResource, DelegationToken, ToolPort};
        use hkask_types::WebID;

        let secret_bytes: [u8; 32] = self
            .a2a_secret
            .get(..32)
            .and_then(|s| s.try_into().ok())
            .unwrap_or_else(|| {
                log::warn!("a2a_secret too short for Ed25519 — using zeroed key");
                [0u8; 32]
            });
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&secret_bytes);
        let webid = WebID::from_persona(b"kask-panel");
        let token = DelegationToken::new(
            DelegationResource::Tool,
            tool.to_string(),
            DelegationAction::Execute,
            webid,
            webid,
            &signing_key,
        );

        let tool_port = self.tool_port.clone();
        let server = server.to_string();
        let tool = tool.to_string();

        self.executor.spawn(async move {
            let result = ToolPort::invoke(&*tool_port, &server, &tool, args, &token)
                .await
                .map_err(|e| e.to_string())?;
            Ok(serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string()))
        })
    }

    fn list_tools(
        &self,
        server: &str,
    ) -> gpui::Task<Result<Vec<kask_panel::ToolDescriptor>, String>> {
        let runtime = self.tool_port.runtime_arc();
        let server = server.to_string();
        self.executor.spawn(async move {
            let servers = runtime.list_servers().await;
            let target = servers
                .into_iter()
                .find(|s| s.id == server)
                .ok_or_else(|| format!("server '{server}' not registered"))?;
            Ok(target
                .tools
                .into_iter()
                .map(|tool| kask_panel::ToolDescriptor {
                    name: tool.name,
                    description: tool.description,
                })
                .collect())
        })
    }
}

/// Adapter implementing `hkask_regulation::AlertSink` by forwarding critical
/// regulation alerts to a GPUI foreground task that dispatches them as toasts.
///
/// The metacognition loop calls `on_alert` from a background tokio task when a
/// critical threshold is breached (well exhaustion, variety deficit, low
/// effectiveness). `AsyncApp` is not `Send` (GPUI is single-threaded), so the
/// sink holds a `tokio::sync::mpsc::UnboundedSender` (which is `Send + Sync`)
/// and a GPUI foreground task drains the receiver and dispatches toasts. This
/// closes the algedonic escalation path from S1 (sensor) to S5 (user-facing
/// policy) so the user is notified even when the Kask panel is closed.
///
/// Toast delivery is best-effort: if no window is open (headless, startup),
/// the toast is silently dropped and the alert remains in the logs + ledger.
struct ToastAlertSink {
    tx: tokio::sync::mpsc::UnboundedSender<hkask_regulation::AlertEvent>,
}

impl ToastAlertSink {
    fn new(tx: tokio::sync::mpsc::UnboundedSender<hkask_regulation::AlertEvent>) -> Self {
        Self { tx }
    }
}

impl hkask_regulation::AlertSink for ToastAlertSink {
    fn on_alert(&self, event: &hkask_regulation::AlertEvent) {
        // Only critical alerts warrant a toast — warnings are surfaced via
        // the Kask panel status bar and the `curator_status` tool.
        if !event.critical {
            return;
        }
        // `try_send` is non-blocking; if the channel is full or the receiver
        // was dropped (app shutting down), the alert is logged and swallowed —
        // alert delivery is best-effort, never a correctness path.
        if let Err(e) = self.tx.send(event.clone()) {
            log::warn!(
                "hKask critical alert dropped (toast channel closed/full): {e} — alert: {}",
                event.message
            );
        }
    }
}

struct KaskCriticalAlertToast;

/// Spawn a GPUI foreground task that drains the alert receiver and dispatches
/// a toast for each critical alert. Returns when the sender is dropped (app
/// shutdown). Must be called from the GPUI foreground thread.
fn spawn_alert_toast_drainer(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<hkask_regulation::AlertEvent>,
    cx: &mut gpui::App,
) {
    cx.spawn(async move |cx| {
        while let Some(event) = rx.recv().await {
            // Only critical alerts reach this point (the sink filters), but
            // double-check in case the sender contract changes.
            if !event.critical {
                continue;
            }
            let message = event.message.clone();
            cx.update(|cx| {
                // Find any active MultiWorkspace and show the toast on its
                // inner workspace. If no window is open, the toast is dropped —
                // the alert still lives in the logs and the regulation ledger.
                if let Some(window) = cx.active_window()
                    && let Some(multi_workspace) = window.downcast::<MultiWorkspace>()
                {
                    let _ = multi_workspace.update(cx, |multi_workspace, _, cx| {
                        let workspace = multi_workspace.workspace();
                        let _ = workspace.update(cx, |workspace, cx| {
                            workspace.show_toast(
                                Toast::new(
                                    NotificationId::unique::<KaskCriticalAlertToast>(),
                                    format!("hKask regulation alert: {message}"),
                                ),
                                cx,
                            );
                        });
                    });
                }
            });
        }
    })
    .detach();
}

fn handle_open_request(request: OpenRequest, app_state: Arc<AppState>, cx: &mut App) {
    if let Some(kind) = request.kind {
        match kind {
            OpenRequestKind::CliConnection(connection) => {
                cx.spawn(async move |cx| handle_cli_connection(connection, app_state, cx).await)
                    .detach();
            }
            OpenRequestKind::FocusApp => {
                cx.spawn(async move |cx| {
                    if workspace::activate_any_workspace_window(cx).is_some() {
                        return anyhow::Ok(());
                    }
                    restore_or_create_workspace(app_state, cx).await
                })
                .detach_and_log_err(cx);
            }
            OpenRequestKind::Extension { extension_id } => {
                cx.spawn(async move |cx| {
                    let workspace =
                        workspace::get_any_active_multi_workspace(app_state, cx.clone()).await?;
                    workspace.update(cx, |_, window, cx| {
                        window.dispatch_action(
                            Box::new(zed_actions::Extensions {
                                category_filter: None,
                                id: Some(extension_id),
                            }),
                            cx,
                        );
                    })
                })
                .detach_and_log_err(cx);
            }
            OpenRequestKind::AgentPanel {
                external_source_prompt,
            } => {
                cx.spawn(async move |cx| {
                    let multi_workspace =
                        workspace::get_any_active_multi_workspace(app_state, cx.clone()).await?;

                    let panels_task = multi_workspace.update(cx, |multi_workspace, _, cx| {
                        multi_workspace
                            .workspace()
                            .update(cx, |workspace, _| workspace.take_panels_task())
                    })?;
                    if let Some(task) = panels_task {
                        task.await.log_err();
                    }

                    multi_workspace.update(cx, |multi_workspace, window, cx| {
                        multi_workspace.workspace().update(cx, |workspace, cx| {
                            if let Some(panel) = workspace.focus_panel::<AgentPanel>(window, cx) {
                                panel.update(cx, |panel, cx| {
                                    panel.new_agent_thread_with_external_source_prompt(
                                        external_source_prompt,
                                        window,
                                        cx,
                                    );
                                });
                            } else {
                                log::warn!(
                                    "{ZED_URL_SCHEME}://agent received but the AgentPanel is not \
                                     registered (is `disable_ai` enabled?)"
                                );
                            }
                        });
                    })
                })
                .detach_and_log_err(cx);
            }
            OpenRequestKind::InstallSkill { content } => {
                cx.spawn(async move |cx| {
                    let multi_workspace =
                        workspace::get_any_active_multi_workspace(app_state, cx.clone()).await?;

                    multi_workspace.update(cx, |_multi_workspace, _window, cx| {
                        settings_ui::open_skill_creator(
                            settings_ui::pages::SkillCreatorOpenMode::Install { content },
                            Some(multi_workspace),
                            cx,
                        );
                    })
                })
                .detach_and_log_err(cx);
            }
            OpenRequestKind::DockMenuAction { index } => {
                cx.perform_dock_menu_action(index);
            }
            OpenRequestKind::BuiltinJsonSchema { schema_path } => {
                workspace::with_active_or_new_workspace(cx, |_workspace, window, cx| {
                    cx.spawn_in(window, async move |workspace, cx| {
                        let res = async move {
                            let json = app_state.languages.language_for_name("JSONC").await.ok();
                            let lsp_store = workspace.update(cx, |workspace, cx| {
                                workspace
                                    .project()
                                    .update(cx, |project, _| project.lsp_store())
                            })?;
                            let uri = format!("zed://schemas/{}", schema_path);
                            let json_schema_content =
                                json_schema_store::handle_schema_request(lsp_store, uri, cx)
                                    .await?;
                            let json_schema_value: serde_json::Value =
                                serde_json::from_str(&json_schema_content)
                                    .context("Failed to parse JSON Schema")?;
                            let json_schema_content =
                                serde_json::to_string_pretty(&json_schema_value)
                                    .context("Failed to serialize JSON Schema as JSON")?;
                            let buffer_task = workspace.update(cx, |workspace, cx| {
                                workspace.project().update(cx, |project, cx| {
                                    project.create_buffer(json, false, cx)
                                })
                            })?;

                            let buffer = buffer_task.await?;

                            workspace.update_in(cx, |workspace, window, cx| {
                                buffer.update(cx, |buffer, cx| {
                                    buffer.edit([(0..0, json_schema_content)], None, cx);
                                    buffer.edit(
                                        [(0..0, format!("// {} JSON Schema\n", schema_path))],
                                        None,
                                        cx,
                                    );
                                });

                                workspace.add_item_to_active_pane(
                                    Box::new(cx.new(|cx| {
                                        let mut editor =
                                            editor::Editor::for_buffer(buffer, None, window, cx);
                                        editor.set_read_only(true);
                                        editor
                                    })),
                                    None,
                                    true,
                                    window,
                                    cx,
                                );
                            })
                        }
                        .await;
                        res.context("Failed to open builtin JSON Schema").log_err();
                    })
                    .detach();
                });
            }
            OpenRequestKind::Setting { setting_path } => {
                // <app-scheme>://settings/languages/$(language)/tab_size  - DONT SUPPORT
                // <app-scheme>://settings/languages/Rust/tab_size  - SUPPORT
                // languages.$(language).tab_size
                // [ languages $(language) tab_size]
                cx.spawn(async move |cx| {
                    let workspace =
                        workspace::get_any_active_multi_workspace(app_state, cx.clone()).await?;

                    workspace.update(cx, |_, window, cx| match setting_path {
                        None => window.dispatch_action(Box::new(zed_actions::OpenSettings), cx),
                        Some(setting_path) => window.dispatch_action(
                            Box::new(zed_actions::OpenSettingsAt {
                                path: setting_path,
                                target: None,
                            }),
                            cx,
                        ),
                    })
                })
                .detach_and_log_err(cx);
            }
            OpenRequestKind::GitClone { repo_url } => {
                workspace::with_active_or_new_workspace(cx, |_workspace, window, cx| {
                    if window.is_window_active() {
                        clone_and_open(
                            repo_url,
                            cx.weak_entity(),
                            window,
                            cx,
                            Arc::new(|workspace: &mut workspace::Workspace, window, cx| {
                                workspace.focus_panel::<ProjectPanel>(window, cx);
                            }),
                        );
                        return;
                    }

                    let subscription = Rc::new(RefCell::new(None));
                    subscription.replace(Some(cx.observe_in(&cx.entity(), window, {
                        let subscription = subscription.clone();
                        let repo_url = repo_url;
                        move |_, workspace_entity, window, cx| {
                            if window.is_window_active() && subscription.take().is_some() {
                                clone_and_open(
                                    repo_url.clone(),
                                    workspace_entity.downgrade(),
                                    window,
                                    cx,
                                    Arc::new(|workspace: &mut workspace::Workspace, window, cx| {
                                        workspace.focus_panel::<ProjectPanel>(window, cx);
                                    }),
                                );
                            }
                        }
                    })));
                });
            }
            OpenRequestKind::GitCommit { sha } => {
                let base_open_options = zed::open_options_for_request(
                    request.open_behavior,
                    &workspace::SerializedWorkspaceLocation::Local,
                    cx,
                );
                cx.spawn(async move |cx| {
                    let paths_with_position =
                        derive_paths_with_position(app_state.fs.as_ref(), request.open_paths).await;
                    let (workspace, _results) = open_paths_with_positions(
                        &paths_with_position,
                        &[],
                        false,
                        app_state,
                        base_open_options,
                        cx,
                    )
                    .await?;

                    workspace
                        .update(cx, |multi_workspace, window, cx| {
                            multi_workspace
                                .workspace()
                                .clone()
                                .update(cx, |workspace, cx| {
                                    let Some(repo) =
                                        workspace.project().read(cx).active_repository(cx)
                                    else {
                                        log::error!("no active repository found for commit view");
                                        return Err(anyhow::anyhow!("no active repository found"));
                                    };

                                    git_ui::commit_view::CommitView::open(
                                        sha,
                                        repo.downgrade(),
                                        workspace.weak_handle(),
                                        None,
                                        None,
                                        window,
                                        cx,
                                    );
                                    Ok(())
                                })
                        })
                        .log_err();

                    anyhow::Ok(())
                })
                .detach_and_log_err(cx);
            }
        }

        return;
    }

    if let Some(connection_options) = request.remote_connection {
        let open_behavior = request.open_behavior;
        let location = workspace::SerializedWorkspaceLocation::Remote(connection_options.clone());
        let base_open_options = zed::open_options_for_request(open_behavior, &location, cx);
        cx.spawn(async move |cx| {
            let paths: Vec<PathBuf> = request.open_paths.into_iter().map(PathBuf::from).collect();
            open_remote_project(connection_options, paths, app_state, base_open_options, cx).await
        })
        .detach_and_log_err(cx);
        return;
    }

    let mut task = None;
    let dev_container = request.dev_container;
    if !request.open_paths.is_empty() || !request.diff_paths.is_empty() {
        let app_state = app_state.clone();
        let base_open_options = zed::open_options_for_request(
            request.open_behavior,
            &workspace::SerializedWorkspaceLocation::Local,
            cx,
        );
        task = Some(cx.spawn(async move |cx| {
            let paths_with_position =
                derive_paths_with_position(app_state.fs.as_ref(), request.open_paths).await;
            let (_window, results) = open_paths_with_positions(
                &paths_with_position,
                &request.diff_paths,
                request.diff_all,
                app_state,
                workspace::OpenOptions {
                    open_in_dev_container: dev_container,
                    ..base_open_options
                },
                cx,
            )
            .await?;
            for result in results.into_iter().flatten() {
                if let Err(err) = result {
                    log::error!("Error opening path: {err:#}");
                }
            }
            anyhow::Ok(())
        }));
    }

    if !request.open_channel_notes.is_empty() || request.join_channel.is_some() {
        cx.spawn(async move |cx| {
            let result = maybe!(async {
                if let Some(task) = task {
                    task.await?;
                }
                let client = app_state.client.clone();
                // we continue even if connection fails as join_channel/ open channel notes will
                // show a visible error message.
                client.connect(true, cx).await.into_response().log_err();

                if let Some(channel_id) = request.join_channel {
                    cx.update(|cx| {
                        workspace::join_channel(
                            client::ChannelId(channel_id),
                            app_state.clone(),
                            None,
                            None,
                            cx,
                        )
                    })
                    .await?;
                }

                let workspace_window =
                    workspace::get_any_active_multi_workspace(app_state, cx.clone()).await?;

                let workspace = workspace_window.read_with(cx, |mw, _| mw.workspace().clone())?;
                let weak_workspace = workspace.downgrade();

                let mut promises = Vec::new();
                for (channel_id, heading) in request.open_channel_notes {
                    promises.push(cx.update_window(workspace_window.into(), |_, window, cx| {
                        ChannelView::open(
                            client::ChannelId(channel_id),
                            heading,
                            workspace.clone(),
                            window,
                            cx,
                        )
                    })?)
                }
                for result in future::join_all(promises).await {
                    result.notify_workspace_async_err(weak_workspace.clone(), cx);
                }
                anyhow::Ok(())
            })
            .await;
            if let Err(err) = result {
                fail_to_open_window_async(err, cx);
            }
        })
        .detach()
    } else if let Some(task) = task {
        cx.spawn(async move |cx| {
            if let Err(err) = task.await {
                fail_to_open_window_async(err, cx);
            }
        })
        .detach();
    }
}

async fn authenticate(client: Arc<Client>, cx: &AsyncApp) -> Result<()> {
    if stdout_is_a_pty() {
        if client::IMPERSONATE_LOGIN.is_some() {
            client.sign_in_with_optional_connect(false, cx).await?;
        } else if client.has_credentials(cx).await {
            client.sign_in_with_optional_connect(true, cx).await?;
        }
    } else if client.has_credentials(cx).await {
        client.sign_in_with_optional_connect(true, cx).await?;
    }

    Ok(())
}

async fn system_id() -> Result<IdType> {
    let key_name = "system_id".to_string();
    let db = GlobalKeyValueStore::global();

    if let Ok(Some(system_id)) = db.read_kvp(&key_name) {
        return Ok(IdType::Existing(system_id));
    }

    let system_id = Uuid::new_v4().to_string();

    db.write_kvp(key_name, system_id.clone()).await?;

    Ok(IdType::New(system_id))
}

async fn installation_id(db: KeyValueStore) -> Result<IdType> {
    let legacy_key_name = "device_id".to_string();
    let key_name = "installation_id".to_string();

    // Migrate legacy key to new key
    if let Ok(Some(installation_id)) = db.read_kvp(&legacy_key_name) {
        db.write_kvp(key_name, installation_id.clone()).await?;
        db.delete_kvp(legacy_key_name).await?;
        return Ok(IdType::Existing(installation_id));
    }

    if let Ok(Some(installation_id)) = db.read_kvp(&key_name) {
        return Ok(IdType::Existing(installation_id));
    }

    let installation_id = Uuid::new_v4().to_string();

    db.write_kvp(key_name, installation_id.clone()).await?;

    Ok(IdType::New(installation_id))
}

pub(crate) async fn restore_or_create_workspace(
    app_state: Arc<AppState>,
    cx: &mut AsyncApp,
) -> Result<()> {
    let kvp = cx.update(|cx| KeyValueStore::global(cx));
    if let Some(multi_workspaces) = restorable_workspaces(cx, &app_state).await {
        let mut error_count = 0;
        for multi_workspace in multi_workspaces {
            let result = match &multi_workspace.active_workspace.location {
                SerializedWorkspaceLocation::Local => {
                    restore_multiworkspace(multi_workspace, app_state.clone(), cx)
                        .await
                        .map(|_| ())
                }
                SerializedWorkspaceLocation::Remote(connection_options) => {
                    let mut connection_options = connection_options.clone();
                    if let RemoteConnectionOptions::Ssh(options) = &mut connection_options {
                        cx.update(|cx| {
                            RemoteSettings::get_global(cx)
                                .fill_connection_options_from_settings(options)
                        });
                    }

                    let paths = multi_workspace
                        .active_workspace
                        .paths
                        .paths()
                        .iter()
                        .map(PathBuf::from)
                        .collect::<Vec<_>>();
                    let state = multi_workspace.state.clone();
                    async {
                        let window = open_remote_project(
                            connection_options,
                            paths,
                            app_state.clone(),
                            workspace::OpenOptions::default(),
                            cx,
                        )
                        .await?;
                        workspace::apply_restored_multiworkspace_state(
                            window,
                            &state,
                            app_state.fs.clone(),
                            cx,
                        )
                        .await;
                        Ok::<(), anyhow::Error>(())
                    }
                    .await
                }
            };

            if let Err(error) = result {
                log::error!("Failed to restore workspace: {error:#}");
                error_count += 1;
            }
        }

        if error_count > 0 {
            let message = if error_count == 1 {
                "Failed to restore 1 workspace. Check logs for details.".to_string()
            } else {
                format!(
                    "Failed to restore {} workspaces. Check logs for details.",
                    error_count
                )
            };

            // Try to find an active workspace to show the toast
            let toast_shown = cx.update(|cx| {
                if let Some(window) = cx.active_window()
                    && let Some(multi_workspace) = window.downcast::<MultiWorkspace>()
                {
                    multi_workspace
                        .update(cx, |multi_workspace, _, cx| {
                            multi_workspace.workspace().update(cx, |workspace, cx| {
                                workspace.show_toast(
                                    Toast::new(NotificationId::unique::<()>(), message.clone()),
                                    cx,
                                )
                            });
                        })
                        .ok();
                    return true;
                }
                false
            });

            // If we couldn't show a toast (no windows opened successfully),
            // open a fallback empty workspace and show the error there
            if !toast_shown {
                log::error!("All workspace restorations failed. Opening fallback empty workspace.");
                cx.update(|cx| {
                    workspace::open_new(
                        Default::default(),
                        app_state.clone(),
                        cx,
                        |workspace, _window, cx| {
                            workspace.show_toast(
                                Toast::new(NotificationId::unique::<()>(), message),
                                cx,
                            );
                        },
                    )
                })
                .await?;
            }
        }

        // If the user cancelled a failed remote connection at startup,
        // open_remote_project returns Ok but removes the window, so error_count
        // stays 0 and the toast fallback above does not trigger. Without this
        // check, Zed would exit silently.
        if cx.update(|cx| cx.windows().is_empty()) {
            cx.update(|cx| {
                workspace::open_new(
                    Default::default(),
                    app_state.clone(),
                    cx,
                    |workspace, window, cx| {
                        let restore_on_startup =
                            WorkspaceSettings::get_global(cx).restore_on_startup;
                        match restore_on_startup {
                            workspace::RestoreOnStartupBehavior::Launchpad => {}
                            _ => {
                                Editor::new_file(workspace, &Default::default(), window, cx);
                            }
                        }
                    },
                )
            })
            .await?;
        }
    } else if matches!(kvp.read_kvp(FIRST_OPEN), Ok(None)) {
        cx.update(|cx| show_onboarding_view(app_state, cx)).await?;
    } else {
        cx.update(|cx| {
            workspace::open_new(
                Default::default(),
                app_state,
                cx,
                |workspace, window, cx| {
                    let restore_on_startup = WorkspaceSettings::get_global(cx).restore_on_startup;
                    match restore_on_startup {
                        workspace::RestoreOnStartupBehavior::Launchpad => {}
                        _ => {
                            Editor::new_file(workspace, &Default::default(), window, cx);
                        }
                    }
                },
            )
        })
        .await?;
    }

    Ok(())
}

async fn restorable_workspaces(
    cx: &mut AsyncApp,
    app_state: &Arc<AppState>,
) -> Option<Vec<workspace::SerializedMultiWorkspace>> {
    let locations = restorable_workspace_locations(cx, app_state).await?;
    Some(cx.update(|cx| workspace::read_serialized_multi_workspaces(locations, cx)))
}

pub(crate) async fn restorable_workspace_locations(
    cx: &mut AsyncApp,
    app_state: &Arc<AppState>,
) -> Option<Vec<SessionWorkspace>> {
    let (mut restore_behavior, db) = cx.update(|cx| {
        (
            WorkspaceSettings::get(None, cx).restore_on_startup,
            workspace::WorkspaceDb::global(cx),
        )
    });

    let session_handle = app_state.session.clone();
    let (last_session_id, last_session_window_stack) = cx.update(|cx| {
        let session = session_handle.read(cx);

        (
            session.last_session_id().map(|id| id.to_string()),
            session.last_session_window_stack(),
        )
    });

    if last_session_id.is_none()
        && matches!(
            restore_behavior,
            workspace::RestoreOnStartupBehavior::LastSession
        )
    {
        restore_behavior = workspace::RestoreOnStartupBehavior::LastWorkspace;
    }

    match restore_behavior {
        workspace::RestoreOnStartupBehavior::LastWorkspace => {
            workspace::last_opened_workspace_location(&db, app_state.fs.as_ref())
                .await
                .map(|(workspace_id, location, paths)| {
                    vec![SessionWorkspace {
                        workspace_id,
                        location,
                        paths,
                        window_id: None,
                    }]
                })
        }
        workspace::RestoreOnStartupBehavior::LastSession => {
            if let Some(last_session_id) = last_session_id {
                let ordered = last_session_window_stack.is_some();

                let mut locations = workspace::last_session_workspace_locations(
                    &db,
                    &last_session_id,
                    last_session_window_stack,
                    app_state.fs.as_ref(),
                )
                .await
                .filter(|locations| !locations.is_empty());

                // Since last_session_window_order returns the windows ordered front-to-back
                // we need to open the window that was frontmost last.
                if ordered && let Some(locations) = locations.as_mut() {
                    locations.reverse();
                }

                locations
            } else {
                None
            }
        }
        _ => None,
    }
}

fn init_paths() -> HashMap<io::ErrorKind, Vec<&'static Path>> {
    [
        paths::config_dir(),
        paths::extensions_dir(),
        paths::languages_dir(),
        paths::debug_adapters_dir(),
        paths::database_dir(),
        paths::logs_dir(),
        paths::temp_dir(),
        paths::hang_traces_dir(),
    ]
    .into_iter()
    .fold(HashMap::default(), |mut errors, path| {
        if let Err(e) = std::fs::create_dir_all(path) {
            errors.entry(e.kind()).or_insert_with(Vec::new).push(path);
        }
        errors
    })
}

pub(crate) static FORCE_CLI_MODE: LazyLock<bool> = LazyLock::new(|| {
    let env_var = std::env::var(FORCE_CLI_MODE_ENV_VAR_NAME).ok().is_some();
    unsafe { std::env::remove_var(FORCE_CLI_MODE_ENV_VAR_NAME) };
    env_var
});

fn stdout_is_a_pty() -> bool {
    !*FORCE_CLI_MODE && io::stdout().is_terminal()
}

#[derive(Parser, Debug)]
#[command(name = "zed", disable_version_flag = true, max_term_width = 100)]
struct Args {
    /// A sequence of space-separated paths or urls that you want to open.
    ///
    /// Use `path:line:row` syntax to open a file at a specific location.
    /// Non-existing paths and directories will ignore `:line:row` suffix.
    ///
    /// URLs can use the `file://` or application URL scheme, or be relative to the server URL.
    paths_or_urls: Vec<String>,

    /// Pairs of file paths to diff. Can be specified multiple times.
    /// When directories are provided, recurses into them and shows all changed files in a single multi-diff view.
    #[arg(long, action = clap::ArgAction::Append, num_args = 2, value_names = ["OLD_PATH", "NEW_PATH"])]
    diff: Vec<String>,

    /// Sets a custom directory for all user data (e.g., database, extensions, logs).
    ///
    /// This overrides the default platform-specific data directory location.
    /// On macOS, the default is `~/Library/Application Support/Zed`.
    /// On Linux/FreeBSD, the default is `$XDG_DATA_HOME/zed`.
    /// On Windows, the default is `%LOCALAPPDATA%\Zed`.
    #[arg(long, value_name = "DIR", verbatim_doc_comment)]
    user_data_dir: Option<String>,

    /// The username and WSL distribution to use when opening paths. If not specified,
    /// Zed will attempt to open the paths directly.
    ///
    /// The username is optional, and if not specified, the default user for the distribution
    /// will be used.
    ///
    /// Example: `me@Ubuntu` or `Ubuntu`.
    ///
    /// WARN: You should not fill in this field by hand.
    #[cfg(target_os = "windows")]
    #[arg(long, value_name = "USER@DISTRO")]
    wsl: Option<String>,

    /// Open the project in a dev container.
    ///
    /// Automatically triggers "Reopen in Dev Container" if a `.devcontainer/`
    /// configuration is found in the project directory.
    #[arg(long)]
    dev_container: bool,

    /// Instructs zed to run as a dev server on this machine. (not implemented)
    #[arg(long)]
    dev_server_token: Option<String>,

    /// Prints system specs.
    ///
    /// Useful for submitting issues on GitHub when encountering a bug that
    /// prevents Zed from starting, so you can't run `zed: copy system specs to
    /// clipboard`
    #[arg(long)]
    system_specs: bool,

    /// Used for recording minidumps on crashes by having Zed run a separate
    /// process communicating over a socket.
    #[arg(long, hide = true)]
    crash_handler: Option<PathBuf>,

    /// Run zed in the foreground, only used on Windows, to match the behavior on macOS.
    #[arg(long)]
    #[cfg(target_os = "windows")]
    #[arg(hide = true)]
    foreground: bool,

    /// The dock action to perform. This is used on Windows only.
    #[arg(long)]
    #[cfg(target_os = "windows")]
    #[arg(hide = true)]
    dock_action: Option<usize>,

    /// Used for SSH/Git password authentication, to remove the need for netcat as a dependency,
    /// by having Zed act like netcat communicating over a Unix socket.
    #[arg(long)]
    #[cfg(not(target_os = "windows"))]
    #[arg(hide = true)]
    askpass: Option<String>,

    #[arg(long, hide = true)]
    dump_all_actions: bool,

    /// Output current environment variables as JSON to stdout
    #[arg(long, hide = true)]
    printenv: bool,

    /// Record an ETW trace. Must be run as administrator.
    #[cfg(target_os = "windows")]
    #[arg(long, hide = true)]
    record_etw_trace: bool,

    /// The PID of the Zed process to trace for heap analysis.
    #[cfg(target_os = "windows")]
    #[arg(long, hide = true, allow_hyphen_values = true)]
    etw_zed_pid: Option<i64>,

    /// Output path for the ETW trace file.
    #[cfg(target_os = "windows")]
    #[arg(long, hide = true)]
    etw_output: Option<PathBuf>,

    /// Unix socket path for IPC with the parent Zed process.
    #[cfg(target_os = "windows")]
    #[arg(long, hide = true)]
    etw_socket: Option<String>,
}

#[derive(Clone, Debug)]
enum IdType {
    New(String),
    Existing(String),
}

impl ToString for IdType {
    fn to_string(&self) -> String {
        match self {
            IdType::New(id) | IdType::Existing(id) => id.clone(),
        }
    }
}

fn parse_url_arg(arg: &str, cx: &App) -> String {
    match std::fs::canonicalize(Path::new(&arg)) {
        Ok(path) => format!("file://{}", path.display()),
        Err(_) => {
            if arg.starts_with("file://")
                || arg.starts_with(&format!("{ZED_URL_SCHEME}://"))
                || arg.starts_with("zed-cli://")
                || arg.starts_with("ssh://")
                || parse_zed_link(arg, cx).is_some()
            {
                arg.into()
            } else {
                format!("file://{arg}")
            }
        }
    }
}

fn load_embedded_fonts(cx: &App) {
    let asset_source = cx.asset_source();
    let font_paths = asset_source.list("fonts").unwrap();
    let embedded_fonts = Mutex::new(Vec::new());
    let executor = cx.background_executor();

    cx.foreground_executor().block_on(executor.scoped(|scope| {
        for font_path in &font_paths {
            if !font_path.ends_with(".ttf") {
                continue;
            }

            scope.spawn(async {
                let font_bytes = asset_source.load(font_path).unwrap().unwrap();
                embedded_fonts.lock().push(font_bytes);
            });
        }
    }));

    cx.text_system()
        .add_fonts(embedded_fonts.into_inner())
        .unwrap();
}

/// Spawns a background task to load the user themes from the themes directory.
fn load_user_themes_in_background(fs: Arc<dyn fs::Fs>, cx: &mut App) {
    cx.spawn({
        let fs = fs.clone();
        async move |cx| {
            let theme_registry = cx.update(|cx| ThemeRegistry::global(cx));
            let themes_dir = paths::themes_dir().as_ref();
            match fs
                .metadata(themes_dir)
                .await
                .ok()
                .flatten()
                .map(|m| m.is_dir)
            {
                Some(is_dir) => {
                    anyhow::ensure!(is_dir, "Themes dir path {themes_dir:?} is not a directory")
                }
                None => {
                    fs.create_dir(themes_dir).await.with_context(|| {
                        format!("Failed to create themes dir at path {themes_dir:?}")
                    })?;
                }
            }

            let mut theme_paths = fs
                .read_dir(themes_dir)
                .await
                .with_context(|| format!("reading themes from {themes_dir:?}"))?;

            while let Some(theme_path) = theme_paths.next().await {
                let Some(theme_path) = theme_path.log_err() else {
                    continue;
                };
                let Some(bytes) = fs.load_bytes(&theme_path).await.log_err() else {
                    continue;
                };

                load_user_theme(&theme_registry, &bytes).log_err();
            }

            cx.update(theme_settings::reload_theme);
            anyhow::Ok(())
        }
    })
    .detach_and_log_err(cx);
}

/// Spawns a background task to watch the themes directory for changes.
fn watch_themes(fs: Arc<dyn fs::Fs>, cx: &mut App) {
    use std::time::Duration;
    cx.spawn(async move |cx| {
        let (mut events, _) = fs
            .watch(paths::themes_dir(), Duration::from_millis(100))
            .await;

        while let Some(paths) = events.next().await {
            for event in paths {
                if fs
                    .metadata(&event.path)
                    .await
                    .ok()
                    .flatten()
                    .is_some_and(|m| !m.is_dir)
                {
                    let theme_registry = cx.update(|cx| ThemeRegistry::global(cx));
                    if let Some(bytes) = fs.load_bytes(&event.path).await.log_err()
                        && load_user_theme(&theme_registry, &bytes).log_err().is_some()
                    {
                        cx.update(theme_settings::reload_theme);
                    }
                }
            }
        }
    })
    .detach()
}

#[cfg(debug_assertions)]
fn watch_languages(fs: Arc<dyn fs::Fs>, languages: Arc<LanguageRegistry>, cx: &mut App) {
    use std::time::Duration;

    cx.background_spawn(async move {
        let languages_src = Path::new("crates/grammars/src");
        let Some(languages_src) = fs.canonicalize(languages_src).await.log_err() else {
            return;
        };

        let (mut events, watcher) = fs.watch(&languages_src, Duration::from_millis(100)).await;

        // add subdirectories since fs.watch is not recursive on Linux
        if let Some(mut paths) = fs.read_dir(&languages_src).await.log_err() {
            while let Some(path) = paths.next().await {
                if let Some(path) = path.log_err()
                    && fs.is_dir(&path).await
                {
                    watcher.add(&path).log_err();
                }
            }
        }

        while let Some(event) = events.next().await {
            let has_language_file = event
                .iter()
                .any(|event| event.path.extension().is_some_and(|ext| ext == "scm"));
            if has_language_file {
                languages.reload();
            }
        }
    })
    .detach();
}

fn dump_all_gpui_actions() {
    #[derive(Debug, serde::Serialize)]
    struct ActionDef {
        name: &'static str,
        human_name: String,
        schema: Option<serde_json::Value>,
        deprecated_aliases: &'static [&'static str],
        deprecation_message: Option<&'static str>,
        documentation: Option<&'static str>,
    }
    let mut generator = settings::KeymapFile::action_schema_generator();
    let mut actions = gpui::generate_list_of_all_registered_actions()
        .map(|action| {
            let schema = (action.json_schema)(&mut generator)
                .map(|s| serde_json::to_value(s).expect("Failed to serialize action schema"));
            ActionDef {
                name: action.name,
                human_name: command_palette::humanize_action_name(action.name),
                schema,
                deprecated_aliases: action.deprecated_aliases,
                deprecation_message: action.deprecation_message,
                documentation: action.documentation,
            }
        })
        .collect::<Vec<ActionDef>>();

    actions.sort_by_key(|a| a.name);

    let schema_definitions = serde_json::to_value(generator.definitions())
        .expect("Failed to serialize schema definitions");

    let output = serde_json::json!({
        "actions": actions,
        "schema_definitions": schema_definitions,
    });

    io::Write::write(
        &mut std::io::stdout(),
        serde_json::to_string_pretty(&output).unwrap().as_bytes(),
    )
    .unwrap();
}

#[cfg(target_os = "windows")]
fn check_for_conpty_dll() {
    use windows::{
        Win32::{Foundation::FreeLibrary, System::LibraryLoader::LoadLibraryW},
        core::w,
    };

    if let Ok(hmodule) = unsafe { LoadLibraryW(w!("conpty.dll")) } {
        unsafe {
            FreeLibrary(hmodule)
                .context("Failed to free conpty.dll")
                .log_err();
        }
    } else {
        log::warn!("Failed to load conpty.dll. Terminal will work with reduced functionality.");
    }
}

#[cfg(all(test, feature = "test-support"))]
mod tests {
    use super::*;
    use hkask_regulation::AlertSink;

    /// The `ToastAlertSink` → `spawn_alert_toast_drainer` channel bridge is
    /// the only integration point between the background tokio metacognition
    /// loop and the GPUI foreground toast dispatcher. This test pins the
    /// channel contract: a critical `AlertEvent` sent via the sink's
    /// `try_send` must be receivable by the drainer's `recv`. If the channel
    /// is closed, full, or the wrong type is sent, the alert would be
    /// silently dropped — the user would never see a critical regulation
    /// alert.
    ///
    /// This test does NOT exercise the GPUI toast dispatch (that requires a
    /// full `Workspace` fixture); it only verifies the channel hop that
    /// bridges the `Send + Sync` boundary.
    #[test]
    fn toast_alert_sink_channel_delivers_critical_events() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<hkask_regulation::AlertEvent>();
        let sink = ToastAlertSink::new(tx);

        // Non-critical alerts must be filtered by the sink — the drainer
        // should never see them.
        sink.on_alert(&hkask_regulation::AlertEvent {
            message: "warning".into(),
            critical: false,
        });
        assert!(
            rx.try_recv().is_err(),
            "non-critical alerts must not reach the channel"
        );

        // Critical alerts must be delivered.
        sink.on_alert(&hkask_regulation::AlertEvent {
            message: "well exhausted".into(),
            critical: true,
        });
        let event = rx
            .try_recv()
            .expect("critical alert should be receivable on the drainer side");
        assert_eq!(event.message, "well exhausted");
        assert!(event.critical);
    }

    /// When the drainer is dropped (app shutdown), the sink must not panic —
    /// `try_send` returns an error which the sink logs and swallows. This
    /// pins the best-effort contract: a shutting-down app must not crash on
    /// a late critical alert.
    #[test]
    fn toast_alert_sink_does_not_panic_when_channel_closed() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<hkask_regulation::AlertEvent>();
        let sink = ToastAlertSink::new(tx);
        drop(rx); // simulate app shutdown

        // This must not panic — the error is logged and swallowed.
        sink.on_alert(&hkask_regulation::AlertEvent {
            message: "late alert".into(),
            critical: true,
        });
    }
}
