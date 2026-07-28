use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// The manifest JSON that sits alongside the tarball in S3 at
/// `kask-skills/{source_user}/{skill_name}/{version}/manifest.json`.
///
/// Mirrors `ExtensionApiManifest` — the catalog metadata the periodic poll
/// fetches from S3 and upserts into Postgres. `source_user` is the
/// publisher's GitHub login (from the authenticated Zed user), enforced
/// client-side at publish time.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct KaskSkillManifest {
    /// Publisher's GitHub login. Bound to the authenticated user at publish time.
    pub source_user: String,
    /// Skill name (matches the SKILL.md frontmatter `name` field).
    pub skill_name: String,
    /// Timestamp-based version string (e.g. "2026-07-27.1"). Only the latest
    /// version is kept in v1; the UI never shows a version picker.
    pub version: String,
    /// Human-readable description (from SKILL.md frontmatter).
    pub description: String,
    /// Skill IDs (`{source_user}/{skill_name}`) this skill depends on.
    /// Enforced at publish time: all dependencies must already be published.
    #[serde(default)]
    pub dependencies: Vec<String>,
    /// SHA256 of the `archive.tar.gz` tarball. Verified on install.
    pub tarball_sha256: String,
}

/// Catalog metadata for a kask skill, returned by `GET /api/kask-skills`.
///
/// Mirrors `ExtensionMetadata` — the manifest plus server-side aggregate
/// counts (downloads, votes).
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct KaskSkillMetadata {
    /// `"{source_user}/{skill_name}"` — the canonical marketplace id.
    pub id: Arc<str>,
    #[serde(flatten)]
    pub manifest: KaskSkillManifest,
    pub published_at: DateTime<Utc>,
    pub download_count: u64,
    pub upvote_count: i64,
    pub downvote_count: i64,
}

/// Response body for `GET /api/kask-skills`.
#[derive(Serialize, Deserialize)]
pub struct GetKaskSkillsResponse {
    pub data: Vec<KaskSkillMetadata>,
}

/// Request body for `POST /api/kask-skills/:id/vote`.
#[derive(Serialize, Deserialize)]
pub struct KaskSkillVoteRequest {
    /// `+1` for upvote, `-1` for downvote.
    pub vote: i8,
}
