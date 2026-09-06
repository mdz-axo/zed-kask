//! Filesystem observations for gallery reconciliation. Scanning never modifies source files.

pub use hkask_storage::GalleryMode;
use hkask_storage::gallery::{AssetObservation, GalleryScan};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use walkdir::WalkDir;

const DEFAULT_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp", "gif", "bmp", "tiff"];

/// Only activation state lives in memory. Counts and asset state come from SQLite.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GalleryState {
    pub path: PathBuf,
    pub mode: GalleryMode,
    pub gallery_id: Option<String>,
}

impl GalleryState {
    pub fn new(path: PathBuf, mode: GalleryMode) -> Self {
        Self {
            path,
            mode,
            gallery_id: None,
        }
    }

    /// Validate before opening a durable gallery or replacing the active state.
    pub fn validate(&mut self) -> Result<(), crate::MediaError> {
        self.path = self.path.canonicalize().map_err(|error| {
            crate::MediaError::Io(format!("Gallery path is not accessible: {error}"))
        })?;
        if !self.path.is_dir() {
            return Err(crate::MediaError::Io(format!(
                "Gallery path is not a directory: {}",
                self.path.display()
            )));
        }
        std::fs::read_dir(&self.path)?;
        Ok(())
    }

    /// expect: An unreadable or partial scan does not make my photos disappear. [P1]
    /// pre: path is the validated canonical root
    /// post: observations hash and decode the same bytes; errors disable absence inference
    pub fn scan(&self, recursive: bool, extensions: Option<&[String]>) -> GalleryScan {
        let extensions: Vec<String> = extensions
            .map(|extensions| {
                extensions
                    .iter()
                    .map(|extension| extension.to_lowercase())
                    .collect()
            })
            .unwrap_or_else(|| {
                DEFAULT_EXTENSIONS
                    .iter()
                    .map(|extension| extension.to_string())
                    .collect()
            });
        let mut scan = GalleryScan {
            root_path: self.path.to_string_lossy().into_owned(),
            recursive,
            extensions,
            entries: Vec::new(),
            errors: Vec::new(),
        };
        let walker = WalkDir::new(&self.path)
            .max_depth(if recursive { usize::MAX } else { 1 })
            .into_iter()
            .filter_entry(|entry| entry.file_name() != ".hkask-gallery");
        for entry in walker {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    scan.errors.push(format!("Walk error: {error}"));
                    continue;
                }
            };
            // Never follow directory symlinks implicitly: their unseen subtree is uncertain.
            if entry.file_type().is_symlink() {
                scan.errors
                    .push(format!("Symlink not scanned: {}", entry.path().display()));
                continue;
            }
            if !entry.file_type().is_file() {
                continue;
            }
            let extension = entry
                .path()
                .extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or("")
                .to_lowercase();
            if !scan.extensions.contains(&extension) {
                continue;
            }
            let observed = (|| -> Result<AssetObservation, crate::MediaError> {
                let path = entry.path().canonicalize()?;
                if !path.starts_with(&self.path) {
                    return Err(crate::MediaError::Io(format!(
                        "Scan path escapes root: {}",
                        path.display()
                    )));
                }
                let bytes = crate::read_image_capped(&path.to_string_lossy())?;
                let image = image::load_from_memory(&bytes).map_err(|error| {
                    crate::MediaError::Io(format!("Decode {}: {error}", path.display()))
                })?;
                Ok(AssetObservation {
                    absolute_path: path.to_string_lossy().into_owned(),
                    hash: format!("{:x}", Sha256::digest(&bytes)),
                    width: image.width(),
                    height: image.height(),
                    format: extension,
                    size_bytes: bytes.len() as u64,
                    media_type: "image".into(),
                })
            })();
            match observed {
                Ok(observation) => scan.entries.push(observation),
                Err(error) => scan
                    .errors
                    .push(format!("{}: {error}", entry.path().display())),
            }
        }
        scan.entries
            .sort_by(|left, right| left.absolute_path.cmp(&right.absolute_path));
        scan
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// expect: Scans report valid observations, never accumulating counts. [P1]
    #[test]
    fn scan_is_repeatable_and_excludes_metadata() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        image::RgbImage::new(2, 3).save(directory.path().join("test.png"))?;
        std::fs::create_dir(directory.path().join(".hkask-gallery"))?;
        image::RgbImage::new(1, 1).save(directory.path().join(".hkask-gallery/hidden.png"))?;
        let mut state = GalleryState::new(directory.path().into(), GalleryMode::ReadOnly);
        state.validate()?;
        for _ in 0..2 {
            let scan = state.scan(true, None);
            assert_eq!(scan.entries.len(), 1);
            assert!(scan.errors.is_empty());
            assert_eq!((scan.entries[0].width, scan.entries[0].height), (2, 3));
        }
        assert!(state.scan(true, Some(&["gif".into()])).entries.is_empty());
        Ok(())
    }

    /// expect: Decode and walk failures are surfaced as uncertain coverage. [P1]
    #[test]
    fn scan_surfaces_decode_and_walk_errors() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        std::fs::write(directory.path().join("broken.png"), b"not an image")?;
        let mut state = GalleryState::new(directory.path().into(), GalleryMode::ReadOnly);
        state.validate()?;
        assert_eq!(state.scan(true, None).errors.len(), 1);
        directory.close()?;
        assert!(!state.scan(true, None).errors.is_empty());
        assert!(state.validate().is_err());
        Ok(())
    }
}
