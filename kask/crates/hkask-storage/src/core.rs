//! Storage core — SQLite foundation for all storage modules.
//!
//! Moved from the hkask-storage-core crate during the storage consolidation.
//! Provides the `Database` connection manager, driver store macros,
//! and path sanitization.

#[macro_use]
pub mod store_macros;
pub mod connection;
pub mod security;

pub use connection::{Database, DatabaseError, embedding_dim, open_database, open_or_repair};
pub use security::sanitize_path;
pub use store_macros::DatabaseDriverTrait;
