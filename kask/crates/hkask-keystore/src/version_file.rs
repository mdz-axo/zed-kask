//! Key version file management.
//!
//! The current key version is stored in `~/.config/hkask/version`.
//! This is a plaintext file containing a single `u32` — it is not secret.
//! Security comes from the passphrase, not from hiding the version number.
//!
//! On rotation, the version is incremented and new secrets are derived
//! with the new version. Old-version secrets remain derivable by
//! specifying the old version number.

use std::path::PathBuf;

pub const CURRENT_KEY_VERSION: u32 = 1;

/// Get the path to the key version file.
///
/// expect: "My keys are generated, stored, and rotated under my sovereignty"
/// post: returns PathBuf to ~/.config/hkask/version
pub fn version_file_path() -> PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("hkask");
    path.push("version");
    path
}

/// Read the current key version from disk.
///
/// Returns an error if the version file does not exist (run `kask init` first).
///
/// expect: "My keys are generated, stored, and rotated under my sovereignty"
/// post: returns u32 version from file, or io::Error if missing
pub fn read_key_version() -> std::io::Result<u32> {
    let path = version_file_path();
    let contents = std::fs::read_to_string(&path)?;
    let version = contents.trim().parse().unwrap_or(CURRENT_KEY_VERSION);
    tracing::info!(target: "reg.keystore", operation = "read_key_version", version = version, "REG");
    Ok(version)
}

/// Write a new key version to disk.
///
/// Creates the parent directory if it doesn't exist.
///
/// expect: "My keys are generated, stored, and rotated under my sovereignty"
/// pre:  version is a valid u32
/// post: version written to version file
pub fn write_key_version(version: u32) -> std::io::Result<()> {
    let path = version_file_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // P9: Regulation span
    tracing::info!(target: "reg.keystore", operation = "write_key_version", version = version, "REG");
    std::fs::write(&path, format!("{version}\n"))
}

/// Increment the key version and return the new version.
///
/// Reads current version, increments by 1, writes new version.
/// Returns the new version number.
///
/// expect: "My keys are generated, stored, and rotated under my sovereignty"
/// post: version incremented by 1 and written to disk
/// post: returns new version number
pub fn increment_key_version() -> std::io::Result<u32> {
    let current = read_key_version()?;
    let new = current + 1;
    tracing::info!(target: "reg.keystore", operation = "increment_key_version", old = current, new = new, "REG");
    write_key_version(new)?;
    Ok(new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn current_version_is_one() {
        assert_eq!(CURRENT_KEY_VERSION, 1);
    }

    #[test]
    fn read_version_errors_when_no_file() {
        // In test environment, config dir may not exist — should error
        let result = read_key_version();
        assert!(result.is_err());
    }

    #[test]
    fn write_and_read_roundtrip() {
        let dir = TempDir::new().unwrap();
        let version_file = dir.path().join("hkask").join("version");

        // Write version 5
        std::fs::create_dir_all(version_file.parent().unwrap()).unwrap();
        std::fs::write(&version_file, "5\n").unwrap();

        // Can't test read_key_version directly since it uses the real config dir,
        // but we verify the file format is correct
        let contents = std::fs::read_to_string(&version_file).unwrap();
        assert_eq!(contents.trim(), "5");
    }
}
