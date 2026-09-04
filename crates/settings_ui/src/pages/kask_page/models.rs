//! Models sub-page — kask-wide model defaults (default inference model,
//! embedding model, classifier model, OCR model, rerank model).

use super::*;

pub(crate) fn render_models_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    // Resolve via `From` so the UI shows the same defaults the runtime uses.
    let models: kask_bridge::KaskModelsSettings = raw
        .and_then(|c| c.models)
        .map(Into::into)
        .unwrap_or_default();
    let default_model = models.default_model;
    let embedding_model = models.embedding_model;
    let classifier_model = models.classifier_model;
    let ocr_model = models.ocr_model;
    let rerank_model = models.rerank_model;

    let default_model_input = kask_string_input(
        "kask-models-default",
        "Default Inference Model",
        "Provider/model-id (required — no hidden default)",
        default_model,
        "models",
        "default_model",
    );
    let embedding_model_input = kask_string_input(
        "kask-models-embedding",
        "Embedding Model",
        "Provider/model-id (required — no hidden default)",
        embedding_model,
        "models",
        "embedding_model",
    );
    let classifier_model_input = kask_string_input(
        "kask-models-classifier",
        "Classifier Model",
        "Provider/model-id (required — no hidden default)",
        classifier_model,
        "models",
        "classifier_model",
    );
    let ocr_model_input = kask_string_input(
        "kask-models-ocr",
        "OCR Model",
        "Provider/model-id (required — no hidden default)",
        ocr_model,
        "models",
        "ocr_model",
    );
    let rerank_model_input = kask_string_input(
        "kask-models-rerank",
        "Rerank Model",
        "Provider/model-id (required — no hidden default)",
        rerank_model,
        "models",
        "rerank_model",
    );

    v_flex()
        .id("kask-models-page")
        .size_full()
        .pt_2p5()
        .px_8()
        .pb_16()
        .gap_4()
        .overflow_y_scroll()
        .track_scroll(scroll_handle)
        .child(
            v_flex()
                .gap_1()
                .child(SettingsSectionHeader::new("Models"))
                .child(
                    Label::new(
                        "Kask-wide model configuration. These provider-prefixed model \
                         names (e.g. \"openrouter/z-ai/glm-5.2\") override the kask \
                         defaults for inference, embedding, classification, OCR, and \
                         rerank.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Default Inference Model"))
                .child(
                    Label::new(
                        "Provider-prefixed model for the Curator, skill execution, and \
                         kask panel inference. Leave empty to use the kask default \
                         (openrouter/z-ai/glm-5.2).",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(default_model_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Embedding Model"))
                .child(
                    Label::new(
                        "Provider-prefixed model for corpus indexing and memory semantic \
                         recall. Leave empty to fall back to the corpus MCP server's \
                         embedding_model setting, then to the kask default.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(embedding_model_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Classifier Model"))
                .child(
                    Label::new(
                        "Provider-prefixed model for guard/regulation classification \
                         tasks. Leave empty to use the kask default.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(classifier_model_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("OCR Model"))
                .child(
                    Label::new(
                        "Provider-prefixed model for scanned document OCR. \
                         Leave empty to use the kask default (RunPod/kask-ocr).",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(ocr_model_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Rerank Model"))
                .child(
                    Label::new(
                        "Provider-prefixed model for the research server's deep-search \
                         rerank stage (per-candidate relevance scoring). Leave empty to \
                         use the kask default (OpenRouter/qwen/qwen3-reranker-8b).",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(rerank_model_input),
        )
        .into_any_element()
}
