//! Research model discovery: registry-confirmed vision models and configured STT.
use crate::types::{MediaModelInfo, ModelInfoRequest, ModelListRequest};
use crate::*;

/// The registry's only analysis capability flag is vision. Do not infer audio
/// support from model names; transcription uses the explicitly configured STT model.
fn build_model_list(
    registry: Vec<hkask_types::ModelEntry>,
    stt: Option<String>,
    vision: Option<String>,
) -> Vec<MediaModelInfo> {
    let mut models = Vec::new();
    if let Some(id) = stt {
        models.push(MediaModelInfo {
            name: strip_provider_prefix(&id).to_string(),
            provider: parse_provider(&id),
            id,
            modality: "audio".to_string(),
            capabilities: vec!["transcribe".to_string()],
            is_default: true,
            description: Some("Configured speech-to-text transcription model".to_string()),
        });
    }
    for entry in registry.into_iter().filter(|entry| entry.supports_vision) {
        if models
            .iter()
            .any(|model| model.id == entry.prefixed_name && model.modality == "vision")
        {
            continue;
        }
        models.push(MediaModelInfo {
            name: entry.model,
            provider: parse_provider(&entry.prefixed_name),
            is_default: vision.as_deref() == Some(entry.prefixed_name.as_str()),
            id: entry.prefixed_name,
            modality: "vision".to_string(),
            capabilities: vec![
                "describe_image".to_string(),
                "gallery_analyze".to_string(),
                "video_caption".to_string(),
            ],
            description: Some(
                "Registry-confirmed vision model for source-media analysis".to_string(),
            ),
        });
    }
    models
}

fn parse_provider(prefixed: &str) -> String {
    prefixed
        .split_once('/')
        .map(|(provider, _)| provider.to_lowercase())
        .unwrap_or_else(|| "unknown".to_string())
}

fn strip_provider_prefix(prefixed: &str) -> &str {
    prefixed
        .split_once('/')
        .map_or(prefixed, |(_, model)| model)
}

impl MediaServer {
    async fn research_models(&self) -> Result<Vec<MediaModelInfo>, McpToolError> {
        let registry =
            self.vision_port.list_models().await.map_err(|error| {
                classify_inference_error("Research model discovery failed", error)
            })?;
        Ok(build_model_list(
            registry,
            models::stt_model(),
            models::vision_model(),
        ))
    }
}

#[tool_router(router = models_router, vis = "pub")]
impl MediaServer {
    #[tool(
        description = "List research media models: configured speech-to-text and registry-confirmed vision analysis models. Optionally filter by provider. Registry failures are surfaced; audio-analysis capabilities not declared by the registry are not inferred."
    )]
    pub async fn model_list(
        &self,
        Parameters(ModelListRequest { provider }): Parameters<ModelListRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "model_list", async {
            let mut models = self.research_models().await?;
            if let Some(filter) = provider.filter(|filter| !filter.is_empty()) {
                models.retain(|model| model.provider.eq_ignore_ascii_case(&filter));
            }
            serde_json::to_value(models)
                .map_err(|error| McpToolError::internal(format!("encode model list: {error}")))
        })
        .await
    }

    #[tool(
        description = "Get details for a research media model returned by model_list (speech-to-text or registry-confirmed vision analysis)."
    )]
    pub async fn model_info(
        &self,
        Parameters(ModelInfoRequest { model_id }): Parameters<ModelInfoRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "model_info", async {
            let model = self.research_models().await?
                .into_iter()
                .find(|model| model.id == model_id)
                .ok_or_else(|| McpToolError::not_found(format!(
                    "Research model not found: {model_id}. Call model_list to see available models."
                )))?;
            serde_json::to_value(model)
                .map_err(|error| McpToolError::internal(format!("encode model info: {error}")))
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct RegistryPort {
        fail: bool,
    }

    impl InferencePort for RegistryPort {
        fn generate(
            &self,
            _: &str,
            _: &hkask_types::template::LLMParameters,
            _: Option<&[hkask_types::ChatToolDefinition]>,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>,
                    > + Send
                    + '_,
            >,
        > {
            panic!("model discovery must not generate content")
        }

        fn list_models(
            &self,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<Vec<hkask_types::ModelEntry>, hkask_types::InferenceError>,
                    > + Send
                    + '_,
            >,
        > {
            Box::pin(async move {
                if self.fail {
                    Err(hkask_types::InferenceError::Connection(
                        "registry sentinel".into(),
                    ))
                } else {
                    Ok(vec![
                        entry("Provider/registered-vision", true),
                        entry("Provider/text-only", false),
                    ])
                }
            })
        }
    }

    fn registry_server(fail: bool) -> Result<MediaServer, Box<dyn std::error::Error>> {
        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        Ok(MediaServer::new(
            hkask_types::WebID::new(),
            Arc::new(RegistryPort { fail }),
            Arc::new(Mutex::new(None)),
            Arc::new(GalleryStore::from_driver(driver)?),
            templates::create_env()?,
            FfmpegRunner::detect(),
            YtDlpRunner::detect(),
            crate::jobs::new_job_store(),
            None,
            Default::default(),
        ))
    }

    /// expect: Model tools enumerate the actual registry, model_info can inspect
    /// its entries, and bridge failures remain errors rather than empty lists.
    #[tokio::test]
    async fn model_tools_use_registry_and_surface_discovery_failure()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = registry_server(false)?;
        let response = server
            .model_list(Parameters(ModelListRequest {
                provider: Some("PROVIDER".into()),
            }))
            .await?;
        let payload =
            hkask_types::tool_response::unwrap_tool_envelope(serde_json::from_str(&response)?);
        assert!(
            payload
                .as_array()
                .expect("model list")
                .iter()
                .any(|entry| entry["id"] == "Provider/registered-vision")
        );
        let response = server
            .model_info(Parameters(ModelInfoRequest {
                model_id: "Provider/registered-vision".into(),
            }))
            .await?;
        let payload =
            hkask_types::tool_response::unwrap_tool_envelope(serde_json::from_str(&response)?);
        assert_eq!(payload["modality"], "vision");
        let error = server
            .model_info(Parameters(ModelInfoRequest {
                model_id: "Provider/text-only".into(),
            }))
            .await
            .expect_err("non-vision registry model excluded");
        assert_eq!(error.kind, hkask_types::McpErrorKind::NotFound);
        let broken = registry_server(true)?;
        for error in [
            broken
                .model_list(Parameters(ModelListRequest { provider: None }))
                .await
                .expect_err("registry failure"),
            broken
                .model_info(Parameters(ModelInfoRequest {
                    model_id: "Provider/registered-vision".into(),
                }))
                .await
                .expect_err("registry failure"),
        ] {
            assert_eq!(error.kind, hkask_types::McpErrorKind::Unavailable);
            assert!(error.message.contains("registry sentinel"));
        }
        Ok(())
    }

    fn entry(id: &str, supports_vision: bool) -> hkask_types::ModelEntry {
        hkask_types::ModelEntry {
            prefixed_name: id.into(),
            model: strip_provider_prefix(id).into(),
            supports_vision,
        }
    }

    /// expect: Research discovery only claims capabilities supported by the
    /// registry or explicit transcription configuration, never creative operations.
    #[test]
    fn discovery_uses_registry_capabilities_and_explicit_defaults() {
        let models = build_model_list(
            vec![
                entry("Provider/vision", true),
                entry("Provider/text", false),
                entry("Provider/vision", true),
                entry("Provider/other-vision", true),
            ],
            Some("Provider/stt".into()),
            Some("Provider/vision".into()),
        );
        assert_eq!(models.len(), 3);
        assert_eq!(models[0].capabilities, ["transcribe"]);
        assert!(models[0].is_default);
        assert_eq!(models[1].id, "Provider/vision");
        assert!(models[1].is_default);
        assert!(!models[2].is_default);
        for model in models {
            assert_eq!(model.provider, "provider");
            assert!(model.capabilities.iter().all(|op| {
                [
                    "transcribe",
                    "describe_image",
                    "gallery_analyze",
                    "video_caption",
                ]
                .contains(&op.as_str())
            }));
        }
    }

    /// expect: Unregistered configured vision names do not masquerade as
    /// confirmed registry capabilities, and no hidden defaults appear.
    #[test]
    fn empty_registry_does_not_invent_vision_or_audio_models() {
        assert!(build_model_list(Vec::new(), None, Some("Provider/absent".into())).is_empty());
        assert_eq!(parse_provider("no-prefix"), "unknown");
        assert_eq!(
            strip_provider_prefix("Provider/family/model"),
            "family/model"
        );
    }
}
