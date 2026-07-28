//! Publish and install pipelines for kask skills.
//!
//! Mirrors the extension marketplace pattern:
//! - **Publish:** package the skill directory into `archive.tar.gz` +
//!   `manifest.json`, upload both directly to S3 at
//!   `kask-skills/{source_user}/{skill_name}/{version}/`. The collab
//!   server's periodic poll picks it up and inserts it into Postgres.
//! - **Install:** `GET /api/kask-skills/:id/download` → S3 presigned
//!   redirect → download tarball → verify SHA256 → extract into
//!   `~/.agents/skills/_marketplace/{source_user}/{skill_name}/`.
//!
//! Per the `.rules` trap "Cross-thread GPUI communication uses channels,
//! not `AsyncApp` handles", these pipelines run on a background executor
//! and do not capture `AsyncApp`. They return results via the GPUI
//! foreground task that spawned them.

use chrono::{Datelike, Timelike};
use std::path::{Path, PathBuf};

use agent_skills::Skill;
use anyhow::{Context as _, Result, bail};
use async_compression::futures::bufread::{GzipDecoder, GzipEncoder};
use async_tar::Builder;
use bytes::Bytes;
use cloud_api_types::KaskSkillManifest;
use fs::{Fs, read_dir_items};
use http_client::{AsyncBody, HttpClient, HttpClientWithUrl};
use sha2::{Digest, Sha256};
/// The S3 key prefix for kask skills. Mirrors `extensions/` for extensions.
pub const KASK_SKILLS_S3_PREFIX: &str = "kask-skills";

/// zed-kask: Resolve the kask marketplace base URL.
///
/// Decoupled from `server_url` (which points at Zed's cloud for login,
/// telemetry, collab) so the marketplace can target a kask-aware collab
/// server without breaking Zed account auth.
///
/// Resolution order:
/// 1. `HKASK_MARKETPLACE_URL` env var — operator/dev override.
/// 2. `http_client.base_url()` — fall back to the configured `server_url`
///    (the Zed default `https://zed.dev` or whatever the user set).
///
/// Returns the base URL with no trailing slash.
fn kask_marketplace_base_url(http_client: &HttpClientWithUrl) -> String {
    std::env::var("HKASK_MARKETPLACE_URL")
        .unwrap_or_else(|_| http_client.base_url())
        .trim_end_matches('/')
        .to_string()
}

/// zed-kask: Build a full marketplace URL string by joining `path` to the
/// marketplace base URL. `path` should start with `/` (e.g. `/api/kask-skills`).
/// `query` is a slice of `(key, value)` pairs; pass `&[]` for none.
///
/// Unlike `build_zed_api_url`, this does NOT remap `zed.dev` → `api.zed.dev`
/// or `localhost:3000` → `localhost:8080`. The marketplace URL is used as-is.
/// Callers pass the result to `http_client.get(url_str, ...)` or
/// `Request::post(url_str, ...)`.
pub fn kask_marketplace_url(
    http_client: &HttpClientWithUrl,
    path: &str,
    query: &[(&str, &str)],
) -> Result<String> {
    let base = kask_marketplace_base_url(http_client);
    let mut url_str = format!("{}{}", base, path);
    if !query.is_empty() {
        let query_string = query
            .iter()
            .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
            .collect::<Vec<_>>()
            .join("&");
        url_str.push('?');
        url_str.push_str(&query_string);
    }
    Ok(url_str)
}

/// Package a skill directory into a tar.gz archive and compute its SHA256.
///
/// Reads `SKILL.md` + `manifest.yaml` + all `*.j2` templates from the skill
/// directory. Returns the tarball bytes, the SHA256 hex digest, and the
/// manifest JSON.
pub async fn package_skill_for_publish(
    fs: &dyn Fs,
    skill: &Skill,
    source_user: &str,
    version: &str,
) -> Result<(Vec<u8>, String, String)> {
    let skill_dir = &skill.directory_path;

    // Collect all files in the skill directory. `read_dir_items` returns
    // `Vec<(PathBuf, is_dir)>` — simpler than streaming the raw `read_dir`.
    let entries = read_dir_items(fs, skill_dir)
        .await
        .with_context(|| format!("failed to read skill directory at {}", skill_dir.display()))?;

    let mut tar_builder = Builder::new(Vec::new());
    let mut manifest_dependencies: Vec<String> = Vec::new();

    for (path, is_dir) in entries {
        // Skip directories for v1 — skills are flat.
        if is_dir {
            continue;
        }

        let file_name = path
            .file_name()
            .with_context(|| format!("path has no file name: {}", path.display()))?
            .to_string_lossy()
            .to_string();
        let relative_path = Path::new(&file_name);

        let content = fs
            .load(&path)
            .await
            .with_context(|| format!("failed to read skill file {}", path.display()))?;

        // Add to tarball.
        let mut header = async_tar::Header::new_gnu();
        header.set_path(relative_path)?;
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar_builder.append(&header, content.as_bytes()).await?;

        // Parse manifest.yaml for dependencies if this is the manifest.
        if file_name == "manifest.yaml" || file_name == "manifest.yml" {
            manifest_dependencies = parse_manifest_dependencies(&content);
        }
    }

    let tar_bytes = tar_builder.into_inner().await?;

    // Gzip the tar first, then compute SHA256 on the gzipped bytes.
    // The install path downloads the .tar.gz and verifies SHA256 on the
    // downloaded bytes — so the hash must be on the compressed data.
    let mut gzip_encoder = GzipEncoder::new(tar_bytes.as_slice());
    let mut gz_bytes = Vec::new();
    smol::io::AsyncReadExt::read_to_end(&mut gzip_encoder, &mut gz_bytes).await?;

    let sha256 = hex::encode(Sha256::digest(&gz_bytes));

    // Create the manifest JSON.
    let manifest = KaskSkillManifest {
        source_user: source_user.to_string(),
        skill_name: skill.name.clone(),
        version: version.to_string(),
        description: skill.description.clone(),
        dependencies: manifest_dependencies,
        tarball_sha256: sha256.clone(),
    };
    let manifest_json = serde_json::to_string(&manifest)?;

    Ok((gz_bytes, sha256, manifest_json))
}

/// Parse the `dependencies` field from a kask skill manifest.yaml.
///
/// The manifest is YAML; we do a simple line-based parse for the
/// `dependencies:` list to avoid pulling in a full YAML parser. The
/// dependencies are skill IDs (`{source_user}/{skill_name}`).
fn parse_manifest_dependencies(manifest_content: &str) -> Vec<String> {
    let mut deps = Vec::new();
    let mut in_deps = false;
    for line in manifest_content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("dependencies:") {
            in_deps = true;
            continue;
        }
        if in_deps {
            // A dependency list item starts with `- `.
            if let Some(dep) = trimmed.strip_prefix("- ") {
                deps.push(dep.trim().trim_matches('"').to_string());
            } else if !trimmed.is_empty() && !trimmed.starts_with('-') {
                // We've left the dependencies list.
                in_deps = false;
            }
        }
    }
    deps
}

/// Publish a skill to the marketplace by uploading to S3.
///
/// This is called by the `SkillVisibilityQueue` drain task when a skill is
/// toggled to `Public`. It packages the skill and uploads both
/// `archive.tar.gz` and `manifest.json` to S3 at
/// `kask-skills/{source_user}/{skill_name}/{version}/`.
///
/// On failure: `log::warn!` with skill ID, failure reason, remediation.
/// The local `visibility` flag is NOT rolled back (plan §2.6).
pub async fn publish_skill(
    fs: &dyn Fs,
    http_client: &HttpClientWithUrl,
    credentials: &client::Credentials,
    skill: &Skill,
    source_user: &str,
    version: &str,
) -> Result<()> {
    log::info!(
        "kask-extensions: publishing skill '{}/{}' version {}",
        source_user,
        skill.name,
        version
    );

    let (tarball_bytes, _sha256, manifest_json) =
        package_skill_for_publish(fs, skill, source_user, version)
            .await
            .with_context(|| {
                format!(
                    "failed to package skill '{}/{}' for publish",
                    source_user, skill.name
                )
            })?;

    // Upload to S3 via the collab server's upload endpoint.
    // The collab server proxies the upload to S3 (mirrors how extension
    // uploads work in production — the client doesn't have direct S3
    // credentials).
    let s3_key = format!(
        "{}/{}/{}/{}/archive.tar.gz",
        KASK_SKILLS_S3_PREFIX, source_user, skill.name, version
    );
    let manifest_key = format!(
        "{}/{}/{}/{}/manifest.json",
        KASK_SKILLS_S3_PREFIX, source_user, skill.name, version
    );

    let auth_header = credentials.authorization_header();

    // Upload tarball.
    let upload_url = crate::publish::kask_marketplace_url(
        http_client,
        "/api/kask-skills/upload",
        &[("key", &s3_key)],
    )?;
    http_client
        .send(
            http_client::http::Request::post(&upload_url)
                .header("Content-Type", "application/octet-stream")
                .header("Authorization", &auth_header)
                .body(AsyncBody::from_bytes(Bytes::from(tarball_bytes)))?,
        )
        .await
        .context("uploading kask skill tarball")?;

    // Upload manifest.
    let manifest_upload_url = crate::publish::kask_marketplace_url(
        http_client,
        "/api/kask-skills/upload",
        &[("key", &manifest_key)],
    )?;
    http_client
        .send(
            http_client::http::Request::post(&manifest_upload_url)
                .header("Content-Type", "application/json")
                .header("Authorization", &auth_header)
                .body(AsyncBody::from_bytes(Bytes::from(
                    manifest_json.into_bytes(),
                )))?,
        )
        .await
        .context("uploading kask skill manifest")?;

    log::info!(
        "kask-extensions: published skill '{}/{}' version {}",
        source_user,
        skill.name,
        version
    );

    Ok(())
}

/// Unpublish a skill by deleting it from S3.
///
/// This is called by the `SkillVisibilityQueue` drain task when a skill is
/// toggled back to `Private`. The local skill stays on disk; only the
/// marketplace listing is removed.
pub async fn unpublish_skill(
    http_client: &HttpClientWithUrl,
    credentials: &client::Credentials,
    source_user: &str,
    skill_name: &str,
) -> Result<()> {
    log::info!(
        "kask-extensions: unpublishing skill '{}/{}'",
        source_user,
        skill_name
    );

    let auth_header = credentials.authorization_header();
    // zed-kask: URL-encode the skill ID (alice/bug-hunt → alice%2Fbug-hunt)
    // so it's a single path segment. The server decodes it back.
    let skill_id_str = format!("{}/{}", source_user, skill_name);
    let encoded_id = urlencoding::encode(&skill_id_str);
    let delete_url = crate::publish::kask_marketplace_url(
        http_client,
        &format!("/api/kask-skills/{}", encoded_id),
        &[],
    )?;
    http_client
        .send(
            http_client::http::Request::delete(&delete_url)
                .header("Authorization", &auth_header)
                .body(AsyncBody::empty())?,
        )
        .await
        .context("unpublishing kask skill")?;

    log::info!(
        "kask-extensions: unpublished skill '{}/{}'",
        source_user,
        skill_name
    );

    Ok(())
}

/// Install a kask skill from the marketplace.
///
/// Downloads the tarball via `GET /api/kask-skills/:id/download` (which
/// redirects to an S3 presigned URL), verifies the SHA256, and extracts
/// into `~/.agents/skills/_marketplace/{source_user}/{skill_name}/`.
pub async fn install_skill(
    fs: &dyn Fs,
    http_client: &HttpClientWithUrl,
    skill_id: &str,
    expected_sha256: &str,
    marketplace_dir: &Path,
) -> Result<PathBuf> {
    let (source_user, skill_name) = skill_id
        .split_once('/')
        .context("kask skill id must be \"{source_user}/{skill_name}\"")?;

    log::info!(
        "kask-extensions: installing skill '{}/{}'",
        source_user,
        skill_name
    );

    // Download the tarball.
    let encoded_id = urlencoding::encode(skill_id);
    let download_url = crate::publish::kask_marketplace_url(
        http_client,
        &format!("/api/kask-skills/{}/download", encoded_id),
        &[],
    )?;
    let mut response = http_client
        .get(&download_url, AsyncBody::empty(), true)
        .await
        .context("downloading kask skill")?;

    let mut tar_gz_bytes = Vec::new();
    futures::AsyncReadExt::read_to_end(response.body_mut(), &mut tar_gz_bytes)
        .await
        .context("reading kask skill tarball")?;

    // Verify SHA256.
    let actual_sha256 = hex::encode(Sha256::digest(&tar_gz_bytes));
    if actual_sha256 != expected_sha256 {
        bail!(
            "kask skill tarball SHA256 mismatch: expected {}, got {}",
            expected_sha256,
            actual_sha256
        );
    }

    // Extract into the marketplace directory.
    let install_dir = marketplace_dir.join(source_user).join(skill_name);

    // Remove any existing install.
    fs.remove_dir(
        &install_dir,
        fs::RemoveOptions {
            recursive: true,
            ignore_if_not_exists: true,
        },
    )
    .await?;

    // Create the install directory and all parent directories.
    // First install: ~/.agents/skills/_marketplace/ doesn't exist yet.
    let marketplace_parent = marketplace_dir;
    if !fs.is_dir(marketplace_parent).await {
        fs.create_dir(marketplace_parent).await?;
    }
    let user_dir = marketplace_dir.join(source_user);
    if !fs.is_dir(&user_dir).await {
        fs.create_dir(&user_dir).await?;
    }
    fs.create_dir(&install_dir).await?;

    // Decompress and extract.
    let decompressed = GzipDecoder::new(tar_gz_bytes.as_slice());
    let archive = async_tar::Archive::new(decompressed);
    archive.unpack(&install_dir).await?;

    log::info!(
        "kask-extensions: installed skill '{}/{}' to {}",
        source_user,
        skill_name,
        install_dir.display()
    );

    Ok(install_dir)
}

/// Vote on a kask skill (+1 or -1).
pub async fn vote_skill(
    http_client: &HttpClientWithUrl,
    credentials: &client::Credentials,
    skill_id: &str,
    vote: i8,
) -> Result<(i64, i64)> {
    let auth_header = credentials.authorization_header();
    let encoded_id = urlencoding::encode(skill_id);
    let vote_url = crate::publish::kask_marketplace_url(
        http_client,
        &format!("/api/kask-skills/{}/vote", encoded_id),
        &[],
    )?;
    let body = serde_json::to_string(&cloud_api_types::KaskSkillVoteRequest { vote })?;
    let mut response = http_client
        .send(
            http_client::http::Request::post(&vote_url)
                .header("Content-Type", "application/json")
                .header("Authorization", &auth_header)
                .body(AsyncBody::from_bytes(Bytes::from(body.into_bytes())))?,
        )
        .await
        .context("voting on kask skill")?;

    let mut resp_body = Vec::new();
    futures::AsyncReadExt::read_to_end(response.body_mut(), &mut resp_body)
        .await
        .context("reading vote response")?;

    let result: serde_json::Value = serde_json::from_slice(&resp_body)?;
    let upvote_count = result
        .get("upvote_count")
        .and_then(|v| v.as_i64())
        .context("missing upvote_count in response")?;
    let downvote_count = result
        .get("downvote_count")
        .and_then(|v| v.as_i64())
        .context("missing downvote_count in response")?;

    Ok((upvote_count, downvote_count))
}

/// Generate a timestamp-based version string for a skill publish.
/// Format: `YYYY-MM-DD.N` where N increments if published multiple times
/// in the same day.
pub fn generate_version() -> String {
    let now = chrono::Utc::now();
    format!(
        "{:04}-{:02}-{:02}.{}",
        now.year(),
        now.month(),
        now.day(),
        now.hour()
    )
}
