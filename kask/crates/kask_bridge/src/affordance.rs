//! Per-turn instrument-affordance injection (spec B v1, D94).
//!
//! The measured problem (`kask/docs/research/native-first-denominator-sample.md`):
//! skill activation is 8/8 when the operator names the skill and 0/3 for
//! primary-contract discovery when the model must find it — the broken arrow
//! is the catalog MATCH, not tool willingness. The mechanism: embed the
//! turn's user prompt, match it against the skill descriptions by cosine
//! similarity, and surface the top-k candidates with their EXACT invocation
//! handles in a bounded block appended at the request tail — the
//! highest-attention position, immediately before generation (the affordance
//! experiment's floor tasks moved exactly this way; `Lost in the Middle`
//! placement thesis).
//!
//! The block rides the same per-turn channel family as memory recall
//! (`agent::ContextInjector`) but on its own hook with END placement, and it
//! never touches the digest-cached system prompt (the prefix-cache
//! contract). The `kask.affordance.enabled` settings gate is read per turn
//! at the agent-side call site, so flips apply without a restart.
//!
//! Degradation contract: an embedding failure surfaces a one-line
//! degradation note IN the block slot — the request can see that
//! affordances were unavailable, not silently absent — plus a warn; a prompt
//! with no match above the floor injects nothing (no fabricated relevance);
//! prompts under the substantive word floor are skipped entirely — the same
//! turns the native-first sensor excludes.

use language_model::{LanguageModelRequestMessage, Role};
use language_model_core::MessageContent;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

/// Prompts under this many words are not substantive work — mirrors the
/// sensor's `SUBSTANTIVE_MIN_WORDS` (`kask_thread_state.rs`) so the block
/// fires on exactly the turns the native-first rate counts.
const MIN_AFFORDANCE_PROMPT_WORDS: usize = 20;

/// How many skill candidates the block surfaces.
const AFFORDANCE_TOP_K: usize = 3;

/// Cosine floor for a description match to be worth surfacing. Below this
/// the block is noise, not affordance. Calibrate from field readings once
/// the native-first baseline and this block coexist (the affordance doc's
/// next experiment, n ≥ 30).
const AFFORDANCE_MATCH_FLOOR: f64 = 0.30;

/// The embedding seam: production wraps the zed-side
/// `LanguageModelEmbeddingPort`; tests wrap a deterministic bag-of-words
/// function (the context-injector test precedent). Manual `Pin<Box>`
/// future — the same trait shape as `agent::ContextInjector`, so no new
/// dependency enters the bridge.
pub trait AffordanceEmbedding: Send + Sync {
    fn embed<'a>(
        &'a self,
        texts: &'a [String],
    ) -> Pin<Box<dyn std::future::Future<Output = Result<Vec<Vec<f32>>, String>> + Send + 'a>>;
}

/// Production embedding over the zed-side port, with the resolved
/// provider-prefixed model string.
pub struct PortAffordanceEmbedding {
    port: crate::inference_embedding::LanguageModelEmbeddingPort,
    model: String,
}

impl PortAffordanceEmbedding {
    pub fn new(
        port: crate::inference_embedding::LanguageModelEmbeddingPort,
        model: String,
    ) -> Self {
        Self { port, model }
    }
}

impl AffordanceEmbedding for PortAffordanceEmbedding {
    fn embed<'a>(
        &'a self,
        texts: &'a [String],
    ) -> Pin<Box<dyn std::future::Future<Output = Result<Vec<Vec<f32>>, String>> + Send + 'a>> {
        Box::pin(async move {
            self.port
                .embed(&self.model, texts)
                .await
                .map_err(|e| e.to_string())
        })
    }
}

/// The per-turn affordance injector (spec B v1). Matches the user prompt
/// against the skill catalog by embedding cosine similarity and renders the
/// task-matched block.
///
/// Caching: description vectors are keyed by `(name, description)` so
/// unchanged skills never re-embed, and the last `(prompt, result)` is
/// memoized so continuation rounds within one turn reuse the match without
/// re-embedding the prompt. Known staleness window: a mid-session catalog
/// edit serves the memoized result for the remainder of that turn's
/// continuation rounds only — every new prompt re-matches.
pub struct AffordanceInjector {
    embedding: Arc<dyn AffordanceEmbedding>,
    description_vectors: Mutex<HashMap<(String, String), Vec<f32>>>,
    last_result: Mutex<Option<(String, Vec<LanguageModelRequestMessage>)>>,
    top_k: usize,
    match_floor: f64,
    min_words: usize,
}

impl AffordanceInjector {
    pub fn new(embedding: Arc<dyn AffordanceEmbedding>) -> Self {
        Self {
            embedding,
            description_vectors: Mutex::new(HashMap::new()),
            last_result: Mutex::new(None),
            top_k: AFFORDANCE_TOP_K,
            match_floor: AFFORDANCE_MATCH_FLOOR,
            min_words: MIN_AFFORDANCE_PROMPT_WORDS,
        }
    }

    async fn compute_block(
        &self,
        catalog: &[(String, String)],
        prompt: &str,
    ) -> Vec<LanguageModelRequestMessage> {
        if prompt.split_whitespace().count() < self.min_words {
            return Vec::new();
        }
        if catalog.is_empty() {
            return Vec::new();
        }

        // Memoized continuation round: same prompt, same result.
        {
            let memo = self.last_result.lock().expect("affordance memo lock");
            if let Some((memo_prompt, memo_result)) = memo.as_ref() {
                if memo_prompt.as_str() == prompt {
                    return memo_result.clone();
                }
            }
        }

        // Embed only the descriptions not yet cached (one batch).
        let missing: Vec<(String, String)> = {
            let cache = self
                .description_vectors
                .lock()
                .expect("affordance cache lock");
            catalog
                .iter()
                .filter(|entry| !cache.contains_key(*entry))
                .cloned()
                .collect()
        };
        if !missing.is_empty() {
            let texts: Vec<String> = missing.iter().map(|(_, d)| d.clone()).collect();
            match self.embedding.embed(&texts).await {
                Ok(vectors) => {
                    let mut cache = self
                        .description_vectors
                        .lock()
                        .expect("affordance cache lock");
                    for (entry, vector) in missing.into_iter().zip(vectors) {
                        cache.insert(entry, vector);
                    }
                }
                Err(error) => {
                    tracing::warn!(
                        target: "reg.affordance",
                        error = %error,
                        "skill-catalog embedding failed — no task-matched affordances this turn"
                    );
                    return self.memo_degradation(prompt, &error);
                }
            }
        }

        // Embed the prompt.
        let prompt_vector = match self.embedding.embed(&[prompt.to_string()]).await {
            Ok(vectors) => match vectors.into_iter().next() {
                Some(vector) if !vector.is_empty() => vector,
                _ => {
                    let error = "empty embedding response for the prompt".to_string();
                    tracing::warn!(
                        target: "reg.affordance",
                        error = %error,
                        "prompt embedding returned no vector — no task-matched affordances this turn"
                    );
                    return self.memo_degradation(prompt, &error);
                }
            },
            Err(error) => {
                tracing::warn!(
                    target: "reg.affordance",
                    error = %error,
                    "prompt embedding failed — no task-matched affordances this turn"
                );
                return self.memo_degradation(prompt, &error);
            }
        };

        // Score, dedupe by name (keep the best description fit), take top-k
        // above the floor. No lock is held across an await anywhere above.
        let mut best_by_name: HashMap<&str, (f64, &str)> = HashMap::new();
        {
            let cache = self
                .description_vectors
                .lock()
                .expect("affordance cache lock");
            for (name, description) in catalog {
                let Some(vector) = cache.get(&(name.clone(), description.clone())) else {
                    continue;
                };
                let score = cosine(&prompt_vector, vector);
                match best_by_name.get(name.as_str()) {
                    Some((best, _)) if *best >= score => {}
                    _ => {
                        best_by_name.insert(name.as_str(), (score, description.as_str()));
                    }
                }
            }
        }
        let mut matches: Vec<(String, String, f64)> = best_by_name
            .into_iter()
            .filter(|(_, (score, _))| *score >= self.match_floor)
            .map(|(name, (score, description))| (name.to_string(), description.to_string(), score))
            .collect();
        matches.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        matches.truncate(self.top_k);

        let result = if matches.is_empty() {
            Vec::new()
        } else {
            vec![system_message(format_block(&matches))]
        };
        {
            let mut memo = self.last_result.lock().expect("affordance memo lock");
            *memo = Some((prompt.to_string(), result.clone()));
        }
        result
    }

    /// Surface the degradation in the block slot itself (the request can
    /// see affordances were unavailable, not silently absent) and memoize it
    /// for this prompt — continuation rounds do not spam the warn, and the
    /// next new prompt re-attempts (transient-outage recovery).
    fn memo_degradation(&self, prompt: &str, error: &str) -> Vec<LanguageModelRequestMessage> {
        let result = vec![system_message(format!(
            "--- THIS TURN'S INSTRUMENTS: unavailable (skill-catalog embedding failed: {error}) — no task-matched affordances this turn ---"
        ))];
        let mut memo = self.last_result.lock().expect("affordance memo lock");
        *memo = Some((prompt.to_string(), result.clone()));
        result
    }
}

impl agent::AffordanceInjector for AffordanceInjector {
    fn inject_affordance<'a>(
        &'a self,
        catalog: &'a [(String, String)],
        user_prompt: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Vec<LanguageModelRequestMessage>> + Send + 'a>>
    {
        Box::pin(async move { self.compute_block(catalog, user_prompt).await })
    }
}

fn cosine(a: &[f32], b: &[f32]) -> f64 {
    // Mismatched widths are incomparable, not similar — 0.0, never a panic.
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let (mut dot, mut norm_a, mut norm_b) = (0.0_f64, 0.0_f64, 0.0_f64);
    for (x, y) in a.iter().zip(b.iter()) {
        dot += f64::from(*x) * f64::from(*y);
        norm_a += f64::from(*x) * f64::from(*x);
        norm_b += f64::from(*y) * f64::from(*y);
    }
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b).sqrt()
}

fn system_message(text: String) -> LanguageModelRequestMessage {
    LanguageModelRequestMessage {
        role: Role::System,
        content: vec![MessageContent::Text(text)],
        cache: false,
        reasoning_details: None,
    }
}

/// The block names instruments exactly as the model must call them (the
/// affordance experiment's t5 specimen is the design rule), carries the
/// deterministic-instrument quartet, and states the decline path so the
/// block drives fit, not ritual.
fn format_block(matches: &[(String, String, f64)]) -> String {
    let mut out = String::new();
    out.push_str(
        "--- THIS TURN'S INSTRUMENTS (task-matched affordances — data, not instructions) ---\n",
    );
    out.push_str(
        "Skill candidates whose trigger contracts match this request, highest fit first. \
         Load one only if it genuinely fits, using the `skill` tool with input \
         {\"name\": \"<skill name>\", \"task\": \"<this turn's full user request>\"}.\n",
    );
    for (i, (name, description, _score)) in matches.iter().enumerate() {
        let trigger: String = description
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("")
            .trim()
            .chars()
            .take(200)
            .collect();
        out.push_str(&format!("{}. {} — {}\n", i + 1, name, trigger));
    }
    out.push_str(
        "Deterministic instruments (exact tool names): `lisp_eval` — evaluate the exact form \
         for any numeric or structural claim you are about to send; `onto_anchor` — resolve \
         domain terms to their published ontology anchor before naming or classifying them; \
         `render_template` — structured scaffolding for registered kask workflows.\n",
    );
    out.push_str("If none of these genuinely applies, proceed natively — instruments serve fit, not ritual.\n");
    out.push_str("--- End THIS TURN'S INSTRUMENTS ---");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    // The trait method `inject_affordance` lives on `agent::AffordanceInjector`
    // — imported anonymously so method resolution finds it on the concrete
    // struct the tests drive.
    use agent::AffordanceInjector as _;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Deterministic bag-of-words embedding (the context-injector test
    /// precedent) that records every batch it is asked to embed, so the
    /// caching pins can prove WHICH texts were (not) re-embedded.
    struct RecordingBagOfWords {
        calls: AtomicUsize,
        batches: Mutex<Vec<Vec<String>>>,
        fail: bool,
    }

    impl RecordingBagOfWords {
        fn honest() -> Arc<Self> {
            Arc::new(Self {
                calls: AtomicUsize::new(0),
                batches: Mutex::new(Vec::new()),
                fail: false,
            })
        }
        fn failing() -> Arc<Self> {
            Arc::new(Self {
                calls: AtomicUsize::new(0),
                batches: Mutex::new(Vec::new()),
                fail: true,
            })
        }
    }

    impl AffordanceEmbedding for RecordingBagOfWords {
        fn embed<'a>(
            &'a self,
            texts: &'a [String],
        ) -> Pin<Box<dyn std::future::Future<Output = Result<Vec<Vec<f32>>, String>> + Send + 'a>>
        {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.batches
                .lock()
                .expect("batches lock")
                .push(texts.to_vec());
            let fail = self.fail;
            let texts = texts.to_vec();
            Box::pin(async move {
                if fail {
                    return Err("connection refused (test fixture)".to_string());
                }
                Ok(texts.iter().map(|t| bow(t)).collect())
            })
        }
    }

    fn bow(text: &str) -> Vec<f32> {
        let mut vec = vec![0.0_f32; 64];
        for word in text.split_whitespace() {
            let hash = word
                .bytes()
                .fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));
            vec[(hash as usize) % 64] += 1.0;
        }
        vec
    }

    fn test_injector(
        embedding: Arc<dyn AffordanceEmbedding>,
        match_floor: f64,
    ) -> AffordanceInjector {
        AffordanceInjector {
            embedding,
            description_vectors: Mutex::new(HashMap::new()),
            last_result: Mutex::new(None),
            top_k: AFFORDANCE_TOP_K,
            match_floor,
            min_words: MIN_AFFORDANCE_PROMPT_WORDS,
        }
    }

    fn catalog() -> Vec<(String, String)> {
        vec![
            (
                "gradient-hunter".to_string(),
                "Investigate a disagreement between expectation and reality with \
                 discriminating probes; diagnose measured gradients in code or telemetry."
                    .to_string(),
            ),
            (
                "essentialist".to_string(),
                "Recursive eliminative interrogation across three gates for artifact reduction."
                    .to_string(),
            ),
        ]
    }

    fn block_text(messages: &[LanguageModelRequestMessage]) -> String {
        assert_eq!(messages.len(), 1, "the block is one System message");
        match &messages[0].content[0] {
            MessageContent::Text(text) => text.to_string(),
            other => panic!("expected text content, got {other:?}"),
        }
    }

    /// The block surfaces the matched skill's exact name, the exact `skill`
    /// tool invocation shape, the deterministic quartet, the decline line,
    /// and both boundary markers.
    #[tokio::test]
    async fn matching_surfaces_exact_names_and_invocation_shape() {
        let embedding = RecordingBagOfWords::honest();
        let injector = test_injector(embedding, 0.05);
        let prompt = "please diagnose the measured gradient in test coverage between the two \
                      modules and investigate why the expectation disagrees with reality here";
        let messages = injector.inject_affordance(&catalog(), prompt).await;
        let text = block_text(&messages);
        assert!(
            text.contains("gradient-hunter"),
            "matched name; got: {text}"
        );
        assert!(
            text.contains("{\"name\": \"<skill name>\", \"task\""),
            "exact invocation shape; got: {text}"
        );
        assert!(text.contains("`lisp_eval`"), "quartet; got: {text}");
        assert!(
            text.contains("proceed natively — instruments serve fit, not ritual"),
            "decline line; got: {text}"
        );
        assert!(
            text.starts_with("--- THIS TURN'S INSTRUMENTS"),
            "opening marker; got: {text}"
        );
        assert!(
            text.ends_with("--- End THIS TURN'S INSTRUMENTS ---"),
            "closing marker; got: {text}"
        );
    }

    /// Short prompts are the turns the sensor excludes — no embed call, no
    /// block (the same substantive boundary, so the block cannot fire on
    /// turns the native-first rate does not count).
    #[tokio::test]
    async fn short_prompts_are_not_affordance_work() {
        let embedding = RecordingBagOfWords::honest();
        let injector = test_injector(embedding.clone(), 0.05);
        let messages = injector
            .inject_affordance(&catalog(), "please fix this quickly")
            .await;
        assert!(
            messages.is_empty(),
            "short prompts inject nothing; got {messages:?}"
        );
        assert_eq!(
            embedding.calls.load(Ordering::SeqCst),
            0,
            "the gate is zero-cost — no embed call"
        );
    }

    /// A prompt with no match above the floor injects nothing — no
    /// fabricated relevance — and the result is memoized so the next
    /// identical prompt costs zero embed calls.
    #[tokio::test]
    async fn no_floor_match_injects_nothing_and_memoizes() {
        let embedding = RecordingBagOfWords::honest();
        let injector = test_injector(embedding.clone(), 0.99);
        let prompt = "please summarize the quarterly revenue forecast for the board meeting \
                      with a clear focus on churn trends and the net retention picture";
        let first = injector.inject_affordance(&catalog(), prompt).await;
        assert!(first.is_empty(), "no fabricated matches; got {first:?}");
        let calls_after_first = embedding.calls.load(Ordering::SeqCst);
        assert!(calls_after_first > 0, "the first call embeds");
        let second = injector.inject_affordance(&catalog(), prompt).await;
        assert!(second.is_empty());
        assert_eq!(
            embedding.calls.load(Ordering::SeqCst),
            calls_after_first,
            "the memoized no-match costs zero embed calls"
        );
    }

    /// An embedding failure surfaces the degradation IN the block slot —
    /// "unavailable", never a silent absence — and memoizes it for the
    /// prompt so continuation rounds do not re-warn.
    #[tokio::test]
    async fn embed_failure_surfaces_the_degradation_note() {
        let embedding = RecordingBagOfWords::failing();
        let injector = test_injector(embedding.clone(), 0.05);
        let prompt = "a substantive prompt that is long enough to clear the twenty word \
                      floor for the affordance path while the embedding provider is under \
                      a full outage here";
        let messages = injector.inject_affordance(&catalog(), prompt).await;
        let text = block_text(&messages);
        assert!(
            text.contains("unavailable (skill-catalog embedding failed"),
            "degradation surfaced, not silent; got: {text}"
        );
        let calls_after_first = embedding.calls.load(Ordering::SeqCst);
        let _ = injector.inject_affordance(&catalog(), prompt).await;
        assert_eq!(
            embedding.calls.load(Ordering::SeqCst),
            calls_after_first,
            "the memoized degradation does not re-attempt within the turn"
        );
    }

    /// Continuation rounds of one turn (tool-result re-requests rebuild the
    /// request and re-call the injector with the same prompt) reuse the
    /// memoized result with zero further embed calls.
    #[tokio::test]
    async fn continuation_rounds_reuse_the_match_without_re_embedding() {
        let embedding = RecordingBagOfWords::honest();
        let injector = test_injector(embedding.clone(), 0.05);
        let prompt = "investigate the disagreement between the measured gradient and the \
                      stated expectation with discriminating probes over the telemetry for \
                      this particular session";
        let first = injector.inject_affordance(&catalog(), prompt).await;
        assert!(!first.is_empty(), "the vocabulary overlaps; got {first:?}");
        let calls_after_first = embedding.calls.load(Ordering::SeqCst);
        let second = injector.inject_affordance(&catalog(), prompt).await;
        assert_eq!(second.len(), first.len());
        assert_eq!(
            embedding.calls.load(Ordering::SeqCst),
            calls_after_first,
            "the second round must not re-embed anything"
        );
    }

    /// A changed description re-embeds only the changed entry; unchanged
    /// descriptions ride the cache (the batch contents prove it, not just
    /// the call count).
    #[tokio::test]
    async fn changed_descriptions_re_embed_only_the_new_entry() {
        let embedding = RecordingBagOfWords::honest();
        let injector = test_injector(embedding.clone(), 0.05);
        let prompt_a = "investigate the disagreement between the measured expectation and \
                        reality with a fresh set of discriminating probes over the current \
                        telemetry";
        let _ = injector.inject_affordance(&catalog(), prompt_a).await;
        let mut changed = catalog();
        changed[1].1 =
            "Eliminative interrogation with the deletion test for module depth.".to_string();
        let prompt_b = "review this module for eliminative interrogation depth against the \
                        deletion test and report exactly what should be removed outright now";
        let _ = injector.inject_affordance(&changed, prompt_b).await;
        let batches = embedding.batches.lock().expect("batches lock").clone();
        let last = batches.last().expect("prompt embed batch");
        assert!(
            last.len() == 1,
            "only the prompt re-embeds when all descriptions are cached; got {last:?}"
        );
        let changed_batch = &batches[batches.len() - 2];
        assert!(
            changed_batch.len() == 1 && changed_batch[0].contains("deletion test"),
            "only the changed description re-embeds; got {changed_batch:?}"
        );
    }
}
