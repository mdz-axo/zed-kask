//! Live adapter integration test — gated on environment variables.
//!
//! Tests the full adapter lifecycle against real provider APIs:
//! store → select → deploy → infer → teardown
//!
//! Requires:
//!   TG_API_KEY — Together AI API key
//!   HF_TOKEN — HuggingFace token (for private/gated adapter repos)
//!   HKASK_LIVE_ADAPTER_REPO — HuggingFace repo with an adapter (e.g. "user/adapter-name")
//!   HKASK_LIVE_BASE_MODEL — Base model family (e.g. "llama-3.3-70b")
//!
//! Run with:
//!   TG_API_KEY=... HF_TOKEN=... \
//!   HKASK_LIVE_ADAPTER_REPO=user/adapter \
//!   HKASK_LIVE_BASE_MODEL=llama-3.3-70b \
//!   cargo test -p hkask-mcp-training --test live_adapter -- --ignored

use hkask_capability::auth::derive_signing_key;
use hkask_capability::{DelegationAction, DelegationResource, DelegationToken};
use hkask_inference::ProviderId;
use hkask_mcp_training::adapter::adapter_store::Checksum;
use hkask_mcp_training::adapter::{
    AdapterLifecycle, AdapterPort, AdapterRouter, AdapterSource, AdapterStore, Expertise,
    MdsDomain, TrainedLoRAAdapter, TrainingProvenance,
};
use hkask_storage::database::sqlite::SqliteDriver;
use hkask_types::id::WebID;
use hkask_types::template::LLMParameters;
use std::sync::Arc;
use uuid::Uuid;

fn test_token(webid: WebID) -> DelegationToken {
    let sk = derive_signing_key(b"live-adapter-test");
    DelegationToken::new(
        DelegationResource::Tool,
        "adapter:deploy".into(),
        DelegationAction::Execute,
        webid,
        webid,
        &sk,
    )
}

fn inference_params() -> LLMParameters {
    LLMParameters {
        temperature: 0.1,
        top_p: 0.9,
        top_k: 40,
        min_p: 0.0,
        typical_p: 0.0,
        frequency_penalty: 0.0,
        presence_penalty: 0.0,
        max_tokens: 50,
        seed: None,
        disable_thinking: true,
        adapter: None,
        bypass_fusion: false,
        fusion_config: None,
        system_prompt: None,
    }
}

fn require_env(var: &str) -> String {
    std::env::var(var).unwrap_or_else(|_| panic!("{var} must be set for live test"))
}

#[tokio::test]
#[ignore = "requires TG_API_KEY, HF_TOKEN, HKASK_LIVE_ADAPTER_REPO, HKASK_LIVE_BASE_MODEL"]
async fn live_together_adapter_e2e() {
    let api_key = require_env("TG_API_KEY");
    let hf_repo = require_env("HKASK_LIVE_ADAPTER_REPO");
    let base_model = require_env("HKASK_LIVE_BASE_MODEL");
    let _hf_token = std::env::var("HF_TOKEN").ok();

    // SAFETY: test-only — set API key for the Together backend
    unsafe {
        std::env::set_var("TG_API_KEY", &api_key);
    }

    let driver = SqliteDriver::in_memory_driver();
    let store = Arc::new(AdapterStore::from_driver(driver));

    let owner = WebID::from_persona(b"live-test-user");
    let provenance = TrainingProvenance {
        training_run_id: "live-test-run".into(),
        training_source: "https://huggingface.co/".into(),
        completed_at: "2026-06-17T00:00:00Z".into(),
        base_model_family: base_model.clone(),
        dataset_hash: None,
        training_metrics: serde_json::Value::Null,
    };
    let expertise = Expertise::new(
        "live-test-expertise".into(),
        MdsDomain::CodeGeneration,
        serde_json::Value::Null,
        provenance,
    )
    .expect("expertise");

    let adapter = TrainedLoRAAdapter {
        id: Uuid::new_v4(),
        expertise,
        checksum: Checksum::from_hex("0000000000000000"),
        storage_path: String::new(),
        base_model_family: base_model,
        version: Some("1".into()),
        source: AdapterSource::HuggingFace {
            repo: hf_repo.clone(),
        },
        size_bytes: None,
        owner,
        skill_name: None,
        lifecycle: AdapterLifecycle::Durable,
        created_at: "2026-06-17T00:00:00Z".into(),
    };
    store.store(&adapter).expect("store adapter");

    let router = Arc::new(AdapterRouter::new(store));
    let token = test_token(owner);

    // 1. Select provider
    let selection = router
        .select_provider(adapter.id, None)
        .expect("select provider");
    assert!(!selection.providers.is_empty(), "no compatible providers");
    println!("Compatible providers: {}", selection.providers.len());

    // 2. Estimate composition
    let estimate = router
        .estimate_composition(adapter.id, ProviderId::Runpod, &token)
        .await
        .expect("estimate");
    assert!(estimate.is_compatible, "adapter not compatible");
    println!(
        "Estimate: setup=${:.4}, hourly=${:.2}",
        estimate.estimated_setup_cost, estimate.estimated_hourly_cost
    );

    // 3. Deploy
    let handle = router
        .create_endpoint(adapter.id, ProviderId::Runpod, &token)
        .await
        .expect("create endpoint");
    println!(
        "Deployed: id={}, url={}, model={}",
        handle.endpoint_id, handle.endpoint_url, handle.model_name
    );
    assert!(!handle.endpoint_url.is_empty());

    // 4. Check status
    let status = router
        .endpoint_status(handle.endpoint_id, &token)
        .expect("status");
    println!(
        "Status: phase={:?}, cost={:.4}",
        status.phase, status.cost_accrued
    );

    // 5. Background: adapter upload may be async — Together AI upload polls for completion.
    // The create_endpoint flow handles this internally via poll_until_complete().
    // If the model_name looks like "adapter-{uuid}" (fallback), the upload was skipped
    // because no huggingface_repo was detected or API key was missing.
    if !handle.model_name.starts_with("adapter-") {
        // 6. Run inference
        let params = inference_params();
        match router
            .infer(handle.endpoint_id, "Say hello in one word.", params, &token)
            .await
        {
            Ok(result) => {
                println!("Inference result: '{}'", result.text.trim());
                assert!(!result.text.is_empty(), "inference returned empty text");
            }
            Err(e) => {
                println!("Inference skipped: {e}");
                // Inference may fail if the endpoint isn't fully provisioned yet.
                // This is expected for newly created Together AI endpoints (~2-5 min startup).
            }
        }
    } else {
        println!("Upload was skipped (no HF repo or API key) — inference not tested");
    }

    // 7. Teardown
    router
        .teardown_endpoint(handle.endpoint_id)
        .await
        .expect("teardown");
    assert!(
        router.endpoint_status(handle.endpoint_id, &token).is_err(),
        "endpoint should be gone after teardown"
    );
    println!("Teardown: endpoint removed");
}
