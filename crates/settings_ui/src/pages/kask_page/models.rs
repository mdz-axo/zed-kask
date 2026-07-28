//! Models sub-page — kask-wide model defaults (default inference model,
//! embedding model, classifier model).

use super::*;

pub(crate) fn render_models_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    let models = raw.and_then(|c| c.models).unwrap_or_default();
    let default_model = models.default_model.unwrap_or_default();
    let embedding_model = models.embedding_model.unwrap_or_default();
    let classifier_model = models.classifier_model.unwrap_or_default();

    let default_model_input = kask_string_input(
        "kask-models-default",
        "Default Inference Model",
        "openrouter/z-ai/glm-5.2",
        default_model,
        "models",
        "default_model",
    );
    let embedding_model_input = kask_string_input(
        "kask-models-embedding",
        "Embedding Model",
        "openrouter/z-ai/glm-5.2",
        embedding_model,
        "models",
        "embedding_model",
    );
    let classifier_model_input = kask_string_input(
        "kask-models-classifier",
        "Classifier Model",
        "openrouter/z-ai/glm-5.2",
        classifier_model,
        "models",
        "classifier_model",
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
                         defaults for inference, embedding, and classification. When \
                         fusion is enabled, the fusion judge model takes precedence \
                         for inference.",
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
                        "Provider-prefixed model for the Curator, skill cascade, and \
                         kask panel inference. Leave empty to use the kask default \
                         (openrouter/z-ai/glm-5.2). When fusion is enabled, the fusion \
                         judge model takes precedence.",
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
        .into_any_element()
}
