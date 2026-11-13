//! The scenario project record's file-backed store (PR-01, PR-07).
//!
//! One `projects.json` snapshot published by atomic rename (the
//! `ForecastStore` compaction pattern: synced same-directory tempfile,
//! rename, then directory sync on Unix). Projects are low-volume by
//! design — created once per subject, updated occasionally — so a
//! per-write full snapshot needs no journal. A snapshot that cannot be read
//! back leaves the store unloaded (`load_error`) and every write refused:
//! the next snapshot would replace the file and drop every project in it.

use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use super::store::{unloaded_error, unloaded_reason};
use crate::types::{EventTree, ScenarioProject};

/// File-backed store for scenario project records.
#[derive(Debug, Default)]
pub struct ProjectStore {
    projects: HashMap<String, ScenarioProject>,
    snapshot_path: Option<PathBuf>,
    /// Set when the snapshot exists and did not load.
    load_error: Option<String>,
}

impl ProjectStore {
    /// Create a store, loading the snapshot from disk when present.
    /// `None` is the in-memory mode (tests, or a data-dir-less run).
    pub fn new(snapshot_path: Option<PathBuf>) -> Self {
        let mut store = Self {
            projects: HashMap::new(),
            snapshot_path,
            load_error: None,
        };
        store.load();
        store
    }

    fn load(&mut self) {
        let Some(path) = self.snapshot_path.clone() else {
            return;
        };
        if !path.exists() {
            return;
        }
        let loaded = fs::read_to_string(&path)
            .map_err(|error| format!("cannot be read ({error})"))
            .and_then(|data| {
                serde_json::from_str::<HashMap<String, ScenarioProject>>(&data)
                    .map_err(|error| format!("does not parse ({error})"))
            });
        match loaded {
            Ok(projects) => self.projects = projects,
            Err(what) => {
                let reason = unloaded_reason(&path, &what);
                tracing::warn!(target: "hkask.mcp.scenarios", %reason, "Project store did not load");
                self.load_error = Some(reason);
            }
        }
    }

    /// Why the snapshot did not load, when it did not. While this is set the
    /// store holds no projects and every write is refused.
    pub fn load_error(&self) -> Option<&str> {
        self.load_error.as_deref()
    }

    /// Look up a project by id.
    pub(crate) fn get(&self, project_id: &str) -> Option<&ScenarioProject> {
        self.projects.get(project_id)
    }

    /// Insert or update a project and publish the snapshot atomically.
    pub(crate) fn upsert(&mut self, project: ScenarioProject) -> io::Result<()> {
        if let Some(reason) = &self.load_error {
            return Err(unloaded_error(reason));
        }
        self.projects.insert(project.project_id.clone(), project);
        self.persist()
    }

    /// The most recently stored tree across all projects: the tree
    /// `scenario_status` shows, in this session and after a restart. Trees
    /// stored before `last_tree_at` existed order by their project's
    /// `updated_at` date, then by project id.
    pub fn last_tree(&self) -> Option<EventTree> {
        self.projects
            .values()
            .filter(|project| project.last_tree.is_some())
            .max_by(|a, b| {
                (a.last_tree_at, a.updated_at, &a.project_id).cmp(&(
                    b.last_tree_at,
                    b.updated_at,
                    &b.project_id,
                ))
            })
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
