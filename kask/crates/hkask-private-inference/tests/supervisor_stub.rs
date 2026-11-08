//! The stub-server supervisor integration suite (slice plan step 6):
//! launch→healthy→stop, the probe refusals, death surfacing, and the
//! occupied-port adopt/refuse matrix — every refusal class reproduced
//! against the fixture WITHOUT the real engine or the 79 GB weights.
//! Run single-threaded (live-mutation suite, the house pattern):
//!
//! ```sh
//! cargo test -p hkask-private-inference --features test-fixture \
//!   --test supervisor_stub -- --test-threads=1 --nocapture
//! ```

#![cfg(feature = "test-fixture")]

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use hkask_private_inference::local_leg::fleet;
use hkask_private_inference::local_leg::gguf::{GgufMeta, GgufTensor};
use hkask_private_inference::local_leg::manifest;
use hkask_private_inference::local_leg::placement::{
    HardwareInventory, PlacementPolicy, VramSource, compile_placement,
};
use hkask_private_inference::local_leg::supervisor::{
    LaunchConfig, PreparedLaunch, StatusSnapshot, SupervisionError, Supervisor,
};

const STUB: &str = env!("CARGO_BIN_EXE_stub-llama-server");

/// A free loopback port (bind :0, read, drop — races are acceptable at
/// single-threaded suite scope).
fn free_port() -> u16 {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .expect("bind :0")
        .local_addr()
        .expect("local addr")
        .port()
}

/// A wrapper script that execs the stub with `prelude` args before the
/// caller's — how a misbehaving engine (wrong commit, dropped flag,
/// dying process) is injected without touching the production config.
fn wrapper(dir: &Path, name: &str, prelude: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\nexec {STUB} {prelude} \"$@\"\n"))
        .expect("write wrapper");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod wrapper");
    path
}

/// The hand-built prepared launch: the guards, the probe and the whole
/// spawn path run for real — only the GGUF header PARSE is replaced
/// (the one step that needs real weight bytes; pinned by the live
/// tests). A CPU-only plan (n_gpu_layers 0) keeps the guard set
/// machine-independent: small declared bytes, ample declared RAM.
fn prepared(port: u16) -> PreparedLaunch {
    let manifest = manifest::shipped_manifest().expect("shipped manifest");
    // The discovered fleet is the brief home (seed + discover — the
    // production path), never a CWD-relative file read.
    let briefs_dir = tempfile::tempdir().expect("briefs dir");
    fleet::seed_briefs(briefs_dir.path()).expect("the seed set writes");
    let models_root = tempfile::tempdir().expect("models root");
    let brief = fleet::discover_model(briefs_dir.path(), models_root.path(), "gemma-4-31b")
        .expect("gemma is in the discovered fleet")
        .brief;
    let meta = GgufMeta {
        gguf_version: 3,
        kv: BTreeMap::new(),
        tensors: vec![GgufTensor {
            name: "stub-tensor".into(),
            ggml_type: 0,
            dims: vec![32_000_000],
            offset: 0,
            approx_bytes: 1_000_000_000,
            is_expert: false,
        }],
        expert_bytes: 1_000_000_000,
        dense_bytes: 1_000_000_000,
        alignment: 32,
        data_start: 4_096,
        param_count: 32_000_000,
    };
    let hardware = HardwareInventory {
        total_ram_bytes: 96_000_000_000,
        available_ram_bytes: 72_000_000_000,
        vram: Some(VramSource::Declared {
            bytes: 8_589_934_592,
        }),
        provenance: "stub test",
    };
    let policy = PlacementPolicy {
        host: "127.0.0.1".into(),
        port,
        n_gpu_layers: 0,
        ..PlacementPolicy::default()
    };
    let plan = compile_placement(
        PathBuf::from("/stub/model.gguf"),
        &meta,
        &hardware,
        &brief,
        &policy,
        &manifest,
    )
    .expect("the stub plan compiles");
    PreparedLaunch { plan, manifest }
}

fn config(state_dir: &Path, binary: &Path, api_key: Option<&str>) -> LaunchConfig {
    LaunchConfig {
        model_id: "gemma-4-31b".into(),
        binary: binary.to_path_buf(),
        api_key: api_key.map(str::to_string),
        state_dir: state_dir.to_path_buf(),
        models_root: state_dir.to_path_buf(),
        // The prepared-launch path never reads the briefs (the plan is
        // pre-built) — the field names where a full prepare_launch
        // would discover from.
        briefs_dir: state_dir.join("model-briefs"),
        policy: PlacementPolicy::default(),
    }
}

async fn wait_for_status(
    supervisor: &Supervisor,
    secs: u64,
    wanted: impl Fn(&StatusSnapshot) -> bool,
) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    // Async sleep — the watch task shares the (multi-thread) runtime;
    // a blocking sync loop here would starve it on a current-thread
    // runtime (found live: the death test never observed the flip).
    loop {
        if wanted(&supervisor.status()) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "status never reached the wanted state: {:?}",
            supervisor.status()
        );
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn launch_reaches_healthy_writes_state_and_stop_drains() {
    let state = tempfile::tempdir().expect("state dir");
    let port = free_port();
    let prepared = prepared(port);
    let supervisor = Supervisor::default();
    let config = config(state.path(), Path::new(STUB), Some("the-token"));

    let serving = supervisor
        .launch_prepared(config.clone(), &prepared)
        .await
        .expect("the stub launches and reaches healthy");
    assert!(!serving.adopted);

    // The record, the 0600 token file, the live status.
    let record = std::fs::read_to_string(state.path().join("launch-record.json"))
        .expect("the record is written after first healthy");
    assert!(record.contains("\"model_id\":\"gemma-4-31b\""));
    let key = std::fs::read(state.path().join("llama-server.key")).expect("the token file");
    assert_eq!(key, b"the-token");
    let mode = std::fs::metadata(state.path().join("llama-server.key"))
        .expect("stat")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    assert_eq!(
        supervisor.status(),
        StatusSnapshot::Serving {
            model_id: "gemma-4-31b".into(),
            endpoint: format!("http://127.0.0.1:{port}/v1"),
            adopted: false,
        }
    );

    supervisor.stop().await.expect("stop drains");
    assert_eq!(supervisor.status(), StatusSnapshot::Stopped);
    assert!(
        !state.path().join("launch-record.json").exists(),
        "record removed"
    );
    assert!(
        !state.path().join("llama-server.key").exists(),
        "token file removed"
    );
    // The port is freed: the next bind succeeds.
    let rebound = std::net::TcpListener::bind(("127.0.0.1", port));
    assert!(rebound.is_ok(), "the stub stopped listening after drain");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_wrong_commit_stub_is_refused_by_the_probe_and_writes_no_state() {
    let state = tempfile::tempdir().expect("state dir");
    let wrappers = tempfile::tempdir().expect("wrapper dir");
    let port = free_port();
    let prepared = prepared(port);
    let supervisor = Supervisor::default();
    let binary = wrapper(
        wrappers.path(),
        "wrong-commit",
        "--stub-emit-commit deadbeef",
    );
    let config = config(state.path(), &binary, Some("tok"));

    let err = supervisor
        .launch_prepared(config.clone(), &prepared)
        .await
        .expect_err("a wrong-commit binary must be refused");
    match err {
        SupervisionError::Probe(probe_err) => {
            let message = probe_err.to_string();
            assert!(
                message.contains("deadbeef") && message.contains("9e2a04a"),
                "{message}"
            );
        }
        other => panic!("expected a probe refusal, got {other:?}"),
    }
    // Fail-closed: nothing was written, nothing is serving.
    assert!(!state.path().join("launch-record.json").exists());
    assert!(!state.path().join("llama-server.key").exists());
    assert_eq!(supervisor.status(), StatusSnapshot::Stopped);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dropped_flag_stub_is_refused_naming_the_missing_flag() {
    let state = tempfile::tempdir().expect("state dir");
    let wrappers = tempfile::tempdir().expect("wrapper dir");
    let port = free_port();
    let prepared = prepared(port);
    let supervisor = Supervisor::default();
    let binary = wrapper(
        wrappers.path(),
        "dropped-flag",
        "--stub-drop-flag --flash-attn",
    );
    let config = config(state.path(), &binary, Some("tok"));

    let err = supervisor
        .launch_prepared(config.clone(), &prepared)
        .await
        .expect_err("a binary missing a required spelling must be refused");
    match err {
        SupervisionError::Probe(probe_err) => {
            let message = probe_err.to_string();
            assert!(message.contains("--flash-attn"), "{message}");
        }
        other => panic!("expected a probe refusal, got {other:?}"),
    }
    assert_eq!(supervisor.status(), StatusSnapshot::Stopped);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_missing_or_blank_token_is_a_typed_refusal() {
    let state = tempfile::tempdir().expect("state dir");
    let port = free_port();
    let prepared = prepared(port);
    let supervisor = Supervisor::default();

    for token in [None, Some("   ")] {
        let config = config(state.path(), Path::new(STUB), token);
        let err = supervisor
            .launch_prepared(config.clone(), &prepared)
            .await
            .expect_err("no token, no launch");
        assert!(matches!(err, SupervisionError::TokenMissing), "{err:?}");
    }
    assert_eq!(supervisor.status(), StatusSnapshot::Stopped);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dead_stub_surfaces_as_a_typed_failure_within_the_poll_interval() {
    let state = tempfile::tempdir().expect("state dir");
    let wrappers = tempfile::tempdir().expect("wrapper dir");
    let port = free_port();
    let prepared = prepared(port);
    let supervisor = Supervisor::default();
    let binary = wrapper(wrappers.path(), "dying", "--stub-exit-after-secs 1");
    let config = config(state.path(), &binary, Some("tok"));

    let serving = supervisor
        .launch_prepared(config.clone(), &prepared)
        .await
        .expect("the stub is healthy before it dies");
    assert!(!serving.adopted);

    // H-P1.2: death surfaces within one poll interval (plus slack) —
    // never a silent Serving.
    wait_for_status(&supervisor, 10, |status| {
        matches!(status, StatusSnapshot::Failed { .. })
    })
    .await;
    match supervisor.status() {
        StatusSnapshot::Failed { reason } => assert!(reason.contains("exited"), "{reason}"),
        other => panic!("expected a typed failure, got {other:?}"),
    }
    // The dead server leaves no adoptable state behind.
    assert!(!state.path().join("launch-record.json").exists());
    assert!(!state.path().join("llama-server.key").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_foreign_port_holder_is_refused_not_adopted() {
    let state = tempfile::tempdir().expect("state dir");
    let port = free_port();
    let prepared = prepared(port);
    let supervisor = Supervisor::default();

    // A raw stub holds the port with NO launch record — the holder the
    // supervisor cannot verify as its own (H-P1.3's foreign arm).
    let mut foreign = tokio::process::Command::new(STUB)
        .args(["--port", &port.to_string()])
        .spawn()
        .expect("spawn the foreign stub");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let config = config(state.path(), Path::new(STUB), Some("tok"));
    let err = supervisor
        .launch_prepared(config.clone(), &prepared)
        .await
        .expect_err("an unverified holder is refused, never adopted");
    match err {
        SupervisionError::PortOccupied { holder } => {
            assert!(holder.contains("no launch record"), "{holder}");
        }
        other => panic!("expected a PortOccupied refusal, got {other:?}"),
    }
    assert_eq!(supervisor.status(), StatusSnapshot::Stopped);

    let _ = foreign.start_kill();
    let _ = foreign.wait().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_matching_live_server_is_adopted_and_stopped_by_pid() {
    let state = tempfile::tempdir().expect("state dir");
    let port = free_port();
    let prepared = prepared(port);

    // Session A launches; the editor "restarts" (a fresh supervisor
    // instance, the same state dir) while the server lives on — D-S3's
    // adoption: verified liveness + digest + health, never the file alone.
    let a = Supervisor::default();
    let config = config(state.path(), Path::new(STUB), Some("tok"));
    a.launch_prepared(config.clone(), &prepared)
        .await
        .expect("session A launches");

    let b = Supervisor::default();
    let adopted = b
        .launch_prepared(config.clone(), &prepared)
        .await
        .expect("session B adopts the verified live server");
    assert!(
        adopted.adopted,
        "no reload — the measured load bound argument"
    );

    // B's stop kills the adopted server by pid and cleans the state.
    b.stop().await.expect("stop the adopted server");
    assert_eq!(b.status(), StatusSnapshot::Stopped);
    assert!(!state.path().join("launch-record.json").exists());
    let rebound = std::net::TcpListener::bind(("127.0.0.1", port));
    assert!(rebound.is_ok(), "the adopted server stopped listening");
    // A (the stale session) sees the death as its own typed failure —
    // never a silent Serving.
    wait_for_status(&a, 10, |status| {
        matches!(
            status,
            StatusSnapshot::Failed { .. } | StatusSnapshot::Stopped
        )
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn launching_another_model_swaps_serving_models() {
    let state = tempfile::tempdir().expect("state dir");
    let port = free_port();
    let prepared = prepared(port);
    let supervisor = Supervisor::default();
    let config = config(state.path(), Path::new(STUB), Some("tok"));
    supervisor
        .launch_prepared(config.clone(), &prepared)
        .await
        .expect("gemma serves");

    // The explicit launch is the swap intent: launching another model
    // stops and drains the current server, then serves the new one —
    // one click, visible in the status, never a typed refusal and never
    // a silent substitution.
    let mut swapped = config;
    swapped.model_id = "qwen3-8-flash-next".into();
    let serving = supervisor
        .launch_prepared(swapped, &prepared)
        .await
        .expect("the explicit launch swaps to the requested model");
    assert_eq!(serving.model_id, "qwen3-8-flash-next");
    assert!(!serving.adopted, "a swap is a fresh spawn, not an adoption");
    assert_eq!(
        supervisor.status(),
        StatusSnapshot::Serving {
            model_id: "qwen3-8-flash-next".into(),
            endpoint: format!("http://127.0.0.1:{port}/v1"),
            adopted: false,
        }
    );
    // The record now names the swapped-in model.
    let record = std::fs::read_to_string(state.path().join("launch-record.json"))
        .expect("the swap wrote its own record");
    assert!(record.contains("\"model_id\":\"qwen3-8-flash-next\""));
    supervisor.stop().await.expect("cleanup stop");
    assert!(!state.path().join("launch-record.json").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_fresh_session_swaps_our_verified_server_by_pid() {
    let state = tempfile::tempdir().expect("state dir");
    let port = free_port();
    let prepared = prepared(port);

    // Session A serves gemma; the editor "restarts" — session B (fresh
    // state, same state dir) launches qwen: the record is OURS and
    // identity-verified (argv[0] == the recorded binary), so B swaps it
    // by pid, then serves qwen.
    let a = Supervisor::default();
    let config = config(state.path(), Path::new(STUB), Some("tok"));
    a.launch_prepared(config.clone(), &prepared)
        .await
        .expect("session A serves");

    let mut swapped = config;
    swapped.model_id = "qwen3-8-flash-next".into();
    let b = Supervisor::default();
    let serving = b
        .launch_prepared(swapped, &prepared)
        .await
        .expect("the fresh session swaps our verified server");
    assert_eq!(serving.model_id, "qwen3-8-flash-next");
    assert!(!serving.adopted, "the swap spawned a new server");

    // A sees its server die — a typed failure, never a silent Serving.
    wait_for_status(&a, 10, |status| {
        matches!(status, StatusSnapshot::Failed { .. })
    })
    .await;

    b.stop().await.expect("cleanup stop");
    assert!(!state.path().join("launch-record.json").exists());
    let rebound = std::net::TcpListener::bind(("127.0.0.1", port));
    assert!(rebound.is_ok(), "the swapped server stopped listening");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_foreign_pid_at_the_recorded_slot_is_never_killed() {
    let state = tempfile::tempdir().expect("state dir");
    let port = free_port();
    let prepared = prepared(port);
    let supervisor = Supervisor::default();

    // A stub holds the port (the occupied surface), and the record
    // claims a pid whose argv[0] is NOT the recorded binary — the
    // pid-recycling kill hazard shape. The swap path must refuse, and
    // the foreign process must SURVIVE.
    let mut foreign = tokio::process::Command::new("sleep")
        .arg("300")
        .spawn()
        .expect("spawn the foreign pid");
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let mut stub_holder = tokio::process::Command::new(STUB)
        .args(["--port", &port.to_string()])
        .spawn()
        .expect("spawn the port holder");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let record = hkask_private_inference::local_leg::supervisor::LaunchRecord {
        pid: foreign.id().unwrap_or(0),
        // A model DIFFERENT from the requested one — the swap branch:
        // the identity check must refuse (argv[0] is `sleep`, not the
        // recorded binary), never kill.
        model_id: "qwen3-8-flash-next".into(),
        plan_digest: "0".repeat(64),
        endpoint: format!("http://127.0.0.1:{port}/v1"),
        started_at_unix: 0,
        binary: STUB.to_string(),
        token_fingerprint: "0123456789abcdef".into(),
    };
    std::fs::write(
        state.path().join("launch-record.json"),
        serde_json::to_string(&record).expect("serializes"),
    )
    .expect("write the forged record");

    let config = config(state.path(), Path::new(STUB), Some("tok"));
    let err = supervisor
        .launch_prepared(config, &prepared)
        .await
        .expect_err("an unverified pid is never swapped or killed");
    match err {
        SupervisionError::PortOccupied { holder } => {
            assert!(holder.contains("cannot verify"), "{holder}");
        }
        other => panic!("expected a PortOccupied refusal, got {other:?}"),
    }

    // The foreign process is untouched — the pid-recycling guard held.
    let foreign_alive = std::fs::metadata(format!("/proc/{}", record.pid)).is_ok();
    assert!(
        foreign_alive,
        "a foreign pid at the recorded slot is never killed"
    );

    let _ = foreign.start_kill();
    let _ = foreign.wait().await;
    let _ = stub_holder.start_kill();
    let _ = stub_holder.wait().await;
}
