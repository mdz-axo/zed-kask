//! Models sub-page — kask-wide model defaults (default inference model,
//! embedding model, classifier model, dedicated QA generator, OCR model, rerank model).

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
    let qa_generation_model = models.qa_generation_model;
    let ocr_model = models.ocr_model;
    let rerank_model = models.rerank_model;
    // The code defaults (what applies when a field is left empty) — rendered
    // from the `Default` impls so the help text can never drift from the
    // actual defaults (operator ruling 2026-09-04: defaults in code,
    // settings override).
    let code_defaults = kask_bridge::KaskModelsSettings::default();
    let embedding_code_default = kask_bridge::KaskSettings::default().corpus.embedding_model;
    let placeholder = "Provider/model-id (leave empty to use the code default)";

    let default_model_input = kask_string_input(
        "kask-models-default",
        "Default Inference Model",
        placeholder,
        default_model,
        "models",
        "default_model",
    );
    let embedding_model_input = kask_string_input(
        "kask-models-embedding",
        "Embedding Model",
        placeholder,
        embedding_model,
        "models",
        "embedding_model",
    );
    let classifier_model_input = kask_string_input(
        "kask-models-classifier",
        "Classifier Model",
        placeholder,
        classifier_model,
        "models",
        "classifier_model",
    );
    let qa_generation_model_input = kask_string_input(
        "kask-models-qa-generation",
        "QA Generation Model",
        "Provider/model-id (required for QA generation)",
        qa_generation_model,
        "models",
        "qa_generation_model",
    );
    let ocr_model_input = kask_string_input(
        "kask-models-ocr",
        "OCR Model",
        placeholder,
        ocr_model,
        "models",
        "ocr_model",
    );
    let rerank_model_input = kask_string_input(
        "kask-models-rerank",
        "Rerank Model",
        placeholder,
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
                    Label::new(format!(
                        "Kask-wide model configuration. These provider-prefixed model \
                         names (e.g. \"{}\") override the kask \
                         defaults for inference, embedding, classification, OCR, and \
                         rerank; QA generation requires its own model.",
                        code_defaults.default_model
                    ))
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
                    Label::new(format!(
                        "Provider-prefixed model for the Curator, skill execution, and \
                         kask panel inference. Leave empty to use the kask default \
                         ({}).",
                        code_defaults.default_model
                    ))
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
                    Label::new(format!(
                        "Provider-prefixed model for corpus indexing and memory semantic \
                         recall. Leave empty to fall back to the corpus MCP server's \
                         embedding_model setting, then to the kask default ({}).",
                        embedding_code_default
                    ))
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
                    Label::new(format!(
                        "Provider-prefixed model for guard/regulation classification \
                         tasks. Leave empty to use the kask default ({}).",
                        code_defaults.classifier_model
                    ))
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(classifier_model_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("QA Generation Model"))
                .child(
                    Label::new(
                        "Dedicated non-thinking generator for corpus QA. Explicit tool model \
                         overrides win. Independent of chat and the training base model. \
                         Empty or invalid configuration fails visibly; no fallback.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(qa_generation_model_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("OCR Model"))
                .child(
                    Label::new(format!(
                        "Provider-prefixed model for scanned document OCR. \
                         Leave empty to use the kask default ({}).",
                        code_defaults.ocr_model
                    ))
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
                         rerank stage (per-candidate relevance scoring). Defaults to \
                         deepinfra/Qwen/Qwen3-Reranker-8B.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(rerank_model_input),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use fs::Fs;

    /// D9: the displayed control and its generic save dispatcher must agree.
    #[test]
    fn qa_generation_control_has_a_save_dispatch_arm() {
        let page = include_str!("models.rs");
        let dispatcher = include_str!("../kask_page.rs");
        assert!(page.contains(concat!("\"models\",\n        ", "\"qa_generation_model\",")));
        assert!(dispatcher.contains(concat!("(\"models\", ", "\"qa_generation_model\") => {")));
        assert!(dispatcher.contains(concat!(
            "kask.models.get_or_insert_default().",
            "qa_generation_model ="
        )));
    }

    /// D9: the rerank control suffered the missing-dispatch-arm defect — the
    /// input rendered but its value never reached the settings file. Pin both
    /// the arm and the control so the pair cannot drift apart again.
    #[test]
    fn rerank_control_has_a_save_dispatch_arm() {
        let page = include_str!("models.rs");
        let dispatcher = include_str!("../kask_page.rs");
        assert!(page.contains(concat!("\"models\",\n        ", "\"rerank_model\",")));
        assert!(dispatcher.contains(concat!("(\"models\", ", "\"rerank_model\") => {")));
        assert!(dispatcher.contains(concat!(
            "kask.models.get_or_insert_default().",
            "rerank_model ="
        )));
    }

    /// Exercise the host's real settings file writer and reload path on FakeFs;
    /// no user settings or provider is touched. Rendered UI interaction is separate.
    #[gpui::test]
    async fn qa_generation_model_persists_and_reloads(cx: &mut gpui::TestAppContext) {
        let fs = fs::FakeFs::new(cx.executor());
        fs.create_dir(paths::settings_file().parent().expect("settings directory"))
            .await
            .expect("create fake settings directory");
        fs.insert_file(paths::settings_file(), b"{}".to_vec()).await;
        cx.update(settings::init);
        for value in ["OpenRouter/~openai/gpt-sol-latest", ""] {
            let completion = cx.update(|cx| {
                SettingsStore::global(cx).update_settings_file_with_completion(
                    fs.clone(),
                    move |settings, _| {
                        settings
                            .kask
                            .get_or_insert_default()
                            .models
                            .get_or_insert_default()
                            .qa_generation_model = Some(value.into());
                    },
                )
            });
            completion
                .await
                .expect("writer finished")
                .expect("settings saved");
            let saved = fs
                .load(paths::settings_file())
                .await
                .expect("read saved settings");
            cx.update(|cx| {
                let mut reloaded = SettingsStore::new(cx, &settings::default_settings());
                reloaded
                    .set_user_settings(&saved, cx)
                    .expect("reload saved settings");
                cx.set_global(reloaded);
                let content = raw_kask_settings(cx).expect("persisted Kask settings");
                let resolved: kask_bridge::KaskSettings = content.into();
                assert_eq!(resolved.models.qa_generation_model, value);
                let env = resolved.mcp_env();
                assert_eq!(
                    env.get("HKASK_QA_GENERATION_MODEL").map(String::as_str),
                    (!value.is_empty()).then_some(value)
                );
            });
        }
    }

    /// Same writer/reload path for the rerank model — the field whose save
    /// dispatch arm was missing (the persistence defect this test pins).
    #[gpui::test]
    async fn rerank_model_persists_and_reloads(cx: &mut gpui::TestAppContext) {
        let fs = fs::FakeFs::new(cx.executor());
        fs.create_dir(paths::settings_file().parent().expect("settings directory"))
            .await
            .expect("create fake settings directory");
        fs.insert_file(paths::settings_file(), b"{}".to_vec()).await;
        cx.update(settings::init);
        for value in ["deepinfra/Qwen/Qwen3-Reranker-8B", ""] {
            let completion = cx.update(|cx| {
                SettingsStore::global(cx).update_settings_file_with_completion(
                    fs.clone(),
                    move |settings, _| {
                        settings
                            .kask
                            .get_or_insert_default()
                            .models
                            .get_or_insert_default()
                            .rerank_model = Some(value.into());
                    },
                )
            });
            completion
                .await
                .expect("writer finished")
                .expect("settings saved");
            let saved = fs
                .load(paths::settings_file())
                .await
                .expect("read saved settings");
            cx.update(|cx| {
                let mut reloaded = SettingsStore::new(cx, &settings::default_settings());
                reloaded
                    .set_user_settings(&saved, cx)
                    .expect("reload saved settings");
                cx.set_global(reloaded);
                let content = raw_kask_settings(cx).expect("persisted Kask settings");
                let resolved: kask_bridge::KaskSettings = content.into();
                assert_eq!(resolved.models.rerank_model, value);
                let env = resolved.mcp_env();
                assert_eq!(
                    env.get("HKASK_RERANK_MODEL").map(String::as_str),
                    (!value.is_empty()).then_some(value)
                );
            });
        }
    }
}
