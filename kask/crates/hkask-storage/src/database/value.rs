//! Typed database values — provider-agnostic parameter and row types.

use super::types::DbError;

/// A single database value — maps to SQLite/PostgreSQL column types.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum DbValue {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
    Bool(bool),
}

impl DbValue {
    /// Extract an i64 from this value.
    ///
    /// expect: "The system provides typed access to database column values"
    /// pre:  value is DbValue::Integer
    /// post: returns Ok(i64) if Integer, Err(DbError) otherwise
    pub fn as_int(&self) -> Result<i64, DbError> {
        match self {
            Self::Integer(i) => Ok(*i),
            other => Err(DbError::Database(format!(
                "expected integer, got {:?}",
                other
            ))),
        }
    }

    /// Extract an f64 from this value (also converts Integer).
    ///
    /// expect: "The system provides typed access to database column values"
    /// pre:  value is DbValue::Real or DbValue::Integer
    /// post: returns Ok(f64) if Real or Integer, Err(DbError) otherwise
    pub fn as_real(&self) -> Result<f64, DbError> {
        match self {
            Self::Real(f) => Ok(*f),
            Self::Integer(i) => Ok(*i as f64),
            other => Err(DbError::Database(format!("expected real, got {:?}", other))),
        }
    }

    /// Extract a &str from this value.
    ///
    /// expect: "The system provides typed access to database column values"
    /// pre:  value is DbValue::Text
    /// post: returns Ok(&str) if Text, Err(DbError) otherwise
    pub fn as_text(&self) -> Result<&str, DbError> {
        match self {
            Self::Text(s) => Ok(s),
            other => Err(DbError::Database(format!("expected text, got {:?}", other))),
        }
    }

    /// Extract a bool from this value (also converts Integer 0/1).
    ///
    /// expect: "The system provides typed access to database column values"
    /// pre:  value is DbValue::Bool or DbValue::Integer(0|1)
    /// post: returns Ok(bool) if Bool or Integer(0|1), Err(DbError) otherwise
    pub fn as_bool(&self) -> Result<bool, DbError> {
        match self {
            Self::Bool(b) => Ok(*b),
            Self::Integer(0) => Ok(false),
            Self::Integer(1) => Ok(true),
            other => Err(DbError::Database(format!("expected bool, got {:?}", other))),
        }
    }

    /// Extract a byte slice from this value.
    ///
    /// expect: "The system provides typed access to database column values"
    /// pre:  value is DbValue::Blob
    /// post: returns Ok(&[u8]) if Blob, Err(DbError) otherwise
    pub fn as_blob(&self) -> Result<&[u8], DbError> {
        match self {
            Self::Blob(b) => Ok(b),
            other => Err(DbError::Database(format!("expected blob, got {:?}", other))),
        }
    }
}

/// Convert a String into a DbValue::Text.
///
/// expect: "The system converts Rust types to database values"
/// post: returns DbValue::Text(s)
impl From<String> for DbValue {
    fn from(s: String) -> Self {
        Self::Text(s)
    }
}
/// Convert an &str into a DbValue::Text.
///
/// expect: "The system converts Rust types to database values"
/// post: returns DbValue::Text(s.to_string())
impl From<&str> for DbValue {
    fn from(s: &str) -> Self {
        Self::Text(s.to_string())
    }
}
/// Convert an i64 into a DbValue::Integer.
///
/// expect: "The system converts Rust types to database values"
/// post: returns DbValue::Integer(i)
impl From<i64> for DbValue {
    fn from(i: i64) -> Self {
        Self::Integer(i)
    }
}
/// Convert an i32 into a DbValue::Integer.
///
/// expect: "The system converts Rust types to database values"
/// post: returns DbValue::Integer(i as i64)
impl From<i32> for DbValue {
    fn from(i: i32) -> Self {
        Self::Integer(i as i64)
    }
}
/// Convert an f64 into a DbValue::Real.
///
/// expect: "The system converts Rust types to database values"
/// post: returns DbValue::Real(f)
impl From<f64> for DbValue {
    fn from(f: f64) -> Self {
        Self::Real(f)
    }
}
/// Convert a bool into a DbValue::Bool.
///
/// expect: "The system converts Rust types to database values"
/// post: returns DbValue::Bool(b)
impl From<bool> for DbValue {
    fn from(b: bool) -> Self {
        Self::Bool(b)
    }
}
/// Convert a `Vec<u8>` into a DbValue::Blob.
///
/// expect: "The system converts Rust types to database values"
/// post: returns DbValue::Blob(b)
impl From<Vec<u8>> for DbValue {
    fn from(b: Vec<u8>) -> Self {
        Self::Blob(b)
    }
}

/// A single row from a query result.
#[derive(Debug, Clone)]
pub struct DbRow {
    columns: Vec<String>,
    values: Vec<DbValue>,
}

impl DbRow {
    /// Create a new DbRow from column names and values.
    ///
    /// expect: "The system provides positional and named access to database query results"
    /// pre:  columns and values have the same length
    /// post: returns a DbRow with the given columns and values
    pub fn new(columns: Vec<String>, values: Vec<DbValue>) -> Self {
        Self { columns, values }
    }

    /// Get a reference to the value at column index `idx`.
    ///
    /// expect: "The system provides positional and named access to database query results"
    /// pre:  idx is within bounds
    /// post: returns Ok(&DbValue) if in bounds, Err(DbError) otherwise
    pub fn get(&self, idx: usize) -> Result<&DbValue, DbError> {
        self.values.get(idx).ok_or_else(|| {
            DbError::Database(format!(
                "column index {} out of bounds ({} columns)",
                idx,
                self.columns.len()
            ))
        })
    }

    /// Get a reference to the value at the named column.
    ///
    /// expect: "The system provides positional and named access to database query results"
    /// pre:  a column with the given name exists
    /// post: returns Ok(&DbValue) if found, Err(DbError) otherwise
    pub fn get_named(&self, name: &str) -> Result<&DbValue, DbError> {
        let idx = self
            .columns
            .iter()
            .position(|c| c == name)
            .ok_or_else(|| DbError::Database(format!("column '{}' not found", name)))?;
        self.get(idx)
    }

    /// Return the number of columns in this row.
    ///
    /// expect: "The system provides positional and named access to database query results"
    /// pre:  none
    /// post: returns the number of columns
    pub fn len(&self) -> usize {
        self.values.len()
    }
    /// Return true if this row has zero columns.
    ///
    /// expect: "The system provides positional and named access to database query results"
    /// pre:  none
    /// post: returns true if no columns, false otherwise
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Column names from the query result.
    ///
    /// expect: "The system provides positional and named access to database query results"
    /// pre:  none
    /// post: returns the column name slice
    pub fn column_names(&self) -> &[String] {
        &self.columns
    }

    // ── Typed indexed accessors (replace row.get::<T>(idx)? from rusqlite) ──

    /// Extract a string from column `idx`.
    ///
    /// expect: "The system provides positional and named access to database query results"
    /// pre:  idx is within bounds; column value is Text
    /// post: returns Ok(&str) if valid, Err(DbError) on bounds or type error
    pub fn get_str(&self, idx: usize) -> Result<&str, DbError> {
        self.get(idx)?.as_text()
    }
    /// Extract an i64 from column `idx`.
    ///
    /// expect: "The system provides positional and named access to database query results"
    /// pre:  idx is within bounds; column value is Integer
    /// post: returns Ok(i64) if valid, Err(DbError) on bounds or type error
    pub fn get_int(&self, idx: usize) -> Result<i64, DbError> {
        self.get(idx)?.as_int()
    }
    /// Extract an f64 from column `idx`.
    ///
    /// expect: "The system provides positional and named access to database query results"
    /// pre:  idx is within bounds; column value is Real or Integer
    /// post: returns Ok(f64) if valid, Err(DbError) on bounds or type error
    pub fn get_real(&self, idx: usize) -> Result<f64, DbError> {
        self.get(idx)?.as_real()
    }
    /// Extract a bool from column `idx`.
    ///
    /// expect: "The system provides positional and named access to database query results"
    /// pre:  idx is within bounds; column value is Bool or Integer(0|1)
    /// post: returns Ok(bool) if valid, Err(DbError) on bounds or type error
    pub fn get_bool(&self, idx: usize) -> Result<bool, DbError> {
        self.get(idx)?.as_bool()
    }
    /// Extract a byte slice from column `idx`.
    ///
    /// expect: "The system provides positional and named access to database query results"
    /// pre:  idx is within bounds; column value is Blob
    /// post: returns Ok(&[u8]) if valid, Err(DbError) on bounds or type error
    pub fn get_blob(&self, idx: usize) -> Result<&[u8], DbError> {
        self.get(idx)?.as_blob()
    }

    // ── Typed named accessors ──

    /// Extract a string from the named column.
    ///
    /// expect: "The system provides positional and named access to database query results"
    /// pre:  column exists; column value is Text
    /// post: returns Ok(&str) if valid, Err(DbError) on missing column or type error
    pub fn get_str_named(&self, name: &str) -> Result<&str, DbError> {
        self.get_named(name)?.as_text()
    }
    /// Extract an i64 from the named column.
    ///
    /// expect: "The system provides positional and named access to database query results"
    /// pre:  column exists; column value is Integer
    /// post: returns Ok(i64) if valid, Err(DbError) on missing column or type error
    pub fn get_int_named(&self, name: &str) -> Result<i64, DbError> {
        self.get_named(name)?.as_int()
    }
    /// Extract an f64 from the named column.
    ///
    /// expect: "The system provides positional and named access to database query results"
    /// pre:  column exists; column value is Real or Integer
    /// post: returns Ok(f64) if valid, Err(DbError) on missing column or type error
    pub fn get_real_named(&self, name: &str) -> Result<f64, DbError> {
        self.get_named(name)?.as_real()
    }
    /// Extract a bool from the named column.
    ///
    /// expect: "The system provides positional and named access to database query results"
    /// pre:  column exists; column value is Bool or Integer(0|1)
    /// post: returns Ok(bool) if valid, Err(DbError) on missing column or type error
    pub fn get_bool_named(&self, name: &str) -> Result<bool, DbError> {
        self.get_named(name)?.as_bool()
    }

    // ── JSON accessor (common pattern: column is TEXT containing JSON) ──

    /// Deserialize a TEXT column as JSON.
    ///
    /// expect: "The system provides positional and named access to database query results"
    /// pre:  idx is within bounds; column value is valid JSON text
    /// post: returns Ok(T) if valid JSON, Err(DbError) on bounds, type, or parse error
    pub fn get_json<T: serde::de::DeserializeOwned>(&self, idx: usize) -> Result<T, DbError> {
        let s = self.get_str(idx)?;
        serde_json::from_str(s)
            .map_err(|e| DbError::Database(format!("JSON deserialize column {}: {}", idx, e)))
    }
}
