//! Local swarm knowledge tools — the kask-vernacular analogs of ABW's
//! `swarm_search_knowledge`, `swarm_generate_prompt`, and `swarm_generate_ontology`.
//!
//! Where ABW backs these with fermi's per-agent dreaming-memory KG + fermi's
//! LLM generation, the local analogs back them with the operator's own
//! `hkask-memory` `SemanticMemory` (the knowledge graph — entity-attribute-value
//! triples, scoped per agent by an `agent:<agent_id>:` prefix) and the local
//! `InferencePort` (Ollama/cloud via the zed IPC bridge). No ABW round-trips.
//!
//! Design rationale: `kask/docs/plans/local-swarm-knowledge-tools.md`.
//!
//! Graceful degradation: `LazyLocalMemory::get_or_init` opens the
//! `SemanticMemory` lazily. The SQLCipher passphrase defaults to `"allostery"`
//! (pre-release kask-wide default) so the tools work out of the box; override
//! via `HKASK_SWARM_MEMORY_PASSPHRASE`. If open fails (e.g., an existing DB was
//! created under a different passphrase), the search tool returns an empty
//! result with a `memory_unconfigured` note (never a panic, never a fabricated
//! hit — the `.rules` unwrap_or(0) trap), and the generate tools proceed
//! unseeded (memory is an enhancement, not a dependency).

use hkask_memory::SemanticMemory;
use std::sync::Arc;

/// The per-agent memory prefix. A local agent's "knowledge graph" is its
/// prefix-scoped slice of the operator's semantic memory.
pub(crate) const AGENT_PREFIX: &str = "agent:";

/// A lazily-opened `SemanticMemory` for the local swarm knowledge tools.
///
/// Mirrors `LazyLocalSwarmRuntime`: the `run_server` factory is sync, so the
/// async `SemanticMemory::open` is deferred to the first tool call. The store
/// is the operator's consolidated semantic memory; per-agent scoping is a
/// prefix (`agent:<agent_id>:`) on the shared store (one store, many
/// namespaces — the deep-module choice over a per-agent store).
pub(crate) struct LazyLocalMemory {
    db_path: String,
    passphrase: String,
    dim: usize,
    inner: tokio::sync::OnceCell<SemanticMemory>,
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
    pub(crate) async fn get_or_init(&self) -> Result<&SemanticMemory, String> {
        self.inner
            .get_or_try_init(|| async {
                if self.passphrase.len() < 8 {
                    return Err(format!(
                        "swarm memory passphrase too short ({} chars — need >=8; set \
                         HKASK_SWARM_MEMORY_PASSPHRASE). Local knowledge tools will degrade.",
                        self.passphrase.len()
                    ));
                }
                // Create the parent directory so a first-run open does not fail
                // on a missing data dir.
                if let Some(parent) = std::path::Path::new(&self.db_path).parent() {
                    if !parent.as_os_str().is_empty() {
                        std::fs::create_dir_all(parent).map_err(|e| {
                            format!(
                                "failed to create swarm memory dir {}: {e}",
                                parent.display()
                            )
                        })?;
                    }
                }
                SemanticMemory::open(&self.db_path, &self.passphrase, self.dim)
                    .map_err(|e| format!("failed to open swarm semantic memory: {e}"))
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
) -> Result<Vec<KnowledgeFragment>, String> {
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
        .map_err(|e| format!("semantic memory query failed: {e}"))?;
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

/// A one-shot LLM generate over the local inference port, with the output
/// guard-scanned (generated prompts/ontologies are LLM output and must not
/// exfiltrate canaries/secrets — the `.rules` GuardedStream caveat does NOT
/// apply; this is a synchronous scan before the result leaves the tool).
///
/// `inference` is the resolved local `InferencePort` (from `LocalSwarmRuntime`);
/// `guard` scans the output. Returns the scanned text.
pub(crate) async fn one_shot_generate(
    inference: &Arc<dyn hkask_types::InferencePort>,
    guard: &Arc<hkask_guard::ContentGuard>,
    prompt: &str,
    temperature: f32,
) -> Result<String, String> {
    let params = hkask_types::template::LLMParameters {
        temperature,
        ..hkask_types::template::LLMParameters::default()
    };
    let result = inference
        .generate(prompt, &params, None)
        .await
        .map_err(|e| format!("local inference generate failed: {e}"))?;
    // Scan the generated output for canary exfiltration / secret leakage.
    // A canary hit is a hard failure (system-prompt exfiltration); a secret
    // hit is logged and the text returned (the generated prompt/ontology may
    // be legitimately useful despite a false-positive secret match — mirrors
    // `AgentExecutor::scan_output`'s asymmetric policy).
    if guard.check_canary(&result.text) {
        return Err(
            "canary token detected in generated output — system prompt exfiltration suspected"
                .to_string(),
        );
    }
    let scan = guard.scan_output(&result.text);
    if !scan.passed {
        tracing::warn!(
            target: "hkask.mcp.swarm",
            "generated output tripped a secret scanner — returned as-is (sanitize-on-read at the consumer)"
        );
    }
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
}
