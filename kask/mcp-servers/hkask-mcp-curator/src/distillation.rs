//! ALWAYS-mode memory distillation — the automatic sibling of the
//! `curator_memory_extract` tool.
//!
//! The extract tool is on-demand: an agent lists a thread's turns and
//! inserts the lessons worth keeping via `memory_insert`. This module is
//! the closed-loop version (operator decision 2026-09-01, "Option A"):
//! a background pass that distills finished threads into candidate
//! lesson h_mems automatically, so lessons survive the session without
//! anyone choosing to save them.
//!
//! Sovereignty contract (memory-system-specification.md §10): the pass is
//! ADDITIVE-ONLY. It inserts lesson h_mems (Shared visibility, 0.5
//! confidence floor, evidence-verified) plus one Private watermark h_mem
//! per distilled thread. It never edits, expires, or deletes an existing
//! h_mem — promotion, contradiction resolution, and pruning remain the
//! user's tools (`memory_update`, `memory_resolve_contradiction`,
//! therapy). Pinned by `distillation_pass_is_additive_only`.
//!
//! Idempotency: each distilled thread carries a watermark h_mem
//! (`curator:distilled:{thread_id}`, attribute `distilled_through`) whose
//! value names the newest turn already distilled. A pass distills only
//! turns newer than the watermark, so restarts and re-runs insert no
//! duplicates. Pinned by `distillation_pass_respects_watermark`.
//!
//! Observability: every pass emits a module-target `tracing::info!`
//! summary plus a `RegulationSpan::Curation` "memory_distilled" span, and
//! its outputs (lessons + watermarks) are queryable via
//! `curator_memory_recall` / `curator_semantic_search`. The consolidation
//! timer's lesson applies here — a loop whose events go nowhere readable
//! is indistinguishable from a broken one.

use crate::CuratorDb;
use hkask_storage::HMem;
use hkask_types::WebID;
use hkask_types::regulation::RegulationSpan;
use hkask_types::template::LLMParameters;
use std::sync::Arc;

/// Default pass cadence. 0 disables the pass (read from
/// `HKASK_MEMORY_DISTILLATION_CADENCE_SECS`, injected from
/// `kask.memory.distillation_cadence_secs`).
pub(crate) const DEFAULT_DISTILLATION_CADENCE_SECS: u64 = 600;

/// A thread counts as finished when its newest turn is at least this old
/// (read from `HKASK_MEMORY_DISTILLATION_IDLE_SECS`, injected from
/// `kask.memory.distillation_idle_secs`).
pub(crate) const DEFAULT_DISTILLATION_IDLE_SECS: u64 = 300;

/// Turns newer than this are not examined yet — the thread may still be
/// active. Bounded so a restart does not re-scan the whole store; turns
/// older than the lookback that were never distilled are missed (raw
/// transcript remains; therapy can still distill them).
const FIRST_PASS_LOOKBACK_SECS: i64 = 6 * 3600;

pub(crate) const WATERMARK_PREFIX: &str = "curator:distilled:";
const MAX_TURNS_PER_PROMPT: usize = 12;
const MAX_TURN_CHARS: usize = 3_000;
const MAX_LESSONS_PER_THREAD: usize = 5;
const MAX_ENTITY_CHARS: usize = 128;
const MAX_TEXT_CHARS: usize = 2_000;
const MAX_EVIDENCE_IDS: usize = 8;

/// Distillation cadence and idle threshold.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DistillationConfig {
    pub cadence_secs: u64,
    pub idle_secs: u64,
}

impl Default for DistillationConfig {
    fn default() -> Self {
        Self {
            cadence_secs: DEFAULT_DISTILLATION_CADENCE_SECS,
            idle_secs: DEFAULT_DISTILLATION_IDLE_SECS,
        }
    }
}

impl DistillationConfig {
    /// Read from env. Malformed values warn naming the value and fall
    /// back to the default — never a silent fallback.
    pub(crate) fn from_env() -> Self {
        Self {
            cadence_secs: parse_env_u64(
                "HKASK_MEMORY_DISTILLATION_CADENCE_SECS",
                DEFAULT_DISTILLATION_CADENCE_SECS,
            ),
            idle_secs: parse_env_u64(
                "HKASK_MEMORY_DISTILLATION_IDLE_SECS",
                DEFAULT_DISTILLATION_IDLE_SECS,
            ),
        }
    }
}

fn parse_env_u64(name: &str, default: u64) -> u64 {
    match std::env::var(name) {
        Ok(raw) => parse_u64_value(name, &raw, default),
        Err(_) => default,
    }
}

fn parse_u64_value(name: &str, raw: &str, default: u64) -> u64 {
    match raw.trim().parse::<u64>() {
        Ok(value) => value,
        Err(_) => {
            tracing::warn!(
                target: "hkask.mcp.curator.distillation",
                env = name,
                value = %raw,
                "Malformed distillation setting — using default"
            );
            default
        }
    }
}

/// Start the background distillation timer. Called from the server
/// factory, where the DB handle, inference port, and webid are all in
/// hand. Cadence 0 disables the pass with an info line, not silence.
pub(crate) fn spawn_distillation_timer(
    db: Arc<CuratorDb>,
    inference_port: Arc<dyn hkask_types::InferencePort>,
    webid: WebID,
) {
    let config = DistillationConfig::from_env();
    if config.cadence_secs == 0 {
        tracing::info!(
            target: "hkask.mcp.curator.distillation",
            "Memory distillation pass disabled (cadence 0)"
        );
        return;
    }
    let handle = match tokio::runtime::Handle::try_current() {
        Ok(handle) => handle,
        Err(error) => {
            tracing::warn!(
                target: "hkask.mcp.curator.distillation",
                %error,
                "No tokio runtime at server construction — memory distillation pass NOT started"
            );
            return;
        }
    };
    let cadence = config.cadence_secs;
    let idle_secs = config.idle_secs;
    handle.spawn(async move {
        let poll_interval = std::time::Duration::from_secs(cadence.clamp(60, 3600));
        let mut interval = tokio::time::interval(poll_interval);
        interval.tick().await; // skip first tick
        let mut last_pass: Option<chrono::DateTime<chrono::Utc>> = None;
        loop {
            interval.tick().await;
            let now = chrono::Utc::now();
            let since = last_pass
                .unwrap_or_else(|| now - chrono::Duration::seconds(FIRST_PASS_LOOKBACK_SECS));
            let outcome =
                run_distillation_pass(&db, inference_port.as_ref(), webid, now, idle_secs, since)
                    .await;
            tracing::info!(
                target: "hkask.mcp.curator.distillation",
                threads_examined = outcome.threads_examined,
                threads_distilled = outcome.threads_distilled,
                lessons_inserted = outcome.lessons_inserted,
                lessons_skipped = outcome.lessons_skipped,
                "Memory distillation pass complete"
            );
            last_pass = Some(now);
        }
    });
}

/// One pass over the curator's own DB. Store or query failures warn and
/// skip the pass — the timer must survive every outcome.
async fn run_distillation_pass(
    db: &CuratorDb,
    inference_port: &dyn hkask_types::InferencePort,
    webid: WebID,
    now: chrono::DateTime<chrono::Utc>,
    idle_secs: u64,
    since: chrono::DateTime<chrono::Utc>,
) -> DistillationOutcome {
    let stores = db.get();
    let Some(memory) = stores.memory.as_ref() else {
        tracing::warn!(
            target: "hkask.mcp.curator.distillation",
            "Curator memory store unavailable — distillation pass skipped (store self-heals on next open)"
        );
        return DistillationOutcome::default();
    };
    distill_store(memory, inference_port, webid, now, idle_secs, since).await
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub(crate) struct DistillationOutcome {
    pub threads_examined: usize,
    pub threads_distilled: usize,
    pub lessons_inserted: usize,
    pub lessons_skipped: usize,
}

/// The distillation core, directly testable against a `MemoryStore`.
///
/// Additive-only: the only store mutation is `store(h_mem)` — no update,
/// expire, or delete call exists in this function.
pub(crate) async fn distill_store(
    memory: &hkask_memory::MemoryStore,
    inference_port: &dyn hkask_types::InferencePort,
    webid: WebID,
    now: chrono::DateTime<chrono::Utc>,
    idle_secs: u64,
    since: chrono::DateTime<chrono::Utc>,
) -> DistillationOutcome {
    let mut outcome = DistillationOutcome::default();
    // Turn discovery is the shared contract (`thread_turns`): the scan runs
    // over the shared-copy prefix, which ingest writes for EVERY turn —
    // curator and non-curator alike — so no turn is invisible to the pass.
    let by_thread = match crate::thread_turns::shared_turns_by_thread_since(memory, since) {
        Ok(by_thread) => by_thread,
        Err(error) => {
            tracing::warn!(
                target: "hkask.mcp.curator.distillation",
                %error,
                "Failed to query recent thread turns — distillation pass skipped"
            );
            return outcome;
        }
    };
    outcome.threads_examined = by_thread.len();
    let idle_cutoff = now - chrono::Duration::seconds(idle_secs as i64);
    for (thread_id, mut turns) in by_thread {
        turns.sort_by_key(|turn| turn.observed_at);
        // An active conversation is not a finished thread — distilling it
        // would race with turns still arriving.
        let Some(newest) = turns.last().map(|turn| turn.observed_at) else {
            continue;
        };
        if newest > idle_cutoff {
            continue;
        }
        let watermark_entity = format!("{WATERMARK_PREFIX}{thread_id}");
        let through = match memory.h_mems_by_entity_prefix(&watermark_entity) {
            Ok(watermarks) => watermarks.iter().filter_map(parse_watermark_through).max(),
            Err(error) => {
                tracing::warn!(
                    target: "hkask.mcp.curator.distillation",
                    thread_id = %thread_id,
                    %error,
                    "Failed to read distillation watermark — thread skipped this pass"
                );
                continue;
            }
        };
        let pending: Vec<&HMem> = turns
            .iter()
            .filter(|turn| through.map_or(true, |watermark| turn.observed_at > watermark))
            .collect();
        if pending.is_empty() {
            continue;
        }
        let prompt = build_distillation_prompt(&thread_id, &pending);
        let generated = match inference_port
            .generate(&prompt, &LLMParameters::default(), None)
            .await
        {
            Ok(result) => result,
            Err(error) => {
                tracing::warn!(
                    target: "hkask.mcp.curator.distillation",
                    thread_id = %thread_id,
                    %error,
                    "Distillation inference failed — thread retried next pass (watermark not advanced)"
                );
                continue;
            }
        };
        let candidates = match parse_lessons(&generated.text) {
            Ok(candidates) => candidates,
            Err(error) => {
                tracing::warn!(
                    target: "hkask.mcp.curator.distillation",
                    thread_id = %thread_id,
                    %error,
                    "Distillation output unparseable — thread retried next pass (watermark not advanced)"
                );
                continue;
            }
        };
        // Advance the watermark BEFORE inserting lessons: a failure after
        // lessons are stored would re-distill the same turns next pass and
        // duplicate them — the exact redundancy this pass exists to end.
        // A failure before lessons loses them once, loudly, with the raw
        // turns still in memory for therapy.
        let through_newest = pending.last().map(|turn| turn.observed_at);
        let Some(through_newest) = through_newest else {
            continue;
        };
        let watermark = HMem::new(
            &watermark_entity,
            "distilled_through",
            serde_json::json!({
                "through": through_newest.to_rfc3339(),
                "turns": pending.len(),
            }),
            webid,
        )
        .with_confidence(hkask_types::Confidence::new(0.5))
        .with_visibility(hkask_types::Visibility::Private);
        if let Err(error) = memory.store(watermark) {
            tracing::warn!(
                target: "hkask.mcp.curator.distillation",
                thread_id = %thread_id,
                %error,
                "Failed to store distillation watermark — thread retried next pass"
            );
            continue;
        }
        let mut inserted = 0usize;
        let mut skipped = 0usize;
        for candidate in candidates.into_iter().take(MAX_LESSONS_PER_THREAD) {
            match insert_lesson(memory, inference_port, &candidate, &thread_id, webid).await {
                Ok(true) => inserted += 1,
                Ok(false) => skipped += 1,
                Err(error) => {
                    tracing::warn!(
                        target: "hkask.mcp.curator.distillation",
                        thread_id = %thread_id,
                        %error,
                        "Failed to store distilled lesson"
                    );
                    skipped += 1;
                }
            }
        }
        outcome.threads_distilled += 1;
        outcome.lessons_inserted += inserted;
        outcome.lessons_skipped += skipped;
    }
    if outcome.threads_distilled > 0 {
        RegulationSpan::Curation.emit("memory_distilled");
    }
    outcome
}

fn parse_watermark_through(h_mem: &HMem) -> Option<chrono::DateTime<chrono::Utc>> {
    h_mem
        .value
        .get("through")
        .and_then(|value| value.as_str())
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map(|parsed| parsed.with_timezone(&chrono::Utc))
}

#[derive(Debug, serde::Deserialize)]
struct LessonCandidate {
    entity: String,
    attribute: String,
    text: String,
    evidence: Vec<String>,
}

/// A distillation-output parse failure. The Display message is surfaced in
/// the retry warn so the operator can see the model emitted unparseable
/// output (the thread is retried next pass, watermark not advanced).
#[derive(Debug, thiserror::Error)]
#[error("lesson array parse: {source}")]
struct LessonParseError {
    #[source]
    source: serde_json::Error,
}

fn parse_lessons(text: &str) -> Result<Vec<LessonCandidate>, LessonParseError> {
    let extracted = hkask_types::json_extract::extract_json_from_response(text);
    let parsed: Vec<LessonCandidate> =
        serde_json::from_str(&extracted).map_err(|source| LessonParseError { source })?;
    Ok(parsed)
}

/// Insert one distilled lesson. Returns `Ok(false)` when the candidate is
/// malformed or cites evidence that does not exist — the same
/// evidence-verification invariant `memory_insert` enforces.
async fn insert_lesson(
    memory: &hkask_memory::MemoryStore,
    inference_port: &dyn hkask_types::InferencePort,
    candidate: &LessonCandidate,
    thread_id: &str,
    webid: WebID,
) -> Result<bool, hkask_memory::MemoryStoreError> {
    let entity = candidate.entity.trim();
    let attribute = candidate.attribute.trim();
    let text = candidate.text.trim();
    if entity.is_empty()
        || attribute.is_empty()
        || text.is_empty()
        || entity.len() > MAX_ENTITY_CHARS
        || attribute.len() > MAX_ENTITY_CHARS
        || candidate.evidence.is_empty()
    {
        tracing::warn!(
            target: "hkask.mcp.curator.distillation",
            "Skipping malformed lesson candidate (empty or oversized entity/attribute, empty text, or no evidence)"
        );
        return Ok(false);
    }
    let evidence: Vec<String> = candidate
        .evidence
        .iter()
        .take(MAX_EVIDENCE_IDS)
        .cloned()
        .collect();
    for id in &evidence {
        let parsed = match id.parse::<hkask_storage::HMemId>() {
            Ok(parsed) => parsed,
            Err(error) => {
                tracing::warn!(
                    target: "hkask.mcp.curator.distillation",
                    evidence_id = %id,
                    %error,
                    "Lesson cites malformed evidence h_mem id — skipping lesson"
                );
                return Ok(false);
            }
        };
        if memory.get_by_id(&parsed).ok().flatten().is_none() {
            tracing::warn!(
                target: "hkask.mcp.curator.distillation",
                evidence_id = %id,
                "Lesson cites nonexistent evidence h_mem — skipping lesson"
            );
            return Ok(false);
        }
    }
    let text = truncate_chars(text, MAX_TEXT_CHARS);
    let value = serde_json::json!({
        "text": text,
        "evidence": evidence,
        "source_thread": thread_id,
    });
    let lesson = HMem::new(entity, attribute, value, webid)
        .with_confidence(hkask_types::Confidence::new(0.5))
        .with_visibility(hkask_types::Visibility::Shared)
        .with_dimension(hkask_types::Dimension::Why);
    memory.store(lesson)?;
    // Embed the lesson text under the lesson's entity so semantic recall
    // finds it by meaning — the shared insert-path embedding contract
    // (`embed_for_semantic_recall`), which also serves `memory_insert` and
    // the skill-use issue path. The embedded text is the same truncated
    // text that was stored, so the vector always represents the durable
    // lesson (the previous inline copy embedded the untruncated original).
    crate::embed_for_semantic_recall(inference_port, memory, entity, &text).await;
    Ok(true)
}

/// Truncate on a character boundary — byte slicing at a fixed index can
/// split a multi-byte character and panic.
fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let boundary = text
        .char_indices()
        .map(|(index, _)| index)
        .nth(max)
        .unwrap_or(text.len());
    format!("{}…", &text[..boundary])
}

fn build_distillation_prompt(thread_id: &str, turns: &[&HMem]) -> String {
    let start = turns.len().saturating_sub(MAX_TURNS_PER_PROMPT);
    let mut turns_json = Vec::new();
    for turn in &turns[start..] {
        let user_input = turn
            .value
            .get("user_input")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let agent_response = turn
            .value
            .get("agent_response")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        turns_json.push(serde_json::json!({
            "h_mem_id": turn.id.to_string(),
            "user_input": truncate_chars(user_input, MAX_TURN_CHARS),
            "agent_response": truncate_chars(agent_response, MAX_TURN_CHARS),
        }));
    }
    format!(
        "You are distilling a finished conversation thread into durable lessons \
         for a long-lived memory system.\n\n\
         Thread: {thread_id}\n\
         Turns (oldest first):\n{turns}\n\n\
         Extract 0-{MAX_LESSONS_PER_THREAD} durable, generalizable lessons — \
         stable facts, preferences, decisions, and corrections a future session \
         should know. Not task narration, not transient details. Each lesson must \
         cite at least one h_mem_id from the turns above as evidence.\n\n\
         Return ONLY a JSON array, no prose, no code fences:\n\
         [{{\"entity\": \"<short-stable-subject-slug>\", \
         \"attribute\": \"<what-is-remembered>\", \
         \"text\": \"<the lesson, one or two sentences>\", \
         \"evidence\": [\"<h_mem_id>\"]}}]\n\n\
         Return [] if nothing durable.",
        turns = serde_json::to_string_pretty(&turns_json).unwrap_or_default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thread_turns::SHARED_TURN_PREFIX;
    use hkask_storage::database::sqlite::SqliteDriver;
    use hkask_types::InferenceError;
    use hkask_types::InferenceResult;
    use hkask_types::InferenceUsage;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;

    fn test_store() -> hkask_memory::MemoryStore {
        let driver = SqliteDriver::in_memory_driver();
        let h_mem_store =
            hkask_storage::HMemStore::from_driver(Arc::clone(&driver)).expect("h_mem store");
        let embedding_store =
            hkask_storage::EmbeddingStore::from_driver(driver, hkask_storage::embedding_dim())
                .expect("embedding store");
        hkask_memory::MemoryStore::new(h_mem_store, embedding_store)
    }

    fn turn_h_mem(
        store: &hkask_memory::MemoryStore,
        thread_id: &str,
        user_input: &str,
        agent_response: &str,
        observed_at: chrono::DateTime<chrono::Utc>,
        webid: WebID,
    ) -> hkask_storage::HMemId {
        let h_mem = HMem::new(
            &format!("{SHARED_TURN_PREFIX}{thread_id}"),
            "turn",
            serde_json::json!({
                "user_input": user_input,
                "agent_response": agent_response,
            }),
            webid,
        );
        let mut h_mem = h_mem;
        h_mem.observed_at = observed_at;
        store.store(h_mem).expect("store turn");
        // Re-read to get the id the store assigned.
        store
            .h_mems_by_entity_prefix(&format!("{SHARED_TURN_PREFIX}{thread_id}"))
            .expect("query turns")
            .into_iter()
            .find(|h| h.observed_at == observed_at)
            .map(|h| h.id)
            .expect("stored turn id")
    }

    /// Scripted distillation port: returns a fixed response for every
    /// `generate` call; `embed` uses the trait default (unavailable), which
    /// the insert path treats as non-fatal.
    struct ScriptedDistillPort {
        response: String,
    }

    impl hkask_types::InferencePort for ScriptedDistillPort {
        fn generate(
            &self,
            _prompt: &str,
            _parameters: &LLMParameters,
            _tools: Option<&[hkask_types::ChatToolDefinition]>,
        ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>>
        {
            let text = self.response.clone();
            Box::pin(async move {
                Ok(InferenceResult {
                    text,
                    model: "test-model".to_string(),
                    usage: InferenceUsage {
                        prompt_tokens: 0,
                        completion_tokens: 0,
                        total_tokens: 0,
                    },
                    finish_reason: "stop".to_string(),
                    tool_calls: Vec::new(),
                    reasoning: None,
                    cost_usd: None,
                })
            })
        }
    }

    fn lesson_response(entity: &str, attribute: &str, text: &str, evidence: &[&str]) -> String {
        let evidence: Vec<String> = evidence.iter().map(|id| id.to_string()).collect();
        serde_json::json!([{
            "entity": entity,
            "attribute": attribute,
            "text": text,
            "evidence": evidence,
        }])
        .to_string()
    }

    #[tokio::test]
    async fn distillation_pass_inserts_lessons_and_watermark() {
        let store = test_store();
        let webid = WebID::from_persona(b"curator");
        let now = chrono::Utc::now();
        let turn_id = turn_h_mem(
            &store,
            "t1",
            "ok - nothing in that was in functional language so its a fail",
            "Goal scored not-achieved — Brier 0.81.",
            now - chrono::Duration::seconds(600),
            webid,
        );
        let port = ScriptedDistillPort {
            response: lesson_response(
                "operator-reporting-standard",
                "report_language",
                "Reports lead with what the user can now do, in functional language.",
                &[&turn_id.to_string()],
            ),
        };
        let outcome = distill_store(
            &store,
            &port,
            webid,
            now,
            DEFAULT_DISTILLATION_IDLE_SECS,
            now - chrono::Duration::seconds(3600),
        )
        .await;
        assert_eq!(outcome.threads_examined, 1);
        assert_eq!(outcome.threads_distilled, 1);
        assert_eq!(outcome.lessons_inserted, 1);
        // The lesson exists at the 0.5 floor, Shared visibility.
        let lessons = store
            .h_mems_by_entity_prefix("operator-reporting-standard")
            .expect("query lessons");
        assert_eq!(lessons.len(), 1);
        assert_eq!(lessons[0].confidence.value(), 0.5);
        assert_eq!(lessons[0].attribute, "report_language");
        // The watermark exists and names the distilled turn.
        let watermarks = store
            .h_mems_by_entity_prefix("curator:distilled:t1")
            .expect("query watermarks");
        assert_eq!(watermarks.len(), 1);
        assert_eq!(watermarks[0].attribute, "distilled_through");
        assert_eq!(
            parse_watermark_through(&watermarks[0]),
            Some(now - chrono::Duration::seconds(600))
        );
    }

    #[tokio::test]
    async fn distillation_pass_is_additive_only() {
        let store = test_store();
        let webid = WebID::from_persona(b"curator");
        let now = chrono::Utc::now();
        // Pre-existing memories the pass must not touch.
        for entity in ["company:AAPL", "company:MSFT", "note:keep"] {
            store
                .store(HMem::new(
                    entity,
                    "fact",
                    serde_json::json!("stable"),
                    webid,
                ))
                .expect("seed h_mem");
        }
        let before: Vec<(String, String, serde_json::Value)> =
            ["company:AAPL", "company:MSFT", "note:keep"]
                .iter()
                .flat_map(|entity| {
                    store
                        .h_mems_by_entity_prefix(entity)
                        .expect("query before")
                        .into_iter()
                        .map(|h| (h.entity, h.attribute, h.value))
                })
                .collect();
        let before_count = store.h_mem_count().expect("count before");
        let turn_id = turn_h_mem(
            &store,
            "t1",
            "please proceed with A",
            "Closing the memory distillation loop.",
            now - chrono::Duration::seconds(600),
            webid,
        );
        let port = ScriptedDistillPort {
            response: lesson_response(
                "distillation-decision",
                "option",
                "Operator chose additive auto-distillation.",
                &[&turn_id.to_string()],
            ),
        };
        let outcome = distill_store(
            &store,
            &port,
            webid,
            now,
            DEFAULT_DISTILLATION_IDLE_SECS,
            now - chrono::Duration::seconds(3600),
        )
        .await;
        assert_eq!(outcome.lessons_inserted, 1);
        // Every pre-existing h_mem is byte-identical.
        let after: Vec<(String, String, serde_json::Value)> =
            ["company:AAPL", "company:MSFT", "note:keep"]
                .iter()
                .flat_map(|entity| {
                    store
                        .h_mems_by_entity_prefix(entity)
                        .expect("query after")
                        .into_iter()
                        .map(|h| (h.entity, h.attribute, h.value))
                })
                .collect();
        assert_eq!(before, after);
        // The store only grew: 3 seeds + 1 turn + 1 lesson + 1 watermark.
        let after_count = store.h_mem_count().expect("count after");
        assert_eq!(after_count, before_count + 3);
    }

    #[tokio::test]
    async fn distillation_pass_respects_watermark() {
        let store = test_store();
        let webid = WebID::from_persona(b"curator");
        let now = chrono::Utc::now();
        let turn_id = turn_h_mem(
            &store,
            "t1",
            "first turn",
            "first response",
            now - chrono::Duration::seconds(600),
            webid,
        );
        let port = ScriptedDistillPort {
            response: lesson_response(
                "subject",
                "lesson",
                "A durable lesson.",
                &[&turn_id.to_string()],
            ),
        };
        let first = distill_store(
            &store,
            &port,
            webid,
            now,
            DEFAULT_DISTILLATION_IDLE_SECS,
            now - chrono::Duration::seconds(3600),
        )
        .await;
        assert_eq!(first.lessons_inserted, 1);
        // Second pass over the same turns (since = epoch): the watermark
        // filters every turn — no duplicate lessons, no duplicate watermarks.
        let second = distill_store(
            &store,
            &port,
            webid,
            now,
            DEFAULT_DISTILLATION_IDLE_SECS,
            chrono::DateTime::from_timestamp(0, 0).expect("epoch"),
        )
        .await;
        assert_eq!(second.threads_distilled, 0);
        assert_eq!(second.lessons_inserted, 0);
        let lessons = store
            .h_mems_by_entity_prefix("subject")
            .expect("query lessons");
        assert_eq!(lessons.len(), 1);
        let watermarks = store
            .h_mems_by_entity_prefix("curator:distilled:t1")
            .expect("query watermarks");
        assert_eq!(watermarks.len(), 1);
    }

    #[tokio::test]
    async fn distillation_pass_skips_active_threads() {
        let store = test_store();
        let webid = WebID::from_persona(b"curator");
        let now = chrono::Utc::now();
        // Newest turn is 10s old — inside the 300s idle window.
        turn_h_mem(
            &store,
            "t1",
            "still typing",
            "still responding",
            now - chrono::Duration::seconds(10),
            webid,
        );
        let port = ScriptedDistillPort {
            response: "[]".to_string(),
        };
        let outcome = distill_store(
            &store,
            &port,
            webid,
            now,
            DEFAULT_DISTILLATION_IDLE_SECS,
            now - chrono::Duration::seconds(3600),
        )
        .await;
        assert_eq!(outcome.threads_examined, 1);
        assert_eq!(outcome.threads_distilled, 0);
        assert_eq!(outcome.lessons_inserted, 0);
        assert!(
            store
                .h_mems_by_entity_prefix("curator:distilled:t1")
                .expect("query watermarks")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn distillation_pass_rejects_unknown_evidence() {
        let store = test_store();
        let webid = WebID::from_persona(b"curator");
        let now = chrono::Utc::now();
        turn_h_mem(
            &store,
            "t1",
            "turn text",
            "response text",
            now - chrono::Duration::seconds(600),
            webid,
        );
        let bogus = "00000000-0000-0000-0000-000000000000";
        let port = ScriptedDistillPort {
            response: lesson_response(
                "subject",
                "lesson",
                "A lesson citing nothing real.",
                &[bogus],
            ),
        };
        let outcome = distill_store(
            &store,
            &port,
            webid,
            now,
            DEFAULT_DISTILLATION_IDLE_SECS,
            now - chrono::Duration::seconds(3600),
        )
        .await;
        assert_eq!(outcome.lessons_inserted, 0);
        assert_eq!(outcome.lessons_skipped, 1);
        // The thread was examined and its watermark advanced — a bad model
        // response must not wedge the thread into an infinite retry.
        assert_eq!(outcome.threads_distilled, 1);
        assert!(
            store
                .h_mems_by_entity_prefix("subject")
                .expect("query lessons")
                .is_empty()
        );
    }

    #[test]
    fn distillation_config_parses_values_with_warn_on_malformed() {
        // Pure parse: malformed values fall back to the default.
        assert_eq!(
            parse_u64_value("HKASK_MEMORY_DISTILLATION_CADENCE_SECS", "120", 600),
            120
        );
        assert_eq!(
            parse_u64_value("HKASK_MEMORY_DISTILLATION_CADENCE_SECS", " 240 ", 600),
            240
        );
        assert_eq!(
            parse_u64_value("HKASK_MEMORY_DISTILLATION_CADENCE_SECS", "soon", 600),
            600
        );
        assert_eq!(
            parse_u64_value("HKASK_MEMORY_DISTILLATION_IDLE_SECS", "-5", 300),
            300
        );
    }
}
