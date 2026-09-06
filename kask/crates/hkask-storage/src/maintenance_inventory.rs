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
    file.lock().map_err(|error| catalog_io(catalog, error))?;
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
