//! Core database types — re-exported from hkask-types.
//!
//! DbError and DbProvider were moved to hkask-types::error to break the
//! circular dependency between hkask-storage, the wallet types crate, and
//! hkask-database. This module re-exports them for backward compatibility.

pub use hkask_types::{DbError, DbProvider};
