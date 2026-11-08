//! The live P1 launch (slice plan step 9, the server-side half — the
//! agent-panel turn stays the operator's checkpoint): the FULL
//! production path — fleet resolve → real shard parse → live hardware
//! probe → guard-verified compile → startup probe against the installed
//! binary → spawn → health-wait → a chat completion → stop. Absent
//! `HKASK_P1_LIVE` the test skips: no fabricated evidence.
//!
//! ```sh
//! HKASK_P1_LIVE=1 cargo test -p hkask-private-inference \
//!   --test live_supervisor -- --ignored --nocapture
//! ```
//!
//! The token is test-supplied (the crate never reads credentials — the
//! bridge resolves the keychain slot in production); the test writes
//! it, the server accepts it, the client sends it — one self-consistent
//! chain.

use std::path::Path;
use std::time::Instant;

use hkask_private_inference::local_leg::placement::{PlacementPolicy, probe_backend};
use hkask_private_inference::local_leg::supervisor::{LaunchConfig, StatusSnapshot, Supervisor};

const MODEL_ID: &str = "qwen3-8-flash-next";
const TOKEN: &str = "p1-live-test-token";

fn models_root() -> std::path::PathBuf {
    hkask_types::agent_paths::resolve_under_artifacts_dir(Path::new("private-inference/models"))
}

fn briefs_dir() -> std::path::PathBuf {
    hkask_types::agent_paths::resolve_under_artifacts_dir(Path::new(
        "private-inference/model-briefs",
    ))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn the_full_production_launch_serves_and_stops_on_the_real_machine() {
    if std::env::var("HKASK_P1_LIVE")
        .map(|v| v != "1")
        .unwrap_or(true)
    {
        eprintln!("HKASK_P1_LIVE unset; skipping the live launch");
        return;
    }

    // The production resolution: the backend probe over the canonical
    // engine root (the repaired path — this asserts it finds the real
    // HIP build, the dead-probe defect's live counter-evidence).
    let backend = probe_backend();
    let binary = match &backend {
        hkask_private_inference::local_leg::placement::GpuBackend::Hip { binary }
        | hkask_private_inference::local_leg::placement::GpuBackend::Vulkan { binary } => {
            binary.clone()
        }
        other => panic!("no engine build at the canonical root: {other:?}"),
    };
    eprintln!("[p1-live] backend: {backend:?}");

    // The discovered briefs dir is seeded before the launch — the
    // production startup path's act, idempotent here.
    hkask_private_inference::local_leg::fleet::seed_briefs(&briefs_dir())
        .expect("the compiled-in seed writes");

    let config = LaunchConfig {
        model_id: MODEL_ID.into(),
        binary,
        api_key: Some(TOKEN.into()),
        state_dir: hkask_types::agent_paths::resolve_under_data_dir(Path::new("private-inference")),
        models_root: models_root(),
        briefs_dir: briefs_dir(),
        policy: PlacementPolicy::default(),
    };
    let supervisor = Supervisor::default();

    let started = Instant::now();
    let serving = supervisor
        .launch(config)
        .await
        .expect("the full production path launches to healthy");
    eprintln!(
        "[p1-live] serving {} at {} after {:.0}s",
        serving.model_id,
        serving.endpoint,
        started.elapsed().as_secs_f32()
    );
    assert!(!serving.adopted, "a clean start is a fresh launch");
    assert_eq!(
        supervisor.status(),
        StatusSnapshot::Serving {
            model_id: MODEL_ID.into(),
            endpoint: serving.endpoint.clone(),
            adopted: false
        }
    );

    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{}", 8010);

    // The identity seam: /v1/models lists the alias the provider cards
    // advertise — the served name IS the card name.
    let models: serde_json::Value = client
        .get(format!("{base}/v1/models"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .expect("the models listing responds")
        .json()
        .await
        .expect("the listing parses");
    let ids: Vec<&str> = models["data"]
        .as_array()
        .expect("a data array")
        .iter()
        .map(|m| m["id"].as_str().unwrap_or_default())
        .collect();
    eprintln!("[p1-live] served ids: {ids:?}");
    assert!(
        ids.iter().any(|id| id.contains(MODEL_ID)),
        "the served alias must match the card name"
    );

    // One chat completion through the OpenAI-compatible surface — the
    // same route the agent panel dispatches over. Qwen3.8 is a
    // thinking model: the content field can be empty while the
    // reasoning carries the tokens, so the assertion is on the
    // measured completion usage (inference HAPPENED), with both
    // fields printed for the record.
    let completion: serde_json::Value = client
        .post(format!("{base}/v1/chat/completions"))
        .bearer_auth(TOKEN)
        .json(&serde_json::json!({
            "model": MODEL_ID,
            "messages": [{"role": "user", "content": "Reply with exactly: OK"}],
            "max_tokens": 96,
            "temperature": 0,
        }))
        .send()
        .await
        .expect("the completion responds")
        .json()
        .await
        .expect("the completion parses");
    let text = completion["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("");
    let reasoning = completion["choices"][0]["message"]["reasoning_content"]
        .as_str()
        .unwrap_or("");
    let completion_tokens = completion["usage"]["completion_tokens"]
        .as_u64()
        .unwrap_or(0);
    eprintln!(
        "[p1-live] completion ({} tokens): content={text:?} reasoning={}",
        completion_tokens,
        if reasoning.is_empty() {
            "<none>"
        } else {
            "<present>"
        }
    );
    assert!(
        completion_tokens > 0,
        "the completion carries generated tokens (content or reasoning)"
    );

    // Stop drains; the port frees; the state cleans.
    supervisor.stop().await.expect("stop drains");
    assert_eq!(supervisor.status(), StatusSnapshot::Stopped);
    assert!(
        !hkask_types::agent_paths::resolve_under_data_dir(Path::new("private-inference"))
            .join("launch-record.json")
            .exists(),
        "the launch record is cleaned"
    );
    assert!(
        tokio::net::TcpStream::connect(("127.0.0.1", 8010))
            .await
            .is_err(),
        "the server stopped listening"
    );
    eprintln!(
        "[p1-live] stopped clean; total wall {:.0}s",
        started.elapsed().as_secs_f32()
    );
}
