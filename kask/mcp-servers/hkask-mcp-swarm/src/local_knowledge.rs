//! Local swarm knowledge tools — the kask-vernacular analogs of ABW's
//! `swarm_search_knowledge`, `swarm_generate_prompt`, and `swarm_generate_ontology`.
//!
//! Where ABW backs these with fermi's per-agent dreaming-memory KG + fermi's
//! LLM generation, the local analogs back them with the operator's own
//! `hkask-memory` `MemoryStore` (the knowledge graph — entity-attribute-value
//! triples, scoped per agent by an `agent:<agent_id>:` prefix) and the local
//! `InferencePort` (Ollama/cloud via the zed IPC bridge). No ABW round-trips.
//!
//! Design rationale: `kask/docs/plans/local-swarm-knowledge-tools.md`.
//!
//! Graceful degradation: `LazyLocalMemory::get_or_init` opens the
//! `MemoryStore` lazily. The SQLCipher passphrase defaults to `"allostery"`
//! (pre-release kask-wide default) so the tools work out of the box; override
//! via `HKASK_SWARM_MEMORY_PASSPHRASE`. If open fails (e.g., an existing DB was
//! created under a different passphrase), the search tool returns an empty
//! result with a `memory_unconfigured` note (never a panic, never a fabricated
//! hit — the `.rules` unwrap_or(0) trap), and the generate tools proceed
//! unseeded (memory is an enhancement, not a dependency).

use hkask_memory::MemoryStore;
use hkask_storage::HMem;
use hkask_types::{HMemOntology, Visibility, WebID};
use std::sync::Arc;

use crate::error::LocalSwarmError;

/// The per-agent memory prefix. A local agent's "knowledge graph" is its
/// prefix-scoped slice of the operator's semantic memory.
pub(crate) const AGENT_PREFIX: &str = "agent:";

/// A lazily-opened `MemoryStore` for the local swarm knowledge tools.
///
/// Mirrors `LazyLocalSwarmRuntime`: the `run_server` factory is sync, so the
/// async `MemoryStore::open` is deferred to the first tool call. The store
/// is the operator's consolidated semantic memory; per-agent scoping is a
/// prefix (`agent:<agent_id>:`) on the shared store (one store, many
/// namespaces — the deep-module choice over a per-agent store).
pub(crate) struct LazyLocalMemory {
    db_path: String,
    passphrase: String,
    dim: usize,
    inner: tokio::sync::OnceCell<MemoryStore>,
}

impl LazyLocalMemory {
    /// Store the config without initializing. The memory is constructed on the
    /// first `get_or_init` call.
    pub(crate) fn lazy(db_path: String, passphrase: String, dim: usize) -> Self {
        Self {
            db_path,
            passphrase,
            dim,
            inner: tokio::sync::OnceCell::new(),
        }
    }

    /// Get the semantic memory, initializing it on the first call. Returns
    /// `Err` if the passphrase is unset/too short or the store fails to open —
    /// callers degrade gracefully (the `.rules` startup-failure-signal rule: a
    /// missing memory is signaled, not silently empty).
    pub(crate) async fn get_or_init(&self) -> Result<&MemoryStore, LocalSwarmError> {
        self.inner
            .get_or_try_init(|| async {
                if self.passphrase.len() < 8 {
                    return Err(LocalSwarmError::InvalidInput(format!(
                        "swarm memory passphrase too short ({} chars — need >=8; set \
                         HKASK_SWARM_MEMORY_PASSPHRASE). Local knowledge tools will degrade.",
                        self.passphrase.len()
                    )));
                }
                // Create the parent directory so a first-run open does not fail
                // on a missing data dir.
                if let Some(parent) = std::path::Path::new(&self.db_path).parent() {
                    if !parent.as_os_str().is_empty() {
                        std::fs::create_dir_all(parent).map_err(|e| {
                            LocalSwarmError::Io(format!(
                                "failed to create swarm memory dir {}: {e}",
                                parent.display()
                            ))
                        })?;
                    }
                }
                MemoryStore::open(&self.db_path, &self.passphrase, self.dim).map_err(|e| {
                    LocalSwarmError::Database(format!("failed to open swarm memory store: {e}"))
                })
            })
            .await
    }
}

/// A knowledge fragment returned by `swarm_search_knowledge_local`. Mirrors
/// the ABW envelope (matching knowledge fragments) but in kask terms: the
/// agent's semantic-memory triples that match the query.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct KnowledgeFragment {
    pub entity: String,
    pub attribute: String,
    pub value: String,
    pub confidence: f64,
}

/// Grounding annotation recorded alongside a delegation's latency and
/// task-success verdict. Closes the paper's §4.1 loop: "is this getting
/// better?" — without recording grounding violations per delegation, the
/// trend is invisible and the check gets quietly disabled.
///
/// `None` (the whole annotation) means grounding did not run for this
/// delegation (no contract for the agent_type — paper Rule 5.3: absence ≠
/// verdict). `Some` with `had_contract: true` means grounding ran; the count
/// fields are `Option<usize>` per the `.rules` no-`unwrap_or(0)` rule — a
/// failed count extraction is `None` (not measured), never `0` (measured
/// zero). When `had_contract: false`, the counts are `None` (not measured).
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub(crate) struct GroundingAnnotation {
    /// Whether a grounding contract existed for this delegation's agent_type.
    pub had_contract: bool,
    /// Count of fields nulled as Unsourced. `None` = not measured (contract
    /// ran but the count could not be extracted — never silently 0).
    pub nulled_fields_count: Option<usize>,
    /// Count of narrative leaks detected. `None` = not measured.
    pub narrative_leaks_count: Option<usize>,
}

/// A grounding trend report for an agent (or the whole swarm when
/// `agent_id` is empty). Answers the paper's §4.1 question: "is this
/// getting better?" The lead metric is `delegations_with_zero_nulled` —
/// deletion-resistant (paper Rule 5.4: a scoreboard that rewards deletion
/// counts falling, so a team that stops recording looks like it's
/// improving). Counting delegations with zero nulled fields cannot be
/// gamed by recording fewer delegations.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub(crate) struct GroundingTrend {
    /// Total delegations recorded for the scope (regardless of grounding
    /// status). The denominator for every rate below.
    pub total_delegations: usize,
    /// Delegations for which a grounding contract existed and ran.
    pub delegations_with_contract: usize,
    /// Delegations for which no grounding contract existed (coverage gap —
    /// paper §6: coverage is itself a metric, not a pass).
    pub delegations_without_contract: usize,
    /// Delegations where grounding ran and zero fields were nulled. The
    /// deletion-resistant scoreboard metric (paper Rule 5.4).
    pub delegations_with_zero_nulled: usize,
    /// Delegations where grounding ran and at least one field was nulled.
    pub delegations_with_nulled: usize,
    /// Delegations where grounding ran and at least one narrative leak was
    /// detected.
    pub delegations_with_narrative_leaks: usize,
}

impl GroundingTrend {
    /// Fraction of grounded delegations (contract ran) with zero nulled
    /// fields. `None` when no grounded delegations exist (absence ≠ 0 —
    /// paper Rule 5.3).
    pub fn clean_rate(&self) -> Option<f64> {
        let grounded = self.delegations_with_zero_nulled + self.delegations_with_nulled;
        if grounded == 0 {
            return None;
        }
        Some(self.delegations_with_zero_nulled as f64 / grounded as f64)
    }

    /// Fraction of delegations that had a grounding contract. `None` when
    /// no delegations exist.
    pub fn coverage_rate(&self) -> Option<f64> {
        if self.total_delegations == 0 {
            return None;
        }
        Some(self.delegations_with_contract as f64 / self.total_delegations as f64)
    }
}

/// Search an agent's prefix-scoped semantic memory for triples whose
/// entity/attribute/value contain the query (case-insensitive substring).
///
/// This is the EAV (graph) retrieval path — "memory as a graph". It does not
/// require an embedding model, so it works whenever the memory store is
/// configured (passphrase set), independent of the embedding backend. Returns
/// an empty vec (not an error) when the agent has no matching memory.
pub(crate) async fn search_agent_knowledge(
    memory: &LazyLocalMemory,
    agent_id: &str,
    query: &str,
    limit: usize,
) -> Result<Vec<KnowledgeFragment>, LocalSwarmError> {
    let store = match memory.get_or_init().await {
        Ok(s) => s,
        Err(reason) => {
            tracing::warn!(target: "hkask.mcp.swarm", error = %reason, "swarm memory unavailable — search returns empty");
            return Err(reason);
        }
    };
    let entity = format!("{AGENT_PREFIX}{agent_id}");
    let triples = store
        .query_deduped(&entity)
        .map_err(|e| LocalSwarmError::Database(format!("semantic memory query failed: {e}")))?;
    let needle = query.to_lowercase();
    let mut fragments: Vec<KnowledgeFragment> = triples
        .into_iter()
        .filter(|t| {
            if needle.is_empty() {
                return true;
            }
            t.entity.to_lowercase().contains(&needle)
                || t.attribute.to_lowercase().contains(&needle)
                || t.value.to_string().to_lowercase().contains(&needle)
        })
        .map(|t| KnowledgeFragment {
            entity: t.entity,
            attribute: t.attribute,
            value: t.value.to_string(),
            confidence: t.confidence.value(),
        })
        .collect();
    fragments.truncate(limit.max(1));
    Ok(fragments)
}

/// Retrieve an agent's seed memory as a prompt-context string (for the
/// generate tools). Returns an empty string when memory is unconfigured or the
/// agent has no memory — the generate tools then proceed unseeded.
pub(crate) async fn agent_memory_seed(
    memory: &LazyLocalMemory,
    agent_id: &str,
    limit: usize,
) -> String {
    match search_agent_knowledge(memory, agent_id, "", limit).await {
        Ok(fragments) if !fragments.is_empty() => {
            let lines: Vec<String> = fragments
                .into_iter()
                .map(|f| format!("- ({}, {}): {}", f.entity, f.attribute, f.value))
                .collect();
            format!(
                "Known facts about agent '{}' from consolidated memory:\n{}",
                agent_id,
                lines.join("\n")
            )
        }
        _ => String::new(),
    }
}

/// Record a delegation performance annotation to the agent's prefix-scoped
/// semantic memory — the ACO stigmergic pheromone trail. After each
/// `swarm_delegate_local`, the latency and task-success verdict are written as
/// `HMem` triples under `agent:<agent_id>:delegation`. The SENSE phase (or any
/// caller) can then query these via `swarm_search_knowledge_local` to assess
/// agent fitness across cascade invocations.
///
/// Failures are logged with `tracing::warn!`, not swallowed (the `.rules` trap
/// on silent error discarding — a failed stigmergy write must be visible in
/// logs, not silently dropped). The delegation result is still returned to the
/// caller regardless of whether the annotation was written.
pub(crate) async fn record_delegation(
    memory: &LazyLocalMemory,
    agent_id: &str,
    latency_ms: u64,
    task_success_pass: Option<bool>,
    grounding: Option<GroundingAnnotation>,
) {
    let store = match memory.get_or_init().await {
        Ok(s) => s,
        Err(reason) => {
            tracing::warn!(
                target: "hkask.mcp.swarm",
                error = %reason,
                "stigmergy write skipped — swarm memory unavailable (non-fatal)"
            );
            return;
        }
    };
    let owner = WebID::for_agent_name("swarm_delegate_local");
    let entity = format!("{AGENT_PREFIX}{agent_id}");

    // Process-axis anchoring (P5.4): a stigmergy annotation is a PKO step
    // execution of the delegation procedure, not a standalone fact. Anchoring
    // it this way is what lets the SENSE phase distinguish pheromone trails
    // (process traces) from consolidated agent facts in the same store.
    let ontology = HMemOntology::episodic("swarm_delegate", "record", agent_id);

    // Write the latency annotation.
    let mut h_mem = HMem::new(
        &entity,
        "delegation:latency_ms",
        serde_json::json!(latency_ms),
        owner,
    )
    .with_ontology(ontology.clone());
    h_mem.access.visibility = Visibility::Shared;
    if let Err(e) = store.store(h_mem) {
        tracing::warn!(
            target: "hkask.mcp.swarm",
            error = %e,
            "stigmergy latency write failed (non-fatal)"
        );
    }

    // Write the task-success annotation only when a verdict was supplied
    // (null task_success = open task, no oracle — do not fabricate).
    if let Some(pass) = task_success_pass {
        let mut h_mem = HMem::new(
            &entity,
            "delegation:task_success",
            serde_json::json!(pass),
            owner,
        )
        .with_ontology(ontology.clone());
        h_mem.access.visibility = Visibility::Shared;
        if let Err(e) = store.store(h_mem) {
            tracing::warn!(
                target: "hkask.mcp.swarm",
                error = %e,
                "stigmergy task_success write failed (non-fatal)"
            );
        }
    }

    // Write the grounding annotation when grounding ran (paper §4.1: the
    // trend ledger). `None` = grounding did not run (no contract) — we still
    // record `had_contract: false` so the coverage gap is visible (paper §6:
    // coverage is itself a metric, not a pass). Counts are `Option<usize>`
    // per the `.rules` no-`unwrap_or(0)` rule — a failed extraction is `None`,
    // never a silent 0.
    let annotation = grounding.unwrap_or_default();
    let mut had_contract_h_mem = HMem::new(
        &entity,
        "delegation:grounding_had_contract",
        serde_json::json!(annotation.had_contract),
        owner,
    )
    .with_ontology(ontology.clone());
    had_contract_h_mem.access.visibility = Visibility::Shared;
    if let Err(e) = store.store(had_contract_h_mem) {
        tracing::warn!(
            target: "hkask.mcp.swarm",
            error = %e,
            "stigmergy grounding_had_contract write failed (non-fatal)"
        );
    }
    if let Some(count) = annotation.nulled_fields_count {
        let mut h_mem = HMem::new(
            &entity,
            "delegation:grounding_nulled",
            serde_json::json!(count),
            owner,
        )
        .with_ontology(ontology.clone());
        h_mem.access.visibility = Visibility::Shared;
        if let Err(e) = store.store(h_mem) {
            tracing::warn!(
                target: "hkask.mcp.swarm",
                error = %e,
                "stigmergy grounding_nulled write failed (non-fatal)"
            );
        }
    }
    if let Some(count) = annotation.narrative_leaks_count {
        let mut h_mem = HMem::new(
            &entity,
            "delegation:grounding_leaks",
            serde_json::json!(count),
            owner,
        )
        .with_ontology(ontology);
        h_mem.access.visibility = Visibility::Shared;
        if let Err(e) = store.store(h_mem) {
            tracing::warn!(
                target: "hkask.mcp.swarm",
                error = %e,
                "stigmergy grounding_leaks write failed (non-fatal)"
            );
        }
    }
}

/// Query the grounding trend for an agent (or the whole swarm when
/// `agent_id` is empty). Reads back the per-delegation grounding
/// annotations written by `record_delegation` and aggregates them into
/// the paper's §4.1 trend report.
///
/// The lead metric is `delegations_with_zero_nulled` — deletion-resistant
/// (paper Rule 5.4: a scoreboard that counts nulled fields falling can be
/// gamed by recording fewer delegations; counting delegations with zero
/// nulled fields cannot).
///
/// Returns `Err` when the memory store is unavailable (the `.rules`
/// broken-feedback-loop trap: a DB outage must not collapse to an empty
/// trend, which would read as "no deviation"). The caller surfaces the
/// error; it does not silently return a zeroed `GroundingTrend`.
///
/// `limit` caps the number of recent delegations scanned (each delegation
/// writes up to 4 h_mems: latency, task_success, had_contract, nulled,
/// leaks — the scan deduplicates by `observed_at` proximity). Defaults to
/// 100 when `0` is passed.
pub(crate) async fn grounding_trend(
    memory: &LazyLocalMemory,
    agent_id: &str,
    limit: usize,
) -> Result<GroundingTrend, LocalSwarmError> {
    let store = memory.get_or_init().await?;
    // Query the `delegation:grounding_had_contract` attribute across the
    // scoped entity (single agent) or the whole store (empty agent_id).
    // Each h_mem at this attribute is one delegation's grounding status.
    let had_contract_entries = if agent_id.is_empty() {
        store
            .query_by_attribute("delegation:grounding_had_contract")
            .map_err(|e| LocalSwarmError::Database(format!("grounding trend query failed: {e}")))?
    } else {
        let entity = format!("{AGENT_PREFIX}{agent_id}");
        store
            .query_deduped(&entity)
            .map_err(|e| LocalSwarmError::Database(format!("grounding trend query failed: {e}")))?
            .into_iter()
            .filter(|t| t.attribute == "delegation:grounding_had_contract")
            .collect()
    };
    // Each had_contract h_mem corresponds to one delegation. The count of
    // these is the total delegations with a grounding status recorded.
    let total = had_contract_entries.len();
    let mut trend = GroundingTrend {
        total_delegations: total,
        ..Default::default()
    };
    // For each delegation, classify it into the trend buckets. We re-query
    // the nulled/leaks counts by entity+attribute to pair them with the
    // had_contract flag. The pairing is by `observed_at` timestamp — the
    // had_contract, nulled, and leaks h_mems for one delegation share the
    // same `observed_at` (written in the same `record_delegation` call).
    //
    // We build a per-entity index of (timestamp → counts) so we can pair
    // the three annotations per delegation without assuming global ordering.
    let nulled_entries = if agent_id.is_empty() {
        store
            .query_by_attribute("delegation:grounding_nulled")
            .map_err(|e| LocalSwarmError::Database(format!("grounding trend query failed: {e}")))?
    } else {
        store
            .query_deduped(&format!("{AGENT_PREFIX}{agent_id}"))
            .map_err(|e| LocalSwarmError::Database(format!("grounding trend query failed: {e}")))?
            .into_iter()
            .filter(|t| t.attribute == "delegation:grounding_nulled")
            .collect()
    };
    let leaks_entries = if agent_id.is_empty() {
        store
            .query_by_attribute("delegation:grounding_leaks")
            .map_err(|e| LocalSwarmError::Database(format!("grounding trend query failed: {e}")))?
    } else {
        store
            .query_deduped(&format!("{AGENT_PREFIX}{agent_id}"))
            .map_err(|e| LocalSwarmError::Database(format!("grounding trend query failed: {e}")))?
            .into_iter()
            .filter(|t| t.attribute == "delegation:grounding_leaks")
            .collect()
    };
    // Index nulled/leaks by (entity, observed_at) for pairing.
    use std::collections::HashMap;
    let mut nulled_by_key: HashMap<(String, chrono::DateTime<chrono::Utc>), usize> = HashMap::new();
    for t in &nulled_entries {
        if let Some(count) = t.value.as_u64().map(|c| c as usize) {
            nulled_by_key.insert((t.entity.clone(), t.observed_at), count);
        }
    }
    let mut leaks_by_key: HashMap<(String, chrono::DateTime<chrono::Utc>), usize> = HashMap::new();
    for t in &leaks_entries {
        if let Some(count) = t.value.as_u64().map(|c| c as usize) {
            leaks_by_key.insert((t.entity.clone(), t.observed_at), count);
        }
    }
    for t in &had_contract_entries {
        let had_contract = t.value.as_bool().unwrap_or(false);
        let key = (t.entity.clone(), t.observed_at);
        if had_contract {
            trend.delegations_with_contract += 1;
            // Pair with nulled/leaks by timestamp. If the pairing is missing
            // (count h_mem write failed), the delegation is counted as
            // "grounded but counts not measured" — it does NOT collapse to
            // zero-nulled (paper Rule 5.3: absence ≠ verdict).
            let nulled = nulled_by_key.get(&key).copied();
            let leaks = leaks_by_key.get(&key).copied();
            match nulled {
                Some(0) => trend.delegations_with_zero_nulled += 1,
                Some(_) => trend.delegations_with_nulled += 1,
                None => {} // counts not measured — neither bucket
            }
            if matches!(leaks, Some(c) if c > 0) {
                trend.delegations_with_narrative_leaks += 1;
            }
        } else {
            trend.delegations_without_contract += 1;
        }
    }
    // The `limit` parameter caps how many delegations we report on. We've
    // already scanned all; truncate the report's totals to the most recent
    // `limit` by slicing the had_contract entries (sorted by observed_at
    // descending). This keeps the report bounded for long-running swarms.
    let cap = if limit == 0 { 100 } else { limit };
    if total > cap {
        // Recompute over the most recent `cap` delegations. Sort by
        // observed_at descending, take the first `cap`, re-aggregate.
        let mut sorted: Vec<_> = had_contract_entries
            .iter()
            .map(|t| {
                (
                    t.entity.clone(),
                    t.observed_at,
                    t.value.as_bool().unwrap_or(false),
                )
            })
            .collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));
        sorted.truncate(cap);
        trend = GroundingTrend::default();
        trend.total_delegations = sorted.len();
        for (entity, observed_at, had_contract) in &sorted {
            let key = (entity.clone(), *observed_at);
            if *had_contract {
                trend.delegations_with_contract += 1;
                let nulled = nulled_by_key.get(&key).copied();
                let leaks = leaks_by_key.get(&key).copied();
                match nulled {
                    Some(0) => trend.delegations_with_zero_nulled += 1,
                    Some(_) => trend.delegations_with_nulled += 1,
                    None => {}
                }
                if matches!(leaks, Some(c) if c > 0) {
                    trend.delegations_with_narrative_leaks += 1;
                }
            } else {
                trend.delegations_without_contract += 1;
            }
        }
    }
    Ok(trend)
}

/// A one-shot LLM generate over the local inference port.
///
/// `inference` is the resolved local `InferencePort` (from `LocalSwarmRuntime`).
/// Returns the generated text.
pub(crate) async fn one_shot_generate(
    inference: &Arc<dyn hkask_types::InferencePort>,
    prompt: &str,
    temperature: f32,
) -> Result<String, LocalSwarmError> {
    let params = hkask_types::template::LLMParameters {
        temperature,
        ..hkask_types::template::LLMParameters::default()
    };
    let result = inference
        .generate(prompt, &params, None)
        .await
        .map_err(|e| {
            LocalSwarmError::Unavailable(format!("local inference generate failed: {e}"))
        })?;
    Ok(result.text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_prefix_is_stable() {
        assert_eq!(AGENT_PREFIX, "agent:");
        assert_eq!(format!("{AGENT_PREFIX}researcher"), "agent:researcher");
    }

    #[test]
    fn lazy_memory_stores_config_without_init() {
        let m = LazyLocalMemory::lazy("/tmp/never.db".to_string(), "short".to_string(), 1024);
        // Construction must not touch the filesystem; the OnceCell is unset.
        assert_eq!(m.dim, 1024);
    }

    #[tokio::test]
    async fn record_delegation_degrades_gracefully_when_memory_unavailable() {
        // A passphrase shorter than 8 chars causes get_or_init to fail.
        // record_delegation must log + return without panicking (the .rules
        // trap on silent error discarding — the failure is visible in logs,
        // not swallowed, and the delegation result is not lost).
        let m = LazyLocalMemory::lazy(
            "/tmp/hkask-swarm-stigmergy-degradation-test.db".to_string(),
            "short".to_string(), // < 8 chars → get_or_init returns Err
            1024,
        );
        // This must not panic.
        record_delegation(&m, "research_agent", 4200, Some(true)).await;
        // If we reach here, the graceful degradation path works.
    }

    #[tokio::test]
    async fn record_delegation_writes_and_reads_back_stigmergy_trail() {
        // Use a temp DB with a valid passphrase. Write a delegation
        // annotation, then read it back via search_agent_knowledge to verify
        // the stigmergic pheromone trail round-trips through semantic memory.
        let dir = std::env::temp_dir().join(format!(
            "hkask-swarm-stigmergy-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("stigmergy.db");
        let m = LazyLocalMemory::lazy(
            db_path.to_string_lossy().to_string(),
            "test-passphrase-123".to_string(), // >= 8 chars
            1024,
        );

        // Write a delegation annotation.
        record_delegation(&m, "research_agent", 4200, Some(true)).await;

        // Read it back — the entity prefix is "agent:research_agent:delegation".
        let fragments = search_agent_knowledge(&m, "research_agent", "delegation", 10)
            .await
            .expect("search should succeed with a valid DB");

        // The latency_ms annotation should be present.
        let has_latency = fragments
            .iter()
            .any(|f| f.attribute == "delegation:latency_ms" && f.value == "4200");
        assert!(
            has_latency,
            "stigmergy trail must contain the latency_ms annotation; got: {fragments:?}"
        );

        // The task_success annotation should also be present.
        let has_task_success = fragments
            .iter()
            .any(|f| f.attribute == "delegation:task_success" && f.value == "true");
        assert!(
            has_task_success,
            "stigmergy trail must contain the task_success annotation; got: {fragments:?}"
        );

        // Cleanup.
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn record_delegation_skips_task_success_when_none() {
        // When task_success_pass is None (open task, no oracle), only the
        // latency annotation is written — the task_success annotation is NOT
        // fabricated.
        let dir = std::env::temp_dir().join(format!(
            "hkask-swarm-stigmergy-skip-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("stigmergy_skip.db");
        let m = LazyLocalMemory::lazy(
            db_path.to_string_lossy().to_string(),
            "test-passphrase-123".to_string(),
            1024,
        );

        // Write with no task_success (None).
        record_delegation(&m, "creative_agent", 1500, None).await;

        let fragments = search_agent_knowledge(&m, "creative_agent", "delegation", 10)
            .await
            .expect("search should succeed");

        // latency_ms should be present.
        let has_latency = fragments
            .iter()
            .any(|f| f.attribute == "delegation:latency_ms" && f.value == "1500");
        assert!(
            has_latency,
            "latency_ms must be written even without task_success"
        );

        // task_success should NOT be present (not fabricated).
        let has_task_success = fragments
            .iter()
            .any(|f| f.attribute == "delegation:task_success");
        assert!(
            !has_task_success,
            "task_success must NOT be written when None (never fabricate)"
        );

        std::fs::remove_dir_all(&dir).ok();
    }
}
