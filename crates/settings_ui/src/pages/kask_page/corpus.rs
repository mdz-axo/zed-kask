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

    let embedding_model_input = kask_string_input(
        "kask-corpus-embedding-model",
        "Embedding Model",
        "DeepInfra/Qwen/Qwen3-Embedding-0.6B",
        embedding_model,
        "corpus",
        "embedding_model",
    );
    let template_root_input = kask_string_input(
        "kask-corpus-template-root",
        "Template Root",
        "registry",
        template_root,
        "corpus",
        "template_root",
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
        .into_any_element()
}
