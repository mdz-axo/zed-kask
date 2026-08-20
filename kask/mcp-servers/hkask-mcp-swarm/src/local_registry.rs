//! Local agent registry — v2 §15 catalogue of local agent cards.
//!
//! Extracted from the swarm server root. Reads agent cards from a local
//! directory (`<id>/agent_card.json`), catalogue only — execution is Slice 9
//! (`swarm_delegate_local`). The cache distinguishes not-loaded from
//! loaded-empty via the `loaded` flag (the `.rules` trap on lazy-load caches).

use crate::error::LocalSwarmError;
use crate::port_registry::{PortRegistry, PortTypeEntry};

/// Rung 1 (Presence): reject cards missing required structural fields.
/// Each clause is falsifiable — see `test_presence_rejects_*`.
/// Does NOT reject empty `accepts`/`produces` — that is the typing rung
/// (Rung 2). Presence is about required structural fields, not port labels.
pub fn validate_presence(card: &LocalAgentCard) -> Result<(), LocalSwarmError> {
    if card.agent_id.trim().is_empty() {
        return Err(LocalSwarmError::InvalidInput(
            "agent_id must be non-empty".to_string(),
        ));
    }
    if card.agent_type.trim().is_empty() {
        return Err(LocalSwarmError::InvalidInput(
            "agent_type must be non-empty".to_string(),
        ));
    }
    if card
        .capabilities
        .system_prompt
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .is_empty()
    {
        return Err(LocalSwarmError::InvalidInput(
            "system_prompt must be non-empty".to_string(),
        ));
    }
    Ok(())
}

/// Rung 2 (Typing): every `accepts`/`produces` label must resolve to a
/// registered type in `PortRegistry`. Unresolved labels are rejected at
/// admission — the paper's "499 labels that match nothing" finding,
/// prevented by construction. Empty `accepts`/`produces` are accepted
/// (absence ≠ contradiction, paper Rule 5.3).
pub fn validate_typing(
    card: &LocalAgentCard,
    registry: &PortRegistry,
) -> Result<(), LocalSwarmError> {
    for label in card.accepts.iter().chain(card.produces.iter()) {
        if !registry.resolves(label) {
            return Err(LocalSwarmError::InvalidInput(format!(
                "agent '{}': port label '{}' does not resolve to a registered type. \
                 Valid built-in labels are {:?}. Update the card's accepts/produces to use \
                 one of these, or clone the card from ABW to import its labels.",
                card.agent_id,
                label,
                crate::port_registry::BUILTIN_PORT_TYPES
            )));
        }
    }
    Ok(())
}

/// A local agent card — the minimal subset of fermi's `AgentCard` we need for
/// catalogue + future execution. Mirrors the JSON shape in
/// `agents/local/curated/<id>/agent_card.json`.
///
/// The `cloud_swarm_id` field tracks the sync link to an ABW agent: when present,
/// the agent is `synced` (exists both locally and on ABW). When absent,
/// the agent is `local` only. The operator sets `cloud_swarm_id` when cloning an
/// ABW agent to local (Slice 11).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct LocalAgentCard {
    pub agent_id: String,
    pub agent_type: String,
    pub description: String,
    /// Human-readable label for UI display. Cloned cards set this to the
    /// cloud agent's display name (e.g. "Xaman Ek (Clone)") so the local
    /// row is distinguishable from the cloud original. Locally-created cards
    /// set this to the operator-supplied name; the panel falls back to
    /// `agent_id` when empty.
    #[serde(default)]
    pub display_name: String,
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
    #[serde(default, rename = "cloud_id")]
    pub cloud_swarm_id: Option<String>,
    /// Tags for local catalogue discovery. `#[serde(default)]` so existing
    /// cards without this field still deserialize.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Visibility level ("public", "private", "unlisted"). Default "private".
    #[serde(default)]
    pub visibility: String,
    /// Sample queries (fermi `has_sample_queries`) — one per entry.
    /// `#[serde(default)]` so existing cards still deserialize.
    #[serde(default)]
    pub sample_queries: Vec<String>,
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
    /// (e.g. `"research/web_search"`). `swarm_delegate_local` declares
    /// these to the model and dispatches tool calls through the zed IPC
    /// bridge's governed `McpRuntime` — the allowlist IS the enforcement:
    /// a call for a tool not listed here is never dispatched.
    #[serde(default)]
    pub mcp_tools: Vec<String>,
    /// Skill ids this agent declares. Carried through create/clone/push.
    /// Skills are available to the agent via the `skill` tool at runtime.
    #[serde(default)]
    pub skills: Vec<String>,
    /// Optional output contract for the agent's structured output.
    #[serde(default)]
    pub output_contract: Option<serde_json::Value>,
}

/// Name of the persisted port-type extension file inside the agents dir.
/// Cloned cards import their (ABW-catalogue) port labels here so the typing
/// gate resolves them on this and every subsequent load.
pub(crate) const PORT_TYPES_FILE: &str = "port_types.json";

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
    port_registry: PortRegistry,
    /// File-backed port-type extensions imported from cloned cards, persisted
    /// as `port_types.json` next to the agent cards.
    port_extensions: std::sync::Mutex<std::collections::HashMap<String, PortTypeEntry>>,
}

impl LocalAgentRegistry {
    /// Construct without loading. Call `load` to populate.
    /// Uses the built-in `PortRegistry` seed for port-label typing checks.
    pub fn new(dir: impl Into<String>) -> Self {
        Self {
            dir: dir.into(),
            cards: std::sync::Mutex::new(None),
            port_registry: PortRegistry::builtin(),
            port_extensions: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// The effective port registry: built-in seed plus file-backed
    /// extensions. Built from scratch per call and returned by value so
    /// import-time labels are visible to every caller (validation, metrics,
    /// output-schema checks) without shared mutability.
    pub fn port_registry(&self) -> PortRegistry {
        let mut merged = self.port_registry.clone();
        let extensions = self.port_extensions.lock().unwrap();
        merged.merge_entries(&extensions);
        merged
    }

    /// Absolute path of the persistence extension file.
    fn port_types_path(&self) -> std::path::PathBuf {
        std::path::Path::new(&self.dir).join(PORT_TYPES_FILE)
    }

    /// (Re)load the file-backed port-type extensions into the in-memory
    /// map. A missing file means "no imports yet" — an empty map, no error.
    /// A corrupt file keeps the previous map and warns (one bad extension
    /// file must not silently blank imported labels).
    fn load_port_extensions(&self) {
        let path = self.port_types_path();
        match std::fs::read_to_string(&path) {
            Ok(json) => {
                match serde_json::from_str::<std::collections::HashMap<String, PortTypeEntry>>(
                    &json,
                ) {
                    Ok(map) => {
                        *self.port_extensions.lock().unwrap() = map;
                    }
                    Err(e) => {
                        tracing::warn!(
                            target: "hkask.mcp.swarm",
                            path = %path.display(),
                            %e,
                            "failed to parse port_types.json — imported port labels may not resolve"
                        );
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                *self.port_extensions.lock().unwrap() = std::collections::HashMap::new();
            }
            Err(e) => {
                tracing::warn!(
                    target: "hkask.mcp.swarm",
                    path = %path.display(),
                    %e,
                    "failed to read port type extensions — imported types may not resolve"
                );
            }
        }
    }

    /// Admit port labels imported with a third-party (cloned ABW) card:
    /// labels that do not already resolve are registered as extension types
    /// and persisted to `<dir>/port_types.json`, so the card passes the
    /// typing gate now and on every future load. Locally-authored cards do
    /// NOT use this path — a hand-written label still requires an explicit
    /// registry entry. First import on a fresh data root creates the agents
    /// directory. Returns once all labels resolve (idempotent).
    pub fn promote_imported_port_types(&self, labels: &[String]) -> Result<(), LocalSwarmError> {
        let fresh: Vec<String> = {
            let merged = self.port_registry();
            labels
                .iter()
                .filter(|label| !merged.resolves(label))
                .cloned()
                .collect()
        };
        if fresh.is_empty() {
            return Ok(());
        }
        let dir = std::path::Path::new(&self.dir);
        // Mirror `write_card`: a fresh data root has no registry dir yet and
        // the first import must create it.
        std::fs::create_dir_all(dir).map_err(|e| {
            LocalSwarmError::Io(format!(
                "failed to create local agents dir '{}': {e}",
                dir.display()
            ))
        })?;
        let mut extensions = self.port_extensions.lock().unwrap();
        for label in &fresh {
            extensions.insert(label.clone(), PortTypeEntry::default());
        }
        let json = serde_json::to_string_pretty(&*extensions).map_err(|e| {
            LocalSwarmError::InvalidInput(format!("failed to serialize port type extensions: {e}"))
        })?;
        let path = dir.join(PORT_TYPES_FILE);
        // Temp + rename so a crashed write cannot leave a truncated file
        // that blanks every imported label at the next load.
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json)
            .map_err(|e| LocalSwarmError::Io(format!("failed to write {}: {e}", tmp.display())))?;
        std::fs::rename(&tmp, &path).map_err(|e| {
            LocalSwarmError::Io(format!("failed to persist {}: {e}", path.display()))
        })?;
        Ok(())
    }

    /// Load (or reload) agent cards from the directory. Returns the number of
    /// cards loaded. A missing directory yields zero cards (not an error) —
    /// the startup warning in `SwarmConfig::from_env` covers this case.
    pub fn load(&self) -> Result<usize, LocalSwarmError> {
        let path = std::path::Path::new(&self.dir);
        // Refresh file-backed port types first: imported labels must be in
        // the effective registry before any card's typing check runs.
        self.load_port_extensions();
        if !path.exists() {
            *self.cards.lock().unwrap() = Some(Vec::new());
            return Ok(0);
        }
        let registry = self.port_registry();
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
            // Rung 1 (Presence) + Rung 2 (Typing): validate at admission.
            // A card that fails presence or typing is skipped with a warning
            // rather than failing the entire load — one bad card must not
            // blank the registry. The warning is the paper's Rule 5.3:
            // silence is not a verdict.
            if let Err(e) = validate_presence(&card) {
                tracing::warn!(
                    target: "hkask.mcp.swarm",
                    card_path = %card_path.display(),
                    %e,
                    "skipping agent card: presence check failed"
                );
                continue;
            }
            if let Err(e) = validate_typing(&card, &registry) {
                tracing::warn!(
                    target: "hkask.mcp.swarm",
                    card_path = %card_path.display(),
                    %e,
                    "skipping agent card: typing check failed"
                );
                continue;
            }
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
    /// Produce a clone-specific agent id that does not collide with any card
    /// already in the registry. The base is the cloud agent's sanitized id;
    /// the result appends `-clone`, then `-clone-2`, `-clone-3`, etc. on
    /// collision. Checks the in-memory cache (loaded once at entry) rather
    /// than re-reading disk per iteration — a collision loop must not do N
    /// disk reads. The caller is responsible for sanitizing the base id
    /// before passing it here.
    pub fn unique_clone_id(&self, base: &str) -> String {
        // Load once so the cache is fresh for all collision checks below.
        if let Err(e) = self.load() {
            tracing::warn!(
                target: "hkask.mcp.swarm",
                "local registry reload failed during clone-id allocation (proceeding with cached cards): {e}"
            );
        }
        let cards = self.cards.lock().unwrap();
        let taken = |candidate: &str| -> bool {
            cards
                .as_ref()
                .map(|cs| cs.iter().any(|c| c.agent_id == candidate))
                .unwrap_or(false)
        };
        let first = format!("{base}-clone");
        if !taken(&first) {
            return first;
        }
        let mut suffix = 2;
        loop {
            let candidate = format!("{first}-{suffix}");
            if !taken(&candidate) {
                return candidate;
            }
            suffix += 1;
        }
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
        // Rung 1 (Presence) + Rung 2 (Typing): validate before writing.
        // Unlike `load` (which skips bad cards), `write_card` rejects —
        // a programmatic write should fail loudly rather than silently
        // writing a card that will be skipped on the next load.
        self.load_port_extensions();
        let registry = self.port_registry();
        validate_presence(card)?;
        validate_typing(card, &registry)?;
        let safe_id = crate::sanitize::sanitize_agent_id(&card.agent_id).ok_or_else(|| {
            LocalSwarmError::Sanitize(format!(
                "agent_id '{}' contains no safe characters (alphanumeric, dash, underscore, dot)",
                card.agent_id
            ))
        })?;
        let registry_root = std::path::Path::new(&self.dir);
        // A fresh data root has no registry dir yet — `load` reads a missing
        // root as "empty", but a first write must create it. Without this,
        // the canonicalize below fails on the first clone/create and the
        // operator sees a permanently-empty local agents list (broken
        // feedback loop: the dir only exists once a write succeeds).
        std::fs::create_dir_all(registry_root).map_err(|e| {
            LocalSwarmError::Io(format!(
                "failed to create local agents dir '{}': {e}",
                registry_root.display()
            ))
        })?;
        let registry_root = registry_root.canonicalize().map_err(|e| {
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
