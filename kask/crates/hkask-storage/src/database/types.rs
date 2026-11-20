//! Core database types — re-exported from hkask-types.
//!
//! DbError was moved to hkask-types::error to break a historical circular
//! dependency (the wallet types crate involved in the original cycle was
//! deleted in 219c74b180). This module re-exports it.

pub use hkask_types::DbError;
