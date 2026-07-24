//! Contract tests for hkask-mcp-corpus — persona and gather tool invariants.
//!
//! Ported from the former hkask-mcp-replica/tests/replica_contract.rs.
//! Tests the public Parameters<T> seam for persona and gather tools.
//!
//! Tested seams:
//! - `cosine_distance` (pure function from hkask-services)
//! - `ProbContractRunner` (probabilistic contract verification)
//! - `corpus_explain` (static info tool)
//! - `corpus_build_persona` (error path: missing config)
//! - `corpus_cache_work` (file write + slug validation)

use hkask_inference::{EmbeddingRouter, InferenceConfig, InferenceRouter};
use hkask_mcp_corpus::CorpusServer;
use hkask_mcp_corpus::ocr::ThresholdConfig;
use hkask_services_compose::cosine_distance;
use hkask_test_harness::ProbContractRunner;
use hkask_types::WebID;
use proptest::prelude::*;
use rmcp::handler::server::wrapper::Parameters;
use std::sync::{Arc, Mutex};

// ── cosine_distance invariants (deterministic contracts) ─────────────────────

#[test]
fn cosine_distance_identity_is_zero() {
    let v = vec![1.0_f32, 2.0, 3.0];
    let d = cosine_distance(&v, &v);
    assert!(
        (d - 0.0).abs() < 1e-6,
        "identical vectors should have distance 0.0, got {d}"
    );
}

#[test]
fn cosine_distance_orthogonal_is_one() {
    let d = cosine_distance(&[1.0_f32, 0.0], &[0.0_f32, 1.0]);
    assert!(
        (d - 1.0).abs() < 1e-6,
        "orthogonal vectors should have distance 1.0, got {d}"
    );
}

#[test]
fn cosine_distance_opposite_is_two() {
    let d = cosine_distance(&[1.0_f32], &[-1.0_f32]);
    assert!(
        (d - 2.0).abs() < 1e-6,
        "opposite vectors should have distance 2.0, got {d}"
    );
}

#[test]
fn cosine_distance_empty_is_two() {
    let d = cosine_distance(&[], &[1.0_f32]);
    assert!(
        (d - 2.0).abs() < 1e-6,
        "empty vectors should return 2.0, got {d}"
    );
}

#[test]
fn cosine_distance_mismatched_is_two() {
    let d = cosine_distance(&[1.0_f32, 2.0], &[3.0_f32]);
    assert!(
        (d - 2.0).abs() < 1e-6,
        "mismatched dimensions should return 2.0, got {d}"
    );
}

proptest! {
    #[test]
    fn cosine_distance_is_symmetric(
        (x1, y1, z1, x2, y2, z2) in (
            0.1f32..10.0f32, 0.1f32..10.0f32, 0.1f32..10.0f32,
            0.1f32..10.0f32, 0.1f32..10.0f32, 0.1f32..10.0f32,
        )
    ) {
        let a = vec![x1, y1, z1];
        let b = vec![x2, y2, z2];
        let d_ab = cosine_distance(&a, &b);
        let d_ba = cosine_distance(&b, &a);
        prop_assert!((d_ab - d_ba).abs() < 1e-6,
            "cosine distance not symmetric: d(a,b)={} d(b,a)={}", d_ab, d_ba);
    }
}

#[test]
fn cosine_distance_zero_norm_is_two() {
    let d = cosine_distance(&[0.0_f32, 0.0], &[1.0_f32, 2.0]);
    assert!(
        (d - 2.0).abs() < 1e-6,
        "zero-norm vector should return 2.0, got {d}"
    );
}

// ── Probabilistic contract: centroid distance ordering ──────────────────────

fn author_centroids() -> Vec<(&'static str, Vec<f32>)> {
    vec![
        ("gentle", vec![1.0_f32, 0.0, 0.0, 1.0]),
        ("hemingway", vec![0.0_f32, 1.0, 0.0, 1.0]),
        ("woolf", vec![0.0_f32, 0.0, 1.0, 1.0]),
    ]
}

#[test]
fn centroid_distance_ordering_is_prob_contract_strong() {
    let centroids = author_centroids();
    let gentle = &centroids[0].1;
    let hemingway = &centroids[1].1;
    let woolf = &centroids[2].1;

    let runner = ProbContractRunner::new(0.95, 0.05, 0);

    let result = runner.evaluate(
        200,
        || {
            let mut rng = rand::rng();
            vec![
                1.0_f32 + (rng.random::<f32>() - 0.5) * 0.6,
                0.0_f32 + (rng.random::<f32>() - 0.5) * 0.6,
                0.0_f32 + (rng.random::<f32>() - 0.5) * 0.6,
                1.0_f32 + (rng.random::<f32>() - 0.5) * 0.6,
            ]
        },
        |test_vec| {
            let d_gentle = cosine_distance(test_vec, gentle);
            let d_hemingway = cosine_distance(test_vec, hemingway);
            let d_woolf = cosine_distance(test_vec, woolf);
            d_gentle < d_hemingway && d_gentle < d_woolf
        },
    );

    assert!(
        result.passed,
        "centroid distance ordering failed: {}/{} trials passed (rate: {:.3}, need >= {:.3})",
        result.successes, result.trials, result.actual_rate, result.target_rate
    );
}

#[test]
fn centroid_distance_ordering_fails_on_noise() {
    let runner = ProbContractRunner::new(0.90, 0.0, 0);

    let result = runner.evaluate(
        100,
        || {
            let mut rng = rand::rng();
            vec![
                rng.random::<f32>(),
                rng.random::<f32>(),
                rng.random::<f32>(),
                rng.random::<f32>(),
            ]
        },
        |test_vec| {
            let centroids = author_centroids();
            let d_gentle = cosine_distance(test_vec, &centroids[0].1);
            let d_hemingway = cosine_distance(test_vec, &centroids[1].1);
            d_gentle < d_hemingway
        },
    );

    assert!(
        !result.passed,
        "random vectors should NOT pass the centroid ordering contract (rate: {:.3})",
        result.actual_rate
    );
}

// ── Mashup monotonicity (probabilistic variant) ─────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]
    #[test]
    fn mashup_monotonicity_probabilistic(
        (angle_a, angle_b) in (0.1f64..5.0f64, 0.1f64..5.0f64)
    ) {
        let diff = (angle_a - angle_b).abs();
        if diff < 0.3 {
            return Ok(()); // skip near-identical vectors
        }

        let a = vec![angle_a.cos() as f32, angle_a.sin() as f32];
        let b = vec![angle_b.cos() as f32, angle_b.sin() as f32];

        let runner = ProbContractRunner::new(0.90, 0.05, 2);
        let result = runner.evaluate(50,
            || {
                let blend: f64 = rand::rng().random::<f64>();
                let blended: Vec<f32> = a.iter().zip(b.iter())
                    .map(|(x, y)| (*x as f64 * (1.0 - blend) + *y as f64 * blend) as f32)
                    .collect();
                let d_a = cosine_distance(&blended, &a);
                let d_b = cosine_distance(&blended, &b);
                (d_a, d_b, blend)
            },
            |(d_a, d_b, blend)| {
                if *blend > 0.5 {
                    d_a > d_b
                } else {
                    d_a < d_b
                }
            },
        );

        prop_assert!(result.passed,
            "mashup monotonicity failed: {}/{} trials (rate: {:.3}, need >= {:.3})",
            result.successes, result.trials, result.actual_rate, result.target_rate);
    }
}

// ── Self-consistency: identity under probabilistic contract ──────────────────

#[test]
fn self_consistency_under_prob_contract() {
    let a = vec![1.0_f32, 2.0, 3.0, 4.0];
    let runner = ProbContractRunner::new(0.99, 0.0, 0);
    let result = runner.evaluate(50, || a.clone(), |v| cosine_distance(&a, v) < 1e-6);
    assert!(
        result.passed,
        "self-consistency failed: {}/{} trials (rate: {:.3})",
        result.successes, result.trials, result.actual_rate
    );
}

#[test]
fn recovery_window_rescues_failing_contract() {
    let call_count = std::sync::atomic::AtomicU32::new(0);
    let runner = ProbContractRunner::new(0.99, 0.0, 9);
    let result = runner.evaluate(
        30,
        || {
            call_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            call_count
                .load(std::sync::atomic::Ordering::Relaxed)
                .is_multiple_of(2)
        },
        |b| *b,
    );
    assert!(
        result.passed,
        "recovery should rescue contract: {}/{} trials (rate: {:.3})",
        result.successes, result.trials, result.actual_rate
    );
}

// ── Tool-behavior contract tests (Parameters<T> seam) ───────────────────────

fn test_server() -> CorpusServer {
    let inference_config = InferenceConfig::from_env();
    let inference_router = Arc::new(InferenceRouter::new(inference_config.clone()));
    let embedding_router = EmbeddingRouter::new(inference_config);
    let llm_ocr = Arc::new(hkask_mcp_corpus::ocr::llm_ocr::LlmOcrExecutor::new(
        Arc::clone(&inference_router),
    ));
    let pipeline_executor = Arc::new(hkask_mcp_corpus::ocr::PipelineExecutor::new(Arc::clone(
        &llm_ocr,
    )));
    CorpusServer::new(
        WebID::new(),
        "test-userpod".into(),
        None,
        None,
        inference_router,
        ThresholdConfig::default(),
        Some(embedding_router),
        Mutex::new(Vec::new()),
        Mutex::new(Vec::new()),
        llm_ocr,
        pipeline_executor,
    )
}

fn parse_content(out: &str) -> serde_json::Value {
    let v: serde_json::Value = serde_json::from_str(out).expect("tool output is JSON");
    v.get("content").cloned().unwrap_or(v)
}

fn error_kind(out: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(out).expect("tool output is JSON");
    v.get("kind").and_then(|e| e.as_str()).map(String::from)
}

#[tokio::test]
async fn corpus_explain_returns_info_via_parameters_seam() {
    let server = test_server();
    let out = server.corpus_explain().await;
    let content = parse_content(&out);
    assert!(
        content.is_object(),
        "explain should return a JSON object: {out}"
    );
}

#[tokio::test]
async fn corpus_build_persona_rejects_missing_config_via_parameters_seam() {
    let server = test_server();
    let req: hkask_mcp_corpus::tools::persona::BuildRequest =
        serde_json::from_value(serde_json::json!({
            "config_path": "/nonexistent/corpus.yaml",
            "db_path": "/tmp/test.db",
            "passphrase": null
        }))
        .expect("deserialize BuildRequest");
    let out = server.corpus_build_persona(Parameters(req)).await;
    let kind = error_kind(&out).expect("expected error kind for missing config");
    assert_eq!(kind, "invalid_argument", "got: {out}");
}

#[tokio::test]
async fn corpus_cache_work_writes_file_via_parameters_seam() {
    let server = test_server();
    let dir = tempfile::tempdir().expect("tempdir");
    let req: hkask_mcp_corpus::tools::gather::CacheWorkRequest =
        serde_json::from_value(serde_json::json!({
            "slug": "test-work",
            "content": "This is test content for caching.",
            "cache_dir": dir.path().to_string_lossy()
        }))
        .expect("deserialize CacheWorkRequest");
    let out = server.corpus_cache_work(Parameters(req)).await;
    let content = parse_content(&out);
    let bytes = content["bytes_written"].as_u64().expect("bytes_written");
    assert!(bytes > 0, "should write bytes: {out}");
    let cached_path = dir.path().join("test-work.txt");
    assert!(
        cached_path.exists(),
        "cache file should exist: {cached_path:?}"
    );
}

#[tokio::test]
async fn corpus_cache_work_rejects_bad_slug_via_parameters_seam() {
    let server = test_server();
    let dir = tempfile::tempdir().expect("tempdir");
    let req: hkask_mcp_corpus::tools::gather::CacheWorkRequest =
        serde_json::from_value(serde_json::json!({
            "slug": "../escape-attempt",
            "content": "malicious",
            "cache_dir": dir.path().to_string_lossy()
        }))
        .expect("deserialize CacheWorkRequest");
    let out = server.corpus_cache_work(Parameters(req)).await;
    let kind = error_kind(&out).expect("expected error kind for bad slug");
    assert_eq!(kind, "invalid_argument", "got: {out}");
}
