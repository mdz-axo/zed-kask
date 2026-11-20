//! Private Inference sub-page — the policy a privacy-marked thread reads
//! from `kask.private_inference`.
//!
//! Private inference is served by the local Ollama or llama.cpp install
//! through Zed's built-in providers; kask launches no model server and
//! registers no provider. This page shows which local models a private
//! thread can run right now, and edits what kask still owns: the MCP
//! servers disabled on private threads and the local models that embed,
//! tag and rerank private memory.

use super::*;

/// The local provider that serves private memory (embedding, tagging,
/// rerank). The bridge wires these legs to its Ollama descriptor only, so
/// the rows refuse any other prefix where the operator types it; the
/// wiring-time refusal stays the enforcement home.
const PRIVATE_MEMORY_PROVIDER: &str = "ollama";

fn split_csv(text: &str) -> Vec<String> {
    text.split(',')
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .collect()
}

/// Parse a CSV row. An empty confirm is NO write: an empty list is not the
/// same as an absent override.
fn parse_csv_row(text: &str) -> Option<Vec<String>> {
    let parsed = split_csv(text);
    (!parsed.is_empty()).then_some(parsed)
}

/// Parse a private-memory model row. Empty is the explicit opt-out and is
/// written as the empty string; anything else must be `ollama/<model>`.
/// A failed parse never rewrites the settings file.
fn parse_private_memory_model(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Some(String::new());
    }
    let (prefix, model) = trimmed.split_once('/')?;
    (prefix.eq_ignore_ascii_case(PRIVATE_MEMORY_PROVIDER) && !model.is_empty())
        .then(|| trimmed.to_string())
}

/// The `{provider}/{model}` labels a private thread may run, from
/// `(provider id, model id, model name)` triples — the same admission
/// decision the thread's gates use (`settings::is_private_thread_local_model`).
fn private_thread_model_labels(
    models: impl IntoIterator<Item = (String, String, String)>,
) -> Vec<String> {
    models
        .into_iter()
        .filter(|(provider, id, _)| settings::is_private_thread_local_model(provider, id))
        .map(|(provider, _, name)| format!("{provider}/{name}"))
        .collect()
}

/// The local models the registered Ollama and llama.cpp providers offer
/// right now. A registry read, never an HTTP call on render.
fn available_private_thread_models(cx: &App) -> Vec<String> {
    let Some(registry) = language_model::LanguageModelRegistry::try_read_global(cx) else {
        return Vec::new();
    };
    let models = settings::PRIVATE_THREAD_LOCAL_PROVIDERS
        .iter()
        .filter_map(|id| {
            registry.provider(&language_model::LanguageModelProviderId::from(
                id.to_string(),
            ))
        })
        .flat_map(|provider| provider.provided_models(cx))
        .map(|model| {
            (
                model.provider_id.0.to_string(),
                model.id.0.to_string(),
                model.name.0.to_string(),
            )
        });
    private_thread_model_labels(models)
}

pub(crate) fn render_private_inference_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    // Resolve via `From` so the UI shows the same defaults the runtime
    // uses (the models-page pattern).
    let policy: kask_bridge::KaskPrivateInferenceSettings = raw
        .as_ref()
        .and_then(|c| c.private_inference.clone())
        .map(Into::into)
        .unwrap_or_default();

    let available = available_private_thread_models(cx);
    let available_line = if available.is_empty() {
        "No local model is available right now — start Ollama or a llama.cpp server \
         and pull a model."
            .to_string()
    } else {
        format!("Available now: {}.", available.join(", "))
    };

    let disabled_servers_input = kask_input_field(
        "kask-private-inference-disabled-servers",
        0,
        policy.private_thread_disabled_servers.join(", "),
        "media, corpus",
        "Private-Thread Disabled Servers",
        parse_csv_row,
        |parsed, settings| {
            settings
                .kask
                .get_or_insert_default()
                .private_inference
                .get_or_insert_default()
                .private_thread_disabled_servers = Some(parsed);
        },
    );

    let embedding_model_input = kask_input_field(
        "kask-private-inference-embedding-model",
        0,
        policy.embedding_model,
        "ollama/<model> — empty turns private semantic recall off",
        "Private Embedding Model",
        parse_private_memory_model,
        |parsed, settings| {
            settings
                .kask
                .get_or_insert_default()
                .private_inference
                .get_or_insert_default()
                .embedding_model = Some(parsed);
        },
    );

    let classifier_model_input = kask_input_field(
        "kask-private-inference-classifier-model",
        0,
        policy.classifier_model,
        "ollama/<model> — empty keeps structural tags only",
        "Private Classifier Model",
        parse_private_memory_model,
        |parsed, settings| {
            settings
                .kask
                .get_or_insert_default()
                .private_inference
                .get_or_insert_default()
                .classifier_model = Some(parsed);
        },
    );

    let rerank_model_input = kask_input_field(
        "kask-private-inference-rerank-model",
        0,
        policy.rerank_model,
        "ollama/<model> — empty skips private rerank",
        "Private Rerank Model",
        parse_private_memory_model,
        |parsed, settings| {
            settings
                .kask
                .get_or_insert_default()
                .private_inference
                .get_or_insert_default()
                .rerank_model = Some(parsed);
        },
    );

    v_flex()
        .id("kask-private-inference-page")
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
                .child(SettingsSectionHeader::new("Private Inference"))
                .child(
                    Label::new(
                        "A private thread (the lock in the thread header) runs only \
                         local models served by Zed's built-in Ollama or llama.cpp \
                         provider, and its turns are stored in private curator memory. \
                         Install and serve models with Ollama or llama.cpp, and set \
                         their server addresses under AI → LLM Providers. Kask launches \
                         no model server. The provider set is fixed: no setting adds a \
                         provider to it, and Ollama's cloud-proxied models are refused.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(
                    Label::new(available_line)
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Tools Off on Private Threads"))
                .child(
                    Label::new(
                        "MCP servers whose tools are disabled on private threads, \
                         comma-separated: their assisting inference cannot run \
                         locally, so the tools are refused rather than sending \
                         private content to cloud models.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(disabled_servers_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Private Memory Models"))
                .child(
                    Label::new(
                        "The local Ollama models that embed, tag and rerank private \
                         memory, as ollama/<model>. The configured cloud models never \
                         see private content. Leaving a field empty turns that step \
                         off for private content: keyword-only recall, structural \
                         tags only, or no rerank.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(embedding_model_input)
                .child(classifier_model_input)
                .child(rerank_model_input),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The private-memory rows accept the empty opt-out and an
    /// `ollama/<model>` value, and refuse everything else — a refused
    /// parse never rewrites the settings file.
    #[test]
    fn private_memory_model_rows_accept_only_ollama_models_or_empty() {
        assert_eq!(parse_private_memory_model(""), Some(String::new()));
        assert_eq!(parse_private_memory_model("   "), Some(String::new()));
        assert_eq!(
            parse_private_memory_model("  ollama/qwen3-embedding:0.6b "),
            Some("ollama/qwen3-embedding:0.6b".to_string())
        );
        assert_eq!(
            parse_private_memory_model("Ollama/qwen3.5:4b"),
            Some("Ollama/qwen3.5:4b".to_string()),
            "the prefix compares case-insensitively, as the wiring does"
        );
        for refused in [
            "OpenRouter/qwen/qwen3-embedding-8b",
            "private-local/flint",
            "llama.cpp/qwen36-35b-a3b",
            "qwen3.5:4b",
            "ollama/",
        ] {
            assert_eq!(
                parse_private_memory_model(refused),
                None,
                "{refused:?} is refused at entry"
            );
        }
    }

    /// The CSV row trims, drops empties, and treats an empty confirm as no
    /// write.
    #[test]
    fn csv_row_trims_and_skips_the_write_when_empty() {
        assert_eq!(
            parse_csv_row(" media , ,corpus"),
            Some(vec!["media".to_string(), "corpus".to_string()])
        );
        assert_eq!(parse_csv_row(""), None);
        assert_eq!(parse_csv_row(" , "), None);
    }

    /// The "available now" list shows exactly what a private thread may
    /// run: local Ollama and llama.cpp models by display name; Ollama cloud
    /// models and every other provider are left out.
    #[test]
    fn available_models_are_the_ones_a_private_thread_may_run() {
        let triple = |provider: &str, id: &str, name: &str| {
            (provider.to_string(), id.to_string(), name.to_string())
        };
        assert_eq!(
            private_thread_model_labels([
                triple("ollama", "qwen3.5:4b", "qwen3.5:4b"),
                triple("ollama", "glm-5.3:cloud", "glm-5.3"),
                triple("llama.cpp", "qwen36-35b-a3b", "Qwen 3.6 35B"),
                triple("openrouter", "z-ai/glm-5.3", "GLM 5.3"),
                triple("private-local", "flint", "flint"),
            ]),
            vec![
                "ollama/qwen3.5:4b".to_string(),
                "llama.cpp/Qwen 3.6 35B".to_string(),
            ]
        );
    }
}
