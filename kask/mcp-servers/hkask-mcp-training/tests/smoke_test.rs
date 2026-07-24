//! Live smoke test — verifies end-to-end training completion detection.
//!
//! Supports three GPU hosts: DeepInfra (cheapest H100), Nebius, and Runpod.
//! The host is auto-detected from env vars (DI_API_KEY, NEBIUS_PROJECT_ID,
//! RUNPOD_API_KEY) or overridden via HKASK_TRAINING_HOST.
//!
//! Run with:
//!   set -a && source .env && set +a && \
//!   cargo test --test smoke_test -- --ignored --nocapture
//!
//! To force a specific host:
//!   HKASK_TRAINING_HOST=deepinfra cargo test --test smoke_test -- --ignored --nocapture
//!
//! Requires: HF_TOKEN, HKASK_HF_ARTIFACT_OWNER, HKASK_HF_DATASET_REPO,
//!           HKASK_HF_MODEL_REPO, plus host-specific credentials:
//!           - DeepInfra: DI_API_KEY
//!           - Nebius:    NEBIUS_PROJECT_ID, NEBIUS_SUBNET_ID
//!           - Runpod:    RUNPOD_API_KEY, RUNPOD_TEMPLATE_ID

use hkask_inference::InferenceConfig;
use hkask_mcp_training::TrainingServer;
use hkask_mcp_training::adapter::AdapterStore;
use hkask_mcp_training::adapters::JobStore;
use hkask_mcp_training::dataset::DatasetPipeline;
use hkask_mcp_training::providers::types::LoraInit;
use hkask_mcp_training::providers::{
    TrainingHarnessId, TrainingHostConfig, TrainingHostId, TrainingParams, create_host,
};
use hkask_mcp_training::types::{TrainStatusRequest, TrainSubmitRequest};
use hkask_storage::database::sqlite::SqliteDriver;
use hkask_types::WebID;
use rmcp::handler::server::wrapper::Parameters;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Parse the success envelope `{"content": <value>}`; falls back to the raw value.
fn parse_content(out: &str) -> serde_json::Value {
    let v: serde_json::Value = serde_json::from_str(out).expect("tool output is JSON");
    v.get("content").cloned().unwrap_or(v)
}

/// Auto-detect the training host from env vars, or use HKASK_TRAINING_HOST override.
fn detect_host() -> TrainingHostId {
    if let Ok(h) = std::env::var("HKASK_TRAINING_HOST") {
        if let Some(id) = TrainingHostId::from_str(&h) {
            return id;
        }
        panic!("HKASK_TRAINING_HOST={h} is not a valid host (runpod|deepinfra|nebius)");
    }
    // Auto-detect: prefer DeepInfra (cheapest H100), then Nebius, then Runpod.
    if std::env::var("DI_API_KEY").is_ok() {
        TrainingHostId::DeepInfra
    } else if std::env::var("NEBIUS_PROJECT_ID").is_ok() {
        TrainingHostId::Nebius
    } else if std::env::var("RUNPOD_API_KEY").is_ok() {
        TrainingHostId::Runpod
    } else {
        panic!(
            "No training host configured — set DI_API_KEY, NEBIUS_PROJECT_ID, or RUNPOD_API_KEY"
        );
    }
}

/// Build the host config for the selected host.
fn build_host_config(host: TrainingHostId) -> TrainingHostConfig {
    match host {
        TrainingHostId::Runpod => TrainingHostConfig {
            host,
            runpod_api_key: std::env::var("RUNPOD_API_KEY").expect("RUNPOD_API_KEY"),
            runpod_template_id: std::env::var("RUNPOD_TEMPLATE_ID").unwrap_or_default(),
            runpod_gpu_type_id: std::env::var("RUNPOD_GPU_TYPE_ID").unwrap_or_default(),
            runpod_container_disk_gb: std::env::var("RUNPOD_CONTAINER_DISK_GB")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),
            runpod_min_memory_gb: std::env::var("RUNPOD_MIN_MEMORY_GB")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),
            runpod_min_vcpu_count: std::env::var("RUNPOD_MIN_VCPU_COUNT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),
            runpod_docker_image: std::env::var("RUNPOD_DOCKER_IMAGE").unwrap_or_default(),
        },
        // DeepInfra and Nebius read their config from env vars at create_host time.
        TrainingHostId::DeepInfra | TrainingHostId::Nebius => TrainingHostConfig {
            host,
            ..Default::default()
        },
    }
}

#[tokio::test]
#[ignore = "requires live GPU host + HuggingFace credentials (~$0.50-2 GPU cost)"]
async fn smoke_test_training_completion() {
    // ── 0. Detect host ────────────────────────────────────────────────────
    let host_id = detect_host();
    let host_label = match host_id {
        TrainingHostId::DeepInfra => "DeepInfra ($3.69/hr B200-180GB, pre-built PyTorch)",
        TrainingHostId::Nebius => "Nebius ($3.85/hr H100, $2.15/hr preemptible)",
        TrainingHostId::Runpod => "Runpod ($2.39/hr H100, pip install required)",
    };
    eprintln!("=== Training Smoke Test ===");
    eprintln!("Host: {host_label}");

    // ── 1. Create a tiny dataset (10 ChatML samples) ──────────────────────
    let dataset_path = std::env::temp_dir().join("hkask_smoke_test_dataset.jsonl");
    let mut dataset = String::new();
    for i in 0..10 {
        let record = serde_json::json!({
            "messages": [
                {"role": "user", "content": format!("What is {i}?")},
                {"role": "assistant", "content": format!("{i} is a number between {i} and {}.", i + 1)}
            ]
        });
        dataset.push_str(&record.to_string());
        dataset.push('\n');
    }
    std::fs::write(&dataset_path, &dataset).expect("write dataset");
    eprintln!("Dataset written: {} (10 samples)", dataset_path.display());

    // ── 2. Set up the TrainingServer with real credentials ────────────────
    let pool = SqliteDriver::in_memory_pool().expect("in-memory pool");
    let driver: Arc<dyn hkask_storage::database::driver::DatabaseDriver> =
        Arc::new(SqliteDriver::new(pool.clone()));
    let adapter_store = Arc::new(AdapterStore::from_driver(driver));
    let job_store = JobStore::new(pool).expect("job store");

    let host_config = build_host_config(host_id);
    let host = create_host(&host_config).expect("create host");

    let server = TrainingServer::new(
        WebID::new(),
        "smoke-test-userpod".into(),
        None,
        None,
        host,
        host_id,
        TrainingHarnessId::Axolotl,
        Mutex::new(DatasetPipeline::new(PathBuf::from(
            "/tmp/hkask-smoke-cache",
        ))),
        adapter_store,
        Some(job_store),
        None,
        InferenceConfig::default(),
    );

    // ── 3. Submit a minimal training job ──────────────────────────────────
    let mut params = TrainingParams::default();
    params.lora.init_lora_weights = Some(LoraInit::Eva);
    params.lora.r = 8;
    params.lora.alpha = 16;
    params.num_epochs = 1;
    params.batch_size = 1;
    params.learning_rate = 1e-4;
    params.optimization.gradient_accumulation_steps = 4;
    params.optimization.lr_scheduler = Some("cosine".to_string());
    params.sequence.sequence_len = Some(2048);
    params.advanced.bf16 = true;
    params.advanced.eval_split_ratio = Some(0.1);

    // Use a small model — Qwen2.5-0.5B-Instruct (~1GB, downloads fast)
    let req: TrainSubmitRequest = serde_json::from_value(serde_json::json!({
        "dataset_path": dataset_path.to_string_lossy(),
        "base_model": "Qwen/Qwen2.5-0.5B-Instruct",
        "params": params,
        "feedback_path": null,
        "skill_name": null,
        "adapter_name": null,
        "merged_output_path": null
    }))
    .expect("deserialize TrainSubmitRequest");

    eprintln!("Submitting training job (Qwen2.5-0.5B, 10 samples, 1 epoch, Axolotl)...");
    let submit_result = server.training_submit(Parameters(req)).await;
    let submit_content = parse_content(&submit_result);

    if submit_content.get("error").is_some() || submit_content.get("detail").is_some() {
        eprintln!("Submit failed: {submit_content}");
        panic!("training_submit failed");
    }

    let job_id = submit_content["job_id"]
        .as_str()
        .expect("job_id in submit response");
    let provider_job_id = submit_content["provider_job_id"]
        .as_str()
        .unwrap_or("unknown");
    eprintln!("Job submitted successfully!");
    eprintln!("  job_id: {job_id}");
    eprintln!("  provider_job_id: {provider_job_id}");

    // ── 4. Poll training_status until completion or timeout ───────────────
    // Expected timeline on H100 with 0.5B model:
    //   DeepInfra: pod creation ~1 min, no pip install, model download ~1 min,
    //              training ~2-5 min, manifest upload ~30 sec → ~5-10 min
    //   Nebius:    VM creation ~2-3 min, pip install ~10-15 min, model download ~1 min,
    //              training ~2-5 min, manifest upload ~30 sec → ~15-25 min
    //   Runpod:    pod creation ~1-2 min, pip install ~10-20 min, model download ~1 min,
    //              training ~2-5 min, manifest upload ~30 sec → ~15-30 min
    // Timeout: 45 min (covers worst case: Runpod with slow pip install)
    let timeout = tokio::time::Instant::now() + tokio::time::Duration::from_secs(2700);
    let mut last_status = String::new();
    let mut poll_count = 0u32;
    let mut ssh_seen = false;

    loop {
        if tokio::time::Instant::now() >= timeout {
            eprintln!("\nTIMEOUT after 45 minutes. Last status: {last_status}");
            eprintln!("Cancelling pod to stop billing...");
            let cancel_req: hkask_mcp_training::types::TrainCancelRequest =
                serde_json::from_value(serde_json::json!({"job_id": job_id}))
                    .expect("deserialize cancel");
            let _ = server.training_cancel(Parameters(cancel_req)).await;
            eprintln!("Pod cancelled.");
            panic!("timeout waiting for training completion");
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
        poll_count += 1;

        let status_req: TrainStatusRequest =
            serde_json::from_value(serde_json::json!({"job_id": job_id}))
                .expect("deserialize status");
        let status_result = server.training_status(Parameters(status_req)).await;
        let status_content = parse_content(&status_result);

        let status = status_content["status"].as_str().unwrap_or("unknown");
        if status != last_status {
            eprintln!(
                "\n[poll #{poll_count}] {time} — Status: {status}",
                time = chrono::Utc::now().format("%H:%M:%S UTC")
            );
            if status == "running" && last_status.is_empty() {
                eprintln!("  Pod is running. Waiting for training to complete...");
            }
            last_status = status.to_string();
        } else {
            eprint!(".");
            use std::io::Write;
            std::io::stderr().flush().ok();
        }

        // Verify SSH info is present once the pod is running (P12: debuggability)
        if status == "running" && !ssh_seen {
            let ssh = status_content["ssh_command"].as_str().unwrap_or("");
            let ip = status_content["ip"].as_str().unwrap_or("");
            if !ssh.is_empty() {
                eprintln!("\n  ✅ SSH available: {ssh}");
                eprintln!("     Public IP: {ip}");
                ssh_seen = true;
            }
        }

        if status == "completed" {
            eprintln!("\n\n=== TRAINING COMPLETED ===");
            eprintln!("Full status response:\n{status_content}");

            // Verify SSH info was available (every pod must be debuggable)
            assert!(
                ssh_seen,
                "SSH command must be available in status response (P12: debuggability)"
            );

            // Verify adapter was auto-registered
            let adapter_registered = status_content["adapter_registered"]
                .as_bool()
                .unwrap_or(false);
            assert!(
                adapter_registered,
                "adapter must be auto-registered on completion"
            );
            eprintln!("\n✅ Adapter auto-registered successfully!");

            if let Some(repo) = status_content["adapter_repository"].as_str() {
                eprintln!("  adapter_repository: {repo}");
            }
            if let Some(path) = status_content["adapter_path"].as_str() {
                eprintln!("  adapter_path: {path}");
            }
            if let Some(name) = status_content["adapter_name"].as_str() {
                eprintln!("  adapter_name: {name}");
            }

            // Cancel the pod to stop billing
            eprintln!("\nCancelling pod to stop billing...");
            let cancel_req: hkask_mcp_training::types::TrainCancelRequest =
                serde_json::from_value(serde_json::json!({"job_id": job_id}))
                    .expect("deserialize cancel");
            let _ = server.training_cancel(Parameters(cancel_req)).await;
            eprintln!("Pod cancelled. Smoke test PASSED on {host_label}!");
            break;
        } else if status == "failed" {
            eprintln!("\n\n=== TRAINING FAILED ===");
            eprintln!("Full status response:\n{status_content}");

            // Print SSH info for debugging
            if let Some(ssh) = status_content["ssh_command"].as_str()
                && !ssh.is_empty()
            {
                eprintln!("\nSSH into the pod to inspect logs: {ssh}");
            }

            // Cancel the pod to stop billing
            let cancel_req: hkask_mcp_training::types::TrainCancelRequest =
                serde_json::from_value(serde_json::json!({"job_id": job_id}))
                    .expect("deserialize cancel");
            let _ = server.training_cancel(Parameters(cancel_req)).await;
            panic!("training failed — check pod logs via SSH for details");
        }
    }
}
