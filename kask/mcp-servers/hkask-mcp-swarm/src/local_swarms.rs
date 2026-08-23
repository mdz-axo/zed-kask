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
    /// The ABW workspace id this local swarm is synced with. `None` =
    /// local-only. Set by `swarm_push_local_swarm` (after ABW workspace
    /// creation) and `swarm_pull_swarm_to_local` (when copying from ABW).
    /// Backward-compatible: existing `swarm.json` files without this field
    /// deserialize with `None`.
    #[serde(default)]
    pub cloud_workspace_id: Option<String>,
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
            member_sources: members
                .iter()
                .map(|id| MemberSource {
                    agent_id: id.clone(),
                    source: "curated_seed".to_string(),
                    added_at: chrono::Utc::now().to_rfc3339(),
                })
                .collect(),
            members,
            created_at: chrono::Utc::now().to_rfc3339(),
            cloud_workspace_id: None,
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

    /// Update a local swarm's display name and mission in place. The
    /// `swarm_id` (on-disk directory name) is **not** changed — it is the
    /// stable identity other systems (Steer conversation, ledger) reference,
    /// and renaming the directory would orphan those references. Only the
    /// human-readable `name` and `mission` fields are updated. Errors if the
    /// swarm does not exist or `name` is empty. The local counterpart of an
    /// ABW metadata-edit endpoint (ABW has none — `PATCH /workspaces/{id}` is
    /// 405, verified live 2026-08-13).
    pub fn update_metadata(
        &self,
        swarm_id: &str,
        name: &str,
        mission: &str,
    ) -> Result<LocalSwarm, LocalSwarmError> {
        if name.trim().is_empty() {
            return Err(LocalSwarmError::InvalidInput(
                "swarm name must be non-empty".to_string(),
            ));
        }
        let mut swarm = self.get(swarm_id).ok_or_else(|| {
            LocalSwarmError::NotFound(format!("local swarm '{swarm_id}' not found"))
        })?;
        swarm.name = name.trim().to_string();
        swarm.mission = mission.to_string();
        self.write_swarm(&swarm)?;
        Ok(swarm)
    }

    /// Clone a local swarm: create a new swarm with a fresh slug id, copying
    /// the source's mission and roster. The new `name` is the source name
    /// suffixed with " (copy)" so the operator can distinguish the duplicate
    /// in the browse list; the caller may rename it via `update_metadata`.
    ///
    /// Member ids are copied as-is — the roster is ids, and resolution happens
    /// at delegation time (mirrors ABW workspaces). An id whose card no longer
    /// exists in `LocalAgentRegistry` is **not** an error here: the clone
    /// preserves the roster faithfully, and the operator sees the gap at
    /// delegation time, not as a silent drop during clone. This satisfies the
    /// "handle agents that no longer exist: skip or tombstone, do not panic"
    /// requirement — we preserve (tombstone-by-identity) rather than panic.
    ///
    /// Errors if the source swarm does not exist.
    pub fn clone_swarm(&self, swarm_id: &str) -> Result<LocalSwarm, LocalSwarmError> {
        let source = self.get(swarm_id).ok_or_else(|| {
            LocalSwarmError::NotFound(format!("local swarm '{swarm_id}' not found"))
        })?;
        let new_name = format!("{} (copy)", source.name);
        self.create(&new_name, &source.mission, source.members.clone())
    }

    /// Record the ABW workspace id this local swarm is synced with. Called
    /// after `swarm_push_local_swarm` (ABW workspace created) or
    /// `swarm_pull_swarm_to_local` (copied from ABW). Passing `None` clears
    /// the link. Errors if the swarm does not exist.
    pub fn set_cloud_workspace_id(
        &self,
        swarm_id: &str,
        cloud_workspace_id: Option<String>,
    ) -> Result<LocalSwarm, LocalSwarmError> {
        let mut swarm = self.get(swarm_id).ok_or_else(|| {
            LocalSwarmError::NotFound(format!("local swarm '{swarm_id}' not found"))
        })?;
        swarm.cloud_workspace_id = cloud_workspace_id;
        self.write_swarm(&swarm)?;
        Ok(swarm)
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
        let root = std::path::Path::new(&self.dir);
        // A fresh data root has no registry dir yet — a first write must
        // create it, mirroring `LocalAgentRegistry::write_card`. Without
        // this, the canonicalize below fails on the first create and a
        // fresh operator never gets a local swarm.
        std::fs::create_dir_all(root).map_err(|e| {
            LocalSwarmError::Io(format!(
                "failed to create local swarms dir '{}': {e}",
                root.display()
            ))
        })?;
        let root = root.canonicalize().map_err(|e| {
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

    fn tmp_registry() -> LocalSwarmRegistry {
        let dir = std::env::temp_dir().join(format!(
            "hkask-swarm-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        LocalSwarmRegistry::new(dir.to_string_lossy().to_string())
    }

    #[test]
    fn update_metadata_changes_name_and_mission_preserving_id() {
        let registry = tmp_registry();
        let swarm = registry
            .create("Alpha Team", "original mission", vec!["a1".into()])
            .unwrap();
        let id = swarm.swarm_id;

        let updated = registry
            .update_metadata(&id, "Beta Team", "new mission")
            .unwrap();

        assert_eq!(updated.swarm_id, id, "swarm_id must be preserved");
        assert_eq!(updated.name, "Beta Team");
        assert_eq!(updated.mission, "new mission");
        assert_eq!(updated.members, vec!["a1".to_string()], "roster preserved");

        // The change persists across a reload (it is on disk, not in-memory).
        let reloaded = registry.get(&id).unwrap();
        assert_eq!(reloaded.name, "Beta Team");
        assert_eq!(reloaded.mission, "new mission");
    }

    #[test]
    fn update_metadata_rejects_empty_name() {
        let registry = tmp_registry();
        let swarm = registry.create("Gamma", "m", vec![]).unwrap();
        let err = registry.update_metadata(&swarm.swarm_id, "   ", "m").unwrap_err();
        assert!(matches!(err, LocalSwarmError::InvalidInput(_)));
    }

    #[test]
    fn update_metadata_errors_on_missing_swarm() {
        let registry = tmp_registry();
        let err = registry.update_metadata("nope", "Name", "m").unwrap_err();
        assert!(matches!(err, LocalSwarmError::NotFound(_)));
    }

    #[test]
    fn clone_swarm_copies_mission_and_roster_with_fresh_id() {
        let registry = tmp_registry();
        let source = registry
            .create("Delta", "recon mission", vec!["d1".into(), "d2".into()])
            .unwrap();

        let clone = registry.clone_swarm(&source.swarm_id).unwrap();

        assert_ne!(clone.swarm_id, source.swarm_id, "clone gets a fresh id");
        assert_eq!(clone.name, "Delta (copy)");
        assert_eq!(clone.mission, "recon mission");
        assert_eq!(clone.members, vec!["d1".to_string(), "d2".to_string()]);
        // Both swarms coexist on disk.
        assert!(registry.get(&source.swarm_id).is_some());
        assert!(registry.get(&clone.swarm_id).is_some());
    }

    #[test]
    fn clone_swarm_preserves_members_that_no_longer_exist() {
        // The roster is ids — a member whose card was deleted must not cause a
        // panic or silent drop during clone. The id is preserved faithfully.
        let registry = tmp_registry();
        let source = registry
            .create("Epsilon", "m", vec!["ghost_agent".into()])
            .unwrap();
        let clone = registry.clone_swarm(&source.swarm_id).unwrap();
        assert_eq!(clone.members, vec!["ghost_agent".to_string()]);
    }

    #[test]
    fn clone_swarm_errors_on_missing_source() {
        let registry = tmp_registry();
        let err = registry.clone_swarm("nope").unwrap_err();
        assert!(matches!(err, LocalSwarmError::NotFound(_)));
    }

    #[test]
    fn cloud_workspace_id_defaults_to_none_on_existing_swarm_json() {
        // A freshly created swarm has no cloud link.
        let registry = tmp_registry();
        let swarm = registry.create("Zeta", "m", vec![]).unwrap();
        assert!(swarm.cloud_workspace_id.is_none());
    }

    #[test]
    fn set_cloud_workspace_id_persists_across_reload() {
        let registry = tmp_registry();
        let swarm = registry.create("Eta", "m", vec!["a1".into()]).unwrap();

        let updated = registry
            .set_cloud_workspace_id(&swarm.swarm_id, Some("ws_abcd".into()))
            .unwrap();
        assert_eq!(updated.cloud_workspace_id.as_deref(), Some("ws_abcd"));

        // Persists on disk.
        let reloaded = registry.get(&swarm.swarm_id).unwrap();
        assert_eq!(reloaded.cloud_workspace_id.as_deref(), Some("ws_abcd"));
    }

    #[test]
    fn set_cloud_workspace_id_can_clear_link() {
        let registry = tmp_registry();
        let swarm = registry.create("Theta", "m", vec![]).unwrap();
        registry
            .set_cloud_workspace_id(&swarm.swarm_id, Some("ws_xyz".into()))
            .unwrap();
        let cleared = registry
            .set_cloud_workspace_id(&swarm.swarm_id, None)
            .unwrap();
        assert!(cleared.cloud_workspace_id.is_none());
    }
}
