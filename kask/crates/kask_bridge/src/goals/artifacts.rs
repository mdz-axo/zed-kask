//! Bounded, consent-gated local artifact snapshots. No execution, network,
//! directory enumeration or symlink traversal. These receipts establish file
//! bytes, not that the producer's claimed command, test or oracle actually ran.
use chrono::{DateTime, Utc};
use hkask_mcp_kata_kanban::{
    GOAL_ARTIFACT_MAX_FILE_BYTES, GOAL_ARTIFACT_MAX_FILES, GOAL_ARTIFACT_MAX_TOTAL_BYTES,
    GoalArtifactReadOutcome as Outcome, GoalArtifactSnapshot, GoalEvidence,
    LocalArtifactAccessApproval,
};
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, path::Path};

pub(super) use hkask_mcp_kata_kanban::local_artifact_path as local_path;

pub(super) fn capture(
    manifest: &[GoalEvidence],
    approval: Option<LocalArtifactAccessApproval>,
    now: &dyn Fn() -> DateTime<Utc>,
) -> Result<Vec<GoalArtifactSnapshot>, String> {
    if manifest.len() > GOAL_ARTIFACT_MAX_FILES {
        return Err("artifact manifest exceeds the approved 16-file bound; no files read".into());
    }
    let mut remaining = GOAL_ARTIFACT_MAX_TOTAL_BYTES;
    Ok(manifest.iter().map(|evidence| {

        let outcome = if approval != Some(LocalArtifactAccessApproval::BoundedUtf8V1) {
            unavailable(None, "not_approved: original native closure did not authorize local artifact reads/sharing".into(), None, 0)
        } else { read(evidence, remaining) };
        let count = match &outcome { Outcome::Fetched { byte_count, .. } => *byte_count, Outcome::Unavailable { bytes_read, .. } => *bytes_read };
        remaining = remaining.saturating_sub(count as usize);
        GoalArtifactSnapshot { evidence_id:evidence.id.clone(), observed_at: now(), outcome }
    }).collect())
}
fn unavailable(
    source: Option<String>,
    reason: String,
    observed_sha256: Option<String>,
    bytes_read: usize,
) -> Outcome {
    Outcome::Unavailable {
        source,
        reason,
        observed_sha256,
        bytes_read: bytes_read as u64,
    }
}
fn read(evidence: &GoalEvidence, remaining: usize) -> Outcome {
    let expected = evidence
        .digest
        .strip_prefix("sha256:")
        .unwrap_or(&evidence.digest);
    if expected.len() != 64 || !expected.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return unavailable(
            None,
            "invalid_digest: expected SHA-256 must be 64 hex digits, optionally prefixed sha256:"
                .into(),
            None,
            0,
        );
    }
    match read_utf8(&evidence.locator, remaining) {
        Outcome::Fetched {
            source,
            sha256,
            byte_count,
            content: _,
        } if !sha256.eq_ignore_ascii_case(expected) => unavailable(
            Some(source),
            format!("digest_mismatch: expected {expected}; observed {sha256}; contents withheld"),
            Some(sha256),
            byte_count as usize,
        ),
        outcome => outcome,
    }
}
/// Bounded nofollow UTF-8 read. Capture applies the declared digest before sharing.
fn read_utf8(locator: &str, remaining: usize) -> Outcome {
    let path = match local_path(locator) {
        Ok(path) => path,
        Err(reason) => return unavailable(None, format!("invalid_locator: {reason}"), None, 0),
    };
    let source = path.to_str().map(ToOwned::to_owned);
    if !cfg!(unix) {
        return unavailable(source, "unsupported_platform: no descriptor-relative no-symlink reader is implemented; no filesystem access attempted".into(), None, 0);
    }
    if remaining == 0 {
        return unavailable(
            source,
            "aggregate_byte_cap: no approved read budget remains".into(),
            None,
            0,
        );
    }
    let canonical = match std::fs::canonicalize(&path) {
        Ok(canonical) => canonical,
        Err(error) => return unavailable(source, format!("path_unavailable: {error}"), None, 0),
    };
    if canonical.as_os_str() != path.as_os_str() {
        return unavailable(
            source,
            "symlink_or_noncanonical_path: canonical target must exactly match the displayed path"
                .into(),
            None,
            0,
        );
    }
    let file = match open_without_symlinks(&path) {
        Ok(file) => file,
        Err(error) => return unavailable(source, format!("safe_open_failed: {error}"), None, 0),
    };
    let before = match file.metadata() {
        Ok(metadata) if metadata.is_file() => metadata,
        Ok(_) => {
            return unavailable(
                source,
                "not_regular_file: only regular file contents may be read".into(),
                None,
                0,
            );
        }
        Err(error) => return unavailable(source, format!("metadata_failed: {error}"), None, 0),
    };
    if before.len() > GOAL_ARTIFACT_MAX_FILE_BYTES as u64 {
        return unavailable(
            source,
            "file_byte_cap: artifact exceeds 64 KiB; no contents read".into(),
            None,
            0,
        );
    }
    if before.len() > remaining as u64 {
        return unavailable(source,"aggregate_byte_cap: artifact exceeds remaining 256 KiB attempt budget; no contents read".into(),None,0);
    }
    // The one-byte growth probe is inside the aggregate budget, never extra.
    let cap = (GOAL_ARTIFACT_MAX_FILE_BYTES + 1).min(remaining);
    let mut bytes = Vec::new();
    if let Err(error) = (&file).take(cap as u64).read_to_end(&mut bytes) {
        return unavailable(source, format!("read_failed: {error}"), None, bytes.len());
    }
    if bytes.len() > GOAL_ARTIFACT_MAX_FILE_BYTES {
        return unavailable(
            source,
            "file_byte_cap: artifact grew beyond 64 KiB during read".into(),
            None,
            bytes.len(),
        );
    }
    let after = match file.metadata() {
        Ok(after) => after,
        Err(error) => {
            return unavailable(
                source,
                format!("readback_metadata_failed: {error}"),
                None,
                bytes.len(),
            );
        }
    };
    let unchanged_time = match (before.modified(), after.modified()) {
        (Ok(before), Ok(after)) => before == after,
        (Err(error), _) | (_, Err(error)) => {
            return unavailable(
                source,
                format!("modification_time_unavailable: {error}"),
                None,
                bytes.len(),
            );
        }
    };
    let unchanged =
        before.len() == after.len() && after.len() == bytes.len() as u64 && unchanged_time;
    let visible_unchanged = match same_visible_file(&path, &after) {
        Ok(unchanged) => unchanged,
        Err(error) => {
            return unavailable(
                source,
                format!("visible_readback_failed: {error}"),
                None,
                bytes.len(),
            );
        }
    };
    if !unchanged || !visible_unchanged {
        return unavailable(
            source,
            "changed_during_read: file identity, length, modification time or visible path changed"
                .into(),
            None,
            bytes.len(),
        );
    }
    let actual = format!("{:x}", Sha256::digest(&bytes));
    match String::from_utf8(bytes) {
        Ok(content) => Outcome::Fetched {
            source: canonical.to_string_lossy().into_owned(),
            sha256: actual,
            byte_count: content.len() as u64,
            content,
        },
        Err(error) => unavailable(
            source,
            "invalid_utf8: contents withheld".into(),
            Some(actual),
            error.as_bytes().len(),
        ),
    }
}
#[cfg(unix)]
fn open_without_symlinks(path: &Path) -> Result<File, String> {
    use rustix::fs::{Mode, OFlags, open, openat};
    use std::path::Component;
    let mut directory = open(
        "/",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| error.to_string())?;
    let names: Vec<_> = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(name) => Some(name),
            _ => None,
        })
        .collect();
    let (last, parents) = names
        .split_last()
        .ok_or_else(|| "artifact path names no file".to_string())?;
    for name in parents {
        directory = openat(
            &directory,
            *name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| error.to_string())?;
    }
    // NONBLOCK prevents a swapped-in FIFO from hanging before fstat rejects it.
    let fd = openat(
        &directory,
        *last,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| error.to_string())?;
    Ok(File::from(fd))
}
#[cfg(not(unix))]
fn open_without_symlinks(_: &Path) -> Result<File, String> {
    Err("unsupported_platform: no descriptor-relative no-symlink reader is implemented".into())
}
#[cfg(unix)]
fn same_visible_file(path: &Path, opened: &std::fs::Metadata) -> Result<bool, String> {
    use std::os::unix::fs::MetadataExt;
    let canonical =
        std::fs::canonicalize(path).map_err(|error| format!("canonical path: {error}"))?;
    let visible =
        std::fs::symlink_metadata(path).map_err(|error| format!("visible metadata: {error}"))?;
    Ok(canonical.as_os_str() == path.as_os_str()
        && visible.is_file()
        && visible.dev() == opened.dev()
        && visible.ino() == opened.ino())
}
#[cfg(not(unix))]
fn same_visible_file(_: &Path, _: &std::fs::Metadata) -> Result<bool, String> {
    Err("unsupported_platform: no descriptor-relative no-symlink reader is implemented".into())
}
