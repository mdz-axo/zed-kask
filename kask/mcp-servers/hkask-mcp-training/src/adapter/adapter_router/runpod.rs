//! Runpod adapter backend — serverless vLLM endpoint provisioning.
//!
//! Docs: <https://docs.runpod.io/serverless/endpoints/manage-endpoints>

use super::AdapterProviderBackend;
use super::openai::openai_compatible_infer;
use crate::adapter::adapter_config::AdapterConfig;
use crate::adapter::adapter_port::AdapterError;
use crate::adapter::adapter_store::TrainedLoRAAdapter;
use crate::adapter::provider_cost::{CostModel, ProviderCapability};
use hkask_types::InferenceResult;
use hkask_types::template::LLMParameters;

pub(super) struct RunpodAdapterBackend {
    cost_model: CostModel,
    capability: ProviderCapability,
    api_key: String,
    client: reqwest::Client,
}

impl RunpodAdapterBackend {
    pub(super) fn new() -> Self {
        Self {
            cost_model: CostModel::runpod(),
            capability: ProviderCapability::runpod(),
            api_key: std::env::var("RUNPOD_API_KEY").unwrap_or_default(),
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait::async_trait]
impl AdapterProviderBackend for RunpodAdapterBackend {
    async fn provision_endpoint(
        &self,
        adapter: &TrainedLoRAAdapter,
    ) -> Result<String, AdapterError> {
        if self.api_key.is_empty() {
            return Err(AdapterError::ProviderUnavailable(
                "RUNPOD_API_KEY not set".into(),
            ));
        }

        let template_id = std::env::var("RUNPOD_TEMPLATE_ID").unwrap_or_default();
        if template_id.is_empty() {
            return Err(AdapterError::ProviderUnavailable(
                "RUNPOD_TEMPLATE_ID not set — required for serverless endpoint provisioning".into(),
            ));
        }

        // The serverless endpoint URL is keyed by the template id (which is also
        // the endpoint id in Runpod's v2 REST surface). The template's startup
        // script is responsible for pulling the adapter from the HuggingFace
        // repo referenced by `adapter.source.repository_id()` at cold start —
        // see `upload_adapter` for the resolution contract.
        let endpoint_url = format!("https://api.runpod.ai/v2/{}/openai/v1", template_id);
        tracing::info!(
            target: "reg.adapter",
            adapter_id = %adapter.id,
            template_id = %template_id,
            endpoint_url = %endpoint_url,
            "Provisioned Runpod serverless endpoint"
        );
        tracing::info!(
            target: "reg.training.provider.runpod.provision",
            template_id = %template_id,
            "RunPod endpoint provisioned"
        );
        Ok(endpoint_url)
    }

    async fn infer(
        &self,
        endpoint_url: &str,
        prompt: &str,
        params: &LLMParameters,
        model_name: &str,
    ) -> Result<InferenceResult, AdapterError> {
        openai_compatible_infer(
            &self.client,
            &self.api_key,
            endpoint_url,
            prompt,
            params,
            model_name,
        )
        .await
    }

    async fn teardown(&self, _endpoint_url: &str) -> Result<(), AdapterError> {
        // RunPod serverless endpoints scale to zero when idle — no explicit
        // teardown is needed. The previous implementation HTTP-DELETEd the
        // inference URL, which was wrong (you don't DELETE an OpenAI-compatible
        // endpoint; serverless endpoints are managed by RunPod's infra).
        //
        // If this backend is ever extended to support dedicated pods (not
        // serverless), teardown should call the GraphQL `podTerminate` mutation
        // with the pod ID — see RunpodHost::drain_all_pods in the training server.
        tracing::info!(
            target: "reg.adapter",
            "Runpod serverless endpoint scales to zero automatically — no teardown needed"
        );
        tracing::info!(
            target: "reg.training.provider.runpod.teardown",
            "RunPod endpoint teardown"
        );
        Ok(())
    }

    async fn upload_adapter(
        &self,
        adapter: &TrainedLoRAAdapter,
        _config: &AdapterConfig,
    ) -> Result<String, AdapterError> {
        // Runpod serverless endpoints load LoRA adapters at container start via
        // the template's startup script (e.g. vLLM `--lora-modules name=adapter
        // repo=<hf_repo>`). The adapter weights must already be published to
        // Hugging Face Hub — we return the HF repo id as the `model_name` that
        // callers pass to the OpenAI-compatible inference endpoint, which vLLM
        // resolves to the LoRA module configured in the template.
        //
        // There is no upload step: the serverless template pulls the adapter
        // from HF at cold start. To deploy a new adapter, update the template's
        // startup script (or env) to reference the new HF repo and redeploy.
        let model_name = adapter.source.repository_id().to_string();
        tracing::info!(
            target: "reg.adapter",
            adapter_id = %adapter.id,
            model_name = %model_name,
            "Runpod adapter resolved to HuggingFace repo (pulled by serverless template at cold start)"
        );
        tracing::info!(
            target: "reg.training.provider.runpod.upload",
            adapter_id = %adapter.id,
            "RunPod adapter uploaded"
        );
        Ok(model_name)
    }

    fn capability(&self) -> ProviderCapability {
        self.capability.clone()
    }

    fn cost_model(&self) -> CostModel {
        self.cost_model.clone()
    }
}
