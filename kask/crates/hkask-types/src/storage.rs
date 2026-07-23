//! Storage port types — the database abstraction layer.
//!
//! Extracted from `hkask-storage::database` to break the dependency on
//! `rusqlite` (which conflicts with zed's `libsqlite3-sys` 0.30.1).
//! hKask crates depend on `StorageDriver` (this trait), not on a concrete
//! driver. The `kask_bridge` implements `StorageDriver` over zed's `sqlez`.
//!
//! See: DIVERGENCE.md "Dependency policy" + seam-specs.md "T0.6-storage".

use crate::DbError;

// ── DbValue ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum DbValue {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
    Bool(bool),
}

impl DbValue {
    pub fn as_int(&self) -> Result<i64, DbError> {
        match self {
            Self::Integer(i) => Ok(*i),
            other => Err(DbError::Database(format!("expected integer, got {:?}", other))),
        }
    }
    pub fn as_real(&self) -> Result<f64, DbError> {
        match self {
            Self::Real(f) => Ok(*f),
            Self::Integer(i) => Ok(*i as f64),
            other => Err(DbError::Database(format!("expected real, got {:?}", other))),
        }
    }
    pub fn as_text(&self) -> Result<&str, DbError> {
        match self {
            Self::Text(s) => Ok(s),
            other => Err(DbError::Database(format!("expected text, got {:?}", other))),
        }
    }
    pub fn as_bool(&self) -> Result<bool, DbError> {
        match self {
            Self::Bool(b) => Ok(*b),
            Self::Integer(0) => Ok(false),
            Self::Integer(1) => Ok(true),
            other => Err(DbError::Database(format!("expected bool, got {:?}", other))),
        }
    }
    pub fn as_blob(&self) -> Result<&[u8], DbError> {
        match self {
            Self::Blob(b) => Ok(b),
            other => Err(DbError::Database(format!("expected blob, got {:?}", other))),
        }
    }
}

impl From<String> for DbValue { fn from(s: String) -> Self { Self::Text(s) } }
impl From<&str> for DbValue { fn from(s: &str) -> Self { Self::Text(s.to_string()) } }
impl From<i64> for DbValue { fn from(i: i64) -> Self { Self::Integer(i) } }
impl From<i32> for DbValue { fn from(i: i32) -> Self { Self::Integer(i as i64) } }
impl From<f64> for DbValue { fn from(f: f64) -> Self { Self::Real(f) } }
impl From<bool> for DbValue { fn from(b: bool) -> Self { Self::Bool(b) } }
impl From<Vec<u8>> for DbValue { fn from(b: Vec<u8>) -> Self { Self::Blob(b) } }

// ── DbRow ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct DbRow {
    columns: Vec<String>,
    values: Vec<DbValue>,
}

impl DbRow {
    pub fn new(columns: Vec<String>, values: Vec<DbValue>) -> Self { Self { columns, values } }
    pub fn get(&self, idx: usize) -> Result<&DbValue, DbError> {
        self.values.get(idx).ok_or_else(|| {
            DbError::Database(format!("column index {} out of bounds ({})", idx, self.columns.len()))
        })
    }
    pub fn get_named(&self, name: &str) -> Result<&DbValue, DbError> {
        let idx = self.columns.iter().position(|c| c == name)
            .ok_or_else(|| DbError::Database(format!("column '{}' not found", name)))?;
        self.get(idx)
    }
    pub fn len(&self) -> usize { self.values.len() }
    pub fn is_empty(&self) -> bool { self.values.is_empty() }
    pub fn column_names(&self) -> &[String] { &self.columns }
    pub fn get_str(&self, idx: usize) -> Result<&str, DbError> { self.get(idx)?.as_text() }
    pub fn get_int(&self, idx: usize) -> Result<i64, DbError> { self.get(idx)?.as_int() }
    pub fn get_real(&self, idx: usize) -> Result<f64, DbError> { self.get(idx)?.as_real() }
    pub fn get_bool(&self, idx: usize) -> Result<bool, DbError> { self.get(idx)?.as_bool() }
    pub fn get_blob(&self, idx: usize) -> Result<&[u8], DbError> { self.get(idx)?.as_blob() }
    pub fn get_str_named(&self, name: &str) -> Result<&str, DbError> { self.get_named(name)?.as_text() }
    pub fn get_int_named(&self, name: &str) -> Result<i64, DbError> { self.get_named(name)?.as_int() }
    pub fn get_real_named(&self, name: &str) -> Result<f64, DbError> { self.get_named(name)?.as_real() }
    pub fn get_bool_named(&self, name: &str) -> Result<bool, DbError> { self.get_named(name)?.as_bool() }
    pub fn get_blob_named(&self, name: &str) -> Result<&[u8], DbError> { self.get_named(name)?.as_blob() }
}

// ── StorageDriver trait (the port) ──────────────────────────────────────────

/// Provider-agnostic database driver — the storage port.
/// hKask crates use `&dyn StorageDriver` instead of raw `rusqlite::Connection`.
/// The `kask_bridge` implements this over zed's `sqlez`.
pub trait StorageDriver: Send + Sync {
    fn execute(&self, sql: &str, params: &[DbValue]) -> Result<usize, DbError>;
    fn execute_batch(&self, sql: &str) -> Result<(), DbError>;
    fn query(&self, sql: &str, params: &[DbValue]) -> Result<Vec<DbRow>, DbError>;
    fn query_optional(&self, sql: &str, params: &[DbValue]) -> Result<Option<DbRow>, DbError>;
    fn commit_tx(&self) -> Result<(), DbError>;
    fn rollback_tx(&self) -> Result<(), DbError>;
}

// ── define_driver_store! macro ───────────────────────────────────────────────

/// Generate a store struct backed by a `StorageDriver`.
/// Usage: `define_driver_store!(WalletStore);`
#[macro_export]
macro_rules! define_driver_store {
    ($name:ident) => {
        #[derive(Clone)]
        pub struct $name {
            driver: std::sync::Arc<dyn $crate::storage::StorageDriver>,
        }
        impl $name {
            pub fn from_driver(driver: std::sync::Arc<dyn $crate::storage::StorageDriver>) -> Self {
                Self::init_schema(&driver);
                Self { driver }
            }
            pub fn driver(&self) -> &std::sync::Arc<dyn $crate::storage::StorageDriver> {
                &self.driver
            }
        }
    };
}

// ── Query helpers ────────────────────────────────────────────────────────────

pub fn query_map<T, F>(driver: &dyn StorageDriver, sql: &str, params: &[DbValue], f: F) -> Result<Vec<T>, DbError>
where F: Fn(&DbRow) -> Result<T, DbError> {
    let rows = driver.query(sql, params)?;
    rows.iter().map(f).collect()
}

pub fn query_row<T, F>(driver: &dyn StorageDriver, sql: &str, params: &[DbValue], f: F) -> Result<Option<T>, DbError>
where F: Fn(&DbRow) -> Result<T, DbError> {
    driver.query_optional(sql, params)?.map(|r| f(&r)).transpose()
}