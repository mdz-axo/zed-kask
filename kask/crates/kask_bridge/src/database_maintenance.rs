//! Read-only settings inventory preparation; never opens databases or reads keys.
pub use hkask_storage::{ConfirmedInventory, DatabaseInventory, InventoryError};
use std::path::{Path, PathBuf};

fn absolute(path: &Path) -> Result<PathBuf, InventoryError> {
    std::path::absolute(path).map_err(|source| InventoryError::Io {
        path: path.into(),
        source,
    })
}

pub fn database_inventory_path() -> Result<PathBuf, InventoryError> {
    if let Some(path) = hkask_storage::database_catalog_path() {
        return Ok(path.into());
    }
    absolute(&hkask_types::agent_paths::resolve_under_data_dir(
        Path::new(hkask_storage::DATABASE_CATALOG_RELATIVE_PATH),
    ))
}

pub fn initialize_database_inventory() -> Result<(), InventoryError> {
    hkask_storage::configure_database_catalog(database_inventory_path()?)
}

pub fn preview_database_inventory(
    settings: &crate::KaskSettings,
    additional: &[PathBuf],
) -> Result<DatabaseInventory, InventoryError> {
    let config = settings.mcp_env();
    let data = config
        .get("HKASK_DATA_DIR")
        .ok_or_else(|| InventoryError::Invalid("Missing resolved data root".into()))?;
    let artifacts = config
        .get("HKASK_ARTIFACTS_DIR")
        .ok_or_else(|| InventoryError::Invalid("Missing resolved artifact root".into()))?;
    let data = absolute(Path::new(data))?;
    let roots = vec![
        absolute(&crate::resolve_data_dir())?,
        absolute(&crate::resolve_artifacts_dir())?,
        data.clone(),
        absolute(Path::new(artifacts))?,
    ];
    let mut configured = crate::identity::kask_db_paths()
        .into_iter()
        .map(|(_, path)| absolute(Path::new(&path)))
        .collect::<Result<Vec<_>, _>>()?;
    for (_, variable, default) in crate::identity::managed_database_layout() {
        let path = config.get(variable).map(PathBuf::from).unwrap_or(default);
        configured.push(if path.is_absolute() {
            path
        } else {
            data.join(path)
        });
    }
    let mut external = additional.to_vec();
    external.extend(hkask_storage::read_database_catalog(
        &database_inventory_path()?,
    )?);
    DatabaseInventory::preview(&configured, &roots, &external)
}
