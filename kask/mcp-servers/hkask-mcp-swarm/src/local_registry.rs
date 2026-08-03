//! Local agent registry — v2 §15 catalogue of local agent cards.
//!
//! Extracted from the swarm server root. Reads agent cards from a local
//! directory (`<id>/agent_card.json`), catalogue only — execution is Slice 9
//! (`swarm_delegate_local`). The cache distinguishes not-loaded from
//! loaded-empty via the `loaded` flag (the `.rules` trap on lazy-load caches).

/// A local agent card — the minimal subset of fermi's `AgentCard` we need for
/// catalogue + future execution. Mirrors the JSON shape in
/// `agents/local/curated/<id>/agent_card.json`.
///
/// The `cloud_id` field tracks the sync link to an ABW agent: when present,
/// the agent is `synced` (exists both locally and on ABW). When absent,
/// the agent is `local` only. The operator sets `cloud_id` when cloning an
/// ABW agent to local (Slice 11).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct LocalAgentCard {
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
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct LocalAgentDependencies {
    #[serde(default)]
    pub required: Vec<String>,
    #[serde(default)]
    pub optional: Vec<String>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct LocalAgentCapabilities {
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
pub(crate) struct LocalAgentRegistry {
    dir: String,
    cards: std::sync::Mutex<Option<Vec<LocalAgentCard>>>,
}

impl LocalAgentRegistry {
    /// Construct without loading. Call `load` to populate.
    pub(crate) fn new(dir: impl Into<String>) -> Self {
        Self {
            dir: dir.into(),
            cards: std::sync::Mutex::new(None),
        }
    }

    /// Load (or reload) agent cards from the directory. Returns the number of
    /// cards loaded. A missing directory yields zero cards (not an error) —
    /// the startup warning in `SwarmConfig::from_env` covers this case.
    pub(crate) fn load(&self) -> Result<usize, String> {
        let path = std::path::Path::new(&self.dir);
        if !path.exists() {
            *self.cards.lock().unwrap() = Some(Vec::new());
            return Ok(0);
        }
        let mut cards = Vec::new();
        let entries = std::fs::read_dir(path)
            .map_err(|e| format!("failed to read local agents dir '{}': {e}", self.dir))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("readdir entry error: {e}"))?;
            let card_path = entry.path().join("agent_card.json");
            if !card_path.exists() {
                continue;
            }
            let json = std::fs::read_to_string(&card_path)
                .map_err(|e| format!("failed to read {}: {e}", card_path.display()))?;
            let card: LocalAgentCard = serde_json::from_str(&json)
                .map_err(|e| format!("failed to parse {}: {e}", card_path.display()))?;
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
    pub(crate) fn list(&self) -> Vec<LocalAgentCard> {
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
    pub(crate) fn get(&self, agent_id: &str) -> Option<LocalAgentCard> {
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
}
