//! `BridgeCascadeContextProvider` — implements `CascadeContextProvider` by
//! delegating to `RealMemoryPort`'s recall methods, applying the participant
//! matrix to select which memory stores to recall from.
//!
//! The participant matrix (determined by `agent_id` + `swarm_id`):
//!
//! | agent_id          | swarm_id | Recall sources          |
//! |-------------------|----------|-------------------------|
//! | ZED_AGENT_ID      | absent   | User store              |
//! | CURATOR_AGENT_ID  | absent   | Curator + User stores   |
//! | ZED_AGENT_ID      | present  | Swarm store             |
//! | CURATOR_AGENT_ID  | present  | Curator + Swarm stores  |
//!
//! Joint recall merges chunks from all sources into a single ranked list,
//! filtered by a saliency floor and truncated to a max-chunks cap. The
//! saliency query is the concatenation of `task` + the recent N turns — the
//! "chat context" that memory chunks should be salient to.
//!
//! Memory is an autonomous feature of processed experiences — not consent-
//! gated. When participants are present in a thread, their memory stores are
//! read by default, mirroring the chat path's `ContextInjector`.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use hkask_types::ports::cascade_context::{
    CascadeContext, CascadeContextError, CascadeContextProvider, CascadeContextRequest,
};
use hkask_types::ports::memory_port::{MemoryPort, MemorySnippet};

use crate::memory::RealMemoryPort;

/// The bridge's implementation of `CascadeContextProvider`.
///
/// Holds an `Arc<RealMemoryPort>` (the same handle the context injectors use)
/// and applies the participant matrix to select recall sources. The
/// short-term messages in the request are passed through unchanged — the
/// provider only adds long-term memory.
pub struct BridgeCascadeContextProvider {
    memory_port: Arc<RealMemoryPort>,
}

impl BridgeCascadeContextProvider {
    pub fn new(memory_port: Arc<RealMemoryPort>) -> Self {
        Self { memory_port }
    }
}

/// The agent ID strings used by the participant matrix. These mirror
/// `agent::ZED_AGENT_ID` and `agent::CURATOR_AGENT_ID` but are string
/// literals to avoid a dependency on the `agent` crate (which would be
/// circular: `agent` depends on `kask_bridge` for the manifest executor).
/// The values are stable and pinned by tests in `agent.rs`.
#[allow(dead_code)]
const ZED_AGENT_ID: &str = "Zed Agent";
const CURATOR_AGENT_ID: &str = "Curator";

impl CascadeContextProvider for BridgeCascadeContextProvider {
    fn gather_context<'a>(
        &'a self,
        request: &'a CascadeContextRequest,
    ) -> Pin<Box<dyn Future<Output = Result<CascadeContext, CascadeContextError>> + Send + 'a>>
    {
        let memory_port = self.memory_port.clone();
        let task = request.task.clone();
        let agent_id = request.agent_id.clone();
        let swarm_id = request.swarm_id.clone();
        let saliency_floor = request.saliency_floor;
        let max_chunks = request.max_chunks as usize;
        let short_term_messages = request.short_term_messages.clone();

        Box::pin(async move {
            // Build the saliency query: task + recent turns concatenated.
            // This is the "chat context" that memory chunks should be
            // salient to. The concatenation approach (Option A) is imprecise
            // but matches the chat path's behavior and avoids per-turn
            // recall cost.
            let query = build_saliency_query(&task, &short_term_messages);

            // Determine which stores to recall from via the participant matrix.
            let (recall_user, recall_curator, recall_swarm) =
                participant_matrix(agent_id.as_deref(), swarm_id.as_deref());

            // Recall from each selected store. Each call returns Ok(vec![])
            // when the store is unavailable (graceful degradation).
            let mut all_snippets: Vec<MemorySnippet> = Vec::new();

            if recall_user {
                match memory_port.recall_context(&query, max_chunks).await {
                    Ok(s) => all_snippets.extend(s),
                    Err(e) => {
                        tracing::warn!(
                            target: "reg.memory",
                            error = %e,
                            "User store recall failed during cascade context gathering"
                        );
                    }
                }
            }

            if recall_curator {
                match memory_port.recall_context_curator(&query, max_chunks).await {
                    Ok(s) => all_snippets.extend(s),
                    Err(e) => {
                        tracing::warn!(
                            target: "reg.memory",
                            error = %e,
                            "Curator store recall failed during cascade context gathering"
                        );
                    }
                }
            }

            if recall_swarm {
                match memory_port.recall_context_swarm(&query, max_chunks).await {
                    Ok(s) => all_snippets.extend(s),
                    Err(e) => {
                        tracing::warn!(
                            target: "reg.memory",
                            error = %e,
                            "Swarm store recall failed during cascade context gathering"
                        );
                    }
                }
            }

            // Merge, dedupe, rank, filter, truncate.
            let long_term_snippets =
                rank_and_filter_snippets(all_snippets, saliency_floor, max_chunks);

            tracing::info!(
                target: "reg.memory",
                injected_count = long_term_snippets.len(),
                recall_user,
                recall_curator,
                recall_swarm,
                "Cascade context gathered — {} memory chunks injected",
                long_term_snippets.len()
            );

            Ok(CascadeContext {
                short_term_messages,
                long_term_snippets,
            })
        })
    }
}

/// Adapter that implements the `agent` crate's `CascadeContextProvider`
/// trait by delegating to `BridgeCascadeContextProvider` (which implements
/// `hkask_types::CascadeContextProvider`).
///
/// The `agent` crate cannot depend on `hkask_types` (circular dependency),
/// so it defines its own local `CascadeContextProvider` trait with local
/// mirror types (`CascadeChatMessage`, `MemorySnippetRecord`). This adapter
/// converts between the two type spaces at the seam.
pub struct AgentCascadeContextProviderAdapter {
    inner: BridgeCascadeContextProvider,
}

impl AgentCascadeContextProviderAdapter {
    pub fn new(memory_port: std::sync::Arc<crate::memory::RealMemoryPort>) -> Self {
        Self {
            inner: BridgeCascadeContextProvider::new(memory_port),
        }
    }
}

impl agent::CascadeContextProvider for AgentCascadeContextProviderAdapter {
    fn gather_context<'a>(
        &'a self,
        request: &'a agent::CascadeContextRequest,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<agent::CascadeContext, String>> + Send + 'a>,
    > {
        let inner = &self.inner;
        Box::pin(async move {
            // Convert the agent crate's local types to hkask_types types.
            let hkask_request = CascadeContextRequest {
                thread_id: request.thread_id.clone(),
                task: request.task.clone(),
                agent_id: request.agent_id.clone(),
                swarm_id: request.swarm_id.clone(),
                short_term_messages: request
                    .short_term_messages
                    .iter()
                    .filter_map(|m| {
                        // Extract text content from the LanguageModelRequestMessage.
                        let content: String = m
                            .content
                            .iter()
                            .filter_map(|c| match c {
                                language_model::MessageContent::Text(text) => Some(text.as_str()),
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        if content.is_empty() {
                            None
                        } else {
                            Some(hkask_types::ports::inference_types::ChatMessage {
                                role: m.role.to_string(),
                                content,
                            })
                        }
                    })
                    .collect(),
                saliency_floor: request.saliency_floor,
                max_chunks: request.max_chunks,
            };

            let result = inner
                .gather_context(&hkask_request)
                .await
                .map_err(|e| e.to_string())?;

            // Convert the hkask_types result back to the agent crate's local types.
            Ok(agent::CascadeContext {
                short_term_messages: Vec::new(), // Not used by the agent crate
                long_term_snippets: result
                    .long_term_snippets
                    .into_iter()
                    .map(|s| agent::MemorySnippetRecord {
                        text: s.text,
                        source: s.source,
                        confidence: s.confidence,
                        relevance_score: s.relevance_score,
                    })
                    .collect(),
            })
        })
    }
}

/// Determine which memory stores to recall from based on the thread's
/// participants.
///
/// The matrix:
/// - User agent, no swarm → user only
/// - Curator agent, no swarm → curator + user (the user is driving the curator)
/// - User agent + swarm → swarm only
/// - Curator agent + swarm → curator + swarm
/// - Unknown agent (None) → user only (graceful default)
fn participant_matrix(agent_id: Option<&str>, swarm_id: Option<&str>) -> (bool, bool, bool) {
    let is_curator = agent_id == Some(CURATOR_AGENT_ID);
    let has_swarm = swarm_id.is_some();

    match (is_curator, has_swarm) {
        (false, false) => (true, false, false), // User agent, no swarm
        (true, false) => (true, true, false),   // Curator + user
        (false, true) => (false, false, true),  // Swarm only
        (true, true) => (false, true, true),    // Curator + swarm
    }
}

/// Build the saliency query by concatenating the task and recent turns.
///
/// The query is `task + "\n" + turn1_content + "\n" + turn2_content + ...`.
/// Only user and assistant turns are included (system and tool messages are
/// not part of the conversational context that memory should be salient to).
fn build_saliency_query(task: &str, messages: &[hkask_types::ChatMessage]) -> String {
    let mut parts = vec![task.to_string()];
    for msg in messages {
        if msg.role == "user" || msg.role == "assistant" {
            parts.push(msg.content.clone());
        }
    }
    parts.join("\n")
}

/// Merge, dedupe, rank, filter, and truncate memory snippets.
///
/// - Dedupe by text similarity (exact match on first 200 chars — a coarse
///   dedupe that catches the same fact appearing in both episodic and
///   semantic stores).
/// - Rank by `relevance_score * confidence` (descending).
/// - Filter: keep only snippets where `relevance_score * confidence >= floor`.
/// - Truncate to `max_chunks`.
fn rank_and_filter_snippets(
    mut snippets: Vec<MemorySnippet>,
    saliency_floor: f64,
    max_chunks: usize,
) -> Vec<MemorySnippet> {
    // Dedupe by first-200-chars prefix. The same fact may appear in both
    // episodic and semantic stores; keeping both wastes context budget.
    let mut seen_prefixes: std::collections::HashSet<String> = std::collections::HashSet::new();
    snippets.retain(|s| {
        let prefix = s.text.chars().take(200).collect::<String>();
        seen_prefixes.insert(prefix)
    });

    // Rank by saliency = relevance_score * confidence (descending).
    snippets.sort_by(|a, b| {
        let sa = a.relevance_score * a.confidence;
        let sb = b.relevance_score * b.confidence;
        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });

    // Filter by saliency floor.
    snippets.retain(|s| (s.relevance_score * s.confidence) >= saliency_floor);

    // Truncate to max_chunks.
    snippets.truncate(max_chunks);
    snippets
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snippet(text: &str, relevance: f64, confidence: f64) -> MemorySnippet {
        MemorySnippet {
            text: text.to_string(),
            source: "test".to_string(),
            confidence,
            relevance_score: relevance,
        }
    }

    #[test]
    fn participant_matrix_user_only() {
        let (u, c, s) = participant_matrix(Some(ZED_AGENT_ID), None);
        assert!((u, c, s) == (true, false, false));
    }

    #[test]
    fn participant_matrix_curator_and_user() {
        let (u, c, s) = participant_matrix(Some(CURATOR_AGENT_ID), None);
        assert!((u, c, s) == (true, true, false));
    }

    #[test]
    fn participant_matrix_swarm_only() {
        let (u, c, s) = participant_matrix(Some(ZED_AGENT_ID), Some("ws-1"));
        assert!((u, c, s) == (false, false, true));
    }

    #[test]
    fn participant_matrix_curator_and_swarm() {
        let (u, c, s) = participant_matrix(Some(CURATOR_AGENT_ID), Some("ws-1"));
        assert!((u, c, s) == (false, true, true));
    }

    #[test]
    fn participant_matrix_unknown_agent_defaults_to_user() {
        let (u, c, s) = participant_matrix(None, None);
        assert!((u, c, s) == (true, false, false));
    }

    #[test]
    fn rank_and_filter_dedupes_by_prefix() {
        let snippets = vec![
            snippet(
                "This is a long fact that appears in both stores and is the same",
                0.9,
                0.8,
            ),
            snippet(
                "This is a long fact that appears in both stores and is the same",
                0.8,
                0.7,
            ),
            snippet("A different fact entirely.", 0.7, 0.6),
        ];
        let result = rank_and_filter_snippets(snippets, 0.0, 10);
        assert_eq!(result.len(), 2, "duplicate prefix should be deduped");
    }

    #[test]
    fn rank_and_filter_filters_by_saliency_floor() {
        let snippets = vec![
            snippet("high saliency", 0.9, 0.9), // 0.81
            snippet("low saliency", 0.3, 0.3),  // 0.09
        ];
        let result = rank_and_filter_snippets(snippets, 0.5, 10);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].text, "high saliency");
    }

    #[test]
    fn rank_and_filter_truncates_to_max_chunks() {
        let snippets = vec![
            snippet("a", 0.9, 0.9),
            snippet("b", 0.8, 0.8),
            snippet("c", 0.7, 0.7),
        ];
        let result = rank_and_filter_snippets(snippets, 0.0, 2);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].text, "a"); // highest saliency first
    }

    #[test]
    fn build_saliency_query_includes_task_and_user_assistant_turns() {
        let messages = vec![
            hkask_types::ChatMessage {
                role: "system".to_string(),
                content: "system prompt".to_string(),
            },
            hkask_types::ChatMessage {
                role: "user".to_string(),
                content: "user message".to_string(),
            },
            hkask_types::ChatMessage {
                role: "assistant".to_string(),
                content: "assistant response".to_string(),
            },
            hkask_types::ChatMessage {
                role: "tool".to_string(),
                content: "tool result".to_string(),
            },
        ];
        let query = build_saliency_query("do the thing", &messages);
        assert!(query.contains("do the thing"));
        assert!(query.contains("user message"));
        assert!(query.contains("assistant response"));
        assert!(!query.contains("system prompt"));
        assert!(!query.contains("tool result"));
    }
}
