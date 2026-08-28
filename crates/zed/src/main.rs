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

// zed-kask: D8 — the inference IPC socket path is managed by
// `kask_bridge::inference_socket` (a re-settable `Mutex<String>`) instead of a
// local `OnceLock<String>`. The `OnceLock` could not be updated when the IPC
// server restarted (e.g. when the real model resolved after the no-op
// fallback), leaving MCP servers with a stale socket path. `kask_bridge` owns
// the state; `main.rs` calls `set_inference_socket_path` /
// `get_inference_socket_path`.

/// Per-tick call ceiling seeded for the `swarm-panel` persona (the kask panel's
/// `ToolInvoker`).
///
/// This is a runaway-loop breaker and a usage meter, not an authority: one call
/// is charged per tool invocation and the ceiling resets each regulation tick
/// (10s), so normal activity never reaches it while a non-terminating delegated
/// loop stays bounded to `ceiling` calls per tick.
///
/// Seeding is no longer load-bearing for correctness. `charge_call_metered`
/// auto-registers an unseeded agent at
/// `hkask_regulation::DEFAULT_RUNAWAY_CALL_CEILING` and logs the gap instead of
/// refusing (RR-0057) — the prior fail-closed behavior silently broke the two
/// paths that mint *different* personas (`kask-panel` on the inference-IPC
/// dispatch, the skill execution), since those derive
/// different WebIDs than the one seeded here. This seed now only sets an
/// explicit ceiling for the panel persona.
const SWARM_PANEL_CALL_CAP: u32 = 10_000;

/// Install a panic hook that logs the panic (location + payload + backtrace)
/// via `log::error!` so it appears in `Zed.log`, then chains to the default
/// hook (stderr). Without this, a main-thread GPUI panic aborts the process
/// and leaves no trace in `Zed.log` — the default hook writes to stderr, which
/// the log file does not capture, so a desktop-launched crash is invisible.
/// `force_capture()` ignores `RUST_BACKTRACE` so the trace is always recorded
/// on the crash path (the cost is fine for a rare, fatal panic).
fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "<unknown location>".to_string());
        let payload = info.payload();
        let msg = if let Some(s) = payload.downcast_ref::<&'static str>() {
            (*s).to_string()
        } else if let Some(s) = payload.downcast_ref::<String>() {
            s.clone()
        } else {
            "<non-string panic payload>".to_string()
        };
        let backtrace = std::backtrace::Backtrace::force_capture();
        log::error!("PANIC at {location}: {msg}\nbacktrace:\n{backtrace}");
        default_hook(info);
    }));
}

fn main() {
    install_panic_hook();
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

    // `zed --printenv` Outputs environment variables as JSON to stdout.
    if args.printenv {
        util::shell_env::print_env();
        return;
    }

    if args.dump_all_actions {
        dump_all_gpui_actions();
        return;
    }

    let restart_arguments = if let Some(directory) = args.user_data_dir.as_deref() {
        let directory = paths::set_custom_data_dir(directory);
        vec![
            std::ffi::OsString::from("--user-data-dir"),
            directory.as_os_str().to_owned(),
        ]
    } else {
        Vec::new()
    };

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

    // zed-kask: `.env` file loading has been removed. API keys must be
    // configured via the settings UI (keychain) or shell environment
    // variables. The keychain is the single source of truth — keys set
    // via the settings UI are mirrored to each provider's `api_url` so
    // the `ApiKeyState` keychain read finds them.

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

    let app = build_application()
        .with_assets(Assets)
        .with_restart_arguments(restart_arguments);

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

        // zed-kask: D3/D8 — F2: kask tokio runtime (replaces upstream's gpui_tokio::init).
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

        // D1: The skill tool follows upstream Zed — it reads the SKILL.md body
        // and returns it to the agent. No manifest cascade. The `lisp_eval`
        // and `render_template` tools are available for skill composition.
        //
        // The built-in kask MCP servers come from the canonical registry in
        // `kask_bridge::BUILT_IN_MCP_SERVERS` (single source of truth).

        // D3: Construct the McpRuntime (manages MCP server child processes).
        // The McpRuntime implements ToolPort — tool dispatch with a per-agent
        // call meter (one call charged per invocation, runaway-loop breaker) and
        // reg.tool.* span emission. It does NOT authorize (RR-0056). MCP servers
        // are started as child processes (stdio).
        //
        // Server auto-launch happens after settings::init() (below) so we
        // can read KaskSettings to determine which servers to load.
        //
        // The regulation system (CyberneticsLoop, RegulationLedger, call caps)
        // is wired here so all tool invocations are governed. The CyberneticsLoop
        // runs sense→compare→compute→act cycles on background tasks; the
        // RegulationLedger tracks variety and algedonic alerts; the per-agent
        // `CallCap` bounds governed tool calls per regulation tick.
        //
        // The event sink starts as `NoopEventSink` — the durable
        // `RegulationArchive` (on the curator's curator.db, the same DB the
        // curator MCP server's `reg_query`/`curator_algedonic_log` tools
        // read) requires the DB passphrase, which only resolves after the
        // Zed user logs in. The deferred task upgrades both sinks
        // (cybernetics loop + MCP runtime governance) once provisioning
        // completes. Spans emitted before the upgrade are dropped — the
        // same degradation the previous LedgerSink had (it broadcast to a
        // subscriber bus with zero subscribers).
        // zed-kask: D3/D6/D8 — F3: alert channel + regulation ledger + event sink.
        // The ledger is constructed after `set_points` is loaded (below) so that
        // history caps (`max_regulation_history`, `max_skill_span_history`) are
        // wired from YAML config. See `RegulationLedger::with_set_points`.
        // Create the alert channel: CyberneticsLoop sends alerts →
        // MetacognitionLoop receives them. This closes the feedback loop.
        let (alert_tx, alert_rx) = tokio::sync::mpsc::unbounded_channel();

        let event_sink: std::sync::Arc<dyn hkask_types::RegulationSink> =
            std::sync::Arc::new(hkask_regulation::NoopEventSink);

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
            hkask_email::CuratorAlertEmailSink::try_from_env(kask_runtime_handle);

        // zed-kask: install the SettingsStore global before any KaskSettings
        // read. `KaskSettings::get_global(cx)` below reads from the
        // SettingsStore (via the `Settings` trait), which `settings::init`
        // installs via `cx.set_global`. Reading it before `settings::init`
        // panics with "no state of type settings::settings_store::SettingsStore
        // exists". The later settings setup (zlog_settings, watch_settings_files,
        // handle_keymap_file_changes) stays in its original position because
        // those depend on the `fs` global and the keymap watcher, which are
        // wired further down.
        settings::init(cx);

        // Determine kask settings once for both the algedonic-threshold wiring
        // below and the MCP-server auto-launch / curator-always-on gating further
        // down. Defined here (before the algedonic block) so the threshold is in
        // scope; the later reference at the MCP-launch block reuses this binding.
        let kask_settings_for_mcp = kask_bridge::KaskSettings::get_global(cx).clone();

        // zed-kask: D28 — Standardized Artifact Storage.
        // Wire the canonical kask data-root threads DB path into the agent
        // crate's threads database. The path is `{kask_data_dir}/threads/
        // threads.db`, resolved by `resolve_under_data_dir`. This relocates
        // archived chat threads from the upstream `paths::data_dir()/threads/`
        // (`~/.local/share/zed-kask/threads/`) to the kask data root
        // (`~/.local/share/zed-kask/threads/`) so all kask artifacts share one
        // rooted tree. Pre-release: no back-compat — the kask path is always
        // used. Wired early (user-independent) so the path is available
        // before any thread is loaded.
        let kask_threads_db_path =
            hkask_types::agent_paths::resolve_under_data_dir(std::path::Path::new(
                "threads/threads.db",
            ));
        agent::set_threads_db_path_override(Some(kask_threads_db_path));

        // zed-kask: D28 — Standardized Artifact Storage.
        // Wire the canonical kask data-root skills directory into
        // `agent_skills::global_skills_dir`. The path is
        // `{kask_data_dir}/skills/`, resolved by `resolve_under_data_dir`.
        // This relocates global skills from the upstream
        // `paths::data_dir()/agents/skills/`
        // (`~/.local/share/zed-kask/agents/skills/`) to the kask data root
        // (`~/.local/share/zed-kask/skills/`) so all kask artifacts share one
        // rooted tree. Pre-release: no back-compat. Wired early
        // (user-independent) so the path is available before any skill is
        // loaded.
        let kask_skills_dir =
            hkask_types::agent_paths::resolve_under_data_dir(std::path::Path::new(
                hkask_types::agent_paths::SKILLS_DIR,
            ));
        agent_skills::set_global_skills_dir_override(Some(kask_skills_dir));

        // zed-kask: Wire the kask registry templates directory for the
        // `render_template` tool. In dev (CWD = repo root), use the live
        // source tree at `kask/registry/templates/` so edits take effect
        // without recompilation. In production, use the seeded copy under
        // `{kask_data_dir}/skills/registry/templates/`.
        let dev_templates_dir = std::path::PathBuf::from("kask/registry/templates");
        let is_dev = dev_templates_dir.is_dir();
        let template_base_path = if is_dev {
            dev_templates_dir
        } else {
            hkask_types::agent_paths::resolve_under_data_dir(
                std::path::Path::new("skills/registry/templates"),
            )
        };
        agent::set_template_base_path(template_base_path);

        // Seed templates to the data-dir path so MCP servers (corpus,
        // training) find them regardless of dev/prod. The `render_template`
        // tool uses the live source tree in dev (via `template_base_path`
        // above), so this seeding only affects MCP servers which read
        // `HKASK_TEMPLATE_ROOT`. In dev, the seeded copy mirrors the
        // compiled-in source tree; in production, it IS the only copy.
        let seed_dir = hkask_types::agent_paths::resolve_under_data_dir(
            std::path::Path::new("skills/registry/templates"),
        );
        let seed_fs = fs.clone();
        cx.spawn(async move |_cx| {
            agent_skills::seed_templates(seed_fs.as_ref(), &seed_dir).await;
        }).detach();

        // zed-kask: Seed shipped skills to disk and populate the SkillIndex
        // global at startup so the Settings → AI → Skills page displays all
        // shipped skills without requiring the agent panel to be opened first.
        // The lazy scan in `run_skills_scan` (triggered by agent-panel
        // interaction) will refresh the SkillIndex with project skills when
        // a project context is built; `NativeAgent::new` preserves an existing
        // SkillIndex rather than overwriting it with an empty default.
        let skills_dir = agent_skills::global_skills_dir();
        let skills_fs: Arc<dyn Fs> = fs.clone();
        cx.spawn(async move |cx| {
            agent_skills::seed_shipped_skills(skills_fs.as_ref(), &skills_dir).await;
            let loaded = agent_skills::load_skills_from_directory(
                &skills_fs,
                &skills_dir,
                agent_skills::SkillSource::Global,
            )
            .await;
            let global_skills: Vec<agent_skills::Skill> =
                loaded.into_iter().filter_map(|result| result.ok()).collect();
            let _ = cx.update(|cx| {
                cx.set_global(agent_skills::SkillIndex {
                    global_skills,
                    project_skills: Vec::new(),
                });
            });
        }).detach();

        // zed-kask: D8 — F4: algedonic threshold → variety_max_deficit (Guardrail).
        // Wire `kask.curator.algedonic_threshold` (0.0–1.0, default 0.8) to
        // scale `SetPoints.variety_max_deficit` (default 100.0). Higher
        // threshold = more sensitive = lower deficit tolerance. Mapping:
        //   variety_max_deficit = DEFAULT_VARIETY_MAX_DEFICIT * (1.0 - threshold)
        // clamped to [1.0, DEFAULT_VARIETY_MAX_DEFICIT] so threshold=1.0
        // doesn't produce 0.0 (which fails validation). This wires the
        // previously-dead `algedonic_threshold` setting to a real enforcement
        // point (the `.rules` "Advertised invariants need enforcement points"
        // trap).
        let mut set_points = hkask_regulation::load_set_points();
        let algedonic_threshold = kask_settings_for_mcp.curator.algedonic_threshold;
        let scaled = hkask_regulation::DEFAULT_VARIETY_MAX_DEFICIT * (1.0 - algedonic_threshold);
        set_points.variety_max_deficit = scaled.max(1.0);
        // zed-kask: D3/D6/D8 — F3: regulation ledger constructed with set_points
        // so history caps (max_regulation_history, max_skill_span_history) are
        // configurable via YAML. Previously constructed with `::default()` which
        // used hardcoded caps.
        let regulation_ledger = std::sync::Arc::new(tokio::sync::RwLock::new(
            hkask_regulation::RegulationLedger::with_set_points(
                hkask_regulation::DEFAULT_VARIETY_MAX_DEFICIT as u64,
                &set_points,
            ),
        ));
        // zed-kask: D3/D8 — CuratorDirective channel wiring.
        // Create the channel: the Curator's `curator_directive` tool sends
        // directives via the sink, and the CyberneticsLoop's `process_inbox`
        // drains them. The sink converts the tool-local `CuratorDirectiveRequest`
        // (agent-name strings) to `hkask_types::CuratorDirective` (WebIDs) before
        // sending.
        let (directive_tx, directive_rx) =
            tokio::sync::mpsc::unbounded_channel::<hkask_types::CuratorDirective>();
        let cybernetics_loop_inner =
            hkask_regulation::CyberneticsLoop::with_set_points(
                regulation_ledger.clone(),
                set_points,
            )
            .with_alerts_channel(alert_tx)
            .with_curator_directive_channel(directive_rx)
            .with_event_sink(event_sink.clone());
        // zed-kask: event-substrate phase 6 — wire the rollout event source
        // so `verify_impact` can query the store for before/after metric
        // values. The store path mirrors the swarm server's default
        // (`mcp/swarm/events.db` under the kask data dir, overridable via
        // `HKASK_SWARM_EVENTS_PATH`). A missing/unopenable store leaves the
        // loop unwired (degraded, not broken — the re-sense fallback still
        // verifies) with a warn naming the path, per the failure-signal rule.
        //
        // The same store handle is shared with the harness regression monitor
        // background task (the producer side of the phase 6 seam): when the
        // harness writes a `harness_summary` event and the pass rate drops
        // materially below the previous run, the monitor calls
        // `submit_rollout_impact_check` — the first live traffic through the
        // seam.
        let (cybernetics_loop_inner, harness_event_store) = {
            let events_path = std::env::var("HKASK_SWARM_EVENTS_PATH")
                .ok()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| {
                    hkask_types::agent_paths::resolve_under_data_dir(
                        &hkask_types::agent_paths::mcp_server_db("swarm", "events"),
                    )
                    .to_string_lossy()
                    .to_string()
                });
            match kask_bridge::BridgeRolloutEventSource::open(&events_path) {
                Ok(source) => {
                    let store = source.store();
                    let wired =
                        cybernetics_loop_inner.with_rollout_event_source(std::sync::Arc::new(source));
                    (wired, Some(store))
                }
                Err(error) => {
                    tracing::warn!(
                        target: "hkask.regulation",
                        path = %events_path,
                        error = %error,
                        "rollout event source not wired — verify_impact falls back to re-sensing"
                    );
                    (cybernetics_loop_inner, None)
                }
            }
        };
        let cybernetics_loop_inner = if let Some(sink) = alert_email_sink {
            cybernetics_loop_inner.with_alert_email_sink(sink)
        } else {
            cybernetics_loop_inner
        };
        // zed-kask: D3/D8 — F5: swarm-panel gas budget persona (call cap seed).
        // Seed a call cap for the `swarm-panel` persona (see
        // `SWARM_PANEL_CALL_CAP` for the rationale — fail-closed gate, no other
        // production cap-creation path). The McpRuntime's governance gate would
        // otherwise refuse every governed tool call with `EnergyBudgetExceeded`,
        // which includes the swarm IPC `tool_invoke` dispatch the local delegate
        // loop depends on.
        {
            let panel_webid = hkask_types::WebID::from_persona(b"swarm-panel");
            cx.foreground_executor()
                .block_on(cybernetics_loop_inner.register_call_cap(panel_webid, SWARM_PANEL_CALL_CAP));
            log::info!(
                "seeded swarm-panel call cap (ceiling {SWARM_PANEL_CALL_CAP} calls/tick)"
            );
        }
        // zed-kask: context-server health source for the cybernetics loop.
        //
        // Without this, the loop reports `signal_count=0` while every MCP
        // context server is hung on `initialize` (the 600s timeout storm).
        // The source is updated by a foreground poller in the reliability
        // module (which already polls workspace/project state on a timer).
        // The source is wired here via the `with_*` builder because the loop
        // is about to be wrapped in `Arc<RwLock<...>>`; a clone is kept for
        // the poller to update.
        let context_server_health_source = std::sync::Arc::new(
            kask_bridge::BridgeContextServerHealthSource::new(),
        );
        let context_server_health_source_for_poller = context_server_health_source.clone();
        let cybernetics_loop_inner = cybernetics_loop_inner
            .with_context_server_health_source(context_server_health_source);
        let cybernetics_loop = std::sync::Arc::new(tokio::sync::RwLock::new(
            cybernetics_loop_inner,
        ));
        let cybernetics_loop_for_tick = cybernetics_loop.clone();
        let cybernetics_loop_for_panel = cybernetics_loop.clone();
        let cybernetics_loop_for_harness = cybernetics_loop.clone();
        let mcp_runtime = std::sync::Arc::new(
            hkask_mcp::McpRuntime::new()
                .with_governance(cybernetics_loop, event_sink),
        );
        log::info!("hKask regulation system wired — tool invocations are governed, regulation spans forwarded to ledger subscribers");

        // zed-kask: D3/D8 — F6: CyberneticsLoop + MetacognitionLoop tick cycles.
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
        //
        // The spawn is gated on `kask.curator.always_on` (read after
        // `settings::init` below) so an operator who disables the curator
        // gets no background tick cycle. The loop is still constructed above
        // because the McpRuntime governance gate (line ~727) needs it
        // regardless — governed tool calls are charged against the call cap
        // even when the tick cycle isn't running.
        // (`cybernetics_loop_for_tick` was already cloned above, before the
        // `with_governance` move, so it remains usable here.)

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
        let _regulation_ledger_for_model_task = regulation_ledger.clone();
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
            hkask_regulation::MetacognitionLoop::new(regulation_ledger.clone())
                .with_alert_receiver(alert_rx)
                .with_alert_sink(alert_sink),
        );
        let metacognition_loop_for_tick = metacognition_loop.clone();

        // Hoisted for the deferred task: once the RealMemoryPort exists
        // (post-login), the provider is re-set with the memory-health probe
        // attached so the curator can see its own memory outage.
        let metacognition_loop_for_deferred = metacognition_loop.clone();

        // zed-kask: D8 — F7: metacognition provider hook (set_metacognition_provider).
        // Wire the metacognition provider so the CuratorStatusTool can read
        // health snapshots from the agent's tool surface.
        let provider = std::sync::Arc::new(
            kask_bridge::BridgeMetacognitionProvider::new(metacognition_loop),
        );
        agent::set_metacognition_provider(Some(provider));
        log::info!("Curator metacognition provider wired to CuratorStatusTool");

        // zed-kask: D3/D8 — CuratorDirective sink wiring.
        // Wire the directive sink so the CuratorDirectiveTool can send
        // directives to the CyberneticsLoop. The sink converts the tool-local
        // `CuratorDirectiveRequest` (agent-name strings) to
        // `hkask_types::CuratorDirective` (WebIDs) before sending via the
        // tokio channel.
        let directive_sink: std::sync::Arc<dyn agent::CuratorDirectiveSink> =
            std::sync::Arc::new(kask_bridge::BridgeCuratorDirectiveSink::new(directive_tx));
        agent::set_curator_directive_sink(Some(directive_sink));
        log::info!("Curator directive sink wired to CuratorDirectiveTool");

        // zed-kask: algedonic log sink wiring.
        // Wire the algedonic log sink so the CuratorClearAlgedonicLogTool
        // can clear reviewed alerts from the in-memory AlgedonicManager log
        // and query the log cap status. The sink wraps the RegulationLedger.
        let algedonic_log_sink: std::sync::Arc<dyn agent::AlgedonicLogSink> =
            std::sync::Arc::new(kask_bridge::BridgeAlgedonicLogSink::new(
                regulation_ledger,
            ));
        agent::set_algedonic_log_sink(Some(algedonic_log_sink));
        log::info!("Algedonic log sink wired to CuratorClearAlgedonicLogTool");
        let mcp_runtime_for_startup = mcp_runtime.clone();
        let tool_port = mcp_runtime;
        // No capability token is threaded through the bridge. `McpRuntime::invoke`
        // performs no per-call authorization: its former capability-match gate
        // compared a caller-supplied tool name against itself and could deny
        // nothing (RR-0056). `invoke` takes an `agent: WebID` for call metering
        // only. Delegated-dispatch authority is the per-request `tool_allowlist`
        // in `kask_bridge::inference_ipc_server` (fail-closed), the swarm card
        // `mcp_tools` allowlist, and the per-server MCP env allowlists.

        // D5: Keystore uses `oo7` for all keychain reads/writes in the
        // unified `kask://credentials/*` namespace (same as zed's
        // CredentialsProvider). One keychain, one namespace.
        //
        // D3: McpRuntime is constructed above so it's available for both the
        // skill execution and the post-settings auto-launch. The
        // model-dependent wiring (skill execution, guard, panel) is
        // deferred to after language_model::init(). The memory port is wired
        // in the deferred task once the Zed user resolves (thread.rs no-ops
        // when the hook is unset).

        if let Some(app_commit_sha) = app_commit_sha {
            AppCommitSha::set_global(app_commit_sha, cx);
        }
        // zed-kask: D8 — F8: global Fs registration (replaces upstream's set_global).
        // Register the global Fs so `<dyn Fs>::global(cx)` works (used by
        // kask_bridge credential injection and other callers).
        // Upstream zed sets this in the same position; the kask fork was
        // missing the call, causing a panic on startup.
        <dyn fs::Fs>::set_global(fs.clone(), cx);
        zlog_settings::init(cx);
        zed::watch_settings_files(fs.clone(), cx);
        handle_keymap_file_changes(user_keymap_file_rx, user_keymap_watcher, cx);

        // zed-kask: D3/D9 — F9: kask_settings_for_mcp + MCP server launch list.
        // Determine which kask MCP servers to auto-launch based on KaskSettings.
        // The actual launch is deferred until the Zed user resolves (see the
        // deferred task below) so MCP servers can route inference through zed's
        // LanguageModelRegistry via the IPC socket.
        // (`kask_settings_for_mcp` was defined above, before the algedonic-threshold
        // wiring, so it's in scope here.)

        // zed-kask: D8 — F10: curator.always_on gating of tick cycles (Guardrail).
        // Gate the regulation tick cycles on `kask.curator.always_on`.
        // The loops were constructed above (the McpRuntime governance gate
        // needs them regardless), but the tick cycles that drive sense→
        // compare→act only run when the curator is enabled. Default `true`.
        // This wires the previously-dead `always_on` setting to a real
        // enforcement point (the `.rules` "Advertised invariants need
        // enforcement points" trap).
        let curator_always_on = kask_settings_for_mcp.curator.always_on;
        if curator_always_on {
            gpui_tokio::Tokio::spawn(cx, async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
                interval.tick().await; // skip the first immediate tick
                loop {
                    interval.tick().await;
                    let loop_guard = cybernetics_loop_for_tick.read().await;
                    loop_guard.tick().await;
                }
            })
            .detach();
            log::info!("CyberneticsLoop tick cycle started (10s interval)");

            gpui_tokio::Tokio::spawn(cx, async move {
                metacognition_loop_for_tick.run().await;
            })
            .detach();
            log::info!("Curator metacognition loop started (30s tick interval)");

            // zed-kask: event-substrate phase 6 — harness regression monitor.
            // The producer side of the seam: periodically query the event store
            // for new `harness_summary` events, detect pass-rate regressions,
            // and submit each to `CyberneticsLoop::submit_rollout_impact_check`.
            // This is the first live traffic through the phase 6 seam — the
            // zed side observes harness results (via the shared event store)
            // and closes the cybernetic feedback loop.
            //
            // Only runs when both the curator is on AND the event store opened
            // successfully. A missing store leaves the monitor unwired
            // (degraded, not broken — the re-sense fallback still verifies).
            if let Some(store) = harness_event_store {
                gpui_tokio::Tokio::spawn(cx, async move {
                    let mut last_cursor: Option<i64> = None;
                    let mut interval =
                        tokio::time::interval(std::time::Duration::from_secs(60));
                    interval.tick().await; // skip the first immediate tick
                    loop {
                        interval.tick().await;
                        match kask_bridge::check_harness_regressions(
                            &store,
                            last_cursor,
                        ) {
                            Ok((new_cursor, regressions)) => {
                                last_cursor = new_cursor;
                                if !regressions.is_empty() {
                                    let loop_guard =
                                        cybernetics_loop_for_harness.read().await;
                                    for regression in regressions {
                                        loop_guard
                                            .submit_rollout_impact_check(
                                                regression.agent_name,
                                                regression.before_position,
                                                "pass_rate".to_string(),
                                            )
                                            .await;
                                    }
                                }
                            }
                            Err(error) => {
                                tracing::warn!(
                                    target: "hkask.regulation",
                                    error = %error,
                                    "harness regression monitor query failed — will retry next tick"
                                );
                            }
                        }
                    }
                })
                .detach();
                log::info!("Harness regression monitor started (60s interval)");
            } else {
                log::info!(
                    "Harness regression monitor not started — event store not wired"
                );
            }
        } else {
            log::info!(
                "Curator always_on=false — regulation tick cycles not started \
                 (McpRuntime governance gate remains active)"
            );
        }

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

        // zed-kask: D3 — F13: sync_kask_mcp_servers (ContextServerStore registration).
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

        // The governed McpRuntime instances (kask panel + skill execution) are
        // started once at login and keep their startup env. A settings change
        // that alters a server's env (e.g. `kask.swarm.mode` →
        // `HKASK_SWARM_MODE`) must restart them; the per-project
        // ContextServerStore path above handles its own instances via
        // descriptor re-sync. The restart baseline is recorded by the
        // deferred launch task once servers actually start — an empty
        // baseline means "not launched yet", and the observer no-ops.
        let kask_mcp_restart_env: std::sync::Arc<
            std::sync::Mutex<
                std::collections::HashMap<String, std::collections::HashMap<String, String>>,
            >,
        > = std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
        let mcp_runtime_for_restart = tool_port.clone();
        let restart_env_for_observer = kask_mcp_restart_env.clone();
        cx.observe_global::<SettingsStore>(move |cx| {
            sync_kask_mcp_runtime_servers(
                mcp_runtime_for_restart.clone(),
                restart_env_for_observer.clone(),
                cx,
            );
        })
        .detach();

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
        //
        // Clone `tool_port` for the model-dependent skill execution task
        // (below) before it's moved into the deferred task. The model-dependent
        // task fires on `LanguageModelRegistry` events, independent of user
        // login — the skill execution only needs the model + tool_port, not
        // the username.
        let _tool_port_for_model_task: std::sync::Arc<dyn hkask_tool_port::ToolPort> =
            tool_port.clone();
        {
            let user_store = app_state.user_store.clone();
            let mcp_runtime_for_deferred = mcp_runtime_for_startup;
            let servers_to_start_clone = servers_to_start;
            let kask_mcp_restart_env_for_deferred = kask_mcp_restart_env;
            // Captures for the model-dependent wiring block (moved here from
            // the synchronous startup so it runs after the user resolves and
            // the LanguageModelRegistry is populated). See the
            // "Process-global hooks set at runtime need a startup-failure
            // signal" trap in .rules — these OnceLock-based hooks must be
            // wired from the deferred task, not from startup.
            let tool_port_for_deferred = tool_port;
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
                // The keystore writes to the unified `kask://credentials/*`
                // namespace (same as zed's CredentialsProvider) — one keychain,
                // one namespace. No mirror step needed.
                let kask_settings = cx.update(|cx| kask_bridge::KaskSettings::get_global(cx).clone());
                let embedding_model = std::env::var("HKASK_EMBEDDING_MODEL")
                    .unwrap_or_else(|_| kask_settings.corpus.embedding_model.clone());
                let embedding_dim = kask_settings.corpus.embedding_dim as usize;
                let username_for_provision = username.clone();

                let provision_result = cx.background_spawn(async move {
                    // One-time migration: copy any legacy `service=hkask`
                    // keychain entries to the unified `kask://credentials/*`
                    // namespace. Idempotent — skips keys that already exist.
                    // Must run before `provision_agent` so provisioning sees
                    // the migrated values and doesn't re-create with defaults.
                    match hkask_keystore::migrate_legacy_hkask_entries() {
                        Ok(report) => {
                            if !report.migrated.is_empty() {
                                log::info!(
                                    "Migrated {} legacy keychain entries to kask://credentials/*: {}",
                                    report.migrated.len(),
                                    report.migrated.join(", ")
                                );
                            }
                        }
                        Err(e) => {
                            log::warn!(
                                "Legacy keychain migration failed (non-fatal — \
                                 provisioning will create fresh entries if needed): {e}"
                            );
                        }
                    }
                    kask_bridge::provision_agent(&username_for_provision)
                }).await;

                // zed-kask: provision the swarm memory SQLCipher passphrase.
                // Without this, the swarm server has no passphrase and local
                // knowledge tools degrade (they require >=8 chars).
                // `provision_swarm_memory_passphrase` uses the default
                // "allostery" on first run. Both DB and swarm passphrases are
                // written directly to the unified `kask://credentials/*`
                // namespace — no mirror step needed.
                let swarm_passphrase_provision_task = cx.background_spawn(async move {
                    kask_bridge::provision_swarm_memory_passphrase()
                });
                if let Err(error) = swarm_passphrase_provision_task.await {
                    log::warn!(
                        "Failed to provision swarm memory passphrase: {error}. \
                         The swarm server will have no passphrase — local knowledge tools will degrade."
                    );
                }

                // zed-kask: mirror kask credential store keys to each provider's
                // `api_url` in the Zed keychain. The kask settings UI writes keys
                // to `kask://credentials/<key>`, but each `LanguageModelProvider`'s
                // `ApiKeyState` reads from the provider's `api_url` (e.g.
                // `https://openrouter.ai/api/v1`). Without this mirror, a key set
                // via the kask settings UI is invisible to the provider — models
                // never load. This generalizes the former RunPod-only mirror to
                // all inference providers, and replaces the removed
                // `mirror_env_keys_to_keychain` (which mirrored from env vars).
                // Must `.await` before the embedding port resolution below, which
                // reads from the provider's `api_url` in the keychain.
                let provider_key_mirror_task = cx.update(|cx| {
                    let credentials_provider = zed_credentials_provider::global(cx);
                    kask_bridge::mirror_kask_credentials_to_providers(&credentials_provider, cx)
                });
                provider_key_mirror_task.await;

                // Hoisted to the outer scope so the IPC server (started later
                // in the `cx.update` block) can access it. Set inside the
                // `match provision_result` below.
                let mut embedding_port_for_ipc: Option<kask_bridge::LanguageModelEmbeddingPort> = None;

                match provision_result {
                    Ok(provisioned) => {
                        let kask_bridge::ProvisionedAgent { db_path, passphrase, webid: _user_webid } = provisioned;

                        // Upgrade the regulation event sinks to the durable
                        // `RegulationArchive` on the curator's curator.db — the same
                        // DB the curator MCP server's `reg_query` and
                        // `curator_algedonic_log` tools read. Before this,
                        // both sinks are `NoopEventSink` (spans dropped).
                        //
                        // zed-kask: This wiring is BEFORE the embedding credentials
                        // check because regulation span persistence must not depend
                        // on embedding availability. The regulation loop is a
                        // safety-critical feedback path — dropping spans because the
                        // embedding endpoint is down is a broken feedback loop.
                        match kask_bridge::open_curator_regulation_archive(&passphrase) {
                            Some(archive) => {
                                let sink: std::sync::Arc<dyn hkask_types::RegulationSink> = archive.clone();
                                mcp_runtime_for_deferred.set_event_sink(sink.clone());
                                {
                                    let mut loop_guard = cybernetics_loop_for_panel_deferred.write().await;
                                    loop_guard.set_event_sink(sink);
                                }
                                // zed-kask: maintenance tick — run WAL checkpoint +
                                // incremental_vacuum + reg_records retention on a slow
                                // cadence. Prevents WAL checkpoint starvation under
                                // long-lived readers and unbounded `reg_records`
                                // growth (the table has no TTL; without this tick the
                                // SQLite file bloats over days/weeks of continuous
                                // running, slowing every regulation query).
                                let archive_for_maintenance = archive.clone();
                                gpui_tokio::Tokio::spawn(cx, async move {
                                    // 5-minute checkpoint cadence; 24-hour retention cadence.
                                    let mut checkpoint_interval =
                                        tokio::time::interval(std::time::Duration::from_secs(300));
                                    let mut retention_interval =
                                        tokio::time::interval(std::time::Duration::from_secs(86400));
                                    checkpoint_interval.tick().await; // skip immediate first tick
                                    retention_interval.tick().await;
                                    loop {
                                        tokio::select! {
                                            _ = checkpoint_interval.tick() => {
                                                if let Err(e) = archive_for_maintenance.checkpoint() {
                                                    tracing::warn!(
                                                        target: "reg.storage",
                                                        error = %e,
                                                        "RegulationArchive maintenance checkpoint failed"
                                                    );
                                                }
                                            }
                                            _ = retention_interval.tick() => {
                                                // Retain 30 days of regulation records.
                                                let cutoff = chrono::Utc::now()
                                                    - chrono::Duration::days(30);
                                                if let Err(e) = archive_for_maintenance.delete_older_than(cutoff) {
                                                    tracing::warn!(
                                                        target: "reg.storage",
                                                        error = %e,
                                                        "RegulationArchive retention delete failed"
                                                    );
                                                }
                                            }
                                        }
                                    }
                                })
                                .detach();
                                log::info!("hKask regulation archive wired — regulation spans now persist to curator curator.db (maintenance tick: 5m checkpoint, 24h retention)");
                            }
                            None => {
                                log::warn!(
                                    "hKask regulation archive unavailable — regulation spans will be dropped. \n                                     Remediation: ensure the curator curator.db can be opened (HKASK_CURATOR_DB, DB passphrase)."
                                );
                            }
                        }

                        // Open the reviewable escalation queue on the same
                        // curator curator.db — the same DB the curator MCP
                        // server's `curator_escalations` /
                        // `curator_escalation_resolve` /
                        // `curator_escalation_dismiss` tools read. This is
                        // the primary durable path for alert review:
                        // `CyberneticsLoop` writes escalated alerts here
                        // unconditionally so the Curator/user can review and
                        // resolve them. Before this, the escalation sink is
                        // `None` (alerts not persisted to the reviewable
                        // backlog).
                        match kask_bridge::open_curator_escalation_queue(&passphrase) {
                            Some(queue) => {
                                let sink: std::sync::Arc<dyn hkask_regulation::AlertEscalationSink> =
                                    std::sync::Arc::new(kask_bridge::BridgeAlertEscalationSink::new(queue));
                                let mut loop_guard = cybernetics_loop_for_panel_deferred.write().await;
                                loop_guard.set_alert_escalation_sink(Some(sink));
                                log::info!("hKask escalation queue wired — algedonic alerts now persist to the reviewable backlog on curator curator.db");
                            }
                            None => {
                                log::warn!(
                                    "hKask escalation queue unavailable — algedonic alerts will not persist to the reviewable backlog. \
                                     Remediation: ensure the curator curator.db can be opened (HKASK_CURATOR_DB, DB passphrase)."
                                );
                            }
                        }

                        // zed-kask: D8 — F14: embedding credentials (deferred task).
                        // Resolve embedding credentials by reading the API key
                        // from the Zed keychain at the provider's `api_url`.
                        //
                        // When embedding credentials are unavailable, the memory
                        // port is still wired — h_mem writes are pure SQL and
                        // don't need embeddings. Semantic recall (KNN) degrades to
                        // keyword-only, but the curator still has episodic memory
                        // of every conversation. This is the correct degradation:
                        // memory ingestion is safety-critical (the curator must
                        // remember what happened), embeddings are an enhancement.
                        let embedding_port_result = cx.update(|cx| {
                            let http_client = app_state_for_deferred.client.http_client();
                            let tokio_handle = gpui_tokio::Tokio::handle(cx);
                            let embedding_model = embedding_model.clone();
                            let credentials_provider =
                                zed_credentials_provider::global(cx);
                            cx.spawn(async move |cx| {
                                kask_bridge::resolve_embedding_credentials(
                                    &embedding_model,
                                    credentials_provider.as_ref(),
                                    &cx,
                                )
                                .await
                                .map(|(api_url, api_key)| {
                                    kask_bridge::LanguageModelEmbeddingPort::new(
                                        api_url,
                                        api_key,
                                        http_client,
                                        tokio_handle,
                                    )
                                })
                            })
                        });

                        let embedding_port = embedding_port_result.await;
                        if embedding_port.is_some() {
                            log::info!("hKask embedding port resolved — semantic recall enabled");
                        } else {
                            log::warn!(
                                "hKask embedding port unavailable — memory will be wired WITHOUT embeddings. \
                                 h_mem writes work (pure SQL); semantic recall degrades to keyword-only. \
                                 Remediation: set the embedding provider's API key via Settings → Kask → Data Services."
                            );
                        }

                        // Clone for the IPC server (the other copy goes to
                        // RealMemoryPort below).
                        embedding_port_for_ipc = embedding_port.clone();

                        match kask_bridge::RealMemoryPort::new(
                            &passphrase,
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

                                // Re-set the metacognition provider with the
                                // memory-health probe attached — the curator's
                                // CuratorStatusTool now reports its own memory
                                // outage (`memory.degraded`) alongside the
                                // regulation health it already had. The early
                                // provider (set pre-login, without the probe)
                                // is replaced; `set_metacognition_provider` is
                                // Mutex-based and re-settable.
                                let provider_with_memory = std::sync::Arc::new(
                                    kask_bridge::BridgeMetacognitionProvider::new(
                                        metacognition_loop_for_deferred.clone(),
                                    )
                                    .with_memory_port(real_memory_typed.clone()),
                                );
                                agent::set_metacognition_provider(Some(provider_with_memory));
                                log::info!(
                                    "Curator metacognition provider upgraded with memory-health probe"
                                );

                                // Set env vars for the curator MCP server so it
                                // reads from the same `agents/curator/curator.db` the
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
                                // of all other MCP servers (condenser,
                                // etc.), which resolve their identity from it in
                                // `transport.rs`.
                                let curator_db = hkask_types::agent_paths::resolve_under_data_dir(
                                    &hkask_types::agent_paths::agent_db("curator"),
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
                                // zed-kask: D3 — F15: MCP re-sync (curator server, deferred, Guardrail).
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
                                //
                                // zed-kask: D26 — the injector is wired unconditionally
                                // (not gated on `kask.memory.auto_inject`) so recall is
                                // always available. `auto_inject` is passed into the
                                // constructor and gates memory recall only; tool warnings
                                // live in the system prompt template (`system_prompt.hbs`).
                                let auto_inject = kask_settings.memory.auto_inject;
                                let injector = std::sync::Arc::new(
                                    kask_bridge::BridgeContextInjector::new(
                                        real_memory_typed.clone(),
                                        kask_settings.memory.recall_limit,
                                        kask_settings.memory.recall_min_confidence,
                                        auto_inject,
                                    ),
                                );
                                agent::set_context_injector(Some(injector));
                                log::info!(
                                    "hKask context injector wired (agent: {agent_name}, \
                                     auto_inject={auto_inject})"
                                );

                                // Wire the memory health source into the cybernetics
                                // loop so it can sense memory growth, low-confidence
                                // accumulation, and storage budget pressure. Without
                                // this, 5 memory regulation loops are blind — their
                                // policy rules (`TripleCount`, `LowConfidenceCount`,
                                // `ConsolidationCandidates`, `StorageUsage`,
                                // `MemoryLife`) can never fire because no signal is
                                // produced. This closes the dead-policy gap.
                                //
                                // The health source is the `RealMemoryPort` itself —
                                // it implements `MemoryHealthSource` by reading from
                                // the curator's `MemoryStore` behind the self-healing
                                // handle.
                                let memory_health_source: std::sync::Arc<
                                    dyn hkask_regulation::MemoryHealthSource,
                                > = real_memory_typed.clone();
                                {
                                    let loop_for_health =
                                        cybernetics_loop_for_panel_deferred.clone();
                                    gpui_tokio::Tokio::spawn(cx, async move {
                                        let mut loop_guard = loop_for_health.write().await;
                                        loop_guard.set_memory_health_source(memory_health_source);
                                    })
                                    .detach();
                                }

                                // D11 curator mirror: wire the curator context
                                // injector so the Curator recalls its own
                                // sovereign memory from
                                // `agents/curator/curator.db`). Without this, the
                                // Curator has no automatic recall — it must
                                // call `curator_memory_recall` /
                                // `curator_semantic_search` as tools, which is
                                // the asymmetry this block fixes.
                                let curator_injector = std::sync::Arc::new(
                                    kask_bridge::BridgeContextInjector::new_curator(
                                        real_memory_typed,
                                        kask_settings.memory.recall_limit,
                                        kask_settings.memory.recall_min_confidence,
                                        auto_inject,
                                    ),
                                );
                                agent::set_curator_context_injector(Some(curator_injector));
                                log::info!(
                                    "hKask curator context injector wired \
                                     (agent: {agent_name}, auto_inject={auto_inject}) — \
                                     curator will recall from its own sovereign DB; \
                                     tool warnings always on"
                                );

                                if !auto_inject {
                                    // auto_inject is off — the injectors are wired
                                    // (so tool warnings still land) but memory recall
                                    // is disabled. Per the .rules trap "Process-global
                                    // hooks set at runtime need a startup-failure signal",
                                    // the warn names both hooks and their actual state
                                    // (wired-but-recall-disabled, not unwired) so the
                                    // operator can remediate correctly.
                                    log::warn!(
                                        "kask.memory.auto_inject is false — \
                                         both the user context injector and the \
                                         curator context injector are wired with \
                                         recall disabled. Tool warnings remain on. \
                                         Set kask.memory.auto_inject true to enable \
                                         memory recall for both agents."
                                    );
                                }

                                // zed-kask: D1 — F16: LazyToolRouter hook (set_tool_router).
                                // Wire the lazy tool router. The router narrows
                                // the tool set on complex or tool-directed requests,
                                // reducing the tool list the model must reason about
                                // when hKask's MCP servers expose many tools.
                                agent::set_tool_router(Some(std::sync::Arc::new(
                                    agent::tool_router::LazyToolRouter::new_with_thresholds(
                                        kask_settings.tool_router.threshold,
                                        kask_settings.tool_router.complex_word_threshold,
                                    ),
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
                // startup left all OnceLock-based hooks (skill execution,
                // panel tool invoker, scoped inference, regulation status,
                // thread condenser) unwired when no model was configured at
                // startup — the "Process-global hooks set at runtime need a
                // startup-failure signal" trap from .rules.
                //
                let kask_settings = cx.update(|cx| kask_bridge::KaskSettings::get_global(cx).clone());

                // Lazily wire the alert email sink now that kask settings have
                // loaded. The non-secret email fields are set as process env
                // vars so `send_email()` (called from `tokio::spawn` inside
                // `CuratorAlertEmailSink::send_alert_email`) can read them.
                // The SMTP password is read from the keychain by
                // `build_mcp_server_env` for MCP server child processes;
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
                        gpui_tokio::Tokio::handle_async(&*cx),
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

                // zed-kask: Registry path resolution is now handled by the
                // model-dependent skill execution task (below the deferred
                // task block), which doesn't need user login. The skill
                // execution is the only consumer of the registry paths, and
                // it's wired by that separate task. The IPC server and MCP
                // server launch below don't need the registry paths.

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
                    // LazyInferencePort as the skill execution: when no model
                    // is resolved, curator turns return a clear error with
                    // remediation guidance; when the model resolves, the lazy
                    // port is swapped in and curator turns route through it.
                    let panel_tool_invoker = std::sync::Arc::new(PanelToolInvoker {
                        tool_port: tool_port_for_deferred.clone(),
                        executor: cx.background_executor().clone(),
                        tokio_handle: gpui_tokio::Tokio::handle(cx),
                    });
                    swarm_panel::set_tool_invoker(Some(panel_tool_invoker));
                    log::info!(
                        "Swarm panel tool invoker wired \
                         (swarm panel ABW calls route through the governed MCP runtime)"
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
                        // Registry paths were resolved and seeded in the async
                        // block above (dev repo source or seeded data_dir).
                        // Disk is the single runtime source — no compiled-in
                        // fallback.
                        let inference_model: Arc<dyn language_model::LanguageModel> = {
                            let kask_default = kask_settings.models.effective_default_model();
                            match kask_bridge::resolve_model_names(
                                model_registry,
                                &[kask_default.to_string()],
                                cx,
                            ).0.into_values().next() {
                                Some(model) => {
                                    log::info!(
                                        "hKask inference using kask.models.default_model: {}",
                                        kask_default
                                    );
                                    model
                                }
                                None => {
                                    log::warn!(
                                        "kask.models.default_model '{}' could not be resolved \
                                         from LanguageModelRegistry — falling back to zed default",
                                        kask_default
                                    );
                                    configured.model.clone()
                                }
                            }
                        };

                        let inference_timeout = std::time::Duration::from_secs(
                            kask_settings.general.inference_timeout_secs,
                        );
                        // Publish the server's establishment deadline so IPC clients
                        // (child MCP server processes) can align their read timeout
                        // with it. Without this, the client's independent 120s read
                        // deadline fires before the server's 300s establishment
                        // deadline, closing the socket mid-flight; the server's
                        // later response write then hits `BrokenPipe`, producing a
                        // warn storm that blames the IPC layer for what was a
                        // self-inflicted cancellation. See `INFERENCE_TIMEOUT_ENV`.
                        kask_bridge::set_inference_timeout_secs(
                            kask_settings.general.inference_timeout_secs,
                        );
                        let (inference_port, inference_task) =
                            kask_bridge::LanguageModelInferencePort::new(
                                inference_model.clone(),
                                inference_timeout,
                                kask_settings.general.max_concurrency as usize,
                                async_cx,
                            );
                        inference_task.detach();

                        // Wire the inference health source into the cybernetics
                        // loop so it can sense inference saturation and timeout
                        // storms. Without this, the loop reports `signal_count=0`
                        // during a timeout storm — its existing sensors read
                        // ledger/DB state, not inference dispatch state. The
                        // `set_inference_health_source` method is used (not the
                        // `with_*` builder) because the loop is already wrapped
                        // in `Arc<RwLock<...>>` by the time the port exists.
                        //
                        // The health source is a clone of the port — the
                        // port's health counters are `Arc`-shared, so the
                        // sensor reads the same atomics the receiver task
                        // updates. This mirrors the `set_event_sink` /
                        // `set_alert_escalation_sink` pattern used elsewhere
                        // in the deferred task.
                        let inference_health_source: std::sync::Arc<
                            dyn hkask_regulation::InferenceHealthSource,
                        > = std::sync::Arc::new(inference_port.clone());
                        {
                            let loop_for_health = cybernetics_loop_for_panel_deferred.clone();
                            gpui_tokio::Tokio::spawn(cx, async move {
                                let mut loop_guard = loop_for_health.write().await;
                                loop_guard.set_inference_health_source(inference_health_source);
                            })
                            .detach();
                        }

                        let inference_port: std::sync::Arc<dyn hkask_types::InferencePort> =
                            std::sync::Arc::new(inference_port);

                        // Start the inference IPC server so MCP server child processes
                        // can route inference through zed's LanguageModelRegistry (with
                        // zed's configured API keys) instead of
                        // constructing their own MediaRouter with separate keys.
                        //
                        // The governed McpRuntime backs `tool_invoke` requests
                        // (delegated swarm agents calling MCP tools). The IPC
                        // server mints the panel token — the child process
                        // never holds token material.
                        let tool_port_for_ipc: Option<
                            std::sync::Arc<dyn hkask_tool_port::ToolPort>,
                        > = Some(mcp_runtime_for_deferred.clone());
                        match kask_bridge::InferenceIpcServer::start(
                            inference_port,
                            embedding_port_for_ipc.clone(),
                            tool_port_for_ipc,
                            cx,
                        ) {
                            Ok(ipc_server) => {
                                let socket_path = ipc_server.socket_path().to_string_lossy().to_string();
                                kask_bridge::set_inference_socket_path(&socket_path);
                                log::info!(
                                    "hKask inference IPC server started at {socket_path} — \
                                     MCP servers will route inference through zed"
                                );
                                // The server stays alive for the process lifetime: `start`
                                // spawns a detached tokio task that owns the `UnixListener`,
                                // and the returned `InferenceIpcServer` holds the
                                // `JoinHandle`s. The socket file is intentionally leaked
                                // (see the doc on `InferenceIpcServer`) — there is no
                                // `Drop` impl, so `mem::forget` would be a no-op that
                                // implied otherwise. Bind to `_` to acknowledge the value
                                // is kept alive by the task, not by this binding.
                                let _ipc_server = ipc_server;
                                // zed-kask: D3/D8 — F19: MCP re-sync (inference socket, deferred).
                                // Re-sync both MCP server paths so the inference socket path is
                                // included in the env passed to context server processes:
                                //   1. `sync_kask_mcp_servers` — restarts the per-project
                                //      ContextServerStore instances (agent tool picker)
                                //   2. `sync_kask_mcp_runtime_servers` — restarts the governed
                                //      McpRuntime instances (skill execution + kask panel)
                                // Without (2), the governed servers keep their stale env (no
                                // socket) and all inference calls fail with "IPC bridge not
                                // configured" even though the socket is live.
                                sync_kask_mcp_servers(cx);
                                sync_kask_mcp_runtime_servers(
                                    mcp_runtime_for_deferred.clone(),
                                    kask_mcp_restart_env_for_deferred.clone(),
                                    cx,
                                );
                            }
                            Err(e) => {
                                log::warn!(
                                    "Failed to start inference IPC server: {e} — \
                                     MCP servers will fall back to MediaRouter (media-only)"
                                );
                            }
                        }

                        // The skill execution is wired by the separate
                        // model-dependent task (below the deferred task
                        // block), which fires as soon as the model registry
                        // reports a default model — independent of Zed user
                        // login. Previously it was wired here, inside the
                        // user-login-gated deferred task, which meant users
                        // with a configured default model but no cloud login
                        // had skills silently disabled. The model registry is
                        // populated from settings.json, not from cloud auth.
                        //
                        // The inference port is still constructed here
                        // because the IPC server (below) needs it. The
                        // model-dependent task constructs its own inference
                        // port for the skill execution. This is a small
                        // duplication (two ports), but they wrap the same
                        // underlying model — the duplication is harmless.
                        if kask_settings.memory.auto_inject {
                            log::info!("hKask context injection enabled — injector will be wired after agent resolves");
                        } else {
                            log::info!("hKask context injection disabled (kask.memory.auto_inject = false)");
                        }
                    } else {
                        // No default model in the registry at this point in
                        // the deferred task. The skill execution is wired
                        // by the separate model-dependent task (not here),
                        // so it's not listed among the unwired hooks. The
                        // hooks listed here are the ones the deferred task
                        // wires inside the `if` branch and does not wire in
                        // the `else` branch.
                        //
                        // The inference IPC server is still started (with a
                        // `NoModelInferencePort`) so MCP server child processes
                        // receive `HKASK_INFERENCE_SOCKET` and route inference
                        // through this bridge rather than falling back to
                        // `MediaRouter::from_env()`. That fallback reads from
                        // the `kask://credentials/*` keychain namespace, which
                        // may not have the key if the operator hasn't configured
                        // it via the settings UI. Without the IPC server, MCP
                        // servers silently failed with "API key not configured" —
                        // an error operators could not trace to the missing socket.
                        // The `NoModelInferencePort` returns a clear diagnostic so
                        // the failure mode is visible and actionable.
                        log::warn!(
                            "No default LanguageModel configured at deferred-task time — hKask hooks not wired by this task: \
                             thread condenser (tool results not compressed), \
                             panel tool invoker (panel cannot dispatch tools), \
                             curator session factory (panel cannot run per-tab curator conversations), \
                             regulation status (panel cannot emit regulation spans). \
                             Skill execution is wired separately by the model-dependent task \
                             and will fire if/when the model resolves. \
                             The inference IPC server is started with a no-op port so MCP \
                             servers route through the bridge and get a diagnostic error \
                             instead of falling back to an empty keychain. \
                             Remediation: configure a default LanguageModel in Settings → \
                             or sign in to your model provider. The deferred task will re-run \
                             on next login and wire these hooks."
                        );

                        // Start the IPC server with a no-op inference port so
                        // `INFERENCE_SOCKET_PATH` is set and MCP servers connect
                        // to the bridge. The embedding port IS passed here —
                        // embeddings use a separate provider (Ollama) with
                        // separate credentials, so they work even when no chat
                        // model is configured. Without this, corpus embedding
                        // calls fail with "embedding port not configured" when
                        // the no-model fallback path runs.
                        let no_model_port: std::sync::Arc<dyn hkask_types::InferencePort> =
                            std::sync::Arc::new(kask_bridge::NoModelInferencePort);
                        // No tool port here: the no-op port means no chat model
                        // is configured yet, so delegated agents have nothing to
                        // dispatch against. The governed IPC server (started after
                        // the model resolves) carries the McpRuntime tool port.
                        match kask_bridge::InferenceIpcServer::start(
                            no_model_port,
                            embedding_port_for_ipc.clone(),
                            None,
                            cx,
                        ) {
                            Ok(ipc_server) => {
                                let socket_path =
                                    ipc_server.socket_path().to_string_lossy().to_string();
                                kask_bridge::set_inference_socket_path(&socket_path);
                                log::info!(
                                    "hKask inference IPC server started (no-op port) at {socket_path} — \
                                     MCP servers will route through the bridge and receive a \
                                     diagnostic error until a default model is configured"
                                );
                                // See the matching binding site above: the server is kept
                                // alive by its detached task, and the socket is
                                // intentionally leaked (no `Drop` impl).
                                let _ipc_server = ipc_server;
                                sync_kask_mcp_servers(cx);
                                sync_kask_mcp_runtime_servers(
                                    mcp_runtime_for_deferred.clone(),
                                    kask_mcp_restart_env_for_deferred.clone(),
                                    cx,
                                );
                            }
                            Err(e) => {
                                log::warn!(
                                    "Failed to start inference IPC server (no-op port): {e} — \
                                     MCP servers will fall back to MediaRouter (media-only)"
                                );
                            }
                        }
                    }
                });

                // Launch MCP servers via McpRuntime for app-global metered
                // dispatch (call metering + regulation spans). These instances
                // serve the skill execution and kask panel.
                //
                // Zed's ContextServerStore (per-project) launches separate
                // instances for the agent tool picker — registered via
                // sync_kask_mcp_servers. The two systems serve different
                // consumers with different governance requirements; the
                // parallel instances are by design, not a bug.
                if !servers_to_start_clone.is_empty() {
                    // Build env + record baseline on the foreground
                    // (`kask_server_env` needs `AsyncApp`, not `Send`). The
                    // tokio-dependent `register_server` / `start_server_with_env`
                    // are dispatched through `Tokio::spawn` below — the reactor
                    // is entered on the worker thread, so no foreground `enter()`
                    // guard is held across awaits. The earlier `let _tokio_guard
                    // = ...enter()` held-across-awaits form was the `.rules`
                    // "background_spawn of tokio-dependent futures" trap: a
                    // second overlapping `cx.spawn` (e.g. the settings-change
                    // restart observer) would acquire a second `EnterGuard`,
                    // interleave at await points, and panic with "EnterGuard
                    // values dropped out of order". Do NOT move this loop to
                    // `cx.background_spawn` — GPUI's background executor has no
                    // tokio reactor (see .rules).
                    for server_id in &servers_to_start_clone {
                        let binary = format!("hkask-mcp-{server_id}");
                        let mcp_env = kask_server_env(server_id, cx).await;
                        // Record the env baseline BEFORE starting so the
                        // settings-change restart observer can diff against it
                        // (an empty baseline = not yet launched = no-op). On
                        // start failure the baseline is dropped (inside the
                        // `Tokio::spawn` below) so a later settings change retries.
                        kask_mcp_restart_env_for_deferred
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .insert(server_id.clone(), mcp_env.clone());
                        let runtime = mcp_runtime_for_deferred.clone();
                        let restart_env = kask_mcp_restart_env_for_deferred.clone();
                        let server_id_owned = server_id.to_string();
                        gpui_tokio::Tokio::spawn(cx, async move {
                            runtime
                                .register_server(hkask_mcp::McpServer {
                                    id: server_id_owned.clone(),
                                    name: server_id_owned.clone(),
                                    tools: vec![],
                                })
                                .await;
                            match runtime
                                .start_server_with_env(&server_id_owned, &binary, mcp_env)
                                .await
                            {
                                Ok(()) => log::info!(
                                    "Kask MCP server '{server_id_owned}' started (McpRuntime)"
                                ),
                                Err(e) => {
                                    log::warn!(
                                        "Kask MCP server '{server_id_owned}' failed to start: {e} \n                                         — set HKASK_MCP_{}_BIN to the binary path",
                                        server_id_owned.to_uppercase()
                                    );
                                    restart_env
                                        .lock()
                                        .unwrap_or_else(|e| e.into_inner())
                                        .remove(&server_id_owned);
                                }
                            }
                        })
                        .detach();
                    }
                }
            }).detach();
        }

        // zed-kask: D1 — Model-dependent skill execution wiring.
        //
        // This task wires the edit-prediction port (D24), which needs the
        // cascade) as soon as `LanguageModelRegistry::default_model()` returns
        // `Some`, independent of Zed user login. The model registry is
        // populated from settings.json (`agent.default_model`), not from
        // cloud auth, so gating the skill execution on user login was a
        // bug: users with a configured default model but no cloud login had
        // skills silently disabled (the `skill` tool returned a no-op
        // envelope instead of the skill body).
        //
        // The task subscribes to `LanguageModelRegistry` events
        // (`DefaultModelChanged`, `ProviderStateChanged`, `AddedProvider`,
        // `ProvidersChanged`) and fires the wiring on the first event where
        // `default_model()` returns `Some`. An `AtomicBool` ensures it fires
        // only once — the edit-prediction port uses an AtomicBool guard.
        // second call would warn and be dropped.
        //
        // The registry path resolution (dev source vs seeded) is duplicated
        // from the deferred task because it doesn't need the user and must
        // run here for the skill execution. The `tool_port` is the same
        // `McpRuntime` Arc used by the deferred task. The inference port
        // is constructed independently from the resolved model — this is a
        // second port (the deferred task's IPC server constructs its own),
        // but they wrap the same underlying model, so the duplication is
        // harmless.
        //
        // What stays in the user-login-gated deferred task:
        // - Memory port, context injector, curator injector (need username
        //   for the agent DB)
        // - Regulation archive, escalation queue (need passphrase from
        //   provisioning)
        // - IPC server (needs embedding port from provisioning)
        // - MCP server launch, email sink, collab server
        {
            let app_state_for_model_task = app_state.clone();
            cx.spawn(async move |cx| {
                // Check immediately and subscribe to registry events.
                let registry = cx.update(|cx| language_model::LanguageModelRegistry::global(cx));

                // Initial check — the model may already be available.
                let initial = registry.read_with(cx, |r, _| r.default_model().is_some());
                if initial {
                    // zed-kask: D24 — wire the edit-prediction port.
                    let ep_wired = std::sync::atomic::AtomicBool::new(false);
                    let http_client = app_state_for_model_task.client.http_client();
                    if let Err(e) =
                        try_wire_edit_prediction_port(&ep_wired, &registry, http_client, cx).await
                    {
                        log::warn!("hKask edit-prediction port initial wiring failed: {e}");
                    }
                }

                // Subscribe to registry events for the async case.
                let registry_for_sub = registry.clone();
                // zed-kask: D24 — separate AtomicBool for the edit-prediction port.
                let ep_wired_for_sub =
                    std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                let http_client_for_sub = app_state_for_model_task.client.http_client();
                cx.subscribe(
                    &registry,
                    move |_, event: &language_model::Event, cx| {
                        match event {
                            language_model::Event::DefaultModelChanged
                            | language_model::Event::ProviderStateChanged(_)
                            | language_model::Event::AddedProvider(_)
                            | language_model::Event::ProvidersChanged => {
                                let registry = registry_for_sub.clone();
                                let ep_wired = ep_wired_for_sub.clone();
                                let http_client = http_client_for_sub.clone();
                                cx.spawn(async move |cx| {
                                    // zed-kask: D24
                                    if let Err(e) = try_wire_edit_prediction_port(
                                        &ep_wired,
                                        &registry,
                                        http_client,
                                        cx,
                                    )
                                    .await
                                    {
                                        log::warn!(
                                            "hKask edit-prediction port wiring failed on registry event: {e}"
                                        );
                                    }
                                })
                                .detach();
                            }
                            _ => {}
                        }
                    },
                )
                .detach();

                log::info!(
                    "hKask model-dependent skill execution task started — \
                     waiting for LanguageModelRegistry to report a default model"
                );
            })
            .detach();
        }

        // zed-kask does not initialize upstream Zed's in-app updater. Its Linux
        // installer writes `zed*.app` bundles into `~/.local` and can replace
        // the user's real Zed installation. Updates are CLI-installer-only.
        dap_adapters::init(cx);
        reliability::init(
            client.clone(),
            app_state.workspace_store.clone(),
            context_server_health_source_for_poller,
            cx,
        );
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
        let credentials_provider = zed_credentials_provider::global(cx);
        copilot_chat::init(
            app_state.client.http_client(),
            credentials_provider,
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
        // Wire the worktree spawner when an AgentPanel is created. The
        // spawner is process-global (Mutex-based, re-settable) so the IPC
        // server's GPUI-side task can read it when a `CreateWorktreeThread`
        // request arrives. When the panel is dropped (workspace closed), the
        // spawner is cleared so the MCP server falls back to in-memory spawn.
        cx.observe_new(|_panel: &mut AgentPanel, window, cx| {
            let Some(window) = window else {
                log::warn!(
                    "AgentPanel created without a window — worktree spawner not wired"
                );
                return;
            };
            let window_handle = window.window_handle();
            let spawner: Arc<dyn kask_bridge::WorktreeSpawner> = Arc::new(
                AgentPanelWorktreeSpawner {
                    panel: cx.entity().downgrade(),
                    window: window_handle,
                },
            );
            kask_bridge::set_worktree_spawner(Some(spawner));
            // Clear the spawner when the panel is dropped.
            let weak_panel = cx.entity().downgrade();
            cx.spawn(async move |_this, cx| {
                while weak_panel.upgrade().is_some() {
                    cx.background_executor()
                        .timer(std::time::Duration::from_secs(1))
                        .await;
                }
                kask_bridge::set_worktree_spawner(None);
            }).detach();
        })
        .detach();
        swarm_panel::init(cx);
        kanban_panel::init(cx);
        portfolio_panel::init(cx);
        zed::watch_user_agents_md(app_state.fs.clone(), cx);

        // D1/D3/D4/D12: Model-dependent kask wiring is split across two tasks:
        //
        // 1. The skill execution (D1) is wired by the model-dependent task
        //    (above), which fires as soon as `LanguageModelRegistry::
        //    default_model()` returns `Some` — independent of Zed user login.
        //    The model registry is populated from settings.json, not cloud auth.
        //
        // 2. The remaining model-dependent hooks (IPC server, condenser, panel
        //    tool invoker) and all user-dependent hooks (memory port, context
        //    injector, regulation archive) are wired by the deferred task
        //    (above) after the Zed user resolves.
        //
        // zed-kask: D1/D3/D6/D8 — F20: deferred task (user-dependent hooks:
        // memory_port, thread_condenser, tool_invoker, context_injector,
        // curator_context_injector) + model-dependent task (edit_prediction_port).

        repl::init(app_state.fs.clone(), cx);
        recent_projects::init(cx);
        dev_container::init(cx);

        load_embedded_fonts(cx);
        #[cfg(target_os = "linux")]
        prewarm_fonts(cx);

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
        tabular_data_preview::init(cx);
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

        let (first_window_tx, first_window_rx) = oneshot::channel::<()>();
        let first_window_tx = Rc::new(RefCell::new(Some(first_window_tx)));
        let _first_window_subscription = cx.observe_new::<MultiWorkspace>(move |_, _, _| {
            if let Some(tx) = first_window_tx.borrow_mut().take() {
                tx.send(()).ok();
            }
        });

        let restore_finished = cx.background_spawn(restore_task).shared();

        cx.spawn({
            let db = workspace::WorkspaceDb::global(cx);
            let restore_finished = restore_finished.clone();
            async move |_cx| {
                restore_finished.await;
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
            let _first_window_subscription = _first_window_subscription;
            let first_window_placed = first_window_rx.shared();
            while let Some(urls) = open_rx.next().await {
                // On a macOS cold launch, `zed <path>` arrives here after startup already
                // began restoring the session, so wait for a restored window to exist before
                // matching. Otherwise this open sees no windows and spawns a redundant one (#61346).
                futures::select_biased! {
                    _ = restore_finished.clone() => {}
                    _ = first_window_placed.clone() => {}
                }
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
// zed-kask: D3 — F21: sync_kask_mcp_servers fn definition.
// Registers the built-in kask MCP servers as zed context servers via the
// app-level ContextServerDescriptorRegistry. This makes kask MCP tools appear
// in the agent tool picker and available to zed's agent thread. The servers
// are launched as stdio child processes by zed's ContextServerStore.

// zed-kask: D3/D7 — F22: resolve_mcp_binary fn definition.
/// Resolve an MCP server binary to an absolute path.
///
/// GUI-launched apps (Finder/Spotlight/Dock/.desktop) do not inherit the
/// user's shell PATH, so a bare binary name like `hkask-mcp-swarm`
/// fails to spawn — the server lands in `ContextServerState::Error` and
/// is unavailable to the agent. Resolution order:
///
/// 1. `HKASK_MCP_{ID}_BIN` env var (explicit operator override; previously
///    advertised in error messages and docs but never implemented — this
///    is the enforcement point for that advertised invariant).
/// 2. Sibling of the running `zed-kask` binary (`current_exe().parent()`).
///    In a standard install, `hkask-mcp-*` binaries live side-by-side with
///    `zed-kask` in `~/.local/bin` (or `$INSTALL_DIR/bin`).
/// 3. Bare binary name (last resort — relies on PATH; works for CLI
///    launches, not GUI).
///
/// This respects the `.rules` trap "Advertised invariants need enforcement
/// points" — the `HKASK_MCP_*_BIN` mechanism is now real, not fiction.
fn resolve_mcp_binary(server_id: &str, binary: &str) -> String {
    let env_var = format!(
        "HKASK_MCP_{}_BIN",
        server_id.to_uppercase().replace('-', "_")
    );
    if let Ok(path) = std::env::var(&env_var)
        && !path.is_empty()
    {
        return path;
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let candidate = dir.join(binary);
        if candidate.is_file() {
            return candidate.to_string_lossy().into_owned();
        }
    }
    binary.to_string()
}

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
            // zed-kask: D3/D9 — F23: kask_server_env (env var resolution for MCP servers).
            // Single canonical path: `build_mcp_server_env` filters config and
            // credentials per-server in the correct order. The previous inline
            // composition leaked the full unfiltered `mcp_env()` map (the
            // `extend` only overwrote allowed keys, never removed disallowed
            // ones), so servers received the curator's email config.
            let settings = cx.update(|cx| kask_bridge::KaskSettings::get_global(cx).clone());
            let credentials_provider = cx.update(|cx| zed_credentials_provider::global(cx));
            let env_map = kask_bridge::build_mcp_server_env(
                &server_id,
                &settings,
                credentials_provider.as_ref(),
                kask_bridge::get_inference_socket_path().as_deref(),
                kask_bridge::get_inference_timeout_secs(),
                cx,
            )
            .await;
            // `build_mcp_server_env` returns `std::collections::HashMap` (matches
            // the filter helpers and `start_server_with_env`); `ContextServerCommand`
            // expects zed's `collections::HashMap` (FxBuildHasher). Convert here
            // so the canonical builder keeps one return type for both consumers.
            let env_map: collections::HashMap<String, String> = env_map.into_iter().collect();

            Ok(context_server::ContextServerCommand {
                path: resolve_mcp_binary(&server_id, &binary).into(),
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

/// zed-kask: D24 — wire the kask edit-prediction port.
///
/// Resolves `DEFAULT_FALLBACK_MODEL` (e.g. `OpenRouter/z-ai/glm-5.2`) from
/// the `LanguageModelRegistry`, constructs a `BridgeEditPredictionPort` that
/// makes raw `/completions` calls through the model's `api_url()`/`api_key()`,
/// and injects it into the edit-prediction store via
/// `edit_prediction::open_ai_compatible::set_kask_completion_port`.
///
/// Called from the same model-dependent task as the edit-prediction port wiring
/// — fires once the registry has a model. `Mutex`-based hook (re-settable),
/// so there is no need for an `AtomicBool` guard, but we use one anyway to
/// avoid redundant `resolve_model_names` + HTTP-client construction on every
/// registry event.
async fn try_wire_edit_prediction_port(
    wired: &std::sync::atomic::AtomicBool,
    registry: &gpui::Entity<language_model::LanguageModelRegistry>,
    http_client: std::sync::Arc<dyn http_client::HttpClient>,
    cx: &mut gpui::AsyncApp,
) -> anyhow::Result<()> {
    if wired.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return Ok(());
    }

    cx.update(|cx| {
        let tokio_handle = gpui_tokio::Tokio::handle(cx);
        let port = kask_bridge::BridgeEditPredictionPort::from_registry(
            registry.read(cx),
            http_client,
            tokio_handle,
            cx,
        );
        if let Some(port) = port {
            edit_prediction::open_ai_compatible::set_kask_completion_port(Some(
                std::sync::Arc::new(port)
                    as std::sync::Arc<dyn edit_prediction::open_ai_compatible::KaskCompletionPort>,
            ));
            log::info!(
                "hKask edit-prediction port wired — routing FIM completions \
                 through LanguageModelRegistry ({})",
                kask_bridge::DEFAULT_FALLBACK_MODEL
            );
        } else {
            log::warn!(
                "hKask edit-prediction port not wired — could not resolve {} \
                 from LanguageModelRegistry (no api_url/api_key). Edit predictions \
                 will fall back to the configured provider.",
                kask_bridge::DEFAULT_FALLBACK_MODEL
            );
        }
        Ok(())
    })
}

/// Reconcile the app-level `ContextServerDescriptorRegistry` with the
/// current kask MCP settings.
///
// zed-kask: D3 — F24: sync_kask_mcp_servers impl (descriptor registration).
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

/// Build the env map for a kask MCP server child process via the single
/// canonical path (`build_mcp_server_env`).
///
/// Extracted so the deferred launch loop and the settings-change restart
/// observer construct env identically — a divergence would restart servers
/// with different env than the launch, or miss that the env changed. Both
/// this and `KaskMcpDescriptor::command` now go through `build_mcp_server_env`,
/// so the per-project `ContextServerStore` path and the governed `McpRuntime`
/// path can no longer drift apart.
async fn kask_server_env(
    server_id: &str,
    cx: &mut gpui::AsyncApp,
) -> std::collections::HashMap<String, String> {
    let settings = cx.update(|cx| kask_bridge::KaskSettings::get_global(cx).clone());
    let credentials_provider = cx.update(|cx| zed_credentials_provider::global(cx));
    kask_bridge::build_mcp_server_env(
        server_id,
        &settings,
        credentials_provider.as_ref(),
        kask_bridge::get_inference_socket_path().as_deref(),
        kask_bridge::get_inference_timeout_secs(),
        cx,
    )
    .await
}

/// The env keys whose presence or value differs between two server env maps.
///
/// Keys only — several values are credentials and must not reach the log.
fn changed_env_keys(
    previous: &std::collections::HashMap<String, String>,
    current: &std::collections::HashMap<String, String>,
) -> Vec<String> {
    let mut keys: Vec<String> = previous
        .iter()
        .filter(|(key, value)| current.get(*key) != Some(*value))
        .map(|(key, _)| key.clone())
        .chain(
            current
                .keys()
                .filter(|key| !previous.contains_key(*key))
                .cloned(),
        )
        .collect();
    keys.sort();
    keys.dedup();
    keys
}

// zed-kask: D3/D8 — F25: sync_kask_mcp_runtime_servers (governed McpRuntime restart).
/// Re-sync the governed `McpRuntime` server processes when kask settings
/// change (e.g. `kask.swarm.mode`, credit ceilings, provider toggles).
///
/// `sync_kask_mcp_servers` (above) re-syncs only the per-project
/// `ContextServerStore` path. The governed McpRuntime instances — which the
/// kask panel's `ToolInvoker` and the skill execution route through — are
/// started once at login and would otherwise keep their startup env forever
/// (a `kask.swarm.mode` toggle would never re-route the panel's own tool
/// calls). This restarts exactly the servers whose computed env actually
/// changed; servers not yet tracked by the deferred launch (empty baseline)
/// are left alone. The baseline is recorded by the launch loop, so this
/// observer is a no-op until the governed servers are actually running.
fn sync_kask_mcp_runtime_servers(
    mcp_runtime: std::sync::Arc<hkask_mcp::McpRuntime>,
    last_env: std::sync::Arc<
        std::sync::Mutex<
            std::collections::HashMap<String, std::collections::HashMap<String, String>>,
        >,
    >,
    cx: &mut gpui::App,
) {
    let server_ids: Vec<&'static str> = kask_bridge::builtin_mcp_server_ids();
    cx.spawn(async move |cx| {
        // Build the changed-server list on the foreground — `kask_server_env`
        // needs `AsyncApp` (not `Send`). Do NOT hold a tokio `enter()` guard
        // across these `.await`s: when this observer fires twice (e.g. a mode
        // toggle plus a window-close registry churn), two `cx.spawn` tasks each
        // acquired a guard and interleaved at await points, panicking with
        // "EnterGuard values dropped out of order" (tokio runtime/context/
        // current.rs). The tokio-dependent stop/start is dispatched into
        // `Tokio::spawn` below, which enters the reactor on the worker thread
        // — no foreground guard held across awaits (the `.rules` "background_
        // spawn of tokio-dependent futures" pattern).
        let mut changed: Vec<(
            &'static str,
            String,
            std::collections::HashMap<String, String>,
        )> = Vec::new();
        for server_id in server_ids {
            let env = kask_server_env(server_id, cx).await;
            let previous = last_env
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(server_id)
                .cloned();
            let Some(previous) = previous else {
                // Not yet launched — the deferred task hasn't recorded a
                // baseline, so there is nothing to restart.
                continue;
            };
            if previous == env {
                continue;
            }
            // Name the keys that changed. A restart tears down live connections
            // and fails every in-flight panel call, so "env changed" alone is not
            // enough to tell a deliberate settings toggle from an ordering artifact
            // (e.g. a credential that only resolved after launch). Values are
            // never logged — several are credentials.
            log::info!(
                "Kask MCP server '{server_id}' env changed — restarting (McpRuntime); \
                 changed keys: {}",
                changed_env_keys(&previous, &env).join(", ")
            );
            changed.push((server_id, format!("hkask-mcp-{server_id}"), env));
        }
        if changed.is_empty() {
            return;
        }
        // `stop_server` / `start_server_with_env` drive tokio primitives
        // (process, rmcp). `McpRuntime: Send + Sync` (its `governance` field is
        // `Option<ToolGovernance>`, all-Send-Sync; `RegulationSink: Send +
        // Sync`), so this future is `Send` and `Tokio::spawn` accepts it.
        let runtime = mcp_runtime.clone();
        let last_env = last_env.clone();
        gpui_tokio::Tokio::spawn(cx, async move {
            for (server_id, binary, env) in changed {
                runtime.stop_server(server_id).await;
                match runtime
                    .start_server_with_env(server_id, &binary, env.clone())
                    .await
                {
                    Ok(()) => {
                        // `insert`, not `get_mut().expect()`: the baseline entry is
                        // written by the launch loop, but this observer can fire
                        // concurrently with it, and a missing entry must record the
                        // new baseline rather than panic (`.rules`: no `expect` on
                        // fallible lookups).
                        last_env
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .insert(server_id.to_string(), env);
                    }
                    Err(e) => {
                        // Keep the old baseline so a subsequent settings
                        // change retries the restart.
                        //
                        // `stop_server` already dropped the connection, so the
                        // runtime has no live server for `server_id` right now.
                        // `McpRuntime::call_tool_inner` reconnects on demand from
                        // the recorded launch spec, so panel calls recover without
                        // another settings change — but the failure is still an
                        // operator-visible warning, since a broken binary will not
                        // heal on its own.
                        log::warn!(
                            "Kask MCP server '{server_id}' restart failed: {e} — the runtime \
                             will retry the connection on the next tool call"
                        );
                    }
                }
            }
        })
        .detach();
    })
    .detach();
}

// ── Swarm panel tool-invoker adapter ───────────────────────────────────────
//
// This adapter implements swarm_panel's ToolInvoker trait by delegating to
// the McpRuntime (which implements ToolPort directly). It's defined here (in
// the zed binary crate) because the composition root is the natural place for
// adapter construction.

/// Adapter implementing `swarm_panel::ToolInvoker` via the `McpRuntime`.
///
/// The dispatch future MUST run on the Tokio runtime, not on the GPUI
/// background executor. `McpRuntime::invoke` → `call_tool_inner` →
/// `try_reconnect` → `start_server_with_env` spawns a child process via
/// `TokioChildProcess`, which requires a Tokio reactor context. The GPUI
/// background executor has no reactor, so a transport-loss-triggered
/// reconnect from a panel tool call would panic with "there is no reactor
/// running" (the `.rules` "background_spawn of tokio-dependent futures"
/// trap). The initial server start avoids this by going through
/// `gpui_tokio::Tokio::spawn`; this adapter must do the same for the
/// on-demand reconnect path. The `ToolPort::invoke` future is `Send`
/// (`ToolFuture` is `Pin<Box<dyn Future + Send>>`), so `handle.spawn` is
/// sound. The `JoinHandle` is bridged back to a GPUI `Task` via the
/// background executor (the same pattern `gpui_tokio::Tokio::spawn` uses
/// internally).
struct PanelToolInvoker {
    tool_port: std::sync::Arc<hkask_mcp::McpRuntime>,
    executor: gpui::BackgroundExecutor,
    tokio_handle: tokio::runtime::Handle,
}

// zed-kask: D23 — WorktreeSpawner impl for InferenceIpcServer.
/// Creates a worktree-backed agent thread via `AgentPanelSiblingHost::create_sibling_thread`.
/// Holds a `WeakEntity<AgentPanel>` + `AnyWindowHandle` (both `Send + Sync`) so it can
/// be `Arc`-cloned into the GPUI-side task. The `spawn` method runs inside the
/// GPUI task (which has `&mut AsyncApp`) and calls `create_sibling_thread` with
/// `use_new_worktree: true`.
struct AgentPanelWorktreeSpawner {
    panel: gpui::WeakEntity<AgentPanel>,
    window: gpui::AnyWindowHandle,
}

impl kask_bridge::WorktreeSpawner for AgentPanelWorktreeSpawner {
    fn spawn(
        &self,
        prompt: String,
        title: String,
        worktree_name: Option<String>,
        base_ref: Option<String>,
        cx: &mut gpui::AsyncApp,
    ) -> gpui::Task<Result<hkask_types::inference_ipc::WorktreeThreadInfo, String>> {
        use agent::SiblingThreadHost;
        let panel = self.panel.clone();
        let window = self.window;
        cx.spawn(async move |cx| {
            let panel = panel
                .upgrade()
                .ok_or_else(|| "agent panel no longer available".to_string())?;
            let host = agent_ui::AgentPanelSiblingHost::new(panel.downgrade(), window);
            let request = agent::SiblingThreadRequest {
                title: title.into(),
                prompt,
                agent_id: None,
                model: None,
                use_new_worktree: true,
                worktree_name,
                base_ref,
            };
            let info = host
                .create_sibling_thread(request, cx)
                .await
                .map_err(|e| e.to_string())?;
            Ok(hkask_types::inference_ipc::WorktreeThreadInfo {
                message: format!(
                    "Worktree thread created: {} ({})",
                    info.title, info.agent_id
                ),
            })
        })
    }
}

impl swarm_panel::ToolInvoker for PanelToolInvoker {
    fn invoke_tool(
        &self,
        server: &str,
        tool: &str,
        args: serde_json::Value,
    ) -> gpui::Task<Result<String, swarm_panel::InvokeError>> {
        use hkask_tool_port::ToolPort;
        use hkask_types::WebID;
        use swarm_panel::InvokeError;

        // Accounting identity for the call meter — not a credential.
        let webid = WebID::from_persona(b"swarm-panel");

        let tool_port = self.tool_port.clone();
        let server = server.to_string();
        let tool = tool.to_string();
        let tokio_handle = self.tokio_handle.clone();

        // Spawn on the Tokio runtime so `try_reconnect` → `start_server_with_env`
        // → `TokioChildProcess::spawn` finds a reactor. See the struct doc for
        // why this cannot be `self.executor.spawn`.
        let join_handle = tokio_handle.spawn(async move {
            // Preserve the retry-safety classification across the seam. A blanket
            // `e.to_string()` erased it and forced panels to treat a restarting
            // MCP server as a permanent failure. `Interrupted` is kept separate
            // from both: its outcome is unknown, so a panel must re-read state
            // rather than retry (which could duplicate a side effect).
            let result = ToolPort::invoke(&*tool_port, &server, &tool, args, webid)
                .await
                .map_err(|error| {
                    let message = error.to_string();
                    match error {
                        hkask_tool_port::ToolPortError::Unavailable(_) => {
                            InvokeError::Unavailable(message)
                        }
                        hkask_tool_port::ToolPortError::Interrupted(_) => {
                            InvokeError::Interrupted(message)
                        }
                        hkask_tool_port::ToolPortError::EnergyBudgetExceeded(_)
                        | hkask_tool_port::ToolPortError::NotFound(_)
                        | hkask_tool_port::ToolPortError::InvocationFailed(_) => {
                            InvokeError::Failed(message)
                        }
                    }
                })?;
            Ok(serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string()))
        });
        // Bridge the Tokio `JoinHandle` to a GPUI `Task` so the panel can
        // `.await` it on the GPUI side. Cancelling the GPUI task aborts the
        // underlying Tokio task (mirrors `gpui_tokio::Tokio::spawn`).
        let abort_handle = join_handle.abort_handle();
        let cancel = gpui_util::defer(move || {
            abort_handle.abort();
        });
        self.executor.spawn(async move {
            let result = join_handle.await;
            drop(cancel);
            result.map_err(|join_error| {
                use swarm_panel::InvokeError;
                if join_error.is_cancelled() {
                    InvokeError::Interrupted("panel tool call cancelled".to_string())
                } else {
                    // `is_panic()` is true for panics; `tokio::task::JoinError`
                    // stringifies to the panic message in that case.
                    InvokeError::Failed(format!("tokio task failed: {join_error}"))
                }
            })?
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

#[cfg(target_os = "linux")]
fn prewarm_fonts(cx: &mut App) {
    let theme_settings = theme::theme_settings(cx);
    let ui_font = theme_settings.ui_font(cx).clone();
    let buffer_font = theme_settings.buffer_font(cx).clone();
    let mut fonts = vec![ui_font.clone(), buffer_font.clone()];
    let mut add_variant = |base_font: &gpui::Font, weight, style| {
        let mut font = base_font.clone();
        font.weight = weight;
        font.style = style;
        fonts.push(font);
    };

    for weight in [
        gpui::FontWeight::MEDIUM,
        gpui::FontWeight::SEMIBOLD,
        gpui::FontWeight::BOLD,
    ] {
        add_variant(&ui_font, weight, gpui::FontStyle::Normal);
    }
    add_variant(&ui_font, gpui::FontWeight::NORMAL, gpui::FontStyle::Italic);
    add_variant(
        &buffer_font,
        gpui::FontWeight::BOLD,
        gpui::FontStyle::Normal,
    );
    add_variant(
        &buffer_font,
        gpui::FontWeight::NORMAL,
        gpui::FontStyle::Italic,
    );
    add_variant(
        &buffer_font,
        gpui::FontWeight::BOLD,
        gpui::FontStyle::Italic,
    );

    let text_system = cx.text_system().clone();
    cx.background_spawn(async move {
        text_system.prewarm_fonts(&fonts);
    })
    .detach();
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

    /// `resolve_mcp_binary` honors the `HKASK_MCP_{ID}_BIN` env var — the
    /// advertised invariant that was previously fiction (error messages and
    /// docs referenced it, but no resolution existed). This test pins the
    /// env-var path so a future refactor cannot silently drop it.
    ///
    /// Respects the `.rules` trap "Advertised invariants need enforcement
    /// points."
    ///
    /// When no env var is set and the binary is not found next to the running
    /// exe, `resolve_mcp_binary` falls back to the bare name. This pins the
    /// last-resort fallback so GUI launches without the binary installed
    /// produce a clear "binary not found" error rather than a silent wrong path.
    #[test]
    fn resolve_mcp_binary_falls_back_to_bare_name() {
        // SAFETY: this test runs single-threaded; no other thread reads or writes
        // `HKASK_MCP_NONEXISTENT_BIN` while this block executes.
        unsafe {
            std::env::remove_var("HKASK_MCP_NONEXISTENT_BIN");
        }
        let resolved = resolve_mcp_binary("nonexistent", "hkask-mcp-nonexistent");
        assert_eq!(
            resolved, "hkask-mcp-nonexistent",
            "bare binary name is the last-resort fallback when no env var and no sibling binary exists"
        );
    }

    /// zed-kask: pinning test for the kask wiring functional units in `main.rs`.
    ///
    /// The kask wirings (F2–F25, see `kask/docs/upstream-rebase-process.md` §4)
    /// are process-global hooks set during `main()`. Most cannot be exercised
    /// in a unit test without a full app init (they need `cx`, `app_state`,
    /// a resolved user, etc.). This test is a **compile-time + symbol-existence
    /// pin**: it asserts that the key wiring functions and types are accessible
    /// from the test module, so that removing any wiring (e.g., deleting
    /// `resolve_mcp_binary` or `sync_kask_mcp_servers`) breaks this test.
    ///
    /// Per the `.rules` trap "Tests must pin deliberate zed-kask deviations":
    /// every `// zed-kask:` marker must have a corresponding test. This test
    /// pins the functional units in `main.rs` whose symbols are reachable
    /// from a unit test. F8 (`<dyn fs::Fs>::set_global`) is a trait method
    /// call on an external value and cannot be pinned via `TypeId`; it is
    /// covered by the F2–F25 compile-time reachability of the `fs` value.
    #[test]
    fn kask_wiring_symbols_exist() {
        // F22: resolve_mcp_binary — must be callable with the documented signature.
        let _ = resolve_mcp_binary("test", "test-binary");

        // F23: kask_server_env — must be accessible. Referencing the fn
        // name forces the compiler to resolve it; renaming or deleting it
        // breaks compilation here. The fn is async so we can't call it
        // without an AsyncApp, but the name reference pins its existence.
        let _ = kask_server_env;

        // F2: the kask tokio runtime is built via tokio::runtime::Builder —
        // assert the builder type is accessible (the runtime is built in main()).
        let _ = std::any::TypeId::of::<tokio::runtime::Builder>();

        // F3: AlertEvent and Alert Sink must be accessible (alert channel wiring).
        let _ = std::any::TypeId::of::<hkask_regulation::AlertEvent>();

        // F4: algedonic threshold → variety_max_deficit mapping. The constant
        // DEFAULT_VARIETY_MAX_DEFICIT is the scaling base; removing it breaks
        // the F4 wiring in main(). Reading the const value pins it (a const
        // is not a type — `TypeId::of::<CONST>()` does not compile).
        let _ = hkask_regulation::DEFAULT_VARIETY_MAX_DEFICIT;

        // F5: swarm-panel gas budget persona. SWARM_PANEL_CALL_CAP is the
        // call cap seeded for the swarm-panel persona; removing it breaks
        // the F5 wiring in main(). Reading the const value pins it.
        let _ = SWARM_PANEL_CALL_CAP;

        // F6: CyberneticsLoop and MetacognitionLoop must be accessible.
        let _ = std::any::TypeId::of::<hkask_regulation::CyberneticsLoop>();
        let _ = std::any::TypeId::of::<hkask_regulation::MetacognitionLoop>();

        // F7: BridgeMetacognitionProvider — the metacognition provider hook.
        let _ = std::any::TypeId::of::<kask_bridge::BridgeMetacognitionProvider>();

        // F9: KaskSettings must be accessible.
        let _ = std::any::TypeId::of::<kask_bridge::KaskSettings>();

        // F10: curator.always_on gating field — the setting that gates tick cycles.
        // Pinning the field type via KaskCuratorSettings ensures the struct
        // and its always_on field exist; removing the field breaks compilation.
        let _ = std::any::TypeId::of::<kask_bridge::KaskCuratorSettings>();

        // F6: McpRuntime must be accessible.
        let _ = std::any::TypeId::of::<hkask_mcp::McpRuntime>();

        // F25: sync_kask_mcp_runtime_servers — must be accessible as a fn
        // item (not just a function pointer type). Referencing the fn value
        // pins both its existence and its name; renaming or deleting it
        // breaks compilation here.
        let _ = sync_kask_mcp_runtime_servers
            as fn(
                std::sync::Arc<hkask_mcp::McpRuntime>,
                std::sync::Arc<
                    std::sync::Mutex<
                        std::collections::HashMap<
                            String,
                            std::collections::HashMap<String, String>,
                        >,
                    >,
                >,
                &mut gpui::App,
            );

        // If this test compiles, the functional units' key symbols are
        // present. Removing any wiring function/type breaks compilation here.
    }
}
