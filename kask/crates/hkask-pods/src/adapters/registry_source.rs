//! Filesystem registry source — concrete I/O adapter
//!
//! Concrete implementation that reads YAML files from the local filesystem.
//! This adapter lives behind the hexagonal boundary — the domain layer
//! never calls `std::fs` directly.

use hkask_types::InfrastructureError;
use std::fs;

/// Filesystem-backed registry source
pub struct FilesystemRegistrySource;

impl Default for FilesystemRegistrySource {
    fn default() -> Self {
        Self::new()
    }
}

impl FilesystemRegistrySource {
    /// expect: "The system loads and adapts agent registries for generative use"
    /// \[P5\] Motivating: Essentialism — filesystem registry source is a unit struct
    /// pre:  (none).
    /// post: Returns a new `FilesystemRegistrySource` (unit struct).
    pub fn new() -> Self {
        Self
    }

    /// Load the content of a YAML file at the given path
    pub fn load_yaml(&self, path: &str) -> Result<String, InfrastructureError> {
        fs::read_to_string(path).map_err(|e| InfrastructureError::Io(e.to_string()))
    }

    /// List all YAML files in the given directory
    pub fn list_yaml_files(&self, directory: &str) -> Result<Vec<String>, InfrastructureError> {
        // If the directory doesn't exist, return an empty list (matching original behavior)
        let dir = match fs::read_dir(directory) {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(InfrastructureError::Io(e.to_string())),
        };
        let mut files = Vec::new();
        for entry in dir {
            let entry = entry.map_err(|e| InfrastructureError::Io(e.to_string()))?;
            let path = entry.path();
            if (path.extension().and_then(|e| e.to_str()) == Some("yaml")
                || path.extension().and_then(|e| e.to_str()) == Some("yml"))
                && let Some(path_str) = path.to_str()
            {
                files.push(path_str.to_string());
            }
        }
        Ok(files)
    }
}
