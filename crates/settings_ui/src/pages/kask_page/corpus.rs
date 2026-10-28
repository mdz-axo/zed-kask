//! Corpus sub-page — embedding model, dimension, and template root.
//!
//! OCR model selection lives on the Models page (`kask.models.ocr_model`).
//! The corpus server gates OCR output quality deterministically; this page
//! has no OCR backend-routing or threshold controls.

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

    let embedding_model_input = kask_string_input(
        "kask-corpus-embedding-model",
        "Embedding Model",
        "Provider/model-id (required — no hidden default)",
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
                         OCR, and QA generation. Configure the embedding settings; \
                         the OCR model is configured on the Models page and OCR \
                         output quality is gated automatically (no thresholds).",
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
                    Label::new(
                        "The embedding model. Required — empty means not configured; \
                         calls that need it fail with a visible error (no hidden default).",
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
        .into_any_element()
}
