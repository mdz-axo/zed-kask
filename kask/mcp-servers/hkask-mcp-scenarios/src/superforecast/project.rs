//! The scenario project record's file-backed store (PR-01, PR-07).
//!
//! One `projects.json` snapshot published by atomic rename (the
//! `ForecastStore` compaction pattern: synced same-directory tempfile,
//! rename, then directory sync on Unix). Projects are low-volume by
//! design — created once per subject, updated occasionally — so a
//! per-write full snapshot needs no journal. A corrupt or unreadable
//! snapshot degrades to an empty store with a warning, never a silent
//! partial load.

use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::types::{EventTree, ScenarioProject};

/// File-backed store for scenario project records.
#[derive(Debug, Default)]
pub struct ProjectStore {
    projects: HashMap<String, ScenarioProject>,
    snapshot_path: Option<PathBuf>,
    /// The most recently updated project — the durable tree cache's key
    /// (PR-07): `contract_price_coherence`'s `tree_implied` default reads
    /// the last-updated project's tree after a restart.
    last_project_id: Option<String>,
}

impl ProjectStore {
    /// Create a store, loading the snapshot from disk when present.
    /// `None` is the in-memory mode (tests, or a data-dir-less run).
    pub fn new(snapshot_path: Option<PathBuf>) -> Self {
        let mut store = Self {
            projects: HashMap::new(),
            snapshot_path,
            last_project_id: None,
        };
        store.load();
        store
    }

    /// Load: a corrupt or unreadable snapshot degrades to an empty store
    /// with a warning — the same posture as `ForecastStore::load`, so a
    /// data failure is never indistinguishable from "no projects yet".
    fn load(&mut self) {
        let Some(path) = &self.snapshot_path else {
            return;
        };
        if !path.exists() {
            return;
        }
        match fs::read_to_string(path) {
            Ok(data) => match serde_json::from_str::<HashMap<String, ScenarioProject>>(&data) {
                Ok(projects) => {
                    // JSON object order is not load-bearing; recover the
                    // last-updated pointer from the records' own
                    // `updated_at` timestamps.
                    self.last_project_id = projects
                        .values()
                        .max_by_key(|project| project.updated_at)
                        .map(|project| project.project_id.clone());
                    self.projects = projects;
                }
                Err(error) => tracing::warn!(
                    target: "hkask.mcp.scenarios",
                    %error,
                    "Project snapshot is corrupt — starting from an empty project store; \
                     persisted framing documents and trees are unavailable this session"
                ),
            },
            Err(error) => tracing::warn!(
                target: "hkask.mcp.scenarios",
                %error,
                "Project snapshot is unreadable — starting from an empty project store"
            ),
        }
    }

    /// Look up a project by id.
    pub(crate) fn get(&self, project_id: &str) -> Option<&ScenarioProject> {
        self.projects.get(project_id)
    }

    /// Insert or update a project and publish the snapshot atomically.
    /// Sets the store's last-updated pointer — the durable tree cache.
    pub(crate) fn upsert(&mut self, project: ScenarioProject) -> io::Result<()> {
        self.last_project_id = Some(project.project_id.clone());
        self.projects.insert(project.project_id.clone(), project);
        self.persist()
    }

    /// The last-updated project's tree — the durable tree cache (PR-07).
    /// `None` when no project carries a tree yet.
    pub fn last_tree(&self) -> Option<EventTree> {
        self.last_project_id
            .as_ref()
            .and_then(|id| self.projects.get(id))
            .and_then(|project| project.last_tree.clone())
    }

    /// Publish the full snapshot: synced same-directory tempfile, atomic
    /// rename, then directory sync on Unix — the `ForecastStore::compact`
    /// ordering. A failed write leaves the previous snapshot readable.
    fn persist(&self) -> io::Result<()> {
        let Some(path) = &self.snapshot_path else {
            return Ok(());
        };
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        let data = serde_json::to_vec_pretty(&self.projects)?;
        let mut snapshot = tempfile::NamedTempFile::new_in(parent)?;
        snapshot.write_all(&data)?;
        snapshot.as_file().sync_all()?;
        snapshot.persist(path).map_err(|error| error.error)?;
        #[cfg(unix)]
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    }
}
