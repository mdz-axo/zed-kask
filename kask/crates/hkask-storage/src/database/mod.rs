//! Database driver abstraction — provider-agnostic SQL execution.
//!
//! Moved from the hkask-database crate during the storage consolidation.
//! See hkask-storage lib.rs for the merged crate overview.

pub mod driver;
pub mod encrypt;
pub mod postgres;
pub(crate) mod regulation;
pub mod sqlite;
pub mod transaction;
pub mod types;
pub mod value;

pub use driver::DatabaseDriver;
pub use postgres::PostgresDriver;
pub use sqlite::{SqliteDriver, WAL_PRAGMA_BATCH, init_wal_pragmas};
pub use types::DbProvider;
