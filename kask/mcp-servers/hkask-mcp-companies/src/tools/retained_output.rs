//! Opt-in, write-once source text for company research runs.
use hkask_mcp_server::server::McpToolError;
use hkask_types::agent_paths::{mcp_artifacts_subdir, resolve_under_artifacts_dir};
use std::io::Write;
use std::path::Path;

fn valid_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 80
        && !value.starts_with('-')
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
        })
}

/// Retain the exact response returned by execute_tool; never reserialize it.
/// With no run folder, preserve the original response and do no filesystem work.
pub(super) fn retain(
    run_folder: Option<&str>,
    output_key: Option<&str>,
    tool: &str,
    result: Result<String, McpToolError>,
) -> Result<String, McpToolError> {
    let Some(folder) = run_folder else {
        return result;
    };
    let text = result?;
    let key = output_key.unwrap_or(tool);
    if !valid_component(folder) || !valid_component(key) {
        return Err(McpToolError::invalid_argument(
            "run_folder and output_key must be 1–80 lowercase ASCII letters, digits, hyphens or underscores, not starting with a hyphen",
        ));
    }
    let root = resolve_under_artifacts_dir(&mcp_artifacts_subdir("companies", "research-runs"));
    write_once(&root, folder, key, text.as_bytes())?;
    Ok(text)
}

#[cfg(target_os = "linux")]
fn write_once(root: &Path, folder: &str, key: &str, bytes: &[u8]) -> Result<(), McpToolError> {
    use std::fs::{self, OpenOptions};
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;

    // Open the directory itself without following the final component. The
    // /proc/self/fd paths below stay attached to these descriptors even when
    // another process renames a parent directory between checks and writes.
    const O_NOFOLLOW: i32 = 0o400000;
    const O_DIRECTORY: i32 = 0o200000;
    let directory = |path: &Path| {
        OpenOptions::new()
            .read(true)
            .custom_flags(O_NOFOLLOW | O_DIRECTORY)
            .open(path)
            .map_err(|error| {
                McpToolError::unavailable(format!("cannot open retained-output directory: {error}"))
            })
    };
    let root_dir = directory(root)?;
    let folder_path = format!("/proc/self/fd/{}/{folder}", root_dir.as_raw_fd());
    match fs::create_dir(&folder_path) {
        Ok(()) => (),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(error) => {
            return Err(McpToolError::unavailable(format!(
                "cannot create research run folder: {error}"
            )));
        }
    }
    let run_dir = directory(Path::new(&folder_path))?;
    let path = format!("/proc/self/fd/{}/{}.txt", run_dir.as_raw_fd(), key);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(O_NOFOLLOW)
        .open(&path)
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::AlreadyExists => McpToolError::invalid_argument(format!(
                "output_key already retained in this run: {key}"
            )),
            _ => McpToolError::unavailable(format!("cannot create retained output: {error}")),
        })?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| {
            McpToolError::unavailable(format!("cannot write retained output: {error}"))
        })
}

#[cfg(not(target_os = "linux"))]
fn write_once(_root: &Path, _folder: &str, _key: &str, _bytes: &[u8]) -> Result<(), McpToolError> {
    Err(McpToolError::unavailable(
        "retained output requires Linux directory-descriptor support",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_exact_bytes_and_rejects_collision_and_escape()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        std::fs::create_dir(temp.path().join("research-runs"))?;
        let root = temp.path().join("research-runs");
        let text = b"{\"content\":\"exact \\n text\"}";
        write_once(&root, "2026-09-26-company", "quote", text)?;
        assert_eq!(
            std::fs::read(root.join("2026-09-26-company/quote.txt"))?,
            text
        );
        assert!(write_once(&root, "2026-09-26-company", "quote", b"changed").is_err());
        assert_eq!(
            std::fs::read(root.join("2026-09-26-company/quote.txt"))?,
            text
        );
        assert!(!valid_component("../outside"));
        assert!(!valid_component("/tmp"));
        assert!(!valid_component("packet.json"));
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn refuses_symlinked_run_folder_and_existing_file() -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("runs");
        std::fs::create_dir(&root)?;
        let outside = temp.path().join("outside");
        std::fs::create_dir(&outside)?;
        symlink(&outside, root.join("linked"))?;
        assert!(write_once(&root, "linked", "quote", b"escape").is_err());
        assert!(!outside.join("quote.txt").exists());
        std::fs::create_dir(root.join("safe"))?;
        symlink(outside.join("target"), root.join("safe/quote.txt"))?;
        assert!(write_once(&root, "safe", "quote", b"escape").is_err());
        assert!(!outside.join("target").exists());
        Ok(())
    }

    #[test]
    fn no_run_folder_preserves_response() -> Result<(), McpToolError> {
        let response = "{\"content\":42}".to_string();
        assert_eq!(
            retain(
                None,
                Some("invalid/path"),
                "stock_quote",
                Ok(response.clone())
            )?,
            response
        );
        Ok(())
    }
}
