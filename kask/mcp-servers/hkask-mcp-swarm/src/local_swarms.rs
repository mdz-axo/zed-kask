//! Local swarm registry — the local replica of an ABW workspace/team.
//!
//! A local swarm is a named grouping of local agent ids: `{swarm_id, name,
//! mission, members}`. It mirrors an ABW workspace (created via
//! `swarm_create_swarm`'s `POST /teams`) but lives on disk, costs nothing, and
//! has no consent gate. Local delegation has no funding gate either — the
//! ledger records spend rather than authorizing it. Membership is just roster edits; agents themselves stay in
//! `LocalAgentRegistry`.
//!
//! Persistence mirrors `LocalAgentRegistry`: one JSON file per swarm under
//! `<dir>/<swarm_id>/swarm.json`, reloaded from disk on every read so
//! operator/external edits appear without a server restart. The cache
//! distinguishes not-loaded from loaded-empty via the `loaded` flag (the
//! `.rules` trap on lazy-load caches).

use std::sync::Mutex;

use crate::error::LocalSwarmError;

/// A local swarm — the local replica of an ABW workspace.
///
/// `swarm_id` is a path-safe slug generated from `name` (see `make_swarm_slug`);
/// it is the on-disk directory name and the stable identity returned to callers.
/// `members` are `LocalAgentCard::agent_id` values — adding an agent that does
/// not exist in the registry is allowed (the roster is just ids; resolution to
/// a card happens at delegation time, mirroring how ABW workspaces carry agent
/// ids that may not yet be hired).
///
/// `member_sources` tracks the provenance of each member's addition (Gap 6
/// fix — the ecology view needs a provenance signal without the full RBAC
/// machinery). Backward-compatible: existing `swarm.json` files without this
/// field deserialize with an empty vec; members added via `add_member` get a
/// `MemberSource` entry with `source = "operator"`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LocalSwarm {
    pub swarm_id: String,
    pub name: String,
    #[serde(default)]
    pub mission: String,
    #[serde(default)]
    pub members: Vec<String>,
    /// Provenance for each member's addition. Aligned with `members` by
    /// `agent_id` — a member without a `MemberSource` entry has unknown
    /// provenance (backward compat with pre-Gap-6 `swarm.json` files).
    #[serde(default)]
    pub member_sources: Vec<MemberSource>,
    #[serde(default)]
    pub created_at: String,
}

/// The provenance of a member's addition to a local swarm. Mirrors fermi's
/// `membership_source` field (`approved` / `curated_seed` / `admin_grant`) but
/// without the RBAC machinery — local mode has no multi-tenant substrate to
/// gate. The ecology view can color members by source without a full RBAC
/// surface.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemberSource {
    pub agent_id: String,
    /// How this member was added: `operator` (manual add), `curated_seed`
    /// (loaded from a seed file), `swarm_intelligence` (added by the
    /// Curator's composition decision), `clone` (cloned from ABW). Default
    /// `operator`.
    pub source: String,
    /// RFC 3339 timestamp of when the member was added.
    pub added_at: String,
}

/// Reads/writes local swarms from a local directory.
///
/// Directory layout:
/// ```text
/// agents/local/swarms/
///   market_team/
///     swarm.json
///   research_pool/
///     swarm.json
/// ```
///
/// A missing directory yields zero swarms (not an error) — the startup warning
/// in `SwarmConfig::from_env` covers the missing-dir case for local mode.
pub struct LocalSwarmRegistry {
    dir: String,
    swarms: Mutex<Option<Vec<LocalSwarm>>>,
}

impl LocalSwarmRegistry {
    /// Construct without loading. Call `load` to populate.
    pub fn new(dir: impl Into<String>) -> Self {
        Self {
            dir: dir.into(),
            swarms: Mutex::new(None),
        }
    }

    /// Load (or reload) swarms from the directory. Returns the count loaded. A
    /// missing directory yields zero swarms (not an error). A malformed
    /// `swarm.json` aborts the load and returns `Err` — a corrupt roster must
    /// not be silently dropped (the operator should see which file failed).
    pub fn load(&self) -> Result<usize, LocalSwarmError> {
        let path = std::path::Path::new(&self.dir);
        if !path.exists() {
            *self.swarms.lock().unwrap() = Some(Vec::new());
            return Ok(0);
        }
        let mut swarms = Vec::new();
        let entries = std::fs::read_dir(path).map_err(|e| {
            LocalSwarmError::Io(format!(
                "failed to read local swarms dir '{}': {e}",
                self.dir
            ))
        })?;
        for entry in entries {
            let entry =
                entry.map_err(|e| LocalSwarmError::Io(format!("readdir entry error: {e}")))?;
            let swarm_path = entry.path().join("swarm.json");
            if !swarm_path.exists() {
                continue;
            }
            let json = std::fs::read_to_string(&swarm_path).map_err(|e| {
                LocalSwarmError::Io(format!("failed to read {}: {e}", swarm_path.display()))
            })?;
            let swarm: LocalSwarm = serde_json::from_str(&json).map_err(|e| {
                LocalSwarmError::InvalidInput(format!(
                    "failed to parse {}: {e}",
                    swarm_path.display()
                ))
            })?;
            swarms.push(swarm);
        }
        swarms.sort_by(|a, b| a.swarm_id.cmp(&b.swarm_id));
        let count = swarms.len();
        *self.swarms.lock().unwrap() = Some(swarms);
        Ok(count)
    }

    /// List all swarms, reloading from disk first. Returns an empty vec if not
    /// loaded or the directory was empty. A reload failure keeps the previous
    /// cache (logged) — a transient unreadable file must not blank the list.
    pub fn list(&self) -> Vec<LocalSwarm> {
        if let Err(e) = self.load() {
            tracing::warn!(
                target: "hkask.mcp.swarm",
                "local swarms reload failed (keeping cached swarms): {e}"
            );
        }
        self.swarms.lock().unwrap().clone().unwrap_or_default()
    }

    /// Look up a single swarm by id, reloading from disk first. Returns `None`
    /// if not loaded or not found.
    pub fn get(&self, swarm_id: &str) -> Option<LocalSwarm> {
        if let Err(e) = self.load() {
            tracing::warn!(
                target: "hkask.mcp.swarm",
                "local swarms reload failed (keeping cached swarms): {e}"
            );
        }
        self.swarms
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|swarms| swarms.iter().find(|s| s.swarm_id == swarm_id).cloned())
    }

    /// Create a new swarm with a slug id derived from `name`, optionally
    /// seeded with `members`. Returns the created swarm. Errors if `name` is
    /// empty or the directory is not writable.
    pub fn create(
        &self,
        name: &str,
        mission: &str,
        members: Vec<String>,
    ) -> Result<LocalSwarm, LocalSwarmError> {
        if name.trim().is_empty() {
            return Err(LocalSwarmError::InvalidInput(
                "swarm name must be non-empty".to_string(),
            ));
        }
        let slug_base: String = name
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect();
        let swarm_id = crate::abw_util::make_swarm_slug(&slug_base, std::time::SystemTime::now());
        let swarm = LocalSwarm {
            swarm_id,
            name: name.to_string(),
            mission: mission.to_string(),
            members,
            member_sources: members
                .iter()
                .map(|id| MemberSource {
                    agent_id: id.clone(),
                    source: "curated_seed".to_string(),
                    added_at: chrono::Utc::now().to_rfc3339(),
                })
                .collect(),
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        self.write_swarm(&swarm)?;
        Ok(swarm)
    }

    /// Add an agent id to a swarm's roster (idempotent — a duplicate add is a
    /// no-op). Errors if the swarm does not exist. Does NOT verify the agent
    /// exists in `LocalAgentRegistry` — the roster is ids; resolution happens
    /// at delegation time (mirrors ABW workspaces, which carry agent ids that
    /// may not yet be hired).
    pub fn add_member(
        &self,
        swarm_id: &str,
        agent_id: &str,
    ) -> Result<LocalSwarm, LocalSwarmError> {
        let mut swarm = self.get(swarm_id).ok_or_else(|| {
            LocalSwarmError::NotFound(format!("local swarm '{swarm_id}' not found"))
        })?;
        if agent_id.trim().is_empty() {
            return Err(LocalSwarmError::InvalidInput(
                "agent_name must be non-empty".to_string(),
            ));
        }
        if !swarm.members.iter().any(|m| m == agent_id) {
            swarm.members.push(agent_id.to_string());
            swarm.member_sources.push(MemberSource {
                agent_id: agent_id.to_string(),
                source: "operator".to_string(),
                added_at: chrono::Utc::now().to_rfc3339(),
            });
            self.write_swarm(&swarm)?;
        }
        Ok(swarm)
    }

    /// Remove an agent id from a swarm's roster (idempotent — removing a
    /// non-member is a no-op). Errors if the swarm does not exist.
    pub fn remove_member(
        &self,
        swarm_id: &str,
        agent_id: &str,
    ) -> Result<LocalSwarm, LocalSwarmError> {
        let mut swarm = self.get(swarm_id).ok_or_else(|| {
            LocalSwarmError::NotFound(format!("local swarm '{swarm_id}' not found"))
        })?;
        let before = swarm.members.len();
        swarm.members.retain(|m| m != agent_id);
        swarm.member_sources.retain(|ms| ms.agent_id != agent_id);
        if swarm.members.len() != before {
            self.write_swarm(&swarm)?;
        }
        Ok(swarm)
    }

    /// Permanently delete a swarm (its directory + roster). The member agents
    /// are NOT touched — they stay in `LocalAgentRegistry`. Errors if the
    /// swarm does not exist or the directory cannot be removed.
    pub fn delete(&self, swarm_id: &str) -> Result<(), LocalSwarmError> {
        let safe_id = crate::sanitize::sanitize_agent_id(swarm_id).ok_or_else(|| {
            LocalSwarmError::Sanitize(format!("swarm_id '{swarm_id}' contains no safe characters"))
        })?;
        let root = std::path::Path::new(&self.dir)
            .canonicalize()
            .map_err(|e| {
                LocalSwarmError::Io(format!(
                    "failed to resolve local swarms dir '{}': {e}",
                    self.dir
                ))
            })?;
        let swarm_dir = root.join(&safe_id);
        if !swarm_dir.starts_with(&root) {
            return Err(LocalSwarmError::Sanitize(
                "refusing to delete a path outside the local swarms dir".to_string(),
            ));
        }
        if !swarm_dir.exists() {
            return Err(LocalSwarmError::NotFound(format!(
                "local swarm '{swarm_id}' not found"
            )));
        }
        std::fs::remove_dir_all(&swarm_dir).map_err(|e| {
            LocalSwarmError::Io(format!("failed to remove {}: {e}", swarm_dir.display()))
        })?;
        self.load()?;
        Ok(())
    }

    /// Write (or overwrite) a swarm to `<dir>/<swarm_id>/swarm.json`, then
    /// reload so the change is visible to subsequent `list`/`get` calls. The
    /// `swarm_id` is re-sanitized before joining the registry root
    /// (path-traversal defense); the file is written under a canonicalized,
    /// path-contained directory — the same invariant
    /// `LocalAgentRegistry::write_card` pins.
    fn write_swarm(&self, swarm: &LocalSwarm) -> Result<(), LocalSwarmError> {
        let safe_id = crate::sanitize::sanitize_agent_id(&swarm.swarm_id).ok_or_else(|| {
            LocalSwarmError::Sanitize(format!(
                "swarm_id '{}' contains no safe characters (alphanumeric, dash, underscore, dot)",
                swarm.swarm_id
            ))
        })?;
        let root = std::path::Path::new(&self.dir)
            .canonicalize()
            .map_err(|e| {
                LocalSwarmError::Io(format!(
                    "failed to resolve local swarms dir '{}': {e}",
                    self.dir
                ))
            })?;
        let swarm_dir = root.join(&safe_id);
        if !swarm_dir.starts_with(&root) {
            return Err(LocalSwarmError::Sanitize(
                "refusing to write a path outside the local swarms dir".to_string(),
            ));
        }
        std::fs::create_dir_all(&swarm_dir).map_err(|e| {
            LocalSwarmError::Io(format!(
                "failed to create swarm dir {}: {e}",
                swarm_dir.display()
            ))
        })?;
        let swarm_path = swarm_dir.join("swarm.json");
        let json = serde_json::to_string_pretty(swarm).map_err(|e| {
            LocalSwarmError::InvalidInput(format!("failed to serialize swarm: {e}"))
        })?;
        std::fs::write(&swarm_path, json).map_err(|e| {
            LocalSwarmError::Io(format!("failed to write {}: {e}", swarm_path.display()))
        })?;
        self.load()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_swarms_dir(label: &str) -> String {
        let dir = std::env::temp_dir().join(format!(
            "hkask-swarm-tests-{}-{}",
            label,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.to_string_lossy().to_string()
    }

    #[test]
    fn missing_dir_loads_zero() {
        let registry = LocalSwarmRegistry::new("/nonexistent/hkask-swarm-test");
        assert_eq!(registry.load().unwrap(), 0);
        assert!(registry.list().is_empty());
    }

    #[test]
    fn create_persists_and_lists() {
        let dir = temp_swarms_dir("create");
        let registry = LocalSwarmRegistry::new(&dir);
        let swarm = registry
            .create("Research Pool", "market research", vec![])
            .unwrap();
        assert!(!swarm.swarm_id.is_empty());
        assert_eq!(swarm.name, "Research Pool");
        assert_eq!(swarm.mission, "market research");
        // Reload from a fresh registry instance to prove it hit disk.
        let registry2 = LocalSwarmRegistry::new(&dir);
        let listed = registry2.list();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].swarm_id, swarm.swarm_id);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn create_seeds_members() {
        let dir = temp_swarms_dir("seed");
        let registry = LocalSwarmRegistry::new(&dir);
        let swarm = registry
            .create(
                "Team",
                "m",
                vec!["agent_a".to_string(), "agent_b".to_string()],
            )
            .unwrap();
        assert_eq!(swarm.members, vec!["agent_a", "agent_b"]);
        // Seeded members get `curated_seed` provenance (Gap 6).
        assert_eq!(swarm.member_sources.len(), 2);
        assert!(
            swarm
                .member_sources
                .iter()
                .all(|ms| ms.source == "curated_seed")
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn add_and_remove_member_roundtrip() {
        let dir = temp_swarms_dir("addremove");
        let registry = LocalSwarmRegistry::new(&dir);
        let swarm = registry.create("Team", "m", vec![]).unwrap();
        let id = swarm.swarm_id;

        let updated = registry.add_member(&id, "agent_a").unwrap();
        assert_eq!(updated.members, vec!["agent_a"]);
        // Added members get `operator` provenance (Gap 6).
        assert_eq!(updated.member_sources.len(), 1);
        assert_eq!(updated.member_sources[0].source, "operator");

        // Idempotent add — member_sources must not duplicate.
        let updated = registry.add_member(&id, "agent_a").unwrap();
        assert_eq!(updated.members, vec!["agent_a"]);
        assert_eq!(updated.member_sources.len(), 1);

        let updated = registry.add_member(&id, "agent_b").unwrap();
        assert_eq!(updated.members, vec!["agent_a", "agent_b"]);
        assert_eq!(updated.member_sources.len(), 2);

        let updated = registry.remove_member(&id, "agent_a").unwrap();
        assert_eq!(updated.members, vec!["agent_b"]);
        // member_sources must be pruned in sync.
        assert_eq!(updated.member_sources.len(), 1);
        assert_eq!(updated.member_sources[0].agent_id, "agent_b");

        // Idempotent remove.
        let updated = registry.remove_member(&id, "agent_a").unwrap();
        assert_eq!(updated.members, vec!["agent_b"]);
        assert_eq!(updated.member_sources.len(), 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn add_member_unknown_swarm_errors() {
        let dir = temp_swarms_dir("unknown");
        let registry = LocalSwarmRegistry::new(&dir);
        assert!(registry.add_member("nope", "agent_a").is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn delete_removes_swarm_keeps_others() {
        let dir = temp_swarms_dir("delete");
        let registry = LocalSwarmRegistry::new(&dir);
        let a = registry.create("A", "m", vec![]).unwrap();
        let b = registry.create("B", "m", vec![]).unwrap();
        registry.delete(&a.swarm_id).unwrap();
        let remaining: Vec<_> = registry.list();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].swarm_id, b.swarm_id);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn delete_unknown_swarm_errors() {
        let dir = temp_swarms_dir("delunknown");
        let registry = LocalSwarmRegistry::new(&dir);
        assert!(registry.delete("nope").is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn legacy_swarm_json_without_member_sources_deserializes() {
        // Backward compat: a `swarm.json` written before Gap 6 (no
        // `member_sources` field) must deserialize with an empty vec,
        // not fail. The `#[serde(default)]` on `member_sources` is what
        // makes this work.
        let dir = temp_swarms_dir("legacy");
        let swarm_dir = std::path::Path::new(&dir).join("legacy_swarm");
        std::fs::create_dir_all(&swarm_dir).unwrap();
        let legacy_json = r#"{"swarm_id":"legacy_swarm","name":"Legacy","mission":"old","members":["agent_a"],"created_at":"2026-01-01T00:00:00Z"}"#;
        std::fs::write(swarm_dir.join("swarm.json"), legacy_json).unwrap();
        let registry = LocalSwarmRegistry::new(&dir);
        let swarms = registry.list();
        assert_eq!(swarms.len(), 1);
        assert_eq!(swarms[0].members, vec!["agent_a"]);
        assert!(
            swarms[0].member_sources.is_empty(),
            "legacy swarm must deserialize with empty member_sources"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
