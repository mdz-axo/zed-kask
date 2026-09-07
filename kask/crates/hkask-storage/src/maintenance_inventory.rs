//! Read-only inventory review for passphrase maintenance. A confirmation attests
//! to a specific path set; it is not a quiescence grant or permission to publish a key.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Bound directory traversal so preview cannot silently turn into an unbounded
/// filesystem crawl. Reaching this cap is an error, never a partial inventory.
pub const MAX_INVENTORY_SCAN_ENTRIES: usize = 100_000;
const LEASE_SUFFIX: &str = ".maintenance-lock";

/// Internal parent-to-child metadata route, not an operator credential or toggle.
pub const DATABASE_CATALOG_ENV: &str = "HKASK_DB_INVENTORY_PATH";
pub const DATABASE_CATALOG_RELATIVE_PATH: &str = "maintenance/database-inventory.jsonl";
const MAX_CATALOG_BYTES: u64 = 16 * 1024 * 1024;
static CATALOG_PATH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Wire before opening managed databases. Library-only callers that do not
/// participate remain outside this catalogue and require historical attestation.
pub fn configure_database_catalog(path: PathBuf) -> Result<(), InventoryError> {
    let path = canonical_path(&path)?;
    if let Some(existing) = CATALOG_PATH.get() {
        return if existing == &path {
            Ok(())
        } else {
            Err(InventoryError::Invalid(
                "Database inventory is already configured at another path; restart is required"
                    .into(),
            ))
        };
    }
    CATALOG_PATH.set(path).map_err(|_| {
        InventoryError::Invalid("Database inventory was configured concurrently".into())
    })
}

pub fn database_catalog_path() -> Option<&'static Path> {
    CATALOG_PATH.get().map(PathBuf::as_path)
}

pub(crate) fn record_managed_database(path: &Path) -> Result<(), InventoryError> {
    if let Some(catalog) = CATALOG_PATH.get() {
        record_catalog_path(catalog, path)
    } else {
        tracing::trace!(target: "reg.storage", "Database opened outside a configured managed inventory");
        Ok(())
    }
}

fn catalog_io(path: &Path, source: std::io::Error) -> InventoryError {
    InventoryError::Io {
        path: path.into(),
        source,
    }
}

fn parse_catalog(
    file: &mut std::fs::File,
    path: &Path,
) -> Result<BTreeSet<PathBuf>, InventoryError> {
    use std::io::Read;
    let mut contents = String::new();
    file.take(MAX_CATALOG_BYTES + 1)
        .read_to_string(&mut contents)
        .map_err(|error| catalog_io(path, error))?;
    if contents.len() as u64 > MAX_CATALOG_BYTES
        || (!contents.is_empty() && !contents.ends_with('\n'))
    {
        return Err(InventoryError::Invalid(format!(
            "Inventory catalogue {} is oversized or has an incomplete last record; recover it before continuing",
            path.display()
        )));
    }
    let mut paths = BTreeSet::new();
    for (index, line) in contents.lines().enumerate() {
        let recorded: PathBuf = serde_json::from_str(line).map_err(|error| {
            InventoryError::Invalid(format!(
                "Invalid inventory record {} in {}: {error}",
                index + 1,
                path.display()
            ))
        })?;
        if !recorded.is_absolute() {
            return Err(InventoryError::Invalid(format!(
                "Non-absolute path in inventory catalogue {}",
                path.display()
            )));
        }
        paths.insert(recorded);
    }
    Ok(paths)
}

/// Read a complete catalogue under a shared lock. Missing/corrupt data is an
/// error, not an empty known inventory. This call never creates files.
pub fn read_database_catalog(path: &Path) -> Result<Vec<PathBuf>, InventoryError> {
    let mut file = std::fs::File::open(path).map_err(|error| catalog_io(path, error))?;
    file.lock_shared()
        .map_err(|error| catalog_io(path, error))?;
    Ok(parse_catalog(&mut file, path)?.into_iter().collect())
}

fn record_catalog_path(catalog: &Path, path: &Path) -> Result<(), InventoryError> {
    use std::io::{Seek, Write};
    let path = canonical_path(path)?;
    let parent = catalog.parent().ok_or_else(|| {
        InventoryError::Invalid("Inventory catalogue needs a parent directory".into())
    })?;
    std::fs::create_dir_all(parent).map_err(|error| catalog_io(parent, error))?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    // The inode stays stable across processes. Lock only metadata I/O, never
    // SQLCipher KDF/schema work; append and sync before database creation.
    let mut file = options
        .open(catalog)
        .map_err(|error| catalog_io(catalog, error))?;
    file.sync_all()
        .map_err(|error| catalog_io(catalog, error))?;
    // Persist the catalogue's directory entry and any newly created parent
    // directories before a successful registration permits DB creation.
    for directory in parent.ancestors() {
        std::fs::File::open(directory)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| catalog_io(directory, error))?;
    }
    let paths = parse_catalog(&mut file, catalog)?;
    if !paths.contains(&path) {
        let record = serde_json::to_string(&path)
            .map_err(|error| InventoryError::Invalid(error.to_string()))?
            + "\n";
        file.seek(std::io::SeekFrom::End(0))
            .map_err(|error| catalog_io(catalog, error))?;
        file.write_all(record.as_bytes())
            .map_err(|error| catalog_io(catalog, error))?;
        file.sync_all()
            .map_err(|error| catalog_io(catalog, error))?;
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum InventoryError {
    #[error("Inventory I/O at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{0}")]
    Invalid(String),
    #[error("Database inventory changed since preview; review the new inventory before confirming")]
    Stale,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct InventoryEntry {
    pub path: PathBuf,
    pub configured: bool,
    pub exists: bool,
    pub recovery_artifact: Option<String>,
    identity: Option<(u64, u64, u64)>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct DatabaseInventory {
    pub entries: Vec<InventoryEntry>,
    pub search_roots: Vec<PathBuf>,
}

/// Created only by explicit confirmation against a fresh preview. This is an
/// inventory receipt, not an indication that maintenance or rotation has run.
#[derive(Clone, Debug, serde::Serialize)]
pub struct ConfirmedInventory {
    inventory: DatabaseInventory,
    rotate_paths: Vec<PathBuf>,
    exclusions: BTreeMap<PathBuf, String>,
}

impl ConfirmedInventory {
    pub fn rotate_paths(&self) -> &[PathBuf] {
        &self.rotate_paths
    }
    pub fn exclusions(&self) -> &BTreeMap<PathBuf, String> {
        &self.exclusions
    }
    pub fn validate_current(&self, current: &DatabaseInventory) -> Result<(), InventoryError> {
        if &self.inventory != current {
            return Err(InventoryError::Stale);
        }
        Ok(())
    }
}

impl DatabaseInventory {
    /// Inspect configured paths, explicitly supplied external paths, and lease
    /// markers below the supplied roots. Never opens databases or creates files.
    /// Directory symlinks are not traversed; their databases must be supplied
    /// explicitly. Historical databases without lease markers are not discoverable.
    pub fn preview(
        configured: &[PathBuf],
        roots: &[PathBuf],
        additional: &[PathBuf],
    ) -> Result<Self, InventoryError> {
        Self::preview_with_limit(configured, roots, additional, MAX_INVENTORY_SCAN_ENTRIES)
    }

    fn preview_with_limit(
        configured: &[PathBuf],
        roots: &[PathBuf],
        additional: &[PathBuf],
        limit: usize,
    ) -> Result<Self, InventoryError> {
        let mut candidates = BTreeMap::new();
        for path in configured {
            insert_candidate(&mut candidates, path, true)?;
        }
        for path in additional {
            insert_candidate(&mut candidates, path, false)?;
        }
        let roots: BTreeSet<_> = roots
            .iter()
            .map(|path| canonical_path(path))
            .collect::<Result<_, _>>()?;
        let mut visited = BTreeSet::new();
        let mut pending: Vec<_> = roots.iter().cloned().collect();
        let mut scanned = 0usize;
        while let Some(directory) = pending.pop() {
            if !visited.insert(directory.clone()) {
                continue;
            }
            let entries = match std::fs::read_dir(&directory) {
                Ok(entries) => entries,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(source) => {
                    return Err(InventoryError::Io {
                        path: directory,
                        source,
                    });
                }
            };
            for entry in entries {
                scanned += 1;
                if scanned > limit {
                    return Err(InventoryError::Invalid(format!(
                        "Inventory scan exceeded {limit} entries; no partial inventory can be confirmed"
                    )));
                }
                let entry = entry.map_err(|source| InventoryError::Io {
                    path: directory.clone(),
                    source,
                })?;
                let kind = entry.file_type().map_err(|source| InventoryError::Io {
                    path: entry.path(),
                    source,
                })?;
                if kind.is_dir() {
                    pending.push(entry.path());
                }
                let name = entry.file_name();
                if name.to_string_lossy().ends_with(LEASE_SUFFIX) {
                    let name = name.to_str().ok_or_else(|| {
                        InventoryError::Invalid("Inventory marker path is not UTF-8".into())
                    })?;
                    let database_name = name
                        .strip_suffix(LEASE_SUFFIX)
                        .filter(|name| !name.is_empty())
                        .ok_or_else(|| {
                            InventoryError::Invalid("Invalid database lease marker name".into())
                        })?;
                    insert_candidate(&mut candidates, &directory.join(database_name), false)?;
                }
            }
        }
        let entries = candidates
            .into_iter()
            .map(|(path, configured)| inspect_entry(path, configured))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            entries,
            search_roots: roots.into_iter().collect(),
        })
    }

    /// The operator must account for historical/external paths and attest that
    /// every non-excluded entry uses the shared key. Known configured DBs cannot
    /// be excluded; independent databases require a nonempty recorded reason.
    pub fn confirm(
        &self,
        current: &Self,
        exclusions: &BTreeMap<PathBuf, String>,
        external_inventory_complete: bool,
    ) -> Result<ConfirmedInventory, InventoryError> {
        if !external_inventory_complete {
            return Err(InventoryError::Invalid("Explicit confirmation of historical/external paths and shared-key scope is required".into()));
        }
        if self != current {
            return Err(InventoryError::Stale);
        }
        let mut normalized = BTreeMap::new();
        for (path, reason) in exclusions {
            if reason.trim().is_empty() {
                return Err(InventoryError::Invalid(format!(
                    "Exclusion for {} needs a reason",
                    path.display()
                )));
            }
            let path = canonical_path(path)?;
            let entry = self
                .entries
                .iter()
                .find(|entry| entry.path == path)
                .ok_or_else(|| {
                    InventoryError::Invalid(format!(
                        "Excluded path {} is not in this inventory",
                        path.display()
                    ))
                })?;
            if entry.configured {
                return Err(InventoryError::Invalid(format!(
                    "Configured shared-key database {} cannot be excluded",
                    path.display()
                )));
            }
            if let Some(previous) = normalized.insert(path.clone(), reason.trim().to_string()) {
                if previous != reason.trim() {
                    return Err(InventoryError::Invalid(format!(
                        "Conflicting exclusions for {}",
                        path.display()
                    )));
                }
            }
        }
        let mut rotate_paths = Vec::new();
        for entry in &self.entries {
            if normalized.contains_key(&entry.path) {
                continue;
            }
            if let Some(artifact) = &entry.recovery_artifact {
                return Err(InventoryError::Invalid(format!(
                    "Recovery required at {artifact}; reconcile it before confirming maintenance"
                )));
            }
            if entry.identity.is_some_and(|(_, _, links)| links != 1) {
                return Err(InventoryError::Invalid(format!(
                    "Hard-linked database {} has ambiguous path ownership",
                    entry.path.display()
                )));
            }
            if entry.exists {
                rotate_paths.push(entry.path.clone());
            }
        }
        if rotate_paths.is_empty() {
            return Err(InventoryError::Invalid(
                "No existing shared-key databases selected for rotation".into(),
            ));
        }
        Ok(ConfirmedInventory {
            inventory: self.clone(),
            rotate_paths,
            exclusions: normalized,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exclusions_cannot_hide_configured_or_unknown_paths() {
        let directory = tempfile::tempdir().expect("tempdir");
        let required = fixture(directory.path(), "required.db");
        let other = fixture(directory.path(), "other.db");
        let preview = DatabaseInventory::preview(&[required.clone()], &[], &[other.clone()])
            .expect("preview");
        for exclusions in [
            BTreeMap::from([(required, "Must not exclude".into())]),
            BTreeMap::from([(other, "  ".into())]),
            BTreeMap::from([(directory.path().join("unknown.db"), "Unlisted".into())]),
        ] {
            assert!(preview.confirm(&preview, &exclusions, true).is_err());
        }
    }

    #[test]
    fn late_paths_invalidate_confirmation_and_receipts() {
        let directory = tempfile::tempdir().expect("tempdir");
        let required = fixture(directory.path(), "required.db");
        let roots = [directory.path().to_path_buf()];
        let before = DatabaseInventory::preview(&[required.clone()], &roots, &[]).expect("preview");
        let receipt = before
            .confirm(&before, &BTreeMap::new(), true)
            .expect("confirm");
        let other = fixture(directory.path(), "late.db");
        std::fs::write(other.with_file_name("late.db.maintenance-lock"), b"").expect("marker");
        let after = DatabaseInventory::preview(&[required], &roots, &[]).expect("new preview");
        assert_eq!(after.entries.len(), 2);
        assert!(matches!(
            before.confirm(&after, &BTreeMap::new(), true),
            Err(InventoryError::Stale)
        ));
        assert!(matches!(
            receipt.validate_current(&after),
            Err(InventoryError::Stale)
        ));
    }

    #[test]
    fn failed_scans_and_recovery_artifacts_are_not_approval() {
        let directory = tempfile::tempdir().expect("tempdir");
        let database = fixture(directory.path(), "required.db");
        assert!(DatabaseInventory::preview(&[], &[database.clone()], &[]).is_err());
        assert!(
            DatabaseInventory::preview_with_limit(&[], &[directory.path().into()], &[], 0).is_err()
        );
        let backup = directory.path().join("required.db.old");
        std::fs::write(&backup, b"preserve").expect("backup");
        let preview = DatabaseInventory::preview(&[database], &[], &[]).expect("preview");
        assert!(preview.entries[0].recovery_artifact.is_some());
        assert!(preview.confirm(&preview, &BTreeMap::new(), true).is_err());
        assert_eq!(std::fs::read(backup).expect("retained"), b"preserve");
    }

    #[cfg(unix)]
    #[test]
    fn aliases_deduplicate_and_directory_links_are_not_followed() {
        let directory = tempfile::tempdir().expect("tempdir");
        let outside = tempfile::tempdir().expect("external");
        let path = fixture(directory.path(), "data.db");
        let alias = directory.path().join("alias.db");
        std::os::unix::fs::symlink(&path, &alias).expect("alias");
        std::os::unix::fs::symlink(outside.path(), directory.path().join("outside"))
            .expect("directory link");
        fixture(outside.path(), "external.db");
        fixture(outside.path(), "external.db.maintenance-lock");
        let preview = DatabaseInventory::preview(&[path], &[directory.path().into()], &[alias])
            .expect("preview");
        assert_eq!(preview.entries.len(), 1);
        assert!(preview.entries[0].configured);
    }

    #[test]
    fn catalogue_serializes_writers_without_losing_external_paths() {
        let directory = tempfile::tempdir().expect("tempdir");
        let catalog = directory.path().join("catalog.jsonl");
        std::thread::scope(|scope| {
            for index in 0..8 {
                let catalog = &catalog;
                let path = directory.path().join(format!("external-{index}.db"));
                scope.spawn(move || {
                    record_catalog_path(catalog, &path).expect("record");
                    record_catalog_path(catalog, &path).expect("idempotent record");
                });
            }
        });
        assert_eq!(read_database_catalog(&catalog).expect("catalogue").len(), 8);
        let content = std::fs::read_to_string(&catalog).expect("content");
        assert_eq!(content.lines().count(), 8);
        std::fs::write(&catalog, "\"incomplete").expect("partial record");
        assert!(read_database_catalog(&catalog).is_err());
        assert!(record_catalog_path(&catalog, &directory.path().join("new.db")).is_err());
        assert_eq!(
            std::fs::read_to_string(catalog).expect("preserved"),
            "\"incomplete"
        );
    }

    #[test]
    #[allow(
        clippy::disallowed_methods,
        reason = "Subprocess isolates the one-time catalogue route; bounded and reaped"
    )]
    fn managed_opener_registers_before_database_creation() {
        if std::env::var_os("HKASK_INVENTORY_TEST_CHILD").is_some() {
            exercise_catalogue_opener();
            return;
        }
        let mut child = std::process::Command::new(
            std::env::current_exe().expect("test executable"),
        )
        .args([
            "--exact",
            "maintenance_inventory::tests::managed_opener_registers_before_database_creation",
            "--nocapture",
        ])
        .env("HKASK_INVENTORY_TEST_CHILD", "1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("child");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while child.try_wait().expect("status").is_none() {
            if std::time::Instant::now() >= deadline {
                child.kill().expect("kill");
                child.wait().expect("reap");
                panic!("inventory child timed out");
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let output = child.wait_with_output().expect("output");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    }

    fn exercise_catalogue_opener() {
        let directory = tempfile::tempdir().expect("isolated process data");
        let catalog = directory.path().join("catalog.jsonl");

        configure_database_catalog(catalog.clone()).expect("configure");
        let path = directory.path().join("managed.db");

        let database = crate::open_or_repair(path.to_str().expect("path"), "test-passphrase")
            .expect("managed open");

        drop(database);

        assert_eq!(
            read_database_catalog(&catalog).expect("registered"),
            vec![path]
        );
        assert!(
            !std::fs::read_to_string(&catalog)
                .expect("catalogue")
                .contains("test-passphrase")
        );

        std::fs::write(&catalog, "broken").expect("corrupt catalogue");
        let denied = directory.path().join("must-not-create.db");
        assert!(matches!(
            crate::open_or_repair(denied.to_str().expect("path"), "test-passphrase"),
            Err(crate::DatabaseError::Inventory(_))
        ));
        assert!(!denied.exists());
    }

    fn fixture(directory: &Path, name: &str) -> PathBuf {
        let path = directory.join(name);
        std::fs::write(&path, b"inventory reads metadata only").expect("fixture");
        path
    }

    #[test]
    fn inventory_requires_explicit_scope_and_preserves_files() {
        let directory = tempfile::tempdir().expect("temporary root");
        let configured = fixture(directory.path(), "managed.db");
        let external = fixture(directory.path(), "independent.db");
        let missing = directory.path().join("new/sub/missing.db");
        let preview = DatabaseInventory::preview(
            &[configured.clone(), missing.clone()],
            &[],
            &[external.clone()],
        )
        .expect("preview");
        assert!(!missing.exists());
        assert!(preview.confirm(&preview, &BTreeMap::new(), false).is_err());
        let exclusions = BTreeMap::from([(external, "Independent key".into())]);
        let receipt = preview
            .confirm(&preview, &exclusions, true)
            .expect("confirmed");
        assert_eq!(receipt.rotate_paths(), &[configured.clone()]);
        assert_eq!(
            std::fs::read(configured).expect("unchanged"),
            b"inventory reads metadata only"
        );
    }
}

fn insert_candidate(
    candidates: &mut BTreeMap<PathBuf, bool>,
    path: &Path,
    configured: bool,
) -> Result<(), InventoryError> {
    let path = canonical_path(path)?;
    candidates
        .entry(path)
        .and_modify(|required| *required |= configured)
        .or_insert(configured);
    Ok(())
}

fn canonical_path(path: &Path) -> Result<PathBuf, InventoryError> {
    if !path.is_absolute() {
        return Err(InventoryError::Invalid(format!(
            "Inventory requires an absolute path: {}",
            path.display()
        )));
    }
    let mut ancestor = path.to_path_buf();
    let mut missing = Vec::new();
    loop {
        match std::fs::canonicalize(&ancestor) {
            Ok(mut canonical) => {
                if !missing.is_empty() && !canonical.is_dir() {
                    return Err(InventoryError::Invalid(format!(
                        "Inventory ancestor {} is not a directory",
                        canonical.display()
                    )));
                }
                for component in missing.iter().rev() {
                    canonical.push(component);
                }
                if canonical.to_str().is_none() {
                    return Err(InventoryError::Invalid(
                        "Canonical inventory path is not UTF-8".into(),
                    ));
                }
                return Ok(canonical);
            }
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                // A dangling symlink is an unresolved identity, not a missing DB.
                if std::fs::symlink_metadata(&ancestor).is_ok() {
                    return Err(InventoryError::Io {
                        path: ancestor,
                        source,
                    });
                }
                let component = ancestor
                    .file_name()
                    .ok_or_else(|| {
                        InventoryError::Invalid(format!(
                            "Cannot resolve inventory path {}",
                            path.display()
                        ))
                    })?
                    .to_os_string();
                missing.push(component);
                ancestor.pop();
            }
            Err(source) => {
                return Err(InventoryError::Io {
                    path: ancestor,
                    source,
                });
            }
        }
    }
}

fn inspect_entry(path: PathBuf, configured: bool) -> Result<InventoryEntry, InventoryError> {
    let identity = match std::fs::metadata(&path) {
        Ok(metadata) => {
            if !metadata.is_file() {
                return Err(InventoryError::Invalid(format!(
                    "Database path {} is not a regular file",
                    path.display()
                )));
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                Some((metadata.dev(), metadata.ino(), metadata.nlink()))
            }
            #[cfg(not(unix))]
            {
                return Err(InventoryError::Invalid(
                    "Maintenance inventory file identity requires Unix".into(),
                ));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(source) => return Err(InventoryError::Io { path, source }),
    };
    let recovery_artifact = match crate::rotation::ensure_no_recovery_artifacts(
        path.to_str()
            .ok_or_else(|| InventoryError::Invalid("Inventory path is not UTF-8".into()))?,
    ) {
        Ok(()) => None,
        Err(crate::RotationError::RecoveryRequired { artifact, .. }) => Some(artifact),
        Err(error) => return Err(InventoryError::Invalid(error.to_string())),
    };
    Ok(InventoryEntry {
        path,
        configured,
        exists: identity.is_some(),
        recovery_artifact,
        identity,
    })
}
