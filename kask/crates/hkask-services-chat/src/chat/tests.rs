#![allow(clippy::items_after_test_module)]

use super::types::TokenUsage;
use crate::memory::MemoryService;
use hkask_capability::{DelegationAction, DelegationResource, DelegationToken, derive_signing_key};
use hkask_memory::{
    EpisodicStoragePort, RecallRequest, RecalledSemantic, SemanticStoragePort, StorageRequest,
};
use hkask_memory::{MemoryPortError, RecalledEpisode};
use hkask_regulation::types::loops::ExperienceClassification;
use hkask_types::{Confidence, WebID};
use std::sync::Arc;

#[test]
fn token_usage_gas_cost_one_to_one() {
    let usage = TokenUsage {
        prompt_tokens: 100,
        completion_tokens: 50,
        total_tokens: 150,
    };
    assert_eq!(usage.gas_cost(), 150, "Gas cost must equal total_tokens");
}

#[test]
fn token_usage_zero_tokens_zero_gas() {
    let usage = TokenUsage {
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
    };
    assert_eq!(usage.gas_cost(), 0);
}

#[test]
fn token_usage_gas_uses_total_not_sum_of_parts() {
    let usage = TokenUsage {
        prompt_tokens: 100,
        completion_tokens: 200,
        total_tokens: 250,
    };
    assert_eq!(usage.gas_cost(), 250);
    assert_ne!(usage.gas_cost(), 300);
}

fn test_token(from: WebID, to: WebID) -> DelegationToken {
    DelegationToken::new(
        DelegationResource::Registry,
        "test".into(),
        DelegationAction::Execute,
        from,
        to,
        &derive_signing_key(b"test-hmac-secret-32-bytes-long!!"),
    )
}

struct MockSemanticPort {
    h_mems: Vec<RecalledSemantic>,
}
impl SemanticStoragePort for MockSemanticPort {
    fn store_semantic(
        &self,
        _: StorageRequest,
        _: &DelegationToken,
    ) -> Result<String, MemoryPortError> {
        Ok("id".into())
    }
    fn recall_semantic(&self, _: &RecallRequest) -> Result<Vec<RecalledSemantic>, MemoryPortError> {
        Ok(self.h_mems.clone())
    }
    fn semantic_storage_usage(&self, _: &str) -> Result<usize, MemoryPortError> {
        Ok(self.h_mems.len())
    }
}

struct MockEpisodicPort {
    last_request: std::sync::Mutex<Option<StorageRequest>>,
}
impl EpisodicStoragePort for MockEpisodicPort {
    fn store_episodic(
        &self,
        r: StorageRequest,
        _: &DelegationToken,
    ) -> Result<String, MemoryPortError> {
        *self.last_request.lock().unwrap() = Some(r);
        Ok("id".into())
    }
    fn recall_episodic(&self, _: &RecallRequest) -> Result<Vec<RecalledEpisode>, MemoryPortError> {
        Ok(vec![])
    }
    fn episodic_storage_usage(&self, _: &WebID) -> Result<usize, MemoryPortError> {
        Ok(0)
    }
    fn episodic_storage_budget(&self) -> usize {
        10_000
    }
    fn store_episodic_classified(
        &self,
        r: StorageRequest,
        _: ExperienceClassification,
        _: Option<Confidence>,
        t: &DelegationToken,
    ) -> Result<String, MemoryPortError> {
        self.store_episodic(r, t)
    }
}

/// Mock that returns pre-configured episodes (in port order: most-recent-first).
struct MockEpisodicPortWithEpisodes {
    episodes: Vec<RecalledEpisode>,
}
impl EpisodicStoragePort for MockEpisodicPortWithEpisodes {
    fn store_episodic(
        &self,
        _: StorageRequest,
        _: &DelegationToken,
    ) -> Result<String, MemoryPortError> {
        Ok("id".into())
    }
    fn recall_episodic(&self, _: &RecallRequest) -> Result<Vec<RecalledEpisode>, MemoryPortError> {
        Ok(self.episodes.clone())
    }
    fn episodic_storage_usage(&self, _: &WebID) -> Result<usize, MemoryPortError> {
        Ok(self.episodes.len())
    }
    fn episodic_storage_budget(&self) -> usize {
        10_000
    }
    fn store_episodic_classified(
        &self,
        r: StorageRequest,
        _: ExperienceClassification,
        _: Option<Confidence>,
        t: &DelegationToken,
    ) -> Result<String, MemoryPortError> {
        self.store_episodic(r, t)
    }
}

/// Build a chat-turn episode for testing.
fn chat_episode(id: &str, user: &str, agent: &str, observed_at: &str) -> RecalledEpisode {
    RecalledEpisode {
        id: id.into(),
        entity: "chatted".into(),
        attribute: "chat_turn".into(),
        value: serde_json::json!({"user_input": user, "agent_response": agent}),
        confidence: Confidence::new(0.7),
        perspective: None,
        visibility: hkask_types::Visibility::Private,
        observed_at: observed_at.into(),
        dimension: None,
    }
}

#[test]
fn recall_semantic_empty_returns_none() {
    let mock: Arc<MockSemanticPort> = Arc::new(MockSemanticPort { h_mems: vec![] });
    let port: Arc<dyn SemanticStoragePort> = mock;
    let w = WebID::new();
    let result = MemoryService::recall_semantic(&port, "q", &test_token(w, w));
    assert!(result.is_none());
}

#[test]
fn recall_semantic_joins_values_with_newlines() {
    let t = |s: &str| RecalledSemantic {
        id: "x".into(),
        entity: "doc".into(),
        attribute: "c".into(),
        value: serde_json::json!(s),
        confidence: Confidence::new(0.9),
        visibility: hkask_types::Visibility::Shared,
        observed_at: "2026-01-01T00:00:00Z".into(),
        dimension: None,
    };
    let mock: Arc<MockSemanticPort> = Arc::new(MockSemanticPort {
        h_mems: vec![t("A"), t("B")],
    });
    let port: Arc<dyn SemanticStoragePort> = mock;
    let w = WebID::new();
    let result = MemoryService::recall_semantic(&port, "q", &test_token(w, w));
    assert_eq!(result, Some("A\nB".into()));
}

#[test]
fn recall_semantic_filters_non_string_values() {
    let t1 = RecalledSemantic {
        id: "x".into(),
        entity: "doc".into(),
        attribute: "c".into(),
        value: serde_json::json!("Text"),
        confidence: Confidence::new(0.9),
        visibility: hkask_types::Visibility::Shared,
        observed_at: "2026-01-01T00:00:00Z".into(),
        dimension: None,
    };
    let t2 = RecalledSemantic {
        id: "y".into(),
        entity: "doc".into(),
        attribute: "c".into(),
        value: serde_json::json!(42),
        confidence: Confidence::new(0.9),
        visibility: hkask_types::Visibility::Shared,
        observed_at: "2026-01-01T00:00:00Z".into(),
        dimension: None,
    };
    let mock: Arc<MockSemanticPort> = Arc::new(MockSemanticPort {
        h_mems: vec![t1, t2],
    });
    let port: Arc<dyn SemanticStoragePort> = mock;
    let w = WebID::new();
    let result = MemoryService::recall_semantic(&port, "q", &test_token(w, w));
    assert_eq!(result, Some("Text".into()));
}

#[test]
fn store_episodic_records_chat_exchange() {
    let mock: Arc<MockEpisodicPort> = Arc::new(MockEpisodicPort {
        last_request: std::sync::Mutex::new(None),
    });
    let port: Arc<dyn EpisodicStoragePort> = mock.clone();
    let w = WebID::from_persona(b"a");
    MemoryService::store_episodic(&port, "Hello", "Hi!", w, &test_token(w, w), "Agent");
    let req = mock.last_request.lock().unwrap();
    let r = req.as_ref().unwrap();
    assert_eq!(r.entity, "chatted");
    assert_eq!(r.attribute, "chat_turn");
    assert_eq!(r.value["user_input"], "Hello");
    assert_eq!(r.value["agent_response"], "Hi!");
}

#[test]
fn store_episodic_uses_fixed_confidence() {
    let mock: Arc<MockEpisodicPort> = Arc::new(MockEpisodicPort {
        last_request: std::sync::Mutex::new(None),
    });
    let port: Arc<dyn EpisodicStoragePort> = mock.clone();
    let w = WebID::from_persona(b"a");
    MemoryService::store_episodic(&port, "in", "out", w, &test_token(w, w), "Agent");
    let req = mock.last_request.lock().unwrap();
    assert!((req.as_ref().unwrap().confidence.value() - 0.7).abs() < 0.001);
}

#[test]
fn store_episodic_never_panics() {
    let mock: Arc<MockEpisodicPort> = Arc::new(MockEpisodicPort {
        last_request: std::sync::Mutex::new(None),
    });
    let port: Arc<dyn EpisodicStoragePort> = mock;
    let w = WebID::from_persona(b"t");
    MemoryService::store_episodic(&port, "", "", w, &test_token(w, w), "");
}

// ── Regression tests for recall ordering bugs ──────────────────────────────
//
// These tests verify that `recall_recent_turns` selects the MOST RECENT
// episodes (not the oldest) and displays them in chronological order, and
// that `recall_raw_episodes` returns messages with correct role ordering
// (user before assistant within each turn) in chronological order.
//
// The port returns episodes most-recent-first (as `EpisodicMemory::query_for_deduped`
// sorts by `observed_at` descending). The mock below simulates that order.

fn recent_test_episodes() -> Vec<RecalledEpisode> {
    // Most recent first (as the port returns):
    vec![
        chat_episode("5", "five", "resp5", "2026-07-05T00:00:00Z"),
        chat_episode("4", "four", "resp4", "2026-07-04T00:00:00Z"),
        chat_episode("3", "three", "resp3", "2026-07-03T00:00:00Z"),
        chat_episode("2", "two", "resp2", "2026-07-02T00:00:00Z"),
        chat_episode("1", "one", "resp1", "2026-07-01T00:00:00Z"),
    ]
}

#[test]
fn recall_recent_turns_returns_most_recent_in_chronological_order() {
    let mock: Arc<MockEpisodicPortWithEpisodes> = Arc::new(MockEpisodicPortWithEpisodes {
        episodes: recent_test_episodes(),
    });
    let port: Arc<dyn EpisodicStoragePort> = mock;
    let w = WebID::from_persona(b"a");
    let result = MemoryService::recall_recent_turns(&port, &w, &test_token(w, w), 3);
    let history = result.expect("should return some history");

    // Should include the 3 most recent episodes (3, 4, 5) — NOT the oldest (1, 2, 3).
    assert!(
        history.contains("three"),
        "should include episode 3 (third most recent)"
    );
    assert!(
        history.contains("four"),
        "should include episode 4 (second most recent)"
    );
    assert!(
        history.contains("five"),
        "should include episode 5 (most recent)"
    );
    assert!(
        !history.contains("one"),
        "should NOT include episode 1 (oldest, excluded by limit)"
    );
    assert!(
        !history.contains("two"),
        "should NOT include episode 2 (second oldest, excluded)"
    );

    // Should be in chronological order: three before four before five.
    let idx3 = history.find("three").unwrap();
    let idx4 = history.find("four").unwrap();
    let idx5 = history.find("five").unwrap();
    assert!(
        idx3 < idx4,
        "episode 3 should appear before episode 4 (chronological)"
    );
    assert!(
        idx4 < idx5,
        "episode 4 should appear before episode 5 (chronological)"
    );
}

#[test]
fn recall_raw_episodes_returns_most_recent_with_correct_roles() {
    let mock: Arc<MockEpisodicPortWithEpisodes> = Arc::new(MockEpisodicPortWithEpisodes {
        episodes: recent_test_episodes(),
    });
    let port: Arc<dyn EpisodicStoragePort> = mock;
    let w = WebID::from_persona(b"a");
    let messages = MemoryService::recall_raw_episodes(&port, &w, &test_token(w, w), 3);

    // Should return 6 messages: 3 episodes × 2 (user + assistant).
    assert_eq!(messages.len(), 6, "3 episodes should produce 6 messages");

    // Should include the 3 most recent episodes — NOT the oldest.
    let contents: Vec<&str> = messages
        .iter()
        .filter_map(|m| m.get("content").and_then(|c| c.as_str()))
        .collect();
    assert!(
        contents.contains(&"five"),
        "should include episode 5 content"
    );
    assert!(
        contents.contains(&"four"),
        "should include episode 4 content"
    );
    assert!(
        contents.contains(&"three"),
        "should include episode 3 content"
    );
    assert!(
        !contents.contains(&"one"),
        "should NOT include episode 1 (oldest)"
    );
    assert!(
        !contents.contains(&"two"),
        "should NOT include episode 2 (second oldest)"
    );

    // First message should have role "user" (not "assistant" — role reversal bug).
    let first_role = messages[0].get("role").and_then(|r| r.as_str());
    assert_eq!(
        first_role,
        Some("user"),
        "first message should be role=user (not assistant)"
    );

    // User message should come before assistant message within each turn.
    let first_content = messages[0].get("content").and_then(|c| c.as_str());
    let second_role = messages[1].get("role").and_then(|r| r.as_str());
    assert_eq!(
        second_role,
        Some("assistant"),
        "second message should be role=assistant"
    );

    // Should be in chronological order: episode 3 first, then 4, then 5.
    assert_eq!(
        first_content,
        Some("three"),
        "first user message should be episode 3 (oldest of selected)"
    );
}
