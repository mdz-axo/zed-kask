//! Corpus sub-page — embedding model, OCR pipeline, and template root.

use super::*;

pub(crate) fn render_corpus_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    // Resolve via `From` so the UI shows the same defaults the runtime uses.
    let corpus: kask_bridge::KaskCorpusSettings = raw
        .and_then(|c| c.corpus)
        .map(Into::into)
        .unwrap_or_default();
    let embedding_model = corpus.embedding_model;
    let template_root = corpus.template_root;
    let embedding_dim = corpus.embedding_dim.to_string();
    let ocr_concurrency = corpus.ocr_concurrency.to_string();
    let ocr_simple_max = corpus.ocr_simple_max.to_string();
    let ocr_moderate_max = corpus.ocr_moderate_max.to_string();
    let ocr_sample_rate = corpus.ocr_sample_rate.to_string();
    let ocr_tuneable = corpus.ocr_tuneable;

    let embedding_model_input = kask_string_input(
        "kask-corpus-embedding-model",
        "Embedding Model",
        "ollama/nomic-embed-text",
        embedding_model,
        "corpus",
        "embedding_model",
    );
    let template_root_input = kask_string_input(
        "kask-corpus-template-root",
        "Template Root",
        "kask/registry",
        template_root,
        "corpus",
        "template_root",
    );
    let embedding_dim_input = kask_string_input(
        "kask-corpus-embedding-dim",
        "Embedding Dimension",
        "1024",
        embedding_dim,
        "corpus",
        "embedding_dim",
    );
    let ocr_concurrency_input = kask_string_input(
        "kask-corpus-ocr-concurrency",
        "OCR Concurrency",
        "4",
        ocr_concurrency,
        "corpus",
        "ocr_concurrency",
    );
    let ocr_simple_max_input = kask_string_input(
        "kask-corpus-ocr-simple-max",
        "OCR Simple Max",
        "0.05",
        ocr_simple_max,
        "corpus",
        "ocr_simple_max",
    );
    let ocr_moderate_max_input = kask_string_input(
        "kask-corpus-ocr-moderate-max",
        "OCR Moderate Max",
        "0.15",
        ocr_moderate_max,
        "corpus",
        "ocr_moderate_max",
    );
    let ocr_sample_rate_input = kask_string_input(
        "kask-corpus-ocr-sample-rate",
        "OCR Sample Rate",
        "0.10",
        ocr_sample_rate,
        "corpus",
        "ocr_sample_rate",
    );
    let ocr_tuneable_toggle = SwitchField::new(
        "kask-corpus-ocr-tuneable",
        Some("OCR Tuneable"),
        Some(
            "Whether OCR tuneable mode is enabled. When enabled, the OCR pipeline \
             adapts processing depth per page. Or set HKASK_OCR_TUNEABLE."
                .into(),
        ),
        if ocr_tuneable {
            ToggleState::Selected
        } else {
            ToggleState::Unselected
        },
        move |state, _window, cx| {
            let value = *state == ToggleState::Selected;
            SettingsStore::global(cx).update_settings_file(
                <dyn fs::Fs>::global(cx),
                move |settings, _| {
                    settings
                        .kask
                        .get_or_insert_default()
                        .corpus
                        .get_or_insert_default()
                        .ocr_tuneable = Some(value);
                },
            );
        },
    )
    .tab_index(0);

    v_flex()
        .id("kask-corpus-page")
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
                .child(SettingsSectionHeader::new("Corpus"))
                .child(
                    Label::new(
                        "The corpus server provides document corpus management, \
                         OCR, and QA generation. Configure embedding and OCR settings."
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Embedding Model"))
                .child(
                    Label::new("Override the embedding model (e.g., DI/Qwen/Qwen3-Embedding-0.6B). Leave empty for default.")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(embedding_model_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Template Root"))
                .child(
                    Label::new("Root directory for Jinja2 templates. Default: registry.")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(template_root_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Embedding Dimension"))
                .child(
                    Label::new(
                        "Embedding vector dimensionality. Must match the embedding \
                         model's output. 0 is treated as the default. Or set \
                         HKASK_EMBEDDING_DIM.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(embedding_dim_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("OCR Concurrency"))
                .child(
                    Label::new(
                        "Number of document pages sent to the vision model in parallel. \
                         0 is treated as the default. Or set HKASK_OCR_CONCURRENCY.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(ocr_concurrency_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("OCR Simple Max"))
                .child(
                    Label::new(
                        "OCR simple threshold (0.0–1.0). Pages scored below this are \
                         processed with the simple pipeline. Or set HKASK_OCR_SIMPLE_MAX.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(ocr_simple_max_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("OCR Moderate Max"))
                .child(
                    Label::new(
                        "OCR moderate threshold (0.0–1.0). Pages above simple but below \
                         this are processed with the moderate pipeline. Or set \
                         HKASK_OCR_MODERATE_MAX.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(ocr_moderate_max_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("OCR Sample Rate"))
                .child(
                    Label::new(
                        "OCR moderate sample rate (0.0–1.0). Fraction of moderate pages \
                         sampled for processing. Or set HKASK_OCR_SAMPLE_RATE.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(ocr_sample_rate_input),
        )
        .child(Divider::horizontal())
        .child(ocr_tuneable_toggle)
        .into_any_element()
}
