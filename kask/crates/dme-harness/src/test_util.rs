//! Test-only temp-dir guard shared by the dme-harness modules.
//!
//! A std-only temp-dir guard: a unique dir under the system temp, removed
//! on drop — the fixtures carry no external dev-dep. One implementation
//! for the modules whose tests need it (the ratchet's duplication gate
//! caught the per-module copies).

use std::path::{Path, PathBuf};

/// A temp dir removed on drop.
pub struct TempDir {
    dir: PathBuf,
}

impl TempDir {
    /// Create a unique temp dir under the system temp.
    pub fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!("dme-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).expect("create the dme-harness test temp dir");
        Self { dir: p }
    }

    /// The directory path.
    pub fn path(&self) -> &Path {
        &self.dir
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
