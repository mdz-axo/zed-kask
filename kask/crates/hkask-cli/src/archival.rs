//! ArchivalService — GitHub REST API for registry archival.
//! # REQ: P4 (Clear Boundaries) — GitHub operations via adapter, not raw HTTP.
//! # expect: "Service boundaries enforce OCAP membranes"

use base64::{Engine, engine::general_purpose::STANDARD as BASE64_STANDARD};
use hkask_mcp_server::server::{api_get, api_put, resolve_credential};
use serde_json::json;

use hkask_services_core::{DomainKind, ErrorKind, ServiceError};

const GITHUB_API_BASE: &str = "https://api.github.com";
const DEFAULT_REGISTRY_PATH: &str = "registry";

/// Result of archiving content to GitHub.
#[derive(Debug, Clone)]
pub struct ArchiveResult {
    /// Path where content was archived in the repository.
    pub path: String,
    /// SHA of the commit that created or updated the file.
    pub commit_sha: String,
}

/// Result of creating a registry snapshot on GitHub.
#[derive(Debug, Clone)]
pub struct SnapshotResult {
    /// SHA of the commit that created the snapshot.
    pub commit_sha: String,
}

/// Service for registry archival operations via GitHub REST API.
///
/// Resolves GitHub credentials from the OS keychain and constructs
/// authenticated HTTP clients internally. Callers provide repository
/// targeting parameters and content.
pub struct ArchivalService;

impl ArchivalService {
    /// Archive content to a GitHub repository.
    ///
    /// Uses the GitHub Contents API to create or update a file. If the file
    /// already exists, its SHA is fetched first for conflict detection.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  repo_owner, repo_name, branch, path, content must be non-empty; GitHub credentials must be in keychain
    /// post: returns ArchiveResult with path and commit_sha; file created or updated on GitHub; Err(Archival) on API failure
    pub async fn archive_to_git(
        repo_owner: &str,
        repo_name: &str,
        branch: &str,
        path: &str,
        content: &str,
    ) -> Result<ArchiveResult, ServiceError> {
        let client = build_github_client()?;

        let encoded_content = BASE64_STANDARD.encode(content.as_bytes());

        // Get the current file SHA if it exists (required for updates)
        let file_url = format!(
            "{GITHUB_API_BASE}/repos/{repo_owner}/{repo_name}/contents/{path}?ref={branch}"
        );

        let current_sha = get_current_file_sha(&client, &file_url).await;

        let url = format!("{GITHUB_API_BASE}/repos/{repo_owner}/{repo_name}/contents/{path}");

        let mut payload = json!({
            "message": format!("chore: archive registry to {path}"),
            "content": encoded_content,
            "branch": branch,
        });

        if let Some(sha) = current_sha {
            payload
                .as_object_mut()
                .expect("json! macro always produces an object")
                .insert("sha".to_string(), json!(sha));
        }

        let result = api_put(&client, "github", &url, &payload)
            .await
            .map_err(|e| {
                let msg = format!("Failed to archive registry: {}", e);
                ServiceError::Domain {
                    kind: ErrorKind::BadRequest,
                    domain: DomainKind::Storage,
                    message: msg,
                    source: Some(Box::new(e)),
                }
            })?;

        let commit_sha = result
            .get("commit")
            .and_then(|c| c.get("sha"))
            .and_then(|s| s.as_str())
            .unwrap_or("unknown")
            .to_string();

        Ok(ArchiveResult {
            path: path.to_string(),
            commit_sha,
        })
    }

    /// Restore content from a GitHub repository.
    ///
    /// Fetches file content using the GitHub Contents API and decodes
    /// the base64-encoded response.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  repo_owner, repo_name, git_ref must be non-empty; target_path defaults to "registry" if "."
    /// post: returns decoded file content as String; Err(Archival) on API failure, missing content, or decode error
    pub async fn restore_from_git(
        repo_owner: &str,
        repo_name: &str,
        git_ref: &str,
        target_path: &str,
    ) -> Result<String, ServiceError> {
        let client = build_github_client()?;

        let remote_path = if target_path == "." {
            DEFAULT_REGISTRY_PATH
        } else {
            target_path
        };

        let url = format!(
            "{GITHUB_API_BASE}/repos/{repo_owner}/{repo_name}/contents/{remote_path}?ref={git_ref}"
        );

        let json_val = api_get(&client, "github", &url).await.map_err(|e| {
            let msg = format!("Failed to fetch file: {}", e);
            ServiceError::Domain {
                kind: ErrorKind::BadRequest,
                domain: DomainKind::Storage,
                message: msg,
                source: Some(Box::new(e)),
            }
        })?;

        let encoded = json_val
            .get("content")
            .and_then(|c| c.as_str())
            .ok_or_else(|| ServiceError::Domain {
                kind: ErrorKind::BadRequest,
                domain: DomainKind::Storage,
                source: None,
                message: "No content field in GitHub response".into(),
            })?;

        let decoded = BASE64_STANDARD.decode(encoded.trim()).map_err(|e| {
            let msg = format!("Failed to decode base64 content: {}", e);
            ServiceError::Domain {
                kind: ErrorKind::BadRequest,
                domain: DomainKind::Storage,
                message: msg,
                source: Some(Box::new(e)),
            }
        })?;

        String::from_utf8(decoded).map_err(|e| {
            let msg = format!("Content is not valid UTF-8: {}", e);
            ServiceError::Domain {
                kind: ErrorKind::BadRequest,
                domain: DomainKind::Storage,
                message: msg,
                source: Some(Box::new(e)),
            }
        })
    }

    /// List archived registry versions (commit SHAs).
    ///
    /// Uses the GitHub Commits API to list commits that touched the
    /// registry file.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  repo_owner, repo_name must be non-empty; GitHub credentials must be in keychain
    /// post: returns `Vec<String>` of commit SHAs; empty Vec if no commits; Err(Archival) on API failure
    pub async fn list_archives(
        repo_owner: &str,
        repo_name: &str,
    ) -> Result<Vec<String>, ServiceError> {
        let client = build_github_client()?;

        let url = format!(
            "{GITHUB_API_BASE}/repos/{repo_owner}/{repo_name}/commits?path={DEFAULT_REGISTRY_PATH}"
        );

        let json_val = api_get(&client, "github", &url).await.map_err(|e| {
            let msg = format!("Failed to list archives: {}", e);
            ServiceError::Domain {
                kind: ErrorKind::BadRequest,
                domain: DomainKind::Storage,
                message: msg,
                source: Some(Box::new(e)),
            }
        })?;

        let commits = json_val.as_array().ok_or_else(|| ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Storage,
            source: None,
            message: "Expected array of commits".into(),
        })?;

        let shas: Vec<String> = commits
            .iter()
            .filter_map(|c| c.get("sha").and_then(|s| s.as_str()).map(|s| s.to_string()))
            .collect();

        Ok(shas)
    }

    /// Create a registry snapshot on GitHub.
    ///
    /// Removed: the agent registry was deleted (consolidation to one
    /// persistent UserPod per user). Registry snapshots are no longer
    /// produced. Returns an error directing the operator to CAS snapshot.
    pub async fn create_snapshot(
        _repo_owner: &str,
        _repo_name: &str,
        _message: &str,
    ) -> Result<SnapshotResult, ServiceError> {
        Err(ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Storage,
            source: None,
            message: "Registry snapshots are no longer supported — the agent registry was removed. Use `kask git cas-snapshot` for content-addressed snapshots.".into(),
        })
    }
}

// ── Internal helpers ────────────────────────────────────────────────────

/// Build an authenticated reqwest::Client for GitHub API calls.
///
/// Resolves the GitHub token from keychain/env and sets default headers
/// (Authorization, Accept, User-Agent).
fn build_github_client() -> Result<reqwest::Client, ServiceError> {
    let token = resolve_credential("HKASK_GITHUB_TOKEN").map_err(|e| {
        let msg = format!("GitHub token not available: {}", e);
        ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Storage,
            message: msg,
            source: Some(Box::new(e)),
        }
    })?;

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::ACCEPT,
        "application/vnd.github+json"
            .parse()
            .expect("static Accept header"),
    );
    headers.insert(
        reqwest::header::USER_AGENT,
        "hKask-archival/0.22.0"
            .parse()
            .expect("static User-Agent header"),
    );
    headers.insert(
        reqwest::header::AUTHORIZATION,
        format!("Bearer {token}")
            .parse()
            .expect("valid Authorization header"),
    );

    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .map_err(|e| {
            let msg = format!("Failed to build HTTP client: {}", e);
            ServiceError::Domain {
                kind: ErrorKind::BadRequest,
                domain: DomainKind::Storage,
                message: msg,
                source: Some(Box::new(e)),
            }
        })
}

/// Get the current file SHA from GitHub, if the file exists.
///
/// Returns `None` if the file doesn't exist (404) or the request fails.
async fn get_current_file_sha(client: &reqwest::Client, url: &str) -> Option<String> {
    match api_get(client, "github", url).await {
        Ok(json) => json
            .get("sha")
            .and_then(|s| s.as_str())
            .map(|s| s.to_string()),
        Err(_) => None,
    }
}

// ── Tests ───────────────────────────────────────────────────────────────
