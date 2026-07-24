//! Memory service — semantic recall, episodic recall, episodic storage, and paired recall.
//!
//! Extracted from ChatService to bring it within the 7-function public surface limit (P5).
//! Memory operations are a separate concern from inference and prompt composition.

use std::sync::Arc;

use hkask_capability::DelegationToken;
use hkask_memory::{
    ChatTurn, EpisodicStoragePort, RecallRequest, RecalledEpisode, RecalledSemantic,
    SemanticStoragePort, StorageRequest,
};
use hkask_types::{Confidence, DataCategory, WebID};

use hkask_services_context::AgentService;

/// Shape for chat turn recall — controls ranking and limit.
///
/// Used by `MemoryService::recall_chat_turns` to share recall + projection
/// logic across `recall_episodic`, `recall_recent_turns`, and
/// `recall_raw_episodes`. Each caller formats the returned `Vec<ChatTurn>`
/// for its own consumer (ADR-060: per-surface rendering).
enum RecallShape {
    /// Rank by keyword overlap with context, take top `limit`.
    Ranked { context: String, limit: usize },
    /// Take the `limit` oldest episodes (port returns most-recent-first;
    /// this reverses and takes the oldest `limit`).
    Recent { limit: usize },
}

pub struct MemoryService;

impl MemoryService {
    /// Check whether the owner has consent for a specific data category.
    ///
    /// \[NORMATIVE\] P1 User Sovereignty / P2 Affirmative Consent.
    /// Fails closed: no consent => no sovereign memory access.
    #[must_use]
    pub fn has_memory_consent(ctx: &AgentService, owner: &WebID, category: &DataCategory) -> bool {
        ctx.governance()
            .consent
            .has_consent(&owner.to_string(), category)
            .unwrap_or(false)
    }

    /// Recall semantic memory h_mems relevant to the input.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence.
    #[must_use]
    pub fn recall_semantic(
        semantic_port: &Arc<dyn SemanticStoragePort>,
        input: &str,
        token: &DelegationToken,
    ) -> Option<String> {
        let request = RecallRequest::semantic(input, token.clone());
        let h_mems = match semantic_port.recall_semantic(&request) {
            Ok(t) if !t.is_empty() => t,
            _ => return None,
        };
        let context: Vec<String> = h_mems
            .iter()
            .filter_map(|t: &RecalledSemantic| t.value.as_str().map(|s| s.to_string()))
            .collect();
        let context: Vec<String> = context.into_iter().take(10).collect();
        if context.is_empty() {
            None
        } else {
            Some(context.join("\n"))
        }
    }

    /// Recall chat turns from episodic memory with the given shape.
    ///
    /// Shared recall + projection logic for `recall_episodic`,
    /// `recall_recent_turns`, and `recall_raw_episodes`. Each caller formats
    /// the returned `Vec<ChatTurn>` for its own consumer.
    fn recall_chat_turns(
        episodic_port: &Arc<dyn EpisodicStoragePort>,
        agent_webid: &WebID,
        token: &DelegationToken,
        shape: &RecallShape,
    ) -> Vec<ChatTurn> {
        let request = RecallRequest::episodic("chatted", *agent_webid, token.clone());
        let episodes: Vec<RecalledEpisode> = match episodic_port.recall_episodic(&request) {
            Ok(v) if !v.is_empty() => v,
            _ => return Vec::new(),
        };
        match shape {
            RecallShape::Ranked { context, limit } => {
                let keywords = hkask_memory::salience::extract_keywords(context);
                let mut scored: Vec<(usize, ChatTurn)> = episodes
                    .iter()
                    .filter_map(|e| {
                        let ct = ChatTurn::from_value(&e.value)?;
                        let combined = format!("{} {}", ct.user_input, ct.agent_response);
                        let score =
                            hkask_memory::salience::keyword_overlap_score(&keywords, &combined);
                        Some((score, ct))
                    })
                    .collect();
                scored.sort_by(|a, b| b.0.cmp(&a.0));
                scored.into_iter().take(*limit).map(|(_, ct)| ct).collect()
            }
            RecallShape::Recent { limit } => episodes
                .iter()
                .take(*limit)
                .filter_map(|e| ChatTurn::from_value(&e.value))
                .collect(),
        }
    }

    /// Recall episodic memories relevant to the input, sorted by salience.
    ///
    /// Mirrors `recall_semantic`: same return type, same take-top-N pattern.
    #[must_use]
    pub fn recall_episodic(
        episodic_port: &Arc<dyn EpisodicStoragePort>,
        input: &str,
        agent_webid: &WebID,
        token: &DelegationToken,
    ) -> Option<String> {
        let turns = Self::recall_chat_turns(
            episodic_port,
            agent_webid,
            token,
            &RecallShape::Ranked {
                context: input.to_string(),
                limit: 10,
            },
        );
        if turns.is_empty() {
            None
        } else {
            Some(
                turns
                    .iter()
                    .map(|ct| format!("User: {}\nAgent: {}", ct.user_input, ct.agent_response))
                    .collect::<Vec<_>>()
                    .join("\n\n"),
            )
        }
    }

    /// Recall recent chat turns as formatted history context.
    #[must_use]
    pub fn recall_recent_turns(
        episodic_port: &Arc<dyn EpisodicStoragePort>,
        agent_webid: &WebID,
        token: &DelegationToken,
        limit: usize,
    ) -> Option<String> {
        let turns = Self::recall_chat_turns(
            episodic_port,
            agent_webid,
            token,
            &RecallShape::Recent { limit },
        );
        if turns.is_empty() {
            None
        } else {
            let formatted = turns
                .iter()
                .rev()
                .map(|ct| format!("User: {}\nAgent: {}", ct.user_input, ct.agent_response))
                .collect::<Vec<_>>()
                .join("\n\n");
            Some(format!(
                "[Previous conversation]\n{}\n[/Previous conversation]\n\n",
                formatted
            ))
        }
    }

    /// Store the chat exchange as an episodic h_mem.
    pub fn store_episodic(
        episodic_port: &Arc<dyn EpisodicStoragePort>,
        input: &str,
        response: &str,
        agent_webid: WebID,
        token: &DelegationToken,
        userpod_name: &str,
    ) {
        let request = StorageRequest::episodic(
            "chatted",
            "chat_turn",
            serde_json::json!({
                "user_input": input,
                "agent_response": response,
            }),
            Confidence::new(0.7),
            agent_webid,
        );
        match episodic_port.store_episodic(request, token) {
            Ok(_) => {
                tracing::debug!(
                    target: "hkask.chat.memory",
                    agent = %userpod_name,
                    "Episodic trace stored"
                );
            }
            Err(e) => {
                tracing::debug!(
                    target: "hkask.chat.memory",
                    agent = %userpod_name,
                    error = %e,
                    "Episodic storage failed — response still returned"
                );
            }
        }
    }

    /// Recall raw episodes for condensation.
    pub(crate) fn recall_raw_episodes(
        episodic_port: &Arc<dyn EpisodicStoragePort>,
        agent_webid: &WebID,
        token: &DelegationToken,
        limit: usize,
    ) -> Vec<serde_json::Value> {
        let turns = Self::recall_chat_turns(
            episodic_port,
            agent_webid,
            token,
            &RecallShape::Recent { limit },
        );
        let mut messages: Vec<serde_json::Value> = Vec::new();
        for ct in turns.iter().rev() {
            messages.push(serde_json::json!({"role": "user", "content": ct.user_input}));
            messages.push(serde_json::json!({"role": "assistant", "content": ct.agent_response}));
        }
        messages
    }

    /// Paired memory recall — returns merged context from both stores.
    #[must_use]
    pub fn recall_memory(
        semantic_port: &Arc<dyn SemanticStoragePort>,
        episodic_port: &Arc<dyn EpisodicStoragePort>,
        input: &str,
        agent_webid: &WebID,
        token: &DelegationToken,
    ) -> Option<String> {
        let semantic = Self::recall_semantic(semantic_port, input, token);
        let episodic = Self::recall_episodic(episodic_port, input, agent_webid, token);
        match (semantic, episodic) {
            (Some(s), Some(e)) => Some(format!("{}\n\n{}", s, e)),
            (Some(s), None) => Some(s),
            (None, Some(e)) => Some(e),
            (None, None) => None,
        }
    }
}
