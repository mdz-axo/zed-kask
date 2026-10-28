//! FromSql/ToSql implementations for hKask domain types
//!
//! These impls live behind the `sql` feature flag so that `hkask-types`
//! doesn't force a rusqlite dependency on downstream crates that don't
//! need database support. `hkask-storage` enables this feature.
//!
//! With these impls, stores can write:
//!     let id: HMemId = row.get(0)?;
//! instead of:
//!     let id_str: String = row.get(0)?;
//!     let id = HMemId::from_str(&id_str)
//!         .map_err(|e| InfrastructureError::Database(...))?;
//!
//! (Fowler C3: Replace Primitive with Object — eliminates manual
//! parse/serialize boilerplate at every DB column boundary.)
//!
//! Note: `DateTime<Utc>` and `Option<T>` impls live in hkask-storage
//! (newtype wrappers) because Rust's orphan rules forbid implementing
//! foreign traits for foreign types.

use crate::id::core::{GoalID, UserID};
use crate::id::{BotID, EventID, HMemId, TemplateID, WebID};
use crate::visibility::Confidence;
use crate::visibility::Visibility;
use rusqlite::ToSql;
use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSqlOutput, ValueRef};
use std::str::FromStr;

// ── ID types (generic Id<T>) ──────────────────────────────────────────

macro_rules! impl_id_sql {
    ($id_type:ty) => {
        impl FromSql for $id_type {
            fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
                let s = String::column_result(value)?;
                <$id_type>::from_str(&s).map_err(|e| {
                    FromSqlError::Other(Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("invalid ID format: {e}"),
                    )))
                })
            }
        }

        impl ToSql for $id_type {
            fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
                Ok(ToSqlOutput::Owned(self.as_uuid().to_string().into()))
            }
        }
    };
}

impl_id_sql!(HMemId);
impl_id_sql!(GoalID);
impl_id_sql!(EventID);
impl_id_sql!(BotID);
impl_id_sql!(TemplateID);

impl_id_sql!(UserID);

// WebID is a separate struct (not Id<T>), needs its own impl

impl FromSql for WebID {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let s = String::column_result(value)?;
        WebID::from_str(&s).map_err(|e| {
            FromSqlError::Other(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid WebID format: {e}"),
            )))
        })
    }
}

impl ToSql for WebID {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Owned(self.as_uuid().to_string().into()))
    }
}

// ── Enum types ──────────────────────────────────────────────────────────

impl FromSql for Visibility {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let s = String::column_result(value)?;
        Visibility::parse_str(&s).ok_or_else(|| {
            FromSqlError::Other(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid visibility: {s}"),
            )))
        })
    }
}

impl ToSql for Visibility {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        self.as_str().to_sql()
    }
}

impl FromSql for Confidence {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let raw: f64 = f64::column_result(value)?;
        Ok(Confidence::new(raw))
    }
}

impl ToSql for Confidence {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        // f64::to_sql borrows from the f64, so we need an owned value.
        Ok(ToSqlOutput::Owned(self.value().into()))
    }
}
