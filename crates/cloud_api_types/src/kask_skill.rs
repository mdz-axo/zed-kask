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
///
/// zed-kask: signed manifests ("Kask Skill Signing & Trust Model" plan, D1/D5).
/// The `public_key`/`signature`/`expires_at` fields are **required** — a
/// manifest without them fails to deserialize and is rejected by the collab
/// server. The signature covers `canonical_signing_bytes()` (all fields except
/// `signature` itself), so it transitively binds the tarball via
/// `tarball_sha256` and commits to `expires_at`.
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
    /// Publisher's Ed25519 public key (32-byte hex). The signing key lives in
    /// the OS keychain under `signing-keys/{source_user}` (hkask-keystore).
    pub public_key: String,
    /// Ed25519 signature (64-byte hex) over `canonical_signing_bytes()`.
    pub signature: String,
    /// RFC 3339 expiration set at signing time. The server accepts
    /// `now < expires_at <= now + KEY_MAX_AGE_DAYS` (120 days), judged by the
    /// server clock; expired manifests are filtered from the catalog and
    /// purged.
    pub expires_at: String,
}

impl KaskSkillManifest {
    /// The canonical bytes a publisher signs and the server verifies: the
    /// manifest serialized with the `signature` field cleared.
    ///
    /// Every other field — including `public_key` and `expires_at` — is
    /// included, so the signature commits to the expiration and binds the
    /// tarball via `tarball_sha256`. Both sides (client `kask_extensions_ui`,
    /// server `collab`) call this same function over the same struct, so
    /// serde's declaration-order serialization is the single source of truth
    /// for the signed bytes (plan D4).
    pub fn canonical_signing_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        let mut canonical = self.clone();
        canonical.signature.clear();
        serde_json::to_vec(&canonical)
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_manifest() -> KaskSkillManifest {
        KaskSkillManifest {
            source_user: "alice".into(),
            skill_name: "essentialist".into(),
            version: "2026-08-02.1".into(),
            description: "test skill".into(),
            dependencies: vec!["bob/deep-module".into()],
            tarball_sha256: "abc123".into(),
            public_key: "aa".repeat(32),
            signature: "bb".repeat(64),
            expires_at: "2026-12-01T00:00:00Z".into(),
        }
    }

    /// Pin the canonical-byte rule (plan D1/D4): the `signature` field's
    /// value is excluded, but `public_key` and `expires_at` are included —
    /// otherwise a publisher could edit `expires_at` after signing and the
    /// change would go undetected.
    #[test]
    fn canonical_signing_bytes_excludes_signature_and_includes_expiry() {
        let manifest = sample_manifest();
        let canonical = String::from_utf8(manifest.canonical_signing_bytes().unwrap()).unwrap();
        assert!(
            !canonical.contains("bbbb"),
            "signature value must be excluded: {canonical}"
        );
        assert!(
            canonical.contains("2026-12-01T00:00:00Z"),
            "expires_at must be included: {canonical}"
        );
        assert!(
            canonical.contains(&"aa".repeat(32)),
            "public_key must be included"
        );
        assert!(
            canonical.contains("abc123"),
            "tarball_sha256 must be included"
        );
    }

    /// Canonical bytes must be deterministic — the server re-serializes the
    /// parsed manifest and must get byte-identical input for verification.
    #[test]
    fn canonical_signing_bytes_is_deterministic() {
        let manifest = sample_manifest();
        assert_eq!(
            manifest.canonical_signing_bytes().unwrap(),
            manifest.canonical_signing_bytes().unwrap()
        );
    }

    /// A manifest missing the required signature fields must fail to
    /// deserialize (plan D5 — deny-by-default, no `#[serde(default)]`).
    #[test]
    fn manifest_requires_signature_fields() {
        let json = serde_json::json!({
            "source_user": "alice",
            "skill_name": "essentialist",
            "version": "2026-08-02.1",
            "description": "unsigned",
            "tarball_sha256": "abc123",
        });
        let result: Result<KaskSkillManifest, _> = serde_json::from_value(json);
        assert!(
            result.is_err(),
            "unsigned manifest must fail to deserialize"
        );
    }
}

/// Request body for `POST /api/kask-skills/:id/vote`.
#[derive(Serialize, Deserialize)]
pub struct KaskSkillVoteRequest {
    /// `+1` for upvote, `-1` for downvote.
    pub vote: i8,
}
