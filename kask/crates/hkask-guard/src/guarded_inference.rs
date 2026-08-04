//! GuardedInferencePort — decorator that wraps any `InferencePort` with
//! mandatory `ContentGuard` scanning at the LLM I/O boundary.
//!
//! Closes the gap where `ManifestExecutor` select/populate and REPL chat
//! turns called `InferencePort::generate` without content scanning. Wrapping
//! the primary `InferenceRouter` at the composition root makes the boundary
//! universal by construction rather than relying on each caller to opt in.
//!
//! **Non-streaming** (`generate`, `generate_with_model`, `generate_with_messages`,
//! `generate_n`, `generate_vision`): scans input before delegation and output
//! after. Rejected input returns `InferenceError::Generation`. Secret-bearing
//! output is redacted in-place (not rejected).
//!
//! **Streaming** (`generate_stream*`): scans input before delegation. Output
//! is scanned post-hoc via `GuardedStream` on stream end — the accumulated
//! text is checked for secrets and a redaction chunk is emitted if needed.
//! The known limitation is that displayed text is not blocked in real time
//! (the stored version is redacted, not the chunks already forwarded to the
//! consumer).

use crate::ContentGuard;
use futures_util::{Future, Stream};
use hkask_types::{
    ChatMessage, ChatToolDefinition, InferenceError, InferencePort, InferenceResult,
    InferenceStreamChunk, LLMParameters,
};
use std::pin::Pin;
use std::sync::Arc;

/// Cap on `GuardedStream` output accumulation. The input side is bounded by a
/// 32K `TokenLimit` scanner; the output side needs an equivalent bound so a
/// runaway stream cannot grow memory without limit. 256KB is generous for
/// normal completions (the largest thinking-mode traces stay well under this)
/// while preventing unbounded growth from a misbehaving provider.
const GUARD_ACCUMULATION_LIMIT: usize = 256 * 1024;

/// A stream wrapper that buffers `text_delta` chunks, forwards them unchanged
/// (preserving streaming latency for the common clean case), and on stream end
/// runs `scan_output` on the full accumulated text.
///
/// If the guard detects secrets in the accumulated output, a final redaction
/// chunk is emitted containing the sanitized replacement. This closes the
/// OWASP LLM07 gap where streaming output was never scanned. The consumer
/// may have already rendered the leaked text, but the *stored* version is
/// redacted and the `reg.guard.output` span fires.
struct GuardedStream<'a> {
    inner: Pin<Box<dyn Stream<Item = Result<InferenceStreamChunk, InferenceError>> + Send + 'a>>,
    guard: Arc<ContentGuard>,
    accumulated: String,
    accumulated_reasoning: String,
    scanned: bool,
}

impl<'a> Stream for GuardedStream<'a> {
    type Item = Result<InferenceStreamChunk, InferenceError>;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let this = self.get_mut();
        if this.scanned {
            return std::task::Poll::Ready(None);
        }
        match this.inner.as_mut().poll_next(cx) {
            std::task::Poll::Ready(None) => {
                // Stream ended — scan the accumulated output.
                let result = this.guard.scan_output(&this.accumulated);
                let reasoning_result = this.guard.scan_output(&this.accumulated_reasoning);
                this.scanned = true;
                let text_modified = result.output.is_modified();
                let reasoning_modified = reasoning_result.output.is_modified();
                if text_modified || reasoning_modified {
                    let sanitized = result.output.content(&this.accumulated).to_string();
                    let sanitized_reasoning = reasoning_result
                        .output
                        .content(&this.accumulated_reasoning)
                        .to_string();
                    // Emit a redaction chunk: the sanitized text replaces the
                    // accumulated text. The consumer concatenates deltas, so
                    // emitting the full sanitized text as a final delta would
                    // duplicate. Instead, emit the *difference* — the sanitized
                    // text with the already-streamed portion removed.
                    //
                    // The simplest correct redaction: emit a chunk whose
                    // text_delta, when appended to the accumulated text,
                    // produces the sanitized version. Since we can't un-emit
                    // the leaked text, we emit a marker that signals redaction
                    // occurred. Downstream storage should use the sanitized
                    // text from the final chunk's `finish_reason` metadata.
                    //
                    // For now: emit the sanitized full text as a delta. The
                    // consumer's accumulation will contain both the leaked
                    // text and the sanitized text; the *stored* assistant
                    // message should be reconstructed from the sanitized
                    // version. This is a known tradeoff — see ART-1 note.
                    // The reasoning channel inherits the same replace-not-append
                    // semantics: a delta-concatenating consumer ends up with
                    // raw + sanitized reasoning, and storage must reconstruct
                    // from the sanitized version.
                    std::task::Poll::Ready(Some(Ok(InferenceStreamChunk {
                        text_delta: if text_modified {
                            sanitized
                        } else {
                            String::new()
                        },
                        reasoning_delta: if reasoning_modified {
                            sanitized_reasoning
                        } else {
                            String::new()
                        },
                        model: String::new(),
                        finish_reason: Some("redacted".to_string()),
                        usage: None,
                        tool_calls: vec![],
                    })))
                } else {
                    std::task::Poll::Ready(None)
                }
            }
            std::task::Poll::Ready(Some(Ok(chunk))) => {
                this.accumulated.push_str(&chunk.text_delta);
                this.accumulated_reasoning.push_str(&chunk.reasoning_delta);
                // Bound output accumulation. Without this a runaway provider
                // stream grows memory without limit — the input side is bounded
                // by the 32K TokenLimit scanner, but the output side had no
                // equivalent. Abort the stream once the combined accumulated
                // text/reasoning exceeds the cap; mark `scanned` so further
                // polls return `Ready(None)` without re-scanning.
                if this.accumulated.len() + this.accumulated_reasoning.len()
                    > GUARD_ACCUMULATION_LIMIT
                {
                    this.scanned = true;
                    std::task::Poll::Ready(Some(Err(InferenceError::Generation(
                        "output exceeded guard accumulation limit".to_string(),
                    ))))
                } else {
                    std::task::Poll::Ready(Some(Ok(chunk)))
                }
            }
            std::task::Poll::Ready(Some(Err(e))) => {
                this.scanned = true;
                std::task::Poll::Ready(Some(Err(e)))
            }
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }
}

/// Decorator enforcing `ContentGuard` scanning at every `InferencePort` call.
///
/// Construct once at the composition root and hand the wrapped `Arc<dyn InferencePort>`
/// to every consumer (executor, chat service, REPL turn, condenser).
pub struct GuardedInferencePort {
    inner: Arc<dyn InferencePort>,
    guard: Arc<ContentGuard>,
}

impl GuardedInferencePort {
    /// Wrap an inference port with a content guard.
    pub fn new(inner: Arc<dyn InferencePort>, guard: ContentGuard) -> Self {
        Self {
            inner,
            guard: Arc::new(guard),
        }
    }
}

/// Scan `result.reasoning` the same way as `result.text`. Thinking-mode
/// models can echo system-prompt content (including the canary token) into
/// the reasoning trace, so skipping it defeats canary/secret detection.
fn sanitize_reasoning(guard: &ContentGuard, result: &mut InferenceResult) {
    if let Some(reasoning) = result.reasoning.take() {
        let out = guard.scan_output(&reasoning);
        result.reasoning = Some(if out.output.is_modified() {
            out.output.content(&reasoning).to_string()
        } else {
            reasoning
        });
    }
}

fn reject_msg(violations: &[crate::GuardViolation]) -> String {
    violations
        .iter()
        .map(|v| format!("{}: {}", v.scanner, v.description))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Pre-delegation input scanning. Scans the prompt, returns the cleaned
/// prompt if it passes, or an `InferenceError::Generation` if blocked.
/// This is the pure function the decorator calls before delegating to the
/// inner `InferencePort` — extracted so it can be proptest-tested without
/// an `InferencePort`.
pub fn guard_input(prompt: &str, guard: &ContentGuard) -> Result<String, InferenceError> {
    let scan = guard.scan_input(prompt);
    if !scan.passed {
        return Err(InferenceError::Generation(reject_msg(&scan.violations)));
    }
    Ok(scan.output.content(prompt).to_string())
}

/// Post-delegation output scanning. Scans the result's text and reasoning
/// for secrets, redacting in-place if detected. Returns the (possibly
/// redacted) result. This is the pure function the decorator calls after
/// the inner `InferencePort` returns — extracted so it can be
/// proptest-tested without an `InferencePort`.
pub fn guard_output(mut result: InferenceResult, guard: &ContentGuard) -> InferenceResult {
    let out = guard.scan_output(&result.text);
    if out.output.is_modified() {
        result.text = out.output.content(&result.text).to_string();
    }
    sanitize_reasoning(guard, &mut result);
    result
}

impl InferencePort for GuardedInferencePort {
    fn generate(
        &self,
        prompt: &str,
        parameters: &LLMParameters,
        tools: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>> {
        let cleaned = match guard_input(prompt, &self.guard) {
            Ok(c) => c,
            Err(e) => return Box::pin(async move { Err(e) }),
        };
        let parameters = parameters.clone();
        let tools = tools.map(|t| t.to_vec());
        let guard = Arc::clone(&self.guard);
        let inner = Arc::clone(&self.inner);
        Box::pin(async move {
            let result = inner
                .generate(&cleaned, &parameters, tools.as_deref())
                .await?;
            Ok(guard_output(result, &guard))
        })
    }

    fn generate_with_model(
        &self,
        prompt: &str,
        parameters: &LLMParameters,
        model_override: Option<&str>,
        tools: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>> {
        let cleaned = match guard_input(prompt, &self.guard) {
            Ok(c) => c,
            Err(e) => return Box::pin(async move { Err(e) }),
        };
        let model = model_override.map(str::to_string);
        let parameters = parameters.clone();
        let tools = tools.map(|t| t.to_vec());
        let guard = Arc::clone(&self.guard);
        let inner = Arc::clone(&self.inner);
        Box::pin(async move {
            let result = inner
                .generate_with_model(&cleaned, &parameters, model.as_deref(), tools.as_deref())
                .await?;
            Ok(guard_output(result, &guard))
        })
    }

    fn generate_with_messages(
        &self,
        messages: &[ChatMessage],
        parameters: &LLMParameters,
        model_override: Option<&str>,
        tools: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>> {
        for msg in messages {
            let scan = self.guard.scan_input(&msg.content);
            if !scan.passed {
                let msg_text = reject_msg(&scan.violations);
                let role = msg.role.clone();
                return Box::pin(async move {
                    Err(InferenceError::Generation(format!(
                        "role={}: {}",
                        role, msg_text
                    )))
                });
            }
        }
        let model = model_override.map(str::to_string);
        let parameters = parameters.clone();
        let tools = tools.map(|t| t.to_vec());
        let guard = Arc::clone(&self.guard);
        let inner = Arc::clone(&self.inner);
        let messages = messages.to_vec();
        Box::pin(async move {
            let mut result = inner
                .generate_with_messages(&messages, &parameters, model.as_deref(), tools.as_deref())
                .await?;
            let out = guard.scan_output(&result.text);
            if out.output.is_modified() {
                result.text = out.output.content(&result.text).to_string();
            }
            sanitize_reasoning(&guard, &mut result);
            Ok(result)
        })
    }

    fn generate_n(
        &self,
        prompt: &str,
        parameters: &LLMParameters,
        n: usize,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<InferenceResult>, InferenceError>> + Send + '_>>
    {
        let cleaned = match guard_input(prompt, &self.guard) {
            Ok(c) => c,
            Err(e) => return Box::pin(async move { Err(e) }),
        };
        let parameters = parameters.clone();
        let guard = Arc::clone(&self.guard);
        let inner = Arc::clone(&self.inner);
        Box::pin(async move {
            let results = inner.generate_n(&cleaned, &parameters, n).await?;
            Ok(results
                .into_iter()
                .map(|r| guard_output(r, &guard))
                .collect())
        })
    }

    fn generate_vision(
        &self,
        prompt: &str,
        images: &[String],
        parameters: &LLMParameters,
        model_override: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>> {
        let cleaned = match guard_input(prompt, &self.guard) {
            Ok(c) => c,
            Err(e) => return Box::pin(async move { Err(e) }),
        };
        let model = model_override.map(str::to_string);
        let parameters = parameters.clone();
        let guard = Arc::clone(&self.guard);
        let inner = Arc::clone(&self.inner);
        let images = images.to_vec();
        Box::pin(async move {
            let result = inner
                .generate_vision(&cleaned, &images, &parameters, model.as_deref())
                .await?;
            Ok(guard_output(result, &guard))
        })
    }

    // ── Streaming: scan input, delegate to inner, then scan output on stream
    //    end. The stream is wrapped in `GuardedStream` which buffers text deltas
    //    and runs `scan_output` when the inner stream completes. This closes the
    //    OWASP LLM07 gap where streaming output was never scanned (ART-1).
    //    The common clean case preserves streaming latency — chunks are
    //    forwarded as-is; only on stream end is the scan run.

    fn generate_stream(
        &self,
        prompt: &str,
        parameters: &LLMParameters,
        tools: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Stream<Item = Result<InferenceStreamChunk, InferenceError>> + Send + '_>> {
        let scan = self.guard.scan_input(prompt);
        if !scan.passed {
            let msg = reject_msg(&scan.violations);
            return Box::pin(futures_util::stream::once(async move {
                Err(InferenceError::Generation(msg))
            }));
        }
        let cleaned = scan.output.content(prompt).to_string();
        let inner = self.inner.generate_stream(&cleaned, parameters, tools);
        Box::pin(GuardedStream {
            inner,
            guard: Arc::clone(&self.guard),
            accumulated: String::new(),
            accumulated_reasoning: String::new(),
            scanned: false,
        })
    }

    fn generate_stream_with_model(
        &self,
        prompt: &str,
        parameters: &LLMParameters,
        model_override: Option<&str>,
        tools: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Stream<Item = Result<InferenceStreamChunk, InferenceError>> + Send + '_>> {
        let scan = self.guard.scan_input(prompt);
        if !scan.passed {
            let msg = reject_msg(&scan.violations);
            return Box::pin(futures_util::stream::once(async move {
                Err(InferenceError::Generation(msg))
            }));
        }
        let cleaned = scan.output.content(prompt).to_string();
        let inner =
            self.inner
                .generate_stream_with_model(&cleaned, parameters, model_override, tools);
        Box::pin(GuardedStream {
            inner,
            guard: Arc::clone(&self.guard),
            accumulated: String::new(),
            accumulated_reasoning: String::new(),
            scanned: false,
        })
    }

    fn generate_stream_with_messages(
        &self,
        messages: &[ChatMessage],
        parameters: &LLMParameters,
        model_override: Option<&str>,
        tools: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Stream<Item = Result<InferenceStreamChunk, InferenceError>> + Send + '_>> {
        for msg in messages {
            let scan = self.guard.scan_input(&msg.content);
            if !scan.passed {
                let msg_text = reject_msg(&scan.violations);
                let role = msg.role.clone();
                return Box::pin(futures_util::stream::once(async move {
                    Err(InferenceError::Generation(format!(
                        "role={}: {}",
                        role, msg_text
                    )))
                }));
            }
        }
        let inner =
            self.inner
                .generate_stream_with_messages(messages, parameters, model_override, tools);
        Box::pin(GuardedStream {
            inner,
            guard: Arc::clone(&self.guard),
            accumulated: String::new(),
            accumulated_reasoning: String::new(),
            scanned: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::GuardedStream;
    use super::guard_output;
    use crate::{ContentGuard, GuardConfig};
    use futures_util::stream::iter as stream_iter;
    use futures_util::{Stream, StreamExt};
    use hkask_types::{InferenceResult, InferenceStreamChunk, InferenceUsage};
    use std::sync::Arc;
    use std::task::{Context, Poll};

    /// Build a `GuardedStream` wrapping a synthetic chunk stream.
    ///
    /// `GuardedStream` is a private struct, so the streaming-path tests must
    /// live in this module (which already has access to private items) rather
    /// than in an integration test.
    fn guarded_stream_from_chunks(
        chunks: Vec<InferenceStreamChunk>,
        guard: std::sync::Arc<ContentGuard>,
    ) -> GuardedStream<'static> {
        let inner: Pin<
            Box<
                dyn Stream<Item = Result<InferenceStreamChunk, hkask_types::InferenceError>> + Send,
            >,
        > = Box::pin(stream_iter(
            chunks.into_iter().map(Ok::<_, hkask_types::InferenceError>),
        ));
        GuardedStream {
            inner,
            guard,
            accumulated: String::new(),
            accumulated_reasoning: String::new(),
            scanned: false,
        }
    }

    fn chunk(text: &str, reasoning: &str) -> InferenceStreamChunk {
        InferenceStreamChunk {
            text_delta: text.to_string(),
            reasoning_delta: reasoning.to_string(),
            model: String::new(),
            finish_reason: None,
            usage: None,
            tool_calls: vec![],
        }
    }

    /// A no-op waker for manually polling `GuardedStream` in synchronous tests.
    fn noop_cx() -> Context<'static> {
        Context::from_waker(futures_util::task::noop_waker())
    }

    /// Clean chunks pass through unchanged and the stream ends cleanly with
    /// no redaction chunk. After the inner stream is exhausted, polling once
    /// more returns `Ready(None)` (the guard found nothing to redact).
    #[test]
    fn guarded_stream_clean_passes_through() {
        let guard = std::sync::Arc::new(ContentGuard::mandatory(&GuardConfig::default()));
        let chunks = vec![
            chunk("hello ", ""),
            chunk("world", ""),
            chunk("", "thinking step"),
        ];
        let mut stream = guarded_stream_from_chunks(chunks, Arc::clone(&guard));

        let collected: Vec<_> = stream.by_ref().collect::<Vec<_>>();

        // All non-redaction chunks pass through as Ok; none carry a
        // `finish_reason: "redacted"`.
        assert_eq!(collected.len(), 3, "all three clean chunks pass through");
        for item in &collected {
            let chunk = item.as_ref().expect("clean chunk should be Ok");
            assert_ne!(
                chunk.finish_reason.as_deref(),
                Some("redacted"),
                "clean stream must not emit a redaction chunk"
            );
        }
        assert_eq!(collected[0].as_ref().unwrap().text_delta, "hello ");
        assert_eq!(collected[1].as_ref().unwrap().text_delta, "world");
        assert_eq!(
            collected[2].as_ref().unwrap().reasoning_delta,
            "thinking step"
        );
    }

    /// A secret (canary) in `text_delta` triggers a redaction chunk with
    /// `finish_reason: "redacted"` once the inner stream ends.
    #[test]
    fn guarded_stream_secret_in_text_emits_redaction() {
        let guard = std::sync::Arc::new(ContentGuard::mandatory(&GuardConfig::default()));
        let canary = guard.canary().as_str().to_string();
        let chunks = vec![chunk("here is the secret: ", ""), chunk(&canary, "")];
        let mut stream = guarded_stream_from_chunks(chunks, Arc::clone(&guard));

        let collected: Vec<_> = stream.collect();

        // Two source chunks plus one redaction chunk.
        assert_eq!(
            collected.len(),
            3,
            "expected 2 source chunks + 1 redaction chunk"
        );
        let redaction = collected[2].as_ref().expect("redaction chunk should be Ok");
        assert_eq!(
            redaction.finish_reason.as_deref(),
            Some("redacted"),
            "redaction chunk must carry finish_reason=redacted"
        );
        // The redaction chunk must not re-emit the leaked canary verbatim.
        assert!(
            !redaction.text_delta.contains(&canary),
            "redaction chunk re-emitted the canary verbatim"
        );
    }

    /// A secret in `reasoning_delta` is redacted via the reasoning channel.
    #[test]
    fn guarded_stream_secret_in_reasoning_emits_redaction() {
        let guard = std::sync::Arc::new(ContentGuard::mandatory(&GuardConfig::default()));
        let canary = guard.canary().as_str().to_string();
        let chunks = vec![
            chunk("clean answer", ""),
            chunk("", &format!("thinking... {canary} ...done")),
        ];
        let mut stream = guarded_stream_from_chunks(chunks, Arc::clone(&guard));

        let collected: Vec<_> = stream.collect();

        assert_eq!(
            collected.len(),
            3,
            "expected 2 source chunks + 1 redaction chunk"
        );
        let redaction = collected[2].as_ref().expect("redaction chunk should be Ok");
        assert_eq!(
            redaction.finish_reason.as_deref(),
            Some("redacted"),
            "reasoning-secret redaction must carry finish_reason=redacted"
        );
        assert!(
            !redaction.reasoning_delta.contains(&canary),
            "redaction chunk re-emitted the canary in reasoning verbatim"
        );
    }

    /// After the stream ends (and `scanned` is set), subsequent polls return
    /// `Ready(None)` without re-scanning. This pins the `scanned` guard against
    /// re-entrancy on the redaction path.
    #[test]
    fn guarded_stream_double_poll_after_end_returns_none() {
        let guard = std::sync::Arc::new(ContentGuard::mandatory(&GuardConfig::default()));
        let chunks = vec![chunk("clean", "")];
        let mut stream = guarded_stream_from_chunks(chunks, Arc::clone(&guard));

        // Drain the stream fully.
        let _ = stream.by_ref().collect::<Vec<_>>();

        // Pin the stream and poll manually; the inner stream is exhausted and
        // `scanned` is set, so this must return `Ready(None)` without invoking
        // the guard again.
        let mut cx = noop_cx();
        let mut pinned = std::pin::pin!(stream);
        let poll1 = pinned.as_mut().poll_next(&mut cx);
        assert!(
            matches!(poll1, Poll::Ready(None)),
            "post-end poll returned {poll1:?}"
        );
        let poll2 = pinned.as_mut().poll_next(&mut cx);
        assert!(
            matches!(poll2, Poll::Ready(None)),
            "second post-end poll returned {poll2:?}"
        );
    }

    fn make_result(text: String, reasoning: Option<String>) -> InferenceResult {
        InferenceResult {
            text,
            model: "test-model".to_string(),
            usage: InferenceUsage {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
            },
            finish_reason: "stop".to_string(),
            token_probabilities: None,
            tool_calls: vec![],
            reasoning,
            cost_usd: None,
        }
    }

    /// RR-0030 enforcement: the canary token must be redacted from the
    /// `reasoning` field of `InferenceResult`, not just from `text`.
    /// Thinking-mode models (Qwen3, GLM-5.2, DeepSeek-R1) routinely echo
    /// system-prompt content — including the canary — into the reasoning
    /// trace; an unscanned reasoning channel silently defeats canary/secret
    /// detection and leaks secrets past Layer 7 output filtering.
    #[test]
    fn canary_in_reasoning_field_is_redacted() {
        let guard = ContentGuard::mandatory(&GuardConfig::default());
        let canary = guard.canary().as_str().to_string();

        // Embed the canary in the reasoning trace, keep the visible text clean.
        let result = make_result(
            "clean output".to_string(),
            Some(format!("thinking about the prompt... {canary} ...done")),
        );
        let guarded = guard_output(result, &guard);

        let guarded_reasoning = guarded.reasoning.expect("reasoning should still be Some");
        assert!(
            !guarded_reasoning.contains(&canary),
            "canary token not redacted from reasoning. output: {guarded_reasoning}"
        );
    }

    /// RR-0030 complement: clean reasoning (no canary) must be preserved
    /// unchanged — no false positives from the reasoning scanner.
    #[test]
    fn clean_reasoning_is_preserved() {
        let guard = ContentGuard::mandatory(&GuardConfig::default());
        let reasoning = "thinking about the problem step by step";
        let result = make_result("answer".to_string(), Some(reasoning.to_string()));
        let guarded = guard_output(result, &guard);

        assert_eq!(
            guarded.reasoning.as_deref(),
            Some(reasoning),
            "clean reasoning was modified by guard_output — false positive"
        );
    }
}
