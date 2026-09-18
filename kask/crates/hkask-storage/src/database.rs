//! Database driver abstraction — provider-agnostic SQL execution.
//!
//! Moved from the hkask-database crate during the storage consolidation.
//! See hkask_storage.rs for the merged crate overview. Multi-statement atomic
//! operations lease one connection and use its rusqlite transaction; separate
//! driver calls do not share transaction ownership.

pub mod driver;
pub mod sqlite;
pub mod types;
pub mod value;

pub use driver::DatabaseDriver;
pub use sqlite::{SqliteDriver, WAL_PRAGMA_BATCH, init_wal_pragmas};
