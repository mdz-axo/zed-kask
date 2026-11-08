//! Model acquisition — fetch weights from HuggingFace through the
//! canonical credential path.
//!
//! `HF_TOKEN` is resolved by the zed-side caller (the bridge's
//! data-services chain reads the keychain) and passed in — this module
//! never reads credentials itself. Anonymous access serves public repos;
//! a gated repo without a token is a typed authorization error naming the
//! env var, never a silent empty result (fail-closed). Downloads resume
//! via HTTP Range and are verified against the LFS sha256 before the
//! file is handed to the placement compiler — the in-flight anonymous
//! P0 download of the public repo is re-verified by this module's
//! checksum once it lands (local-leg plan §5, reuse map).

use std::path::Path;

use serde::Deserialize;
use sha2::{Digest, Sha256};

/// The credential this module is the read site for (the data-services row
/// is reinstated WITH this module — never a credential row without a
/// consumer).
pub const HF_CREDENTIAL_ENV: &str = "HF_TOKEN";

const HF_BASE: &str = "https://huggingface.co";

#[derive(Debug, thiserror::Error)]
pub enum AcquisitionError {
    #[error("huggingface api request for {repo} failed: {status} {url}")]
    Api {
        repo: String,
        status: u16,
        url: String,
    },
    #[error(
        "authorization required for {repo} — {HF_CREDENTIAL_ENV} not supplied; \
         resolve it through the credential chain"
    )]
    AuthorizationRequired { repo: String },
    #[error("http: {0}")]
    Http(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("size mismatch for {path}: expected {expected} bytes, wrote {actual}")]
    SizeMismatch {
        path: String,
        expected: u64,
        actual: u64,
    },
    #[error(
        "checksum mismatch for {path}: expected {expected}, computed {computed} \
         (the partial file was removed so a retry restarts clean)"
    )]
    ChecksumMismatch {
        path: String,
        expected: String,
        computed: String,
    },
    #[error(
        "resume rejected for {url}: server returned {status} for a Range request \
         at offset {offset}; refusing to touch the partial file"
    )]
    ResumeRejected {
        url: String,
        status: u16,
        offset: u64,
    },
}

/// One file of a HuggingFace repo tree, with its LFS content sha256 when
/// the file is LFS-backed (the GGUF shards are).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoFile {
    pub path: String,
    pub size: u64,
    pub lfs_sha256: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TreeEntry {
    #[serde(rename = "type")]
    kind: String,
    path: String,
    size: Option<u64>,
    lfs: Option<LfsPointer>,
}

#[derive(Debug, Deserialize)]
struct LfsPointer {
    oid: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchOutcome {
    Completed { bytes: u64 },
    Resumed { resumed_from: u64, bytes: u64 },
}

/// The resolve URL for one repo file.
pub fn resolve_url(repo: &str, file_path: &str) -> String {
    format!("{HF_BASE}/{repo}/resolve/main/{file_path}")
}

/// The tree-listing URL for one repo subpath.
fn tree_url(repo: &str, subpath: &str) -> String {
    format!("{HF_BASE}/api/models/{repo}/tree/main/{subpath}")
}

/// Parse a repo-tree API response into its files. A `file` entry without
/// a size is malformed metadata — skipped, never guessed at.
fn parse_tree(body: &[u8]) -> Result<Vec<RepoFile>, AcquisitionError> {
    let entries: Vec<TreeEntry> = serde_json::from_slice(body)
        .map_err(|e| AcquisitionError::Http(format!("tree parse failed: {e}")))?;
    Ok(entries
        .into_iter()
        .filter(|entry| entry.kind == "file")
        .filter_map(|entry| {
            let size = entry.size?;
            Some(RepoFile {
                path: entry.path,
                size,
                lfs_sha256: entry.lfs.map(|lfs| lfs.oid),
            })
        })
        .collect())
}

/// List the files of a repo subpath. Anonymous access serves public
/// repos; auth-gated repos without a token fail closed.
pub async fn list_repo_files(
    repo: &str,
    subpath: &str,
    token: Option<&str>,
) -> Result<Vec<RepoFile>, AcquisitionError> {
    let url = tree_url(repo, subpath);
    let client = reqwest::Client::new();
    let mut request = client.get(&url);
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    let response = request
        .send()
        .await
        .map_err(|e| AcquisitionError::Http(e.to_string()))?;
    match response.status().as_u16() {
        200 => {}
        401..=403 => {
            return Err(AcquisitionError::AuthorizationRequired {
                repo: repo.to_string(),
            });
        }
        status => {
            return Err(AcquisitionError::Api {
                repo: repo.to_string(),
                status,
                url,
            });
        }
    }
    let body = response
        .bytes()
        .await
        .map_err(|e| AcquisitionError::Http(e.to_string()))?;
    parse_tree(&body)
}

/// Fetch one repo file with resume and LFS sha256 verification. The
/// verification is the whole point: a resumed or re-run download is
/// byte-proven against the hub's content hash before anything downstream
/// consumes it.
pub async fn fetch_file(
    repo: &str,
    file: &RepoFile,
    dest: &Path,
    token: Option<&str>,
) -> Result<FetchOutcome, AcquisitionError> {
    let url = resolve_url(repo, &file.path);
    let client = reqwest::Client::new();

    let resume_from = match tokio::fs::metadata(dest).await {
        Ok(meta) => meta.len(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => 0,
        Err(err) => return Err(err.into()),
    };

    // Already complete (dual-axis review P4): a fully downloaded file is
    // verified and returned instead of sending `bytes={size}-` (a 416 the
    // generic Api arm turns into an opaque dead-end every retry hits).
    if resume_from == file.size {
        if let Some(expected) = &file.lfs_sha256 {
            let computed = sha256_file(dest).await?;
            if &computed != expected {
                tokio::fs::remove_file(dest).await?;
                return Err(AcquisitionError::ChecksumMismatch {
                    path: dest.display().to_string(),
                    expected: expected.clone(),
                    computed,
                });
            }
        }
        return Ok(FetchOutcome::Completed { bytes: resume_from });
    }
    // An over-long partial is corrupt beyond the expected size — remove it
    // so a retry starts clean instead of 416-looping (dual-axis review P4).
    if resume_from > file.size {
        tokio::fs::remove_file(dest).await?;
        return Err(AcquisitionError::SizeMismatch {
            path: dest.display().to_string(),
            expected: file.size,
            actual: resume_from,
        });
    }

    let mut request = client.get(&url);
    if resume_from > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={resume_from}-"));
    }
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    let response = request
        .send()
        .await
        .map_err(|e| AcquisitionError::Http(e.to_string()))?;
    let status = response.status().as_u16();
    match status {
        200 | 206 => {}
        401..=403 => {
            return Err(AcquisitionError::AuthorizationRequired {
                repo: repo.to_string(),
            });
        }
        other => {
            return Err(AcquisitionError::Api {
                repo: repo.to_string(),
                status: other,
                url,
            });
        }
    }
    // A resume request answered 200 means the server ignored the Range —
    // refuse rather than blind-append onto or overwrite the partial file.
    if resume_from > 0 && status != 206 {
        return Err(AcquisitionError::ResumeRejected {
            url,
            status,
            offset: resume_from,
        });
    }

    let mut writer = if resume_from > 0 {
        tokio::fs::OpenOptions::new()
            .append(true)
            .open(dest)
            .await?
    } else {
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::File::create(dest).await?
    };
    let mut written = resume_from;
    let mut response = response;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| AcquisitionError::Http(e.to_string()))?
    {
        tokio::io::AsyncWriteExt::write_all(&mut writer, &chunk).await?;
        written += chunk.len() as u64;
    }

    if written != file.size {
        if written > file.size {
            // Over-long transfer (dual-axis review P4): remove the partial
            // so a retry starts clean — a short partial stays resumable.
            tokio::fs::remove_file(dest).await?;
        }
        return Err(AcquisitionError::SizeMismatch {
            path: dest.display().to_string(),
            expected: file.size,
            actual: written,
        });
    }
    if let Some(expected) = &file.lfs_sha256 {
        let computed = sha256_file(dest).await?;
        if &computed != expected {
            // Remove the corrupt partial so a retry starts clean instead of
            // resuming from corrupt bytes.
            tokio::fs::remove_file(dest).await?;
            return Err(AcquisitionError::ChecksumMismatch {
                path: dest.display().to_string(),
                expected: expected.clone(),
                computed,
            });
        }
    }

    Ok(if resume_from > 0 {
        FetchOutcome::Resumed {
            resumed_from: resume_from,
            bytes: written,
        }
    } else {
        FetchOutcome::Completed { bytes: written }
    })
}

async fn sha256_file(path: &Path) -> Result<String, AcquisitionError> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let read = tokio::io::AsyncReadExt::read(&mut file, &mut buffer).await?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sha256_hex_of(bytes: &[u8]) -> String {
        use sha2::Digest as _;
        format!("{:x}", sha2::Sha256::digest(bytes))
    }

    /// A complete local file is verified and returned without any request
    /// (dual-axis review P4) — the 416 dead-end is unreachable for
    /// complete files.
    #[tokio::test]
    async fn a_complete_partial_is_verified_and_returned_without_a_request() {
        let dest = std::env::temp_dir().join("hkask-p4-complete-probe.gguf");
        let _ = std::fs::remove_file(&dest);
        let bytes = b"complete-file-bytes";
        std::fs::write(&dest, bytes).expect("seed complete file");
        let file = RepoFile {
            path: "model.gguf".to_string(),
            size: bytes.len() as u64,
            lfs_sha256: Some(sha256_hex_of(bytes)),
        };
        let outcome = fetch_file("repo", &file, &dest, None)
            .await
            .expect("complete file short-circuits before any request");
        assert!(
            matches!(outcome, FetchOutcome::Completed { bytes } if bytes == file.size),
            "got: {outcome:?}"
        );
        let _ = std::fs::remove_file(&dest);
    }

    /// An over-long partial is removed and refused (dual-axis review P4)
    /// — the retry starts clean instead of 416-looping on the stale bytes.
    #[tokio::test]
    async fn an_overlong_partial_is_removed_and_refused() {
        let dest = std::env::temp_dir().join("hkask-p4-overlong-probe.gguf");
        let _ = std::fs::remove_file(&dest);
        std::fs::write(&dest, b"far-too-long").expect("seed over-long partial");
        let file = RepoFile {
            path: "model.gguf".to_string(),
            size: 3,
            lfs_sha256: None,
        };
        let error = fetch_file("repo", &file, &dest, None)
            .await
            .expect_err("over-long partial refused before any request");
        assert!(
            matches!(error, AcquisitionError::SizeMismatch { .. }),
            "got: {error:?}"
        );
        assert!(
            !dest.exists(),
            "the corrupt over-long partial is removed for a clean retry"
        );
    }

    const REAL_TREE_RESPONSE: &str = r#"[
        {"type":"file","path":"UD-Q2_K_XL/Qwen3.8-Flash-Next-UD-Q2_K_XL-00001-of-00004.gguf","size":10946618,"lfs":{"oid":"aabbcc","size":10946618,"pointerSize":133}},
        {"type":"directory","path":"UD-Q2_K_XL","oid":null,"size":null},
        {"type":"file","path":"UD-Q2_K_XL/README.md","size":512}
    ]"#;

    #[test]
    fn parses_tree_entries_and_keeps_only_sized_files() {
        let files = parse_tree(REAL_TREE_RESPONSE.as_bytes()).expect("fixture parses");
        assert_eq!(files.len(), 2);
        assert_eq!(
            files[0],
            RepoFile {
                path: "UD-Q2_K_XL/Qwen3.8-Flash-Next-UD-Q2_K_XL-00001-of-00004.gguf".into(),
                size: 10946618,
                lfs_sha256: Some("aabbcc".into()),
            }
        );
        // A plain (non-LFS) file carries no sha — fetch verifies size only.
        assert_eq!(files[1].lfs_sha256, None);
    }

    #[test]
    fn urls_are_built_from_the_canonical_bases() {
        assert_eq!(
            resolve_url("unsloth/Qwen3.8-Flash-Next-GGUF", "UD-Q2_K_XL/x.gguf"),
            "https://huggingface.co/unsloth/Qwen3.8-Flash-Next-GGUF/resolve/main/UD-Q2_K_XL/x.gguf"
        );
        assert_eq!(
            tree_url("unsloth/Qwen3.8-Flash-Next-GGUF", "UD-Q2_K_XL"),
            "https://huggingface.co/api/models/unsloth/Qwen3.8-Flash-Next-GGUF/tree/main/UD-Q2_K_XL"
        );
    }
}
