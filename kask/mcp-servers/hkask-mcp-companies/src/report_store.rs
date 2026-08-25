//! Report & screen persistence for the companies MCP server.
//!
//! Stores JSON artifacts produced by tools (`expectations_gap`,
//! `company_screener`, `stock_universe`) and by skills (company-research-flash,
//! company-research-deep) under the companies server's subtree in the kask
//! data root, per the Standardized Artifact Storage layout:
//!
//! - Screens  → `{data_dir}/mcp/companies/screens/{screen_name}.json`
//! - Reports → `{data_dir}/mcp/companies/reports/{report_name}.json`
//!
//! The store is fail-soft: a write failure (read-only data dir, full disk,
//! permissions) surfaces as an `Err` the tool propagates to the agent, never
//! a silent discard (per .rules: no `let _ =` on fallible operations).

use std::path::PathBuf;

/// The artifact kind — selects the subdirectory under `mcp/companies/`.
#[derive(Debug, Clone, Copy)]
pub enum ArtifactKind {
    Screen,
    Report,
}

impl ArtifactKind {
    fn subdir(self) -> &'static str {
        match self {
            ArtifactKind::Screen => "screens",
            ArtifactKind::Report => "reports",
        }
    }
}

/// Persists JSON artifacts under `mcp/companies/{kind}/` in the kask data root.
///
/// The store resolves its root once at construction (from `HKASK_DATA_DIR` →
/// XDG → HOME via `resolve_under_data_dir`), then writes each artifact as a
/// pretty-printed JSON file named `{name}.json`. Names are sanitized to
/// prevent path traversal — a `..` or `/` in the name is rejected, not
/// silently rewritten.
///
/// Writes are atomic (temp file + rename) so a crashed `save` never leaves a
/// truncated artifact — a reader either sees the previous version or the new
/// one, never a partial write. The MCP server serializes tool calls per
/// session, so no mutex is needed for concurrency.
pub struct ReportStore {
    root: PathBuf,
}

impl ReportStore {
    /// Resolve the store root from the kask data dir.
    ///
    /// Root is `{data_dir}/mcp/companies/`. The `screens/` and `reports/`
    /// subdirectories are created lazily on first write, not at construction —
    /// a read-only data dir must not abort server startup, only surface when
    /// a write is actually attempted.
    pub fn new() -> Self {
        Self::with_root(hkask_types::agent_paths::resolve_under_data_dir(
            &hkask_types::agent_paths::mcp_server_subdir("companies", ""),
        ))
    }

    /// Construct with an explicit root path. Used by `new()` and by tests
    /// that want to point at a tempdir without touching the process env
    /// (the crate is `#![forbid(unsafe_code)]`, so `std::env::set_var` —
    /// unsafe in edition 2024 — is not available).
    fn with_root(root: PathBuf) -> Self {
        Self { root }
    }

    /// Persist a JSON artifact. Returns the full path on success so the
    /// tool response can tell the operator exactly where the artifact landed.
    pub fn save(
        &self,
        kind: ArtifactKind,
        name: &str,
        value: &serde_json::Value,
    ) -> Result<PathBuf, String> {
        let sanitized = sanitize_artifact_name(name)?;
        let dir = self.root.join(kind.subdir());
        std::fs::create_dir_all(&dir).map_err(|e| {
            format!(
                "failed to create {} directory {}: {e}",
                kind.subdir(),
                dir.display()
            )
        })?;
        let path = dir.join(format!("{sanitized}.json"));
        let pretty = serde_json::to_string_pretty(value)
            .map_err(|e| format!("failed to serialize {} JSON: {e}", kind.subdir()))?;
        // Atomic write: temp file + rename. A crash mid-write leaves the
        // previous version intact (or no file on first write), never a
        // truncated artifact. `rename` is atomic on POSIX; on Windows it
        // is atomic within the same volume.
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, &pretty).map_err(|e| {
            format!(
                "failed to write {} temp file {}: {e}",
                kind.subdir(),
                tmp.display()
            )
        })?;
        std::fs::rename(&tmp, &path).map_err(|e| {
            // Clean up the temp file if rename failed — otherwise it
            // accumulates as litter.
            let _ = std::fs::remove_file(&tmp);
            format!(
                "failed to rename {} temp to {}: {e}",
                kind.subdir(),
                path.display()
            )
        })?;
        Ok(path)
    }

    /// Load a JSON artifact by name. Returns `None` if the file does not
    /// exist (a missing artifact is not an error — the operator may not have
    /// produced one yet). A corrupt file surfaces as `Err`.
    pub fn load(
        &self,
        kind: ArtifactKind,
        name: &str,
    ) -> Result<Option<serde_json::Value>, String> {
        let sanitized = sanitize_artifact_name(name)?;
        let path = self
            .root
            .join(kind.subdir())
            .join(format!("{sanitized}.json"));
        if !path.exists() {
            return Ok(None);
        }
        let data = std::fs::read_to_string(&path)
            .map_err(|e| format!("failed to read {} {}: {e}", kind.subdir(), path.display()))?;
        let value: serde_json::Value = serde_json::from_str(&data).map_err(|e| {
            format!(
                "failed to parse {} JSON {}: {e}",
                kind.subdir(),
                path.display()
            )
        })?;
        Ok(Some(value))
    }

    /// List artifact names (without extension) for a kind, sorted lexically.
    pub fn list(&self, kind: ArtifactKind) -> Result<Vec<String>, String> {
        let dir = self.root.join(kind.subdir());
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut names = Vec::new();
        for entry in std::fs::read_dir(&dir).map_err(|e| {
            format!(
                "failed to read {} dir {}: {e}",
                kind.subdir(),
                dir.display()
            )
        })? {
            let entry =
                entry.map_err(|e| format!("failed to read {} dir entry: {e}", kind.subdir()))?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    names.push(stem.to_string());
                }
            }
        }
        names.sort();
        Ok(names)
    }
}

impl Default for ReportStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Reject names that could escape the artifact directory. A valid name is
/// non-empty, contains no path separators, and is not `.` or `..`. This is
/// the path-traversal gate — do not bypass it.
fn sanitize_artifact_name(name: &str) -> Result<String, String> {
    if name.is_empty() {
        return Err("artifact name must not be empty".to_string());
    }
    if name == "." || name == ".." {
        return Err(format!("artifact name `{name}` is reserved"));
    }
    if name.contains('/') || name.contains('\\') {
        return Err(format!(
            "artifact name `{name}` must not contain path separators — use a flat name"
        ));
    }
    // Reject any control characters and the handful of filesystem-reserved
    // chars that `sanitize_name` in agent_paths also rejects. We don't reuse
    // `sanitize_name` directly because it rewrites spaces to dashes — we want
    // the operator's chosen name to round-trip exactly.
    for c in name.chars() {
        if c.is_control() || matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|') {
            return Err(format!(
                "artifact name `{name}` contains reserved character `{c}`"
            ));
        }
    }
    Ok(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    fn tmp_store() -> (ReportStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("tempdir");
        // Bypass `resolve_under_data_dir` (which reads `HKASK_DATA_DIR`) —
        // the crate is `#![forbid(unsafe_code)]` so `std::env::set_var`
        // (unsafe in edition 2024) is unavailable. `with_root` points the
        // store directly at the tempdir's `companies/` subtree.
        let store = ReportStore::with_root(dir.path().join("companies"));
        let _ = SEQ.fetch_add(1, Ordering::SeqCst);
        (store, dir)
    }

    #[test]
    fn save_then_load_round_trips() {
        let (store, _dir) = tmp_store();
        let value = serde_json::json!({"symbol": "AAPL", "gap": 0.12});
        let path = store
            .save(ArtifactKind::Screen, "expectations-gap-aapl", &value)
            .expect("save");
        assert!(
            path.to_string_lossy()
                .ends_with("screens/expectations-gap-aapl.json")
        );
        let loaded = store
            .load(ArtifactKind::Screen, "expectations-gap-aapl")
            .expect("load");
        assert_eq!(loaded, Some(value));
    }

    #[test]
    fn load_missing_returns_none() {
        let (store, _dir) = tmp_store();
        let loaded = store
            .load(ArtifactKind::Report, "never-produced")
            .expect("load");
        assert_eq!(loaded, None);
    }

    #[test]
    fn list_returns_sorted_stems() {
        let (store, _dir) = tmp_store();
        store
            .save(ArtifactKind::Report, "bravo", &serde_json::json!({}))
            .expect("save");
        store
            .save(ArtifactKind::Report, "alpha", &serde_json::json!({}))
            .expect("save");
        let names = store.list(ArtifactKind::Report).expect("list");
        assert_eq!(names, vec!["alpha", "bravo"]);
    }

    #[test]
    fn list_when_dir_missing_returns_empty() {
        let (store, _dir) = tmp_store();
        let names = store.list(ArtifactKind::Screen).expect("list");
        assert!(names.is_empty());
    }

    #[test]
    fn path_traversal_is_rejected() {
        let (store, _dir) = tmp_store();
        let err = store
            .save(ArtifactKind::Screen, "../escape", &serde_json::json!({}))
            .expect_err("traversal must be rejected");
        assert!(err.contains("path separators"));
        let err = store
            .save(ArtifactKind::Screen, "sub/dir", &serde_json::json!({}))
            .expect_err("subdir must be rejected");
        assert!(err.contains("path separators"));
    }

    #[test]
    fn empty_name_is_rejected() {
        let (store, _dir) = tmp_store();
        store
            .save(ArtifactKind::Screen, "", &serde_json::json!({}))
            .expect_err("empty name must be rejected");
    }

    #[test]
    fn reserved_chars_are_rejected() {
        let (store, _dir) = tmp_store();
        for bad in ["a:b", "a*b", "a?b", "a<b", "a>b", "a|b", "a\"b"] {
            let err = store
                .save(ArtifactKind::Screen, bad, &serde_json::json!({}))
                .expect_err("reserved char must be rejected");
            assert!(
                err.contains("reserved character"),
                "name `{bad}` should be rejected: {err}"
            );
        }
    }

    #[test]
    fn screens_and_reports_are_separate_subdirs() {
        let (store, _dir) = tmp_store();
        store
            .save(
                ArtifactKind::Screen,
                "s1",
                &serde_json::json!({"k": "screen"}),
            )
            .expect("save screen");
        store
            .save(
                ArtifactKind::Report,
                "r1",
                &serde_json::json!({"k": "report"}),
            )
            .expect("save report");
        // A screen and a report with the same stem don't collide.
        store
            .save(
                ArtifactKind::Screen,
                "shared",
                &serde_json::json!({"who": "screen"}),
            )
            .expect("save screen shared");
        store
            .save(
                ArtifactKind::Report,
                "shared",
                &serde_json::json!({"who": "report"}),
            )
            .expect("save report shared");
        let screen_shared = store.load(ArtifactKind::Screen, "shared").expect("load");
        let report_shared = store.load(ArtifactKind::Report, "shared").expect("load");
        assert_eq!(screen_shared.unwrap()["who"], "screen");
        assert_eq!(report_shared.unwrap()["who"], "report");
    }
}
