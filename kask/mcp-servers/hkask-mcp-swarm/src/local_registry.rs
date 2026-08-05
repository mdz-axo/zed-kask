//! Local agent registry — v2 §15 catalogue of local agent cards.
//!
//! Extracted from the swarm server root. Reads agent cards from a local
//! directory (`<id>/agent_card.json`), catalogue only — execution is Slice 9
//! (`swarm_delegate_local`). The cache distinguishes not-loaded from
//! loaded-empty via the `loaded` flag (the `.rules` trap on lazy-load caches).

use crate::error::LocalSwarmError;

/// A local agent card — the minimal subset of fermi's `AgentCard` we need for
/// catalogue + future execution. Mirrors the JSON shape in
/// `agents/local/curated/<id>/agent_card.json`.
///
/// The `cloud_id` field tracks the sync link to an ABW agent: when present,
/// the agent is `synced` (exists both locally and on ABW). When absent,
/// the agent is `local` only. The operator sets `cloud_id` when cloning an
/// ABW agent to local (Slice 11).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct LocalAgentCard {
    pub agent_id: String,
    pub agent_type: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub accepts: Vec<String>,
    #[serde(default)]
    pub produces: Vec<String>,
    #[serde(default)]
    pub dependencies: LocalAgentDependencies,
    #[serde(default)]
    pub capabilities: LocalAgentCapabilities,
    /// The ABW agent id this local card is synced with. `None` = local-only.
    /// When set, the panel shows a "synced" badge and the operator can push
    /// local changes to ABW or pull ABW changes to local.
    #[serde(default)]
    pub cloud_id: Option<String>,
    /// Tags for local catalogue discovery. `#[serde(default)]` so existing
    /// cards without this field still deserialize.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Visibility level ("public", "private", "unlisted"). Default "private".
    #[serde(default)]
    pub visibility: String,
    /// Valence / personality encoding. Optional — not all local agents need it.
    #[serde(default)]
    pub valence: Option<LocalAgentValence>,
}

/// Valence parameters mirroring the ABW `metadata.valence` object, for local
/// agent cards. Stored as a struct so local cards round-trip through JSON.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct LocalAgentValence {
    #[serde(default)]
    pub arousal: Option<f64>,
    #[serde(default)]
    pub valence: Option<f64>,
    #[serde(default)]
    pub primary_affect: Option<String>,
    #[serde(default)]
    pub personality_traits: Vec<String>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct LocalAgentDependencies {
    #[serde(default)]
    pub required: Vec<String>,
    #[serde(default)]
    pub optional: Vec<String>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct LocalAgentCapabilities {
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub min_provider_class: String,
    #[serde(default)]
    pub system_prompt: Option<String>,
    /// MCP tools this agent may call, as qualified `server/tool` names
    /// (e.g. `"codegraph/codegraph_query"`). `swarm_delegate_local` declares
    /// these to the model and dispatches tool calls through the zed IPC
    /// bridge's governed `McpRuntime` — the allowlist IS the enforcement:
    /// a call for a tool not listed here is never dispatched.
    #[serde(default)]
    pub mcp_tools: Vec<String>,
    /// Skill ids this agent declares. `swarm_delegate_local` executes each
    /// declared skill (capped at 3) against the task through the zed IPC
    /// bridge's `ManifestExecutor` before the LLM call, and injects the
    /// cascade output into the prompt as context (guard-scanned). Carried
    /// through create/clone/push as well.
    #[serde(default)]
    pub skills: Vec<String>,
}

/// Reads agent cards from a local directory. Catalogue only — no execution.
///
/// The directory layout mirrors fermi's `agents/curated/`:
/// ```text
/// agents/local/curated/
///   market_research/
///     agent_card.json
///   sentiment_analyzer/
///     agent_card.json
/// ```
///
/// The cache distinguishes not-loaded from loaded-empty via the `loaded` flag
/// (the `.rules` trap on lazy-load caches). A missing directory is not an
/// error at load time — it surfaces as an empty list + a startup warning
/// (emitted by `SwarmConfig::from_env`).
pub struct LocalAgentRegistry {
    dir: String,
    cards: std::sync::Mutex<Option<Vec<LocalAgentCard>>>,
}

impl LocalAgentRegistry {
    /// Construct without loading. Call `load` to populate.
    pub fn new(dir: impl Into<String>) -> Self {
        Self {
            dir: dir.into(),
            cards: std::sync::Mutex::new(None),
        }
    }

    /// Load (or reload) agent cards from the directory. Returns the number of
    /// cards loaded. A missing directory yields zero cards (not an error) —
    /// the startup warning in `SwarmConfig::from_env` covers this case.
    pub fn load(&self) -> Result<usize, LocalSwarmError> {
        let path = std::path::Path::new(&self.dir);
        if !path.exists() {
            *self.cards.lock().unwrap() = Some(Vec::new());
            return Ok(0);
        }
        let mut cards = Vec::new();
        let entries = std::fs::read_dir(path).map_err(|e| {
            LocalSwarmError::Io(format!(
                "failed to read local agents dir '{}': {e}",
                self.dir
            ))
        })?;
        for entry in entries {
            let entry =
                entry.map_err(|e| LocalSwarmError::Io(format!("readdir entry error: {e}")))?;
            let card_path = entry.path().join("agent_card.json");
            if !card_path.exists() {
                continue;
            }
            let json = std::fs::read_to_string(&card_path).map_err(|e| {
                LocalSwarmError::Io(format!("failed to read {}: {e}", card_path.display()))
            })?;
            let card: LocalAgentCard = serde_json::from_str(&json).map_err(|e| {
                LocalSwarmError::InvalidInput(format!(
                    "failed to parse {}: {e}",
                    card_path.display()
                ))
            })?;
            cards.push(card);
        }
        cards.sort_by(|a, b| a.agent_id.cmp(&b.agent_id));
        let count = cards.len();
        *self.cards.lock().unwrap() = Some(cards);
        Ok(count)
    }

    /// List all loaded cards, reloading from disk first so operator-added
    /// cards appear without a server restart. Returns an empty slice if not
    /// yet loaded or the directory was empty. A reload failure keeps the
    /// previous cache (logged) — a transient unreadable card must not blank
    /// the list.
    pub fn list(&self) -> Vec<LocalAgentCard> {
        if let Err(e) = self.load() {
            tracing::warn!(
                target: "hkask.mcp.swarm",
                "local registry reload failed (keeping cached cards): {e}"
            );
        }
        self.cards.lock().unwrap().clone().unwrap_or_default()
    }

    /// Look up a single card by agent id, reloading from disk first (same
    /// staleness policy as `list`). Returns `None` if not loaded or not
    /// found.
    pub fn get(&self, agent_id: &str) -> Option<LocalAgentCard> {
        if let Err(e) = self.load() {
            tracing::warn!(
                target: "hkask.mcp.swarm",
                "local registry reload failed (keeping cached cards): {e}"
            );
        }
        self.cards
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|cards| cards.iter().find(|c| c.agent_id == agent_id).cloned())
    }

    /// Whether `load` has been called (regardless of result). Used to
    /// distinguish not-loaded from loaded-empty.
    #[cfg(test)]
    pub(crate) fn is_loaded(&self) -> bool {
        self.cards.lock().unwrap().is_some()
    }

    /// Write (or overwrite) a local agent card to
    /// `<dir>/<agent_id>/agent_card.json`, then reload the registry so the new
    /// card is visible to subsequent `list`/`get` calls. Cybernetic Swarm Plan
    /// C6 — the `reconfigure_agent` DECIDE action rewrites a blamed agent's
    /// `system_prompt` in place via this method (seed `swarm_generate_prompt`
    /// with the failure log, write the new card, reload). The `agent_id` is
    /// re-sanitized before joining the registry root (path-traversal defense);
    /// the card is written under a canonicalized, path-contained directory —
    /// the same invariant `swarm_create_local_agent`/`swarm_remove_local` pin.
    /// Returns the written card path on success.
    pub fn write_card(&self, card: &LocalAgentCard) -> Result<String, LocalSwarmError> {
        let safe_id = crate::sanitize::sanitize_agent_id(&card.agent_id).ok_or_else(|| {
            LocalSwarmError::Sanitize(format!(
                "agent_id '{}' contains no safe characters (alphanumeric, dash, underscore, dot)",
                card.agent_id
            ))
        })?;
        let registry_root = std::path::Path::new(&self.dir)
            .canonicalize()
            .map_err(|e| {
                LocalSwarmError::Io(format!(
                    "failed to resolve local agents dir '{}': {e}",
                    self.dir
                ))
            })?;
        let card_dir = registry_root.join(&safe_id);
        // Defense-in-depth: refuse to write outside the registry root.
        if !card_dir.starts_with(&registry_root) {
            return Err(LocalSwarmError::Sanitize(
                "refusing to write a path outside the local agents dir".to_string(),
            ));
        }
        std::fs::create_dir_all(&card_dir).map_err(|e| {
            LocalSwarmError::Io(format!(
                "failed to create agent dir {}: {e}",
                card_dir.display()
            ))
        })?;
        let card_path = card_dir.join("agent_card.json");
        let json = serde_json::to_string_pretty(card)
            .map_err(|e| LocalSwarmError::InvalidInput(format!("failed to serialize card: {e}")))?;
        std::fs::write(&card_path, json).map_err(|e| {
            LocalSwarmError::Io(format!("failed to write {}: {e}", card_path.display()))
        })?;
        self.load()?;
        Ok(card_path.to_string_lossy().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_registry_missing_dir_loads_zero() {
        let dir = std::env::temp_dir().join("hkask_swarm_test_nonexistent_dir");
        let _ = std::fs::remove_dir_all(&dir); // clean slate
        let registry = LocalAgentRegistry::new(dir.to_string_lossy().to_string());
        assert!(!registry.is_loaded());
        let count = registry.load().expect("missing dir should not error");
        assert_eq!(count, 0);
        assert!(registry.is_loaded());
        assert!(registry.list().is_empty());
        assert!(registry.get("any_agent").is_none());
    }

    #[test]
    fn local_registry_loads_cards_from_dir() {
        let dir = std::env::temp_dir().join("hkask_swarm_test_local_registry");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("alpha_agent")).unwrap();
        std::fs::write(
            dir.join("alpha_agent").join("agent_card.json"),
            serde_json::json!({
                "agent_id": "alpha_agent",
                "agent_type": "research",
                "description": "Alpha test agent",
                "accepts": ["query"],
                "produces": ["analysis"],
                "dependencies": { "required": [], "optional": [] },
                "capabilities": {
                    "model": "ollama/qwen3:8b",
                    "min_provider_class": "local",
                    "system_prompt": "You are alpha."
                }
            })
            .to_string(),
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("beta_agent")).unwrap();
        std::fs::write(
            dir.join("beta_agent").join("agent_card.json"),
            serde_json::json!({
                "agent_id": "beta_agent",
                "agent_type": "sentiment"
            })
            .to_string(),
        )
        .unwrap();

        let registry = LocalAgentRegistry::new(dir.to_string_lossy().to_string());
        let count = registry.load().expect("load should succeed");
        assert_eq!(count, 2);
        let cards = registry.list();
        // Sorted by agent_id.
        assert_eq!(cards[0].agent_id, "alpha_agent");
        assert_eq!(cards[1].agent_id, "beta_agent");
        let alpha = registry.get("alpha_agent").expect("alpha should be found");
        assert_eq!(alpha.agent_type, "research");
        assert_eq!(alpha.accepts, vec!["query".to_string()]);
        assert_eq!(alpha.produces, vec!["analysis".to_string()]);
        assert_eq!(alpha.capabilities.model, "ollama/qwen3:8b");
        assert_eq!(alpha.capabilities.min_provider_class, "local");
        // Beta has minimal fields — defaults should fill in.
        let beta = registry.get("beta_agent").expect("beta should be found");
        assert!(beta.accepts.is_empty());
        assert!(beta.produces.is_empty());
        assert!(beta.dependencies.required.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn local_registry_write_card_updates_prompt_and_preserves_fields() {
        // Cybernetic Swarm Plan C6 enforcement point: write_card rewrites an
        // existing card in place (preserving agent_id/type/description/accepts/
        // produces/cloud_id) and reloads the registry so the new prompt is
        // visible to the next delegation. A reconfigure that clobbered cloud_id
        // would silently break the sync link — this pins it.
        let dir = std::env::temp_dir().join("hkask_swarm_test_write_card");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("synced_agent")).unwrap();
        std::fs::write(
            dir.join("synced_agent").join("agent_card.json"),
            serde_json::json!({
                "agent_id": "synced_agent",
                "agent_type": "research",
                "description": "Original",
                "accepts": ["query"],
                "produces": ["analysis"],
                "dependencies": { "required": ["dep_a"], "optional": [] },
                "capabilities": {
                    "model": "ollama/qwen3:8b",
                    "min_provider_class": "local",
                    "system_prompt": "You are the original prompt."
                },
                "cloud_id": "abw-uuid-123"
            })
            .to_string(),
        )
        .unwrap();

        let registry = LocalAgentRegistry::new(dir.to_string_lossy().to_string());
        registry.load().expect("initial load");
        let mut card = registry.get("synced_agent").expect("card present");
        assert_eq!(
            card.capabilities.system_prompt.as_deref(),
            Some("You are the original prompt.")
        );
        assert_eq!(card.cloud_id.as_deref(), Some("abw-uuid-123"));

        // Reconfigure: change only the system_prompt.
        card.capabilities.system_prompt = Some("You are the improved prompt.".to_string());
        let path = registry.write_card(&card).expect("write succeeds");
        assert!(path.contains("synced_agent"));

        // Reload picked up the new prompt; cloud_id and other fields preserved.
        let reloaded = registry.get("synced_agent").expect("reloaded");
        assert_eq!(
            reloaded.capabilities.system_prompt.as_deref(),
            Some("You are the improved prompt.")
        );
        assert_eq!(
            reloaded.cloud_id.as_deref(),
            Some("abw-uuid-123"),
            "cloud_id preserved"
        );
        assert_eq!(reloaded.agent_type, "research");
        assert_eq!(reloaded.dependencies.required, vec!["dep_a".to_string()]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn local_registry_write_card_rejects_unsafe_agent_id() {
        // Path-traversal defense: a card with an agent_id that sanitizes to
        // nothing (only dots/slashes) must not write outside the registry root.
        let dir = std::env::temp_dir().join("hkask_swarm_test_write_unsafe");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let registry = LocalAgentRegistry::new(dir.to_string_lossy().to_string());
        let card = LocalAgentCard {
            agent_id: "..".to_string(),
            agent_type: "x".to_string(),
            description: String::new(),
            accepts: vec![],
            produces: vec![],
            dependencies: LocalAgentDependencies::default(),
            capabilities: LocalAgentCapabilities::default(),
            cloud_id: None,
            tags: vec![],
            visibility: String::new(),
            valence: None,
        };
        assert!(
            registry.write_card(&card).is_err(),
            "unsafe id must be rejected"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn local_registry_skips_dirs_without_card() {
        let dir = std::env::temp_dir().join("hkask_swarm_test_skip_dirs");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("has_card")).unwrap();
        std::fs::write(
            dir.join("has_card").join("agent_card.json"),
            serde_json::json!({ "agent_id": "has_card", "agent_type": "test" }).to_string(),
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("no_card")).unwrap(); // no agent_card.json

        let registry = LocalAgentRegistry::new(dir.to_string_lossy().to_string());
        let count = registry.load().expect("load should succeed");
        assert_eq!(count, 1);
        assert!(registry.get("has_card").is_some());
        assert!(registry.get("no_card").is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn local_registry_reload_replaces_cache() {
        let dir = std::env::temp_dir().join("hkask_swarm_test_reload");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("first")).unwrap();
        std::fs::write(
            dir.join("first").join("agent_card.json"),
            serde_json::json!({ "agent_id": "first", "agent_type": "test" }).to_string(),
        )
        .unwrap();

        let registry = LocalAgentRegistry::new(dir.to_string_lossy().to_string());
        assert_eq!(registry.load().unwrap(), 1);
        assert!(registry.get("first").is_some());

        // Add a second card and reload.
        std::fs::create_dir_all(dir.join("second")).unwrap();
        std::fs::write(
            dir.join("second").join("agent_card.json"),
            serde_json::json!({ "agent_id": "second", "agent_type": "test" }).to_string(),
        )
        .unwrap();
        assert_eq!(registry.load().unwrap(), 2);
        assert!(registry.get("first").is_some());
        assert!(registry.get("second").is_some());

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── Property-based tests ──────────────────────────────────────────────

    use proptest::prelude::*;

    fn arb_agent_card() -> BoxedStrategy<LocalAgentCard> {
        (
            "[a-z][a-z0-9_-]*",
            "[a-z][a-z0-9_-]*",
            "[a-z ]*",
            prop::collection::vec("[a-z_]+", 0..5),
            prop::collection::vec("[a-z_]+", 0..5),
            prop::collection::vec("[a-z_]+", 0..3),
            prop::collection::vec("[a-z_]+", 0..3),
            "[a-z0-9_./:-]*",
            "[a-z0-9_.-]*",
            proptest::option::of("[a-z0-9_-]+"),
        )
            .prop_map(
                |(
                    agent_id,
                    agent_type,
                    description,
                    accepts,
                    produces,
                    required,
                    optional,
                    model,
                    min_provider_class,
                    cloud_id,
                )| {
                    LocalAgentCard {
                        agent_id,
                        agent_type,
                        description,
                        accepts,
                        produces,
                        dependencies: LocalAgentDependencies { required, optional },
                        capabilities: LocalAgentCapabilities {
                            model,
                            min_provider_class,
                            system_prompt: Some("test prompt".to_string()),
                            mcp_tools: vec![],
                            skills: vec![],
                        },
                        cloud_id,
                        tags: vec![],
                        visibility: String::new(),
                        valence: None,
                    }
                },
            )
            .boxed()
    }

    proptest! {
        // LocalAgentCard survives a serialize → deserialize round-trip.
        #[test]
        fn agent_card_round_trips_through_json(card in arb_agent_card()) {
            let json = serde_json::to_string(&card).expect("serialization must succeed");
            let deserialized: LocalAgentCard = serde_json::from_str(&json)
                .expect("deserialization must succeed");
            prop_assert_eq!(deserialized.agent_id, card.agent_id, "round-trip lost agent_id");
            prop_assert_eq!(deserialized.agent_type, card.agent_type, "round-trip lost agent_type");
            prop_assert_eq!(deserialized.description, card.description, "round-trip lost description");
            prop_assert_eq!(deserialized.accepts, card.accepts, "round-trip lost accepts");
            prop_assert_eq!(deserialized.produces, card.produces, "round-trip lost produces");
            prop_assert_eq!(deserialized.dependencies.required, card.dependencies.required, "round-trip lost dependencies.required");
            prop_assert_eq!(deserialized.dependencies.optional, card.dependencies.optional, "round-trip lost dependencies.optional");
            prop_assert_eq!(deserialized.capabilities.model, card.capabilities.model, "round-trip lost capabilities.model");
            prop_assert_eq!(deserialized.cloud_id, card.cloud_id, "round-trip lost cloud_id");
        }
    }
}
