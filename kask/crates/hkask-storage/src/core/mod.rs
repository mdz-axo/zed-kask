//! Storage core — SQLite foundation for all storage modules.
//!
//! Moved from the hkask-storage-core crate during the storage consolidation.
//! Provides the `Database` connection manager, driver store macros,
//! and path sanitization.

#[macro_use]
pub mod store_macros;
pub mod database;
pub mod security;

pub use database::{Database, DatabaseError, check_passphrase, open_database, open_or_repair};
pub use security::sanitize_path;
pub use store_macros::DatabaseDriverTrait;
