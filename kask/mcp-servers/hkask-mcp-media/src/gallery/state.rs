//! Gallery state management — init, scan, info.
//!
//! Manages a local image directory with a SQLite index for metadata,
//! tags, captions, objects, and faces. Supports three modes:
//! - `read-only`: files are read-only, never modified
//! - `copy-on-write`: files can be edited, originals preserved elsewhere
//! - `destructive`: files may be edited in-place, original data may be lost

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use walkdir::WalkDir;

use hkask_types::time::now_rfc3339;

pub use hkask_storage::GalleryMode;

/// Supported image extensions for gallery scanning.
const DEFAULT_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp", "gif", "bmp", "tiff"];

/// Configuration and state for an active gallery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GalleryState {
    /// Absolute path to the gallery root directory.
    pub path: PathBuf,
    /// Operating mode.
    pub mode: GalleryMode,
    /// Path to the .hkask-gallery metadata directory.
    pub meta_dir: PathBuf,
    /// Total number of indexed images.
    pub image_count: u64,
    /// Total size of indexed images in bytes.
    pub total_size_bytes: u64,
    /// Timestamp of the last scan (ISO 8601).
    pub last_scan: Option<String>,
    /// Number of unique tags in the index.
    pub tags_count: u64,
    /// SQLite gallery ID (set after gallery_set_root creates the record).
    pub gallery_id: Option<String>,
}

/// Result of a gallery scan operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    pub added: u32,
    pub removed: u32,
    pub unchanged: u32,
    pub total: u32,
    pub errors: Vec<String>,
    /// Discovered image entries ready for SQLite persistence.
    pub entries: Vec<ImageEntry>,
}

/// A single indexed image entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageEntry {
    /// Relative path from gallery root.
    pub relative_path: String,
    /// SHA-256 checksum of file contents.
    pub checksum: String,
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// File format (extension without dot).
    pub format: String,
    /// File size in bytes.
    pub size_bytes: u64,
    /// ISO 8601 timestamp when added to index.
    pub added_at: String,
}

impl GalleryState {
    /// Create a new gallery state from a path and mode.
    ///
    /// Does not scan — use `scan()` to populate counts.
    pub fn new(path: PathBuf, mode: GalleryMode) -> Self {
        let meta_dir = path.join(".hkask-gallery");
        Self {
            path,
            mode,
            meta_dir,
            image_count: 0,
            total_size_bytes: 0,
            last_scan: None,
            tags_count: 0,
            gallery_id: None,
        }
    }

    /// Validate that the gallery path exists and is a directory.
    /// Canonicalizes the path to resolve `..` components and prevent
    /// path traversal attacks.
    pub fn validate(&mut self) -> Result<(), crate::MediaError> {
        self.path = self
            .path
            .canonicalize()
            .map_err(|e| crate::MediaError::Io(format!("Gallery path is not accessible: {}", e)))?;
        if !self.path.exists() {
            return Err(crate::MediaError::Io(format!(
                "Gallery path does not exist: {}",
                self.path.display()
            )));
        }
        if !self.path.is_dir() {
            return Err(crate::MediaError::Io(format!(
                "Gallery path is not a directory: {}",
                self.path.display()
            )));
        }
        Ok(())
    }

    /// Ensure the .hkask-gallery metadata directory exists.
    pub fn ensure_meta_dir(&self) -> Result<(), crate::MediaError> {
        std::fs::create_dir_all(&self.meta_dir).map_err(|e| {
            crate::MediaError::Io(format!(
                "Failed to create metadata directory {}: {}",
                self.meta_dir.display(),
                e
            ))
        })
    }

    /// Scan the gallery directory for images.
    ///
    /// Walks the directory tree, computes SHA-256 checksums for deduplication,
    /// and returns a ScanResult with counts.
    pub fn scan(&mut self, recursive: bool, extensions: Option<&[String]>) -> ScanResult {
        let exts: Vec<String> = extensions
            .map(|e| e.iter().map(|s| s.to_lowercase()).collect())
            .unwrap_or_else(|| DEFAULT_EXTENSIONS.iter().map(|s| s.to_string()).collect());

        let mut added = 0u32;
        let mut errors = Vec::new();
        let mut entries = Vec::new();

        let walker = if recursive {
            WalkDir::new(&self.path).into_iter()
        } else {
            WalkDir::new(&self.path).max_depth(1).into_iter()
        };

        for entry in walker {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    errors.push(format!("Walk error: {}", e));
                    continue;
                }
            };

            if !entry.file_type().is_file() {
                continue;
            }

            let path = entry.path();
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_lowercase())
                .unwrap_or_default();

            if !exts.contains(&ext) {
                continue;
            }

            // Compute checksum
            match std::fs::read(path) {
                Ok(data) => {
                    let mut hasher = Sha256::new();
                    hasher.update(&data);
                    let _checksum = format!("{:x}", hasher.finalize());

                    // Read dimensions
                    let (width, height) = match image::image_dimensions(path) {
                        Ok(dims) => dims,
                        Err(e) => {
                            errors.push(format!(
                                "Failed to read dimensions for {}: {}",
                                path.display(),
                                e
                            ));
                            continue;
                        }
                    };

                    let size_bytes = data.len() as u64;
                    self.image_count += 1;
                    self.total_size_bytes += size_bytes;
                    added += 1;

                    let entry = ImageEntry {
                        relative_path: path
                            .strip_prefix(&self.path)
                            .unwrap_or(path)
                            .to_string_lossy()
                            .to_string(),
                        checksum: _checksum,
                        width,
                        height,
                        format: ext,
                        size_bytes,
                        added_at: now_rfc3339(),
                    };
                    entries.push(entry);
                }
                Err(e) => {
                    errors.push(format!("Failed to read {}: {}", path.display(), e));
                }
            }
        }

        self.last_scan = Some(now_rfc3339());

        ScanResult {
            added,
            removed: 0,
            unchanged: 0,
            total: self.image_count as u32,
            errors,
            entries,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn setup_test_gallery() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let gallery_path = dir.path().to_path_buf();

        // Create a test image file (1x1 PNG)
        let img_path = gallery_path.join("test.png");
        let img_data: Vec<u8> = vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG signature
            0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR
            0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, // 1x1
            0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49,
            0x44, 0x41, // IDAT
            0x54, 0x08, 0xD7, 0x63, 0x60, 0x60, 0x60, 0x00, 0x00, 0x00, 0x04, 0x00, 0x01, 0x27,
            0x34, 0x0A, 0x1E, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, // IEND
            0x44, 0xAE, 0x42, 0x60, 0x82,
        ];
        let mut file = std::fs::File::create(&img_path).unwrap();
        file.write_all(&img_data).unwrap();

        (dir, gallery_path)
    }

    #[test]
    fn gallery_new_creates_state() {
        let state = GalleryState::new(PathBuf::from("/tmp/test"), GalleryMode::ReadOnly);
        assert_eq!(state.path, PathBuf::from("/tmp/test"));
        assert_eq!(state.mode, GalleryMode::ReadOnly);
        assert_eq!(state.image_count, 0);
        assert_eq!(state.total_size_bytes, 0);
        assert!(state.last_scan.is_none());
    }

    #[test]
    fn validate_rejects_missing_path() {
        let mut state = GalleryState::new(
            PathBuf::from("/nonexistent/path/12345"),
            GalleryMode::ReadOnly,
        );
        let result = state.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not accessible"));
    }

    #[test]
    fn validate_accepts_existing_dir() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = GalleryState::new(dir.path().to_path_buf(), GalleryMode::ReadOnly);
        assert!(state.validate().is_ok());
    }

    #[test]
    fn ensure_meta_dir_creates_directory() {
        let dir = tempfile::tempdir().unwrap();
        let state = GalleryState::new(dir.path().to_path_buf(), GalleryMode::ReadOnly);
        state.ensure_meta_dir().unwrap();
        assert!(state.meta_dir.exists());
        assert!(state.meta_dir.is_dir());
    }

    #[test]
    fn scan_finds_images() {
        let (_dir, gallery_path) = setup_test_gallery();
        let mut state = GalleryState::new(gallery_path.clone(), GalleryMode::ReadOnly);
        let result = state.scan(true, None);
        assert_eq!(result.added, 1);
        assert_eq!(state.image_count, 1);
        assert!(state.total_size_bytes > 0);
        assert!(state.last_scan.is_some());
    }

    #[test]
    fn scan_respects_extension_filter() {
        let (_dir, gallery_path) = setup_test_gallery();
        let mut state = GalleryState::new(gallery_path.clone(), GalleryMode::ReadOnly);
        let result = state.scan(true, Some(&["gif".to_string(), "bmp".to_string()]));
        assert_eq!(
            result.added, 0,
            "PNG should be excluded by extension filter"
        );
    }
}
