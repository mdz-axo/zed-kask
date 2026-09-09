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
//! per distilled thread. It never edits or deletes an existing
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
//! Pending work: a thread skipped as active, or failing before its
//! watermark advances (inference, parse, or watermark-store failure), is
//! carried in the timer's in-memory pending set and re-examined on every
//! later pass — its turns fall behind the scan cursor, so without this
//! tracking no later pass would ever see them again. The set is bounded
//! (`MAX_PENDING_THREADS`); overflow evicts the longest-pending thread
//! with a warn naming it and its re-discovery paths (a new turn, or the
//! first-pass recovery scan). A pass that cannot read the store does not
//! advance the cursor, so turns stored during an outage stay visible to
//! the healed pass. Pending state is in-memory only: a restart clears
//! it, and the first pass recovers from durable state — it scans from
//! the oldest stored turn and the watermark check skips covered threads,
//! so no thread is ever permanently missed.
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
use std::collections::HashMap;
use std::sync::Arc;

/// Default pass cadence. 0 disables the pass (read from
/// `HKASK_MEMORY_DISTILLATION_CADENCE_SECS`, injected from
/// `kask.memory.distillation_cadence_secs`).
pub(crate) const DEFAULT_DISTILLATION_CADENCE_SECS: u64 = 600;

/// A thread counts as finished when its newest turn is at least this old
/// (read from `HKASK_MEMORY_DISTILLATION_IDLE_SECS`, injected from
/// `kask.memory.distillation_idle_secs`).
pub(crate) const DEFAULT_DISTILLATION_IDLE_SECS: u64 = 300;

/// The non-thinking model the lesson-extraction `generate` calls route to
/// (read from `HKASK_CLASSIFIER_MODEL`, injected from
/// `kask.models.classifier_model`). Distillation needs output tokens, not
/// reasoning tokens — the same workload shape as the turn-tagging
/// classifier (operator ruling 2026-09-04). The port default model is
/// reasoning-mandatory and rejects `thinking_allowed: false` ("Reasoning
/// is mandatory for this endpoint and cannot be disabled" — observed live
/// 2026-09-09: every distillation call failed for 5 days while the failure
/// lived only in logs). `None` falls back to the port default via
/// `generate_with_model`'s trait default; per-thread failure warns surface
/// that misconfiguration.
pub(crate) const WATERMARK_PREFIX: &str = "curator:distilled:";
const MAX_TURNS_PER_PROMPT: usize = 12;
const MAX_TURN_CHARS: usize = 3_000;
const MAX_LESSONS_PER_THREAD: usize = 5;
const MAX_ENTITY_CHARS: usize = 128;
const MAX_TEXT_CHARS: usize = 2_000;
const MAX_EVIDENCE_IDS: usize = 8;
/// Bound on the timer's in-memory pending set (threads with un-distilled
/// turns that fell behind the scan cursor). Overflow evicts the
/// longest-pending thread with a warn — bounded memory, non-silent loss
/// of revisiting. An evicted thread is re-discovered by a new turn or
/// the first-pass recovery scan: exactly the status quo for every
/// skipped thread before pending tracking existed.
const MAX_PENDING_THREADS: usize = 128;

/// Distillation cadence, idle threshold, and the non-thinking extraction
/// model. Not `Copy`: `model` owns a `String`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DistillationConfig {
    pub cadence_secs: u64,
    pub idle_secs: u64,
    /// Non-thinking model for lesson extraction (`HKASK_CLASSIFIER_MODEL`).
    pub model: Option<String>,
}

impl Default for DistillationConfig {
    fn default() -> Self {
        Self {
            cadence_secs: DEFAULT_DISTILLATION_CADENCE_SECS,
            idle_secs: DEFAULT_DISTILLATION_IDLE_SECS,
            model: None,
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
            model: std::env::var("HKASK_CLASSIFIER_MODEL").ok(),
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
    config: DistillationConfig,
) {
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
    let model = config.model;
    // Forgetting rides the distillation timer — it consumes what
    // distillation produces (the watermark is the extraction proof), so
    // cadence 0 disables both.
    let forgetting_days = crate::forgetting::ForgettingConfig::from_env().days;
    if forgetting_days == 0 {
        tracing::info!(
            target: "hkask.mcp.curator.forgetting",
            "Memory forgetting pass disabled (forgetting days 0)"
        );
    }
    handle.spawn(async move {
        // Poll at the configured cadence (minimum 60s). The cursor, not
        // the poll interval, bounds what a pass sees, so a long cadence
        // must not be silently shortened — the same one-hour-cap defect
        // the T17 consolidation repair removed.
        let poll_interval = std::time::Duration::from_secs(cadence.max(60));
        let mut interval = tokio::time::interval(poll_interval);
        interval.tick().await; // skip first tick
        let mut cursor = DistillationCursor::new();
        loop {
            interval.tick().await;
            let now = chrono::Utc::now();
            let outcome = run_pass(
                &db,
                inference_port.as_ref(),
                webid,
                &mut cursor,
                now,
                idle_secs,
                model.as_deref(),
            )
            .await;
            tracing::info!(
                target: "hkask.mcp.curator.distillation",
                threads_examined = outcome.threads_examined,
                threads_distilled = outcome.threads_distilled,
                lessons_inserted = outcome.lessons_inserted,
                lessons_skipped = outcome.lessons_skipped,
                extraction_failures = outcome.extraction_failures,
                threads_pending = cursor.pending.len(),
                "Memory distillation pass complete"
            );
            // Distillation-gated forgetting (operator ruling 2026-09-04):
            // delete the shared-copy turns of threads whose
            // distillation watermark has aged past the forgetting
            // threshold — the goldfish principle's automatic leg.
            if forgetting_days > 0 {
                let forgetting_outcome =
                    crate::forgetting::run_forgetting_pass(&db, now, forgetting_days);
                tracing::info!(
                    target: "hkask.mcp.curator.forgetting",
                    threads_examined = forgetting_outcome.threads_examined,
                    threads_forgotten = forgetting_outcome.threads_forgotten,
                    turns_deleted = forgetting_outcome.turns_deleted,
                    embeddings_deleted = forgetting_outcome.embeddings_deleted,
                    orphans_swept = forgetting_outcome.orphans_swept,
                    "Memory forgetting pass complete"
                );
            }
        }
    });
}

/// Cross-pass state for the distillation timer: the scan cursor plus the
/// threads whose un-distilled turns fell behind it.
///
/// `last_pass` bounds the next scan (turns observed at or after it are
/// visible). Threads skipped as active, or failing before the watermark
/// advanced, are carried in `pending` and re-examined explicitly — their
/// turns are older than the cursor, so the scan alone would never see
/// them again. Pending state is in-memory: a restart clears it, and the
/// first pass recovers from durable state instead — it scans from the
/// oldest stored turn and the per-thread watermark check skips every
/// thread already proven distilled (the fixed 6h startup lookback it
/// replaced permanently missed older threads, observed live 2026-09-09:
/// watermarks stopped advancing for 5 days across restarts).
pub(crate) struct DistillationCursor {
    last_pass: Option<chrono::DateTime<chrono::Utc>>,
    /// thread_id -> when the thread was first noticed pending (eviction
    /// order; preserved across passes).
    pending: HashMap<String, chrono::DateTime<chrono::Utc>>,
}

impl DistillationCursor {
    pub(crate) fn new() -> Self {
        Self {
            last_pass: None,
            pending: HashMap::new(),
        }
    }

    /// The scan window start for a pass at `now`: the previous pass's
    /// time, or the Unix epoch on the first pass after a restart — the
    /// durable recovery. The scan loads every stored turn, but the
    /// per-thread watermark check skips covered threads in O(1), and the
    /// forgetting pass (watermark-gated) bounds the turn store to the
    /// forgetting horizon, so the recovery scan stays proportional to
    /// live episodic memory, not history.
    fn scan_since(&self, _now: chrono::DateTime<chrono::Utc>) -> chrono::DateTime<chrono::Utc> {
        self.last_pass
            .unwrap_or_else(|| chrono::DateTime::from_timestamp(0, 0).expect("epoch is valid"))
    }

    /// Merge a pass outcome. A failed scan leaves the cursor untouched —
    /// turns stored during the outage must stay ahead of the cursor so
    /// the healed pass sees them. Resolved threads leave `pending`;
    /// still-unresolved threads enter it, keeping their earlier
    /// first-seen time. Overflow evicts the longest-pending threads with
    /// a warn naming each.
    fn merge(&mut self, outcome: &DistillationOutcome, now: chrono::DateTime<chrono::Utc>) {
        if outcome.scan_failed {
            return;
        }
        self.last_pass = Some(now);
        let mut merged: HashMap<String, chrono::DateTime<chrono::Utc>> = outcome
            .threads_pending
            .iter()
            .map(|(thread_id, at)| {
                (
                    thread_id.clone(),
                    self.pending.get(thread_id).copied().unwrap_or(*at),
                )
            })
            .collect();
        if merged.len() > MAX_PENDING_THREADS {
            let mut by_age: Vec<(String, chrono::DateTime<chrono::Utc>)> =
                merged.into_iter().collect();
            by_age.sort_by_key(|(_, at)| *at);
            let evicted = by_age.len() - MAX_PENDING_THREADS;
            for (thread_id, _) in by_age.drain(..evicted) {
                tracing::warn!(
                    target: "hkask.mcp.curator.distillation",
                    thread_id = %thread_id,
                    pending_threads = MAX_PENDING_THREADS,
                    "Pending-distillation set full — longest-pending thread \
                     evicted; it is re-discovered by a new turn or the \
                     first-pass recovery scan"
                );
            }
            merged = by_age.into_iter().collect();
        }
        self.pending = merged;
    }
}

/// One production pass over the curator's own DB, updating the cursor.
///
/// A pass that cannot read the store (or whose scan query fails) leaves
/// the cursor unchanged — the next pass rescans the same window, so
/// turns stored during the outage stay visible. Store failures warn and
/// skip; the timer must survive every outcome.
async fn run_pass(
    db: &CuratorDb,
    inference_port: &dyn hkask_types::InferencePort,
    webid: WebID,
    cursor: &mut DistillationCursor,
    now: chrono::DateTime<chrono::Utc>,
    idle_secs: u64,
    model: Option<&str>,
) -> DistillationOutcome {
    let stores = db.get();
    let Some(memory) = stores.memory.as_ref() else {
        tracing::warn!(
            target: "hkask.mcp.curator.distillation",
            "Curator memory store unavailable — distillation pass skipped (store self-heals on next open)"
        );
        let mut outcome = DistillationOutcome::default();
        outcome.scan_failed = true;
        return outcome;
    };
    let since = cursor.scan_since(now);
    let revisit: Vec<String> = cursor.pending.keys().cloned().collect();
    let outcome = distill_store(
        memory,
        inference_port,
        webid,
        now,
        idle_secs,
        since,
        &revisit,
        model,
    )
    .await;
    cursor.merge(&outcome, now);
    outcome
}

#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct DistillationOutcome {
    pub threads_examined: usize,
    pub threads_distilled: usize,
    pub lessons_inserted: usize,
    pub lessons_skipped: usize,
    /// The pass could not read the store — the caller's cursor must not
    /// advance, or turns stored during the outage would fall behind it.
    pub scan_failed: bool,
    /// Threads still holding un-distilled turns after this pass (skipped
    /// as active, or failed before the watermark advanced), mapped to
    /// the pass time. The caller's cursor merges these, preserving
    /// earlier first-seen times for eviction ordering.
    pub threads_pending: HashMap<String, chrono::DateTime<chrono::Utc>>,
    /// Generate/parse failures this pass — threads that could not be
    /// extracted (model rejection, unparseable output). The pass emits a
    /// `memory_distillation_stalled` regulation span when this is
    /// nonzero so a dead extraction loop is a sensed regulation event,
    /// not log noise (observed live 2026-09-09: five days of failures
    /// surfaced nowhere readable).
    pub extraction_failures: usize,
}

/// The distillation core, directly testable against a `MemoryStore`.
///
/// Additive-only: the only store mutation is `store(h_mem)` — no update
/// or delete call exists in this function.
pub(crate) async fn distill_store(
    memory: &hkask_memory::MemoryStore,
    inference_port: &dyn hkask_types::InferencePort,
    webid: WebID,
    now: chrono::DateTime<chrono::Utc>,
    idle_secs: u64,
    since: chrono::DateTime<chrono::Utc>,
    revisit: &[String],
    model: Option<&str>,
) -> DistillationOutcome {
    let mut outcome = DistillationOutcome::default();
    // Turn discovery is the shared contract (`thread_turns`): the scan runs
    // over the shared-copy prefix, which ingest writes for EVERY turn —
    // curator and non-curator alike — so no turn is invisible to the pass.
    let mut by_thread = match crate::thread_turns::shared_turns_by_thread_since(memory, since) {
        Ok(by_thread) => by_thread,
        Err(error) => {
            tracing::warn!(
                target: "hkask.mcp.curator.distillation",
                %error,
                "Failed to query recent thread turns — distillation pass skipped"
            );
            outcome.scan_failed = true;
            return outcome;
        }
    };
    // Pending threads from earlier passes: their turns fall behind the
    // cursor, so reload each thread's complete turn set (a superset of
    // whatever the window scan returned for it).
    for thread_id in revisit {
        match crate::thread_turns::thread_turns(memory, thread_id) {
            Ok(turns) => {
                by_thread.insert(thread_id.clone(), turns);
            }
            Err(error) => {
                tracing::warn!(
                    target: "hkask.mcp.curator.distillation",
                    thread_id = %thread_id,
                    %error,
                    "Failed to re-read pending thread's turns — retried next pass"
                );
                outcome.threads_pending.insert(thread_id.clone(), now);
            }
        }
    }
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
            outcome.threads_pending.insert(thread_id, now);
            continue;
        }
        let watermark_entity = format!("{WATERMARK_PREFIX}{thread_id}");
        // PREFIX read, not exact-match: safe only because production thread
        // ids are UUIDs (no UUID is a prefix of another). A future non-UUID
        // thread-id source would read a sibling thread's watermark here —
        // switch to an exact-match read before introducing one (the T09
        // prefix-collision observation, recorded 2026-09-07).
        let through = match memory.h_mems_by_entity_prefix(&watermark_entity) {
            Ok(watermarks) => watermarks.iter().filter_map(parse_watermark_through).max(),
            Err(error) => {
                tracing::warn!(
                    target: "hkask.mcp.curator.distillation",
                    thread_id = %thread_id,
                    %error,
                    "Failed to read distillation watermark — thread retried next pass"
                );
                outcome.threads_pending.insert(thread_id, now);
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
        // Batch oldest-first so every pending turn is shown to the model
        // exactly once before the watermark covering it advances. The
        // single-prompt form showed only the last MAX_TURNS_PER_PROMPT
        // turns while the watermark covered ALL of them — early turns of
        // long threads were marked extracted without ever being seen,
        // then forgotten as covered (22 threads affected at the
        // 2026-09-09 audit). A batch failure stops the thread there:
        // earlier batches are already covered by their own watermarks,
        // and the failed batch's turns stay pending for the next pass.
        let mut thread_distilled = true;
        for batch in pending.chunks(MAX_TURNS_PER_PROMPT) {
            let prompt = build_distillation_prompt(&thread_id, batch);
            // Route to the non-thinking model: the port default is
            // reasoning-mandatory and rejects `thinking_allowed: false`.
            // `generate_with_model` falls back to the port default when
            // `model` is None (unset `HKASK_CLASSIFIER_MODEL`).
            let generated = match inference_port
                .generate_with_model(&prompt, &LLMParameters::default(), model, None)
                .await
            {
                Ok(result) => result,
                Err(error) => {
                    tracing::warn!(
                        target: "hkask.mcp.curator.distillation",
                        thread_id = %thread_id,
                        %error,
                        "Distillation inference failed — batch retried next pass (watermark not advanced)"
                    );
                    outcome.extraction_failures += 1;
                    outcome.threads_pending.insert(thread_id.clone(), now);
                    thread_distilled = false;
                    break;
                }
            };
            let candidates = match parse_lessons(&generated.text) {
                Ok(candidates) => candidates,
                Err(error) => {
                    tracing::warn!(
                        target: "hkask.mcp.curator.distillation",
                        thread_id = %thread_id,
                        %error,
                        "Distillation output unparseable — batch retried next pass (watermark not advanced)"
                    );
                    outcome.extraction_failures += 1;
                    outcome.threads_pending.insert(thread_id.clone(), now);
                    thread_distilled = false;
                    break;
                }
            };
            // Advance the watermark BEFORE inserting lessons: a failure
            // after lessons are stored would re-distill the same turns
            // next pass and duplicate them — the exact redundancy this
            // pass exists to end. A failure before lessons loses them
            // once, loudly, with the raw turns still in memory for
            // therapy.
            let through_newest = batch
                .last()
                .expect("chunks yields non-empty slices")
                .observed_at;
            let watermark = HMem::new(
                &watermark_entity,
                "distilled_through",
                serde_json::json!({
                    "through": through_newest.to_rfc3339(),
                    "turns": batch.len(),
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
                    "Failed to store distillation watermark — batch retried next pass"
                );
                outcome.threads_pending.insert(thread_id.clone(), now);
                thread_distilled = false;
                break;
            }
            for candidate in candidates.into_iter().take(MAX_LESSONS_PER_THREAD) {
                match insert_lesson(memory, inference_port, &candidate, &thread_id, webid).await {
                    Ok(true) => outcome.lessons_inserted += 1,
                    Ok(false) => outcome.lessons_skipped += 1,
                    Err(error) => {
                        tracing::warn!(
                            target: "hkask.mcp.curator.distillation",
                            thread_id = %thread_id,
                            %error,
                            "Failed to store distilled lesson"
                        );
                        outcome.lessons_skipped += 1;
                    }
                }
            }
        }
        if thread_distilled {
            outcome.threads_distilled += 1;
        }
    }
    if outcome.threads_distilled > 0 {
        RegulationSpan::Curation.emit("memory_distilled");
    }
    if outcome.extraction_failures > 0 {
        RegulationSpan::Curation.emit("memory_distillation_stalled");
    }
    outcome
}

/// Parse a watermark's `through` position — the `observed_at` of the newest
/// turn the pass distilled. Shared with the forgetting pass, which uses it
/// as the coverage boundary: a turn is covered by the watermark (its lessons
/// proven extracted) exactly when its `observed_at` ≤ `through` — the same
/// predicate this pass uses to select pending turns (strictly newer).
pub(crate) fn parse_watermark_through(h_mem: &HMem) -> Option<chrono::DateTime<chrono::Utc>> {
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
        // Chunk rows are plain strings — the cleaned passage with
        // `user:` / `assistant:` role prefixes inline. The value IS the
        // content; there is no envelope to parse (the pre-2026-09-04
        // JSON-envelope rows were retired with the single-copy ruling —
        // no backward compatibility requirement, operator ruling
        // 2026-09-04).
        turns_json.push(serde_json::json!({
            "h_mem_id": turn.id.to_string(),
            "text": truncate_chars(turn.value.as_str().unwrap_or(""), MAX_TURN_CHARS),
        }));
    }
    format!(
        "You are distilling a finished conversation thread into durable lessons \
         for a long-lived memory system.\n\n\
         Thread: {thread_id}\n\
         Passages (oldest first):\n{turns}\n\n\
         Extract 0-{MAX_LESSONS_PER_THREAD} durable, generalizable lessons — \
         stable facts, preferences, decisions, and corrections a future session \
         should know. Not task narration, not transient details. Each lesson must \
         cite at least one h_mem_id from the passages above as evidence.\n\n\
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
        // The real chunk shape: a plain string with role prefixes inline.
        let h_mem = HMem::new(
            &format!("{SHARED_TURN_PREFIX}{thread_id}"),
            "chunk:0",
            serde_json::json!(format!("user: {user_input}\n\nassistant: {agent_response}")),
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
    /// `generate` call after `failures` transient errors; `embed` uses the
    /// trait default (unavailable), which the insert path treats as
    /// non-fatal.
    struct ScriptedDistillPort {
        response: String,
        failures: std::sync::atomic::AtomicUsize,
    }

    impl hkask_types::InferencePort for ScriptedDistillPort {
        fn generate(
            &self,
            _prompt: &str,
            _parameters: &LLMParameters,
            _tools: Option<&[hkask_types::ChatToolDefinition]>,
        ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>>
        {
            // Decide (and count) the failure synchronously so the returned
            // future captures no reference to self.
            if self
                .failures
                .fetch_update(
                    std::sync::atomic::Ordering::SeqCst,
                    std::sync::atomic::Ordering::SeqCst,
                    |remaining| remaining.checked_sub(1),
                )
                .is_ok()
            {
                let error = InferenceError::Timeout("transient test failure".to_string());
                return Box::pin(async move { Err(error) });
            }
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

    /// Records the model override each `generate_with_model` call
    /// receives, then answers with `response` — pins the distillation
    /// pass's model routing (the port default is reasoning-mandatory and
    /// rejects the pass's non-thinking parameters, the five-day total
    /// failure the 2026-09-09 repair ends).
    struct ModelRecordingPort {
        response: String,
        seen_model: std::sync::Mutex<Vec<Option<String>>>,
    }

    impl hkask_types::InferencePort for ModelRecordingPort {
        fn generate(
            &self,
            _prompt: &str,
            _parameters: &LLMParameters,
            _tools: Option<&[hkask_types::ChatToolDefinition]>,
        ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>>
        {
            // If distillation regresses to the model-less `generate`, the
            // routing pin below fails with this visible error.
            Box::pin(async move {
                Err(InferenceError::Timeout(
                    "distillation must call generate_with_model, not generate".to_string(),
                ))
            })
        }

        fn generate_with_model(
            &self,
            _prompt: &str,
            _parameters: &LLMParameters,
            model_override: Option<&str>,
            _tools: Option<&[hkask_types::ChatToolDefinition]>,
        ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>>
        {
            self.seen_model
                .lock()
                .expect("model recorder mutex")
                .push(model_override.map(str::to_string));
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

    /// The 2026-09-09 wiring repair: distillation routes its extraction
    /// call to the configured non-thinking model, never the port default.
    #[tokio::test]
    async fn distill_store_routes_extraction_to_the_configured_model() {
        let store = test_store();
        let webid = WebID::from_persona(b"curator");
        let now = chrono::Utc::now();
        let turn_id = turn_h_mem(
            &store,
            "t-model",
            "please proceed",
            "done — all green",
            now - chrono::Duration::seconds(600),
            webid,
        );
        let port = ModelRecordingPort {
            response: lesson_response(
                "probe-entity",
                "probe-attribute",
                "probe lesson",
                &[&turn_id.to_string()],
            ),
            seen_model: std::sync::Mutex::new(Vec::new()),
        };
        let outcome = distill_store(
            &store,
            &port,
            webid,
            now,
            DEFAULT_DISTILLATION_IDLE_SECS,
            now - chrono::Duration::seconds(3600),
            &[],
            Some("OpenRouter/z-ai/glm-5.2"),
        )
        .await;
        assert_eq!(outcome.lessons_inserted, 1);
        assert_eq!(
            port.seen_model.lock().expect("recorder mutex").as_slice(),
            &[Some("OpenRouter/z-ai/glm-5.2".to_string())],
            "the extraction call must carry the configured model override"
        );
    }

    /// A generate failure is counted as an extraction failure so the
    /// stalled-pass regulation span has a real signal, and the thread
    /// stays pending for retry (watermark not advanced).
    #[tokio::test]
    async fn extraction_failures_are_counted_and_the_thread_stays_pending() {
        let store = test_store();
        let webid = WebID::from_persona(b"curator");
        let now = chrono::Utc::now();
        turn_h_mem(
            &store,
            "t-fail",
            "please proceed",
            "done — all green",
            now - chrono::Duration::seconds(600),
            webid,
        );
        let port = ScriptedDistillPort {
            failures: std::sync::atomic::AtomicUsize::new(1),
            response: "[]".to_string(),
        };
        let outcome = distill_store(
            &store,
            &port,
            webid,
            now,
            DEFAULT_DISTILLATION_IDLE_SECS,
            now - chrono::Duration::seconds(3600),
            &[],
            None,
        )
        .await;
        assert_eq!(outcome.extraction_failures, 1);
        assert_eq!(outcome.threads_distilled, 0);
        assert!(
            outcome.threads_pending.contains_key("t-fail"),
            "the failed thread must stay pending for the next pass"
        );
    }

    /// Fails on the Nth `generate` call (1-based), succeeds otherwise —
    /// pins the batch loop's partial-failure semantics: a mid-thread
    /// batch failure must leave earlier batches covered and later turns
    /// pending.
    struct CallCountingPort {
        response: String,
        fail_on_call: usize,
        calls: std::sync::atomic::AtomicUsize,
    }

    impl hkask_types::InferencePort for CallCountingPort {
        fn generate(
            &self,
            _prompt: &str,
            _parameters: &LLMParameters,
            _tools: Option<&[hkask_types::ChatToolDefinition]>,
        ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>>
        {
            let call = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            if call == self.fail_on_call {
                return Box::pin(async move {
                    Err(InferenceError::Timeout(
                        "scripted batch failure".to_string(),
                    ))
                });
            }
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

    /// Store `count` turns for one thread, one per second, and return
    /// their ids oldest-first.
    fn seed_turns(
        store: &hkask_memory::MemoryStore,
        thread_id: &str,
        count: usize,
        webid: WebID,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Vec<hkask_storage::HMemId> {
        (0..count)
            .map(|offset| {
                turn_h_mem(
                    store,
                    thread_id,
                    &format!("user: turn {offset}"),
                    &format!("assistant: response {offset}"),
                    now - chrono::Duration::seconds(600) + chrono::Duration::seconds(offset as i64),
                    webid,
                )
            })
            .collect()
    }

    /// The 2026-09-09 coverage repair: a thread with more pending turns
    /// than one prompt holds is distilled in oldest-first batches, each
    /// batch advancing its own watermark — every turn is shown to the
    /// model exactly once before the watermark covering it advances
    /// (the single-prompt form marked early turns extracted without ever
    /// showing them, then the forgetting pass deleted them as covered).
    #[tokio::test]
    async fn long_threads_distill_in_batches_with_per_batch_watermarks() {
        let store = test_store();
        let webid = WebID::from_persona(b"curator");
        let now = chrono::Utc::now();
        let turns = seed_turns(&store, "t-batch", 15, webid, now);
        let port = ScriptedDistillPort {
            failures: std::sync::atomic::AtomicUsize::new(0),
            response: lesson_response(
                "batch-entity",
                "batch-attribute",
                "batch lesson",
                &[&turns[14].to_string()],
            ),
        };
        let outcome = distill_store(
            &store,
            &port,
            webid,
            now,
            DEFAULT_DISTILLATION_IDLE_SECS,
            now - chrono::Duration::seconds(3600),
            &[],
            None,
        )
        .await;
        assert_eq!(outcome.threads_examined, 1);
        assert_eq!(outcome.threads_distilled, 1);
        assert_eq!(
            outcome.lessons_inserted, 2,
            "both batches (12 turns + 3 turns) must extract"
        );
        assert_eq!(outcome.extraction_failures, 0);
        let watermarks = store
            .h_mems_by_entity_prefix("curator:distilled:t-batch")
            .expect("watermarks");
        assert_eq!(watermarks.len(), 2, "each batch advances its own watermark");
        let throughs: Vec<chrono::DateTime<chrono::Utc>> = watermarks
            .iter()
            .filter_map(parse_watermark_through)
            .collect();
        let turn12 = store
            .h_mems_by_entity_prefix("curator:thread:t-batch")
            .expect("turns")
            .into_iter()
            .find(|h| {
                h.observed_at
                    == now - chrono::Duration::seconds(600) + chrono::Duration::seconds(11)
            })
            .expect("turn 12");
        assert!(
            throughs.contains(&turn12.observed_at),
            "the first batch's watermark must cover exactly its 12 turns"
        );
        let turn15 = store
            .h_mems_by_entity_prefix("curator:thread:t-batch")
            .expect("turns")
            .into_iter()
            .find(|h| {
                h.observed_at
                    == now - chrono::Duration::seconds(600) + chrono::Duration::seconds(14)
            })
            .expect("turn 15");
        assert!(
            throughs.contains(&turn15.observed_at),
            "the second batch's watermark must cover the thread's newest turn"
        );
    }

    /// A mid-thread batch failure leaves earlier batches covered (their
    /// watermarks stand) and the failed batch's turns pending — the next
    /// pass re-covers exactly the remainder, not the whole thread.
    #[tokio::test]
    async fn batch_failure_keeps_earlier_batches_covered_and_remaining_pending() {
        let store = test_store();
        let webid = WebID::from_persona(b"curator");
        let now = chrono::Utc::now();
        let turns = seed_turns(&store, "t-partial", 15, webid, now);
        let failing = CallCountingPort {
            response: lesson_response(
                "partial-entity",
                "partial-attribute",
                "partial lesson",
                &[&turns[0].to_string()],
            ),
            fail_on_call: 2,
            calls: std::sync::atomic::AtomicUsize::new(0),
        };
        let outcome = distill_store(
            &store,
            &failing,
            webid,
            now,
            DEFAULT_DISTILLATION_IDLE_SECS,
            now - chrono::Duration::seconds(3600),
            &[],
            None,
        )
        .await;
        assert_eq!(outcome.extraction_failures, 1);
        assert_eq!(
            outcome.threads_distilled, 0,
            "a partial thread is not distilled"
        );
        assert!(outcome.threads_pending.contains_key("t-partial"));
        let watermarks = store
            .h_mems_by_entity_prefix("curator:distilled:t-partial")
            .expect("watermarks");
        assert_eq!(
            watermarks.len(),
            1,
            "only the succeeded batch advanced a watermark"
        );
        let covered = parse_watermark_through(&watermarks[0]).expect("through");
        let turn12 = store
            .h_mems_by_entity_prefix("curator:thread:t-partial")
            .expect("turns")
            .into_iter()
            .find(|h| {
                h.observed_at
                    == now - chrono::Duration::seconds(600) + chrono::Duration::seconds(11)
            })
            .expect("turn 12");
        assert_eq!(covered, turn12.observed_at);

        // Second pass with a working port: only the 3 remaining turns are
        // pending — one batch, covering the thread's newest turn.
        let working = CallCountingPort {
            response: lesson_response(
                "partial-entity",
                "partial-attribute",
                "partial lesson two",
                &[&turns[14].to_string()],
            ),
            fail_on_call: 0,
            calls: std::sync::atomic::AtomicUsize::new(0),
        };
        let second = distill_store(
            &store,
            &working,
            webid,
            now,
            DEFAULT_DISTILLATION_IDLE_SECS,
            now - chrono::Duration::seconds(3600),
            &[],
            None,
        )
        .await;
        assert_eq!(second.threads_distilled, 1);
        assert_eq!(second.lessons_inserted, 1);
        assert_eq!(
            working.calls.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "the retry must process only the 3 remaining turns, not the whole thread"
        );
        let turn15 = store
            .h_mems_by_entity_prefix("curator:thread:t-partial")
            .expect("turns")
            .into_iter()
            .find(|h| {
                h.observed_at
                    == now - chrono::Duration::seconds(600) + chrono::Duration::seconds(14)
            })
            .expect("turn 15");
        let watermarks_after = store
            .h_mems_by_entity_prefix("curator:distilled:t-partial")
            .expect("watermarks");
        assert!(
            watermarks_after
                .iter()
                .filter_map(parse_watermark_through)
                .any(|through| through == turn15.observed_at),
            "the retry's watermark must cover the thread's newest turn"
        );
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
            failures: std::sync::atomic::AtomicUsize::new(0),
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
            &[],
            None,
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
            failures: std::sync::atomic::AtomicUsize::new(0),
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
            &[],
            None,
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
            failures: std::sync::atomic::AtomicUsize::new(0),
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
            &[],
            None,
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
            &[],
            None,
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
            failures: std::sync::atomic::AtomicUsize::new(0),
            response: "[]".to_string(),
        };
        let outcome = distill_store(
            &store,
            &port,
            webid,
            now,
            DEFAULT_DISTILLATION_IDLE_SECS,
            now - chrono::Duration::seconds(3600),
            &[],
            None,
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
            failures: std::sync::atomic::AtomicUsize::new(0),
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
            &[],
            None,
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

    /// The distillation prompt presents each chunk's plain-string value as
    /// its text — role prefixes inline, no envelope.
    #[test]
    fn distillation_prompt_reads_chunk_text_as_content() {
        let webid = hkask_types::WebID::new();
        let h_mem = HMem::new(
            "curator:thread:t1",
            "chunk:0",
            serde_json::json!("user: please review the design"),
            webid,
        );
        let prompt = build_distillation_prompt("t1", &[&h_mem]);
        assert!(
            prompt.contains("user: please review the design"),
            "the chunk's text must appear in the prompt verbatim"
        );
    }

    // ---- T04: pending-work revisit through the production cursor ----

    fn curator_db_with_memory(
        store: hkask_memory::MemoryStore,
    ) -> (Arc<CuratorDb>, Arc<hkask_memory::MemoryStore>) {
        let store = Arc::new(store);
        let db = Arc::new(CuratorDb::from_stores(crate::CuratorStores {
            escalation_queue: None,
            regulation_store: None,
            memory: Some(Arc::clone(&store)),
        }));
        (db, store)
    }

    /// T04: a thread skipped as active at one pass is distilled by a later
    /// pass once idle — no new turn, no restart. Pre-fix, the cursor
    /// advanced past the thread's turns and no later pass ever saw them.
    #[tokio::test]
    async fn later_pass_distills_thread_that_went_idle_without_a_new_turn() {
        let (db, store) = curator_db_with_memory(test_store());
        let webid = WebID::from_persona(b"curator");
        let turn_id = turn_h_mem(
            &store,
            "t1",
            "active conversation",
            "mid-flight response",
            chrono::Utc::now() - chrono::Duration::seconds(10),
            webid,
        );
        let port = ScriptedDistillPort {
            response: lesson_response(
                "subject",
                "lesson",
                "Distilled once idle.",
                &[&turn_id.to_string()],
            ),
            failures: std::sync::atomic::AtomicUsize::new(0),
        };
        let mut cursor = DistillationCursor::new();
        // Pass 1: the thread is active (newest turn 10s old) — skipped,
        // carried as pending.
        let pass1_now = chrono::Utc::now() + chrono::Duration::seconds(1);
        let first = run_pass(
            &db,
            &port,
            webid,
            &mut cursor,
            pass1_now,
            DEFAULT_DISTILLATION_IDLE_SECS,
            None,
        )
        .await;
        assert_eq!(first.threads_examined, 1);
        assert_eq!(first.threads_distilled, 0);
        assert_eq!(first.threads_pending.len(), 1);
        // Pass 2 (no new turn, no restart): the thread is idle — distilled.
        let pass2_now = pass1_now + chrono::Duration::seconds(400);
        let second = run_pass(
            &db,
            &port,
            webid,
            &mut cursor,
            pass2_now,
            DEFAULT_DISTILLATION_IDLE_SECS,
            None,
        )
        .await;
        assert_eq!(
            second.threads_distilled, 1,
            "an idle thread must be distilled by a later pass"
        );
        assert_eq!(second.lessons_inserted, 1);
        assert_eq!(second.threads_pending.len(), 0);
        assert_eq!(
            store
                .h_mems_by_entity_prefix("subject")
                .expect("query lessons")
                .len(),
            1
        );
        assert_eq!(
            store
                .h_mems_by_entity_prefix("curator:distilled:t1")
                .expect("query watermarks")
                .len(),
            1
        );
    }

    /// T04: a transient inference failure before the watermark advances is
    /// retried on the next pass; the retry succeeds and distills once.
    #[tokio::test]
    async fn transient_inference_failure_is_retried_on_the_next_pass() {
        let (db, store) = curator_db_with_memory(test_store());
        let webid = WebID::from_persona(b"curator");
        let turn_id = turn_h_mem(
            &store,
            "t1",
            "question",
            "answer",
            chrono::Utc::now() - chrono::Duration::seconds(600),
            webid,
        );
        let port = ScriptedDistillPort {
            response: lesson_response(
                "subject",
                "lesson",
                "Learned on retry.",
                &[&turn_id.to_string()],
            ),
            failures: std::sync::atomic::AtomicUsize::new(1),
        };
        let mut cursor = DistillationCursor::new();
        let pass1_now = chrono::Utc::now() + chrono::Duration::seconds(1);
        let first = run_pass(
            &db,
            &port,
            webid,
            &mut cursor,
            pass1_now,
            DEFAULT_DISTILLATION_IDLE_SECS,
            None,
        )
        .await;
        assert_eq!(first.threads_distilled, 0);
        assert!(first.threads_pending.contains_key("t1"));
        assert!(
            store
                .h_mems_by_entity_prefix("curator:distilled:t1")
                .expect("query watermarks")
                .is_empty()
        );
        // Next pass: inference succeeds — the thread is retried and distilled.
        let pass2_now = pass1_now + chrono::Duration::seconds(60);
        let second = run_pass(
            &db,
            &port,
            webid,
            &mut cursor,
            pass2_now,
            DEFAULT_DISTILLATION_IDLE_SECS,
            None,
        )
        .await;
        assert_eq!(second.threads_distilled, 1, "failed thread must be retried");
        assert_eq!(second.lessons_inserted, 1);
        assert_eq!(
            store
                .h_mems_by_entity_prefix("subject")
                .expect("query lessons")
                .len(),
            1
        );
    }

    /// T04 control: a distilled thread is not re-examined once the cursor
    /// has moved past it (no replay), and a genuinely new turn is picked up
    /// by the scan window (real cursor progression).
    #[tokio::test]
    async fn cursor_progresses_and_successful_work_is_not_replayed() {
        let (db, store) = curator_db_with_memory(test_store());
        let webid = WebID::from_persona(b"curator");
        let t1_turn = turn_h_mem(
            &store,
            "t1",
            "first thread",
            "response",
            chrono::Utc::now() - chrono::Duration::seconds(600),
            webid,
        );
        let port = ScriptedDistillPort {
            response: lesson_response(
                "subject",
                "lesson",
                "Once per thread.",
                &[&t1_turn.to_string()],
            ),
            failures: std::sync::atomic::AtomicUsize::new(0),
        };
        let mut cursor = DistillationCursor::new();
        let pass1_now = chrono::Utc::now();
        let first = run_pass(
            &db,
            &port,
            webid,
            &mut cursor,
            pass1_now,
            DEFAULT_DISTILLATION_IDLE_SECS,
            None,
        )
        .await;
        assert_eq!(first.threads_distilled, 1);
        // A new turn is OBSERVED after pass 1 — ahead of the cursor.
        turn_h_mem(
            &store,
            "t2",
            "second thread",
            "response",
            pass1_now + chrono::Duration::seconds(60),
            webid,
        );
        let pass2_now = pass1_now + chrono::Duration::seconds(400);
        let second = run_pass(
            &db,
            &port,
            webid,
            &mut cursor,
            pass2_now,
            DEFAULT_DISTILLATION_IDLE_SECS,
            None,
        )
        .await;
        // Only the new thread is examined: t1 is behind the cursor and not
        // pending — successful work is not replayed.
        assert_eq!(second.threads_examined, 1);
        assert_eq!(second.threads_distilled, 1);
        // One lesson per thread and one watermark per thread — no duplicates.
        assert_eq!(
            store
                .h_mems_by_entity_prefix("subject")
                .expect("query lessons")
                .len(),
            2
        );
        assert_eq!(
            store
                .h_mems_by_entity_prefix("curator:distilled:t1")
                .expect("query t1 watermarks")
                .len(),
            1
        );
        assert_eq!(
            store
                .h_mems_by_entity_prefix("curator:distilled:t2")
                .expect("query t2 watermarks")
                .len(),
            1
        );
    }

    /// T04: a pass that cannot read the store must not advance the cursor —
    /// turns stored before the outage stay visible to the healed pass.
    #[tokio::test]
    async fn scan_failure_does_not_advance_the_cursor() {
        let (db, store) = curator_db_with_memory(test_store());
        let webid = WebID::from_persona(b"curator");
        let turn_id = turn_h_mem(
            &store,
            "t1",
            "before outage",
            "response",
            chrono::Utc::now() - chrono::Duration::seconds(600),
            webid,
        );
        let port = ScriptedDistillPort {
            response: lesson_response(
                "subject",
                "lesson",
                "Seen after healing.",
                &[&turn_id.to_string()],
            ),
            failures: std::sync::atomic::AtomicUsize::new(0),
        };
        // A DB whose memory store is unavailable (the outage).
        let down_db = Arc::new(CuratorDb::from_stores(crate::CuratorStores::empty()));
        let mut cursor = DistillationCursor::new();
        let pass1_now = chrono::Utc::now() + chrono::Duration::seconds(1);
        let first = run_pass(
            &down_db,
            &port,
            webid,
            &mut cursor,
            pass1_now,
            DEFAULT_DISTILLATION_IDLE_SECS,
            None,
        )
        .await;
        assert!(first.scan_failed);
        // The healed pass must still see the pre-outage turn: the cursor
        // did not advance past it.
        let pass2_now = pass1_now + chrono::Duration::seconds(60);
        let second = run_pass(
            &db,
            &port,
            webid,
            &mut cursor,
            pass2_now,
            DEFAULT_DISTILLATION_IDLE_SECS,
            None,
        )
        .await;
        assert_eq!(
            second.threads_distilled, 1,
            "pre-outage turn must stay visible after the store heals"
        );
        assert_eq!(second.lessons_inserted, 1);
    }

    /// T04: the pending set is bounded — overflow evicts the longest-pending
    /// cohort with a warn, and the newest pending thread survives (it is
    /// still distilled once idle).
    #[tokio::test]
    async fn pending_threads_are_bounded_with_explicit_eviction() {
        let (db, store) = curator_db_with_memory(test_store());
        let webid = WebID::from_persona(b"curator");
        let port = ScriptedDistillPort {
            response: "[]".to_string(),
            failures: std::sync::atomic::AtomicUsize::new(0),
        };
        let mut cursor = DistillationCursor::new();
        // Fill the pending set: MAX_PENDING_THREADS active threads.
        let pass1_now = chrono::Utc::now();
        // Zero-padded ids: the watermark and turn reads are entity-PREFIX
        // queries, so unpadded t1 would collide with t10..t127.
        for index in 0..MAX_PENDING_THREADS {
            turn_h_mem(
                &store,
                &format!("t{index:03}"),
                "active",
                "response",
                chrono::Utc::now() - chrono::Duration::seconds(10),
                webid,
            );
        }
        let first = run_pass(
            &db,
            &port,
            webid,
            &mut cursor,
            pass1_now,
            DEFAULT_DISTILLATION_IDLE_SECS,
            None,
        )
        .await;
        assert_eq!(first.threads_pending.len(), MAX_PENDING_THREADS);
        assert_eq!(cursor.pending.len(), MAX_PENDING_THREADS);
        // One more active thread, observed after pass 1, overflows the set.
        turn_h_mem(
            &store,
            "t-new",
            "active",
            "response",
            pass1_now + chrono::Duration::seconds(30),
            webid,
        );
        let pass2_now = pass1_now + chrono::Duration::seconds(60);
        let second = run_pass(
            &db,
            &port,
            webid,
            &mut cursor,
            pass2_now,
            DEFAULT_DISTILLATION_IDLE_SECS,
            None,
        )
        .await;
        // All 129 examined threads are still active: the pass reports one
        // more pending thread than the bound before the cursor evicts.
        assert_eq!(second.threads_pending.len(), MAX_PENDING_THREADS + 1);
        assert_eq!(
            cursor.pending.len(),
            MAX_PENDING_THREADS,
            "pending set must stay bounded"
        );
        assert!(
            cursor.pending.contains_key("t-new"),
            "the newest pending thread survives eviction"
        );
        // The survivors (including t-new) are distilled once idle — the
        // eviction dropped exactly one thread of the oldest cohort.
        let pass3_now = pass2_now + chrono::Duration::seconds(400);
        let third = run_pass(
            &db,
            &port,
            webid,
            &mut cursor,
            pass3_now,
            DEFAULT_DISTILLATION_IDLE_SECS,
            None,
        )
        .await;
        assert_eq!(third.threads_distilled, MAX_PENDING_THREADS);
        assert_eq!(third.threads_pending.len(), 0);
        assert!(
            !store
                .h_mems_by_entity_prefix("curator:distilled:t-new")
                .expect("query t-new watermarks")
                .is_empty(),
            "the newest pending thread must be distilled once idle"
        );
    }

    /// T04 control, post-2026-09-09 repair: the first pass scans from the
    /// oldest stored turn (durable recovery) — never a bounded window —
    /// so a restart cannot permanently miss undistilled threads. Covered
    /// threads are skipped by the per-thread watermark check, and the
    /// forgetting pass bounds the turn store to the forgetting horizon.
    #[test]
    fn first_pass_scan_recovers_from_durable_state() {
        let now = chrono::Utc::now();
        let cursor = DistillationCursor::new();
        assert_eq!(
            cursor.scan_since(now),
            chrono::DateTime::from_timestamp(0, 0).expect("epoch is valid"),
            "first pass must scan from the beginning — the watermark check skips covered threads"
        );
        let mut advanced = cursor;
        advanced.merge(
            &DistillationOutcome {
                threads_examined: 1,
                ..DistillationOutcome::default()
            },
            now,
        );
        assert_eq!(
            advanced.scan_since(now),
            now,
            "subsequent passes scan from the previous pass time"
        );
    }

    /// T17's lesson applied to distillation: the spawned timer actually
    /// fires — first tick skipped, first pass one interval in — and
    /// distills an idle thread end to end.
    #[tokio::test(start_paused = true)]
    async fn distillation_timer_fires_after_the_first_interval_and_distills() {
        let (db, store) = curator_db_with_memory(test_store());
        let webid = WebID::from_persona(b"curator");
        let turn_id = turn_h_mem(
            &store,
            "t1",
            "timer question",
            "timer answer",
            chrono::Utc::now() - chrono::Duration::seconds(600),
            webid,
        );
        let port = Arc::new(ScriptedDistillPort {
            response: lesson_response(
                "subject",
                "lesson",
                "Distilled by the timer.",
                &[&turn_id.to_string()],
            ),
            failures: std::sync::atomic::AtomicUsize::new(0),
        });
        spawn_distillation_timer(
            Arc::clone(&db),
            port,
            webid,
            DistillationConfig {
                cadence_secs: 60,
                idle_secs: 0,
                model: None,
            },
        );
        // Before the first interval: nothing.
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        assert!(
            store
                .h_mems_by_entity_prefix("subject")
                .expect("query lessons")
                .is_empty()
        );
        // One interval in: the pass fires and distills.
        tokio::time::sleep(std::time::Duration::from_secs(31)).await;
        for _ in 0..100 {
            if !store
                .h_mems_by_entity_prefix("subject")
                .expect("query lessons")
                .is_empty()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(
            store
                .h_mems_by_entity_prefix("subject")
                .expect("query lessons")
                .len(),
            1
        );
        assert_eq!(
            store
                .h_mems_by_entity_prefix("curator:distilled:t1")
                .expect("query watermarks")
                .len(),
            1
        );
    }

    /// T04/T17: a cadence longer than one hour is honored — the obsolete
    /// 3600s poll clamp silently shortened it.
    #[tokio::test(start_paused = true)]
    async fn distillation_timer_honors_cadences_longer_than_one_hour() {
        let (db, store) = curator_db_with_memory(test_store());
        let webid = WebID::from_persona(b"curator");
        let turn_id = turn_h_mem(
            &store,
            "t1",
            "cadence question",
            "cadence answer",
            chrono::Utc::now() - chrono::Duration::seconds(600),
            webid,
        );
        let port = Arc::new(ScriptedDistillPort {
            response: lesson_response(
                "subject",
                "lesson",
                "Distilled on cadence.",
                &[&turn_id.to_string()],
            ),
            failures: std::sync::atomic::AtomicUsize::new(0),
        });
        spawn_distillation_timer(
            Arc::clone(&db),
            port,
            webid,
            DistillationConfig {
                cadence_secs: 7200,
                idle_secs: 0,
                model: None,
            },
        );
        // One hour in: the old clamp would have fired a pass here.
        tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        assert!(
            store
                .h_mems_by_entity_prefix("subject")
                .expect("query lessons")
                .is_empty(),
            "a 7200s cadence must not fire a pass at 3600s"
        );
        // At the configured cadence: the pass fires.
        tokio::time::sleep(std::time::Duration::from_secs(3601)).await;
        for _ in 0..100 {
            if !store
                .h_mems_by_entity_prefix("subject")
                .expect("query lessons")
                .is_empty()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(
            store
                .h_mems_by_entity_prefix("subject")
                .expect("query lessons")
                .len(),
            1
        );
    }
}
