//! Shared spreadsheet contracts — Zed-free and LogiSheets-free.
//!
//! These are the wire contracts of the spreadsheet capability
//! (`kask/docs/plans/logisheets-spreadsheet-capability-plan.md` §5–§7): the
//! analytical MCP servers (producers), the spreadsheet MCP server (mutation
//! owner), and the GPUI widget (presentation) all consume them without
//! learning engine internals. The LogiSheets-backed behavior lives in
//! `hkask-spreadsheet` (Phase 2); nothing here imports it.
//!
//! ## Admission-limit discipline
//!
//! As in `media_limits`, the caps below are admission limits: validating
//! constructors REJECT zero-size or over-cap requests where the operation
//! requires at least one item. Nothing clamps and nothing silently truncates —
//! a caller that exceeds a presentation limit receives a typed
//! [`SpreadsheetError::TooLargeForInline`] and explicitly chooses a different
//! presentation mode (plan §6: no hidden row-count threshold switches modes).
//!
//! ## Boundary invariants (plan §2)
//!
//! - [`AnalyticalTable`] carries typed columns and rows produced by an
//!   analytical operation; it never carries GPUI or LogiSheets types.
//! - [`SpreadsheetArtifactRef`] and [`SpreadsheetBlock`] never carry absolute
//!   filesystem paths or complete workbook bytes: artifact identity is opaque,
//!   single-segment, and resolved beneath the canonical spreadsheet artifact
//!   root by the engine (Phase 2).
//! - [`BlockProvenance`]-backed mutation endpoints are server-authored; the
//!   model never authors dispatch authority (an incomplete provenance is
//!   rejected at construction).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::block_provenance::BlockProvenance;

// ── Bounds (admission limits; reject, never clamp) ─────────────────────────

/// Excel-compatible sheet ceiling — the document-independent coordinate bound.
/// A coordinate at or beyond this is invalid regardless of any document.
pub const MAX_SHEET_ROWS: usize = 1_048_576;
pub const MAX_SHEET_COLS: usize = 16_384;

/// AnalyticalTable construction caps — generous for any analytical report
/// (the proving slice is hundreds of rows) and far below the sheet ceiling.
pub const MAX_TABLE_ROWS: usize = 100_000;
pub const MAX_TABLE_COLS: usize = 512;
pub const MAX_TABLE_CELLS: usize = 500_000;

/// InlineTable presentation caps — the bounded transport of a block carrying
/// rows directly (plan §6: no unbounded row arrays in a block).
pub const MAX_INLINE_ROWS: usize = 1_000;
pub const MAX_INLINE_COLS: usize = 64;
pub const MAX_INLINE_CELLS: usize = 10_000;

/// Viewport caps — what a widget pane can meaningfully receive as its initial
/// window (the widget virtualizes beyond this).
pub const MAX_VIEWPORT_ROWS: usize = 200;
pub const MAX_VIEWPORT_COLS: usize = 32;

/// One transaction's edit count — bounded transport for a mutation request.
pub const MAX_EDITS_PER_TRANSACTION: usize = 10_000;

/// Wire schema version of the spreadsheet blocks (plan §6).
pub const SPREADSHEET_BLOCK_SCHEMA_VERSION: u32 = 1;

/// The fenced-block viz discriminator for all spreadsheet presentation blocks.
pub const SPREADSHEET_VIZ: &str = "spreadsheet";

// ── Typed values ───────────────────────────────────────────────────────────

/// A typed cell value in an [`AnalyticalTable`].
///
/// Analytical outputs are data, never formulas: formulas enter through
/// [`CellEdit::SetFormula`] in an [`EditTransaction`] against a WhatIf workbook.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TableValue {
    Number(f64),
    Text(String),
    Boolean(bool),
    /// An absent reading (a holding with no metric coverage, an empty cell).
    /// Distinct from `Text("")`: a measured empty, not an empty string.
    Empty,
}

/// The declared type of a table column. A conversion hint for the engine and
/// the widget; the wire stays `TableValue`-typed per cell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColumnKind {
    Number,
    Text,
    Boolean,
    Date,
    Currency,
    Percent,
}

/// One typed column of an [`AnalyticalTable`]. `id` is the column identity:
/// stable across revisions, used for edit-coordinate addressing by column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableColumn {
    pub id: String,
    pub label: String,
    pub kind: ColumnKind,
}

// ── AnalyticalTable ────────────────────────────────────────────────────────

/// Typed columns and rows produced by an analytical operation (plan §2).
/// Rectangular by construction: every row has exactly one value per column.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnalyticalTable {
    pub title: String,
    /// The sheet the publisher intends this table to occupy in a workbook.
    pub sheet_name: String,
    pub columns: Vec<TableColumn>,
    pub rows: Vec<Vec<TableValue>>,
}

impl AnalyticalTable {
    /// The only validating constructor. Rejects (never repairs) invalid
    /// shapes: empty title/sheet, missing columns, duplicate column
    /// identities, nonrectangular rows, and over-cap dimensions.
    pub fn new(
        title: String,
        sheet_name: String,
        columns: Vec<TableColumn>,
        rows: Vec<Vec<TableValue>>,
    ) -> Result<Self, SpreadsheetError> {
        let table = Self {
            title,
            sheet_name,
            columns,
            rows,
        };
        table.validate()?;
        Ok(table)
    }

    /// Verify every construction invariant. Callers that receive a table
    /// over the wire (deserialization) run this before trusting the shape.
    pub fn validate(&self) -> Result<(), SpreadsheetError> {
        if self.title.trim().is_empty() {
            return Err(SpreadsheetError::InvalidTable {
                detail: "table title is empty".into(),
            });
        }
        if self.sheet_name.trim().is_empty() {
            return Err(SpreadsheetError::InvalidTable {
                detail: "table sheet_name is empty".into(),
            });
        }
        if self.columns.is_empty() {
            return Err(SpreadsheetError::InvalidTable {
                detail: "table has no columns".into(),
            });
        }
        let width = self.columns.len();
        if width > MAX_TABLE_COLS {
            return Err(SpreadsheetError::InvalidTable {
                detail: format!("{width} columns exceed the {MAX_TABLE_COLS}-column table cap"),
            });
        }
        let mut seen = std::collections::HashSet::with_capacity(width);
        for column in &self.columns {
            if column.id.trim().is_empty() {
                return Err(SpreadsheetError::DuplicateColumn {
                    id: "(empty id)".into(),
                });
            }
            if !seen.insert(column.id.as_str()) {
                return Err(SpreadsheetError::DuplicateColumn {
                    id: column.id.clone(),
                });
            }
        }
        if self.rows.len() > MAX_TABLE_ROWS {
            return Err(SpreadsheetError::InvalidTable {
                detail: format!(
                    "{} rows exceed the {MAX_TABLE_ROWS}-row table cap",
                    self.rows.len()
                ),
            });
        }
        for (index, row) in self.rows.iter().enumerate() {
            if row.len() != width {
                return Err(SpreadsheetError::NonRectangular {
                    detail: format!(
                        "row {index} has {} values but the table has {width} columns",
                        row.len()
                    ),
                });
            }
        }
        let cell_count = self.rows.len().saturating_mul(width);
        if cell_count > MAX_TABLE_CELLS {
            return Err(SpreadsheetError::InvalidTable {
                detail: format!("{cell_count} cells exceed the {MAX_TABLE_CELLS}-cell table cap"),
            });
        }
        // Non-finite numbers cannot cross the wire (serde_json rejects them)
        // and have no spreadsheet representation; reject at validation rather
        // than fail at serialization time.
        for (row_index, row) in self.rows.iter().enumerate() {
            for (col_index, value) in row.iter().enumerate() {
                if let TableValue::Number(n) = value {
                    if !n.is_finite() {
                        return Err(SpreadsheetError::InvalidTable {
                            detail: format!(
                                "non-finite number at row {row_index} column {col_index}"
                            ),
                        });
                    }
                }
            }
        }
        Ok(())
    }
}

// ── Coordinates and viewports ──────────────────────────────────────────────

/// A cell position, document-independent-validatable: the sheet must be
/// named and the coordinates must sit below the Excel sheet ceiling.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellCoordinate {
    pub sheet: String,
    pub row: usize,
    pub col: usize,
}

impl CellCoordinate {
    pub fn new(sheet: String, row: usize, col: usize) -> Result<Self, SpreadsheetError> {
        let coordinate = Self { sheet, row, col };
        coordinate.validate()?;
        Ok(coordinate)
    }

    pub fn validate(&self) -> Result<(), SpreadsheetError> {
        if self.sheet.trim().is_empty() {
            return Err(SpreadsheetError::InvalidCoordinate {
                detail: "coordinate sheet is empty".into(),
            });
        }
        if self.row >= MAX_SHEET_ROWS {
            return Err(SpreadsheetError::InvalidCoordinate {
                detail: format!("row {} exceeds the sheet bound {MAX_SHEET_ROWS}", self.row),
            });
        }
        if self.col >= MAX_SHEET_COLS {
            return Err(SpreadsheetError::InvalidCoordinate {
                detail: format!("col {} exceeds the sheet bound {MAX_SHEET_COLS}", self.col),
            });
        }
        Ok(())
    }
}

/// A bounded rectangular window into a sheet — the transport form of the
/// widget's initial viewport (plan §6: bounded, never whole-sheet dumps).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpreadsheetViewport {
    pub sheet: String,
    pub start_row: usize,
    pub start_col: usize,
    pub row_count: usize,
    pub col_count: usize,
}

impl SpreadsheetViewport {
    pub fn new(
        sheet: String,
        start_row: usize,
        start_col: usize,
        row_count: usize,
        col_count: usize,
    ) -> Result<Self, SpreadsheetError> {
        let viewport = Self {
            sheet,
            start_row,
            start_col,
            row_count,
            col_count,
        };
        viewport.validate()?;
        Ok(viewport)
    }

    pub fn validate(&self) -> Result<(), SpreadsheetError> {
        if self.sheet.trim().is_empty() {
            return Err(SpreadsheetError::InvalidCoordinate {
                detail: "viewport sheet is empty".into(),
            });
        }
        if self.row_count == 0 || self.col_count == 0 {
            return Err(SpreadsheetError::InvalidCoordinate {
                detail: "viewport dimensions must be nonzero".into(),
            });
        }
        if self.row_count > MAX_VIEWPORT_ROWS {
            return Err(SpreadsheetError::InvalidCoordinate {
                detail: format!(
                    "{row} rows exceed the {MAX_VIEWPORT_ROWS}-row viewport cap",
                    row = self.row_count
                ),
            });
        }
        if self.col_count > MAX_VIEWPORT_COLS {
            return Err(SpreadsheetError::InvalidCoordinate {
                detail: format!(
                    "{col} cols exceed the {MAX_VIEWPORT_COLS}-col viewport cap",
                    col = self.col_count
                ),
            });
        }
        if self.start_row.saturating_add(self.row_count) > MAX_SHEET_ROWS {
            return Err(SpreadsheetError::InvalidCoordinate {
                detail: "viewport extends past the sheet row bound".into(),
            });
        }
        if self.start_col.saturating_add(self.col_count) > MAX_SHEET_COLS {
            return Err(SpreadsheetError::InvalidCoordinate {
                detail: "viewport extends past the sheet column bound".into(),
            });
        }
        Ok(())
    }
}

// ── Artifact identity ───────────────────────────────────────────────────────

/// Opaque identity of one immutable XLSX revision beneath the canonical
/// spreadsheet artifact root (plan §7). Never carries a filesystem path —
/// the engine resolves the opaque ids; a multi-segment id is a path-escape
/// attempt and is rejected here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpreadsheetArtifactRef {
    pub artifact_id: String,
    pub revision_id: String,
    /// Hex SHA-256 of the revision bytes. The engine's save is
    /// byte-deterministic (Phase 0 admission record), so this identifies the
    /// revision exactly; a mismatch on apply is a [`SpreadsheetError::Conflict`].
    pub content_digest: String,
}

impl SpreadsheetArtifactRef {
    pub fn new(
        artifact_id: String,
        revision_id: String,
        content_digest: String,
    ) -> Result<Self, SpreadsheetError> {
        let reference = Self {
            artifact_id,
            revision_id,
            content_digest,
        };
        reference.validate()?;
        Ok(reference)
    }

    pub fn validate(&self) -> Result<(), SpreadsheetError> {
        for (kind, id) in [
            ("artifact", &self.artifact_id),
            ("revision", &self.revision_id),
        ] {
            if id.trim().is_empty() {
                return Err(SpreadsheetError::InvalidArtifactRef {
                    detail: format!("{kind} id is empty"),
                });
            }
            if id.contains('/') || id.contains('\\') || id == "." || id == ".." {
                return Err(SpreadsheetError::PathEscape { id: id.clone() });
            }
        }
        let digest_bytes = self.content_digest.as_bytes();
        if digest_bytes.len() != 64
            || !digest_bytes
                .iter()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return Err(SpreadsheetError::InvalidArtifactRef {
                detail: format!(
                    "content_digest is not a 64-char lowercase hex SHA-256: {}",
                    self.content_digest
                ),
            });
        }
        Ok(())
    }
}

// ── Presentation mode and origin ────────────────────────────────────────────

/// The presentation mode an analytical caller explicitly chooses (plan §6).
/// There is no hidden row-count threshold that silently changes modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpreadsheetAccess {
    /// Existing JSON for agent reasoning. No block, no persistence.
    DataOnly,
    /// Native bounded sortable/scrollable table. No persistence.
    InlineTable,
    /// Editable workbook with formulas and undo/redo. Immutable XLSX revisions.
    WorkbookWhatIf,
}

/// The analytical origin of a table: which server and tool produced it, with
/// which arguments. The server-authoritative input of publication and the
/// provenance a block carries about its source (distinct from the mutation
/// endpoint, which addresses the spreadsheet server).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactOrigin {
    pub server: String,
    pub tool: String,
    #[serde(default)]
    pub arguments: Value,
}

impl ArtifactOrigin {
    pub fn new(server: String, tool: String, arguments: Value) -> Result<Self, SpreadsheetError> {
        let origin = Self {
            server,
            tool,
            arguments,
        };
        origin.validate()?;
        Ok(origin)
    }

    pub fn validate(&self) -> Result<(), SpreadsheetError> {
        if self.server.trim().is_empty() || self.tool.trim().is_empty() {
            return Err(SpreadsheetError::IncompleteProvenance {
                detail: "artifact origin requires a server and a tool".into(),
            });
        }
        Ok(())
    }
}

// ── Edit transactions ───────────────────────────────────────────────────────

/// One typed cell edit inside an [`EditTransaction`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CellEdit {
    SetCell {
        coordinate: CellCoordinate,
        value: TableValue,
    },
    /// Sets a formula (`"=SUM(B2:B3)"`). Engine-level validity (parse, supported
    /// functions) is checked at apply time; the contract checks shape only.
    SetFormula {
        coordinate: CellCoordinate,
        formula: String,
    },
    ClearCell {
        coordinate: CellCoordinate,
    },
}

impl CellEdit {
    pub fn validate(&self) -> Result<(), SpreadsheetError> {
        match self {
            CellEdit::SetCell { coordinate, .. } | CellEdit::ClearCell { coordinate } => {
                coordinate.validate()
            }
            CellEdit::SetFormula {
                coordinate,
                formula,
            } => {
                coordinate.validate()?;
                if !formula.starts_with('=') {
                    return Err(SpreadsheetError::FormulaInvalid {
                        detail: format!("formula does not start with '=': {formula}"),
                    });
                }
                if formula.trim().len() < 2 {
                    return Err(SpreadsheetError::FormulaInvalid {
                        detail: "formula is empty".into(),
                    });
                }
                Ok(())
            }
        }
    }
}

/// One mutation request against a base revision (plan §7): the base artifact
/// and revision, the base content digest (optimistic concurrency — a mismatch
/// is a [`SpreadsheetError::Conflict`], never a silent overwrite of a stale
/// base), the typed edits, the idempotency key for interrupted-operation
/// reconciliation, and the access mode the caller expects.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditTransaction {
    pub base_artifact: SpreadsheetArtifactRef,
    pub idempotency_key: String,
    pub expected_access: SpreadsheetAccess,
    pub edits: Vec<CellEdit>,
}

impl EditTransaction {
    pub fn new(
        base_artifact: SpreadsheetArtifactRef,
        idempotency_key: String,
        expected_access: SpreadsheetAccess,
        edits: Vec<CellEdit>,
    ) -> Result<Self, SpreadsheetError> {
        let transaction = Self {
            base_artifact,
            idempotency_key,
            expected_access,
            edits,
        };
        transaction.validate()?;
        Ok(transaction)
    }

    pub fn validate(&self) -> Result<(), SpreadsheetError> {
        self.base_artifact.validate()?;
        if self.idempotency_key.trim().is_empty() {
            return Err(SpreadsheetError::InvalidTransaction {
                detail: "idempotency key is empty".into(),
            });
        }
        if self.edits.is_empty() {
            return Err(SpreadsheetError::InvalidTransaction {
                detail: "transaction carries no edits".into(),
            });
        }
        if self.edits.len() > MAX_EDITS_PER_TRANSACTION {
            return Err(SpreadsheetError::InvalidTransaction {
                detail: format!(
                    "{} edits exceed the {MAX_EDITS_PER_TRANSACTION}-edit transaction cap",
                    self.edits.len()
                ),
            });
        }
        for edit in &self.edits {
            edit.validate()?;
        }
        Ok(())
    }
}

// ── Presentation blocks ────────────────────────────────────────────────────

/// The bounded InlineTable presentation block: rows carried directly, no
/// persistence, no artifact, no mutation endpoint (plan §6). Rejecting an
/// over-cap table is the contract's job — the caller then explicitly chooses
/// `WorkbookWhatIf`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InlineTableBlock {
    pub viz: String,
    pub schema_version: u32,
    pub title: String,
    pub access: SpreadsheetAccess,
    pub origin: ArtifactOrigin,
    pub table: AnalyticalTable,
}

impl InlineTableBlock {
    /// Build the block from a validated table, rejecting over-cap shapes
    /// with [`SpreadsheetError::TooLargeForInline`] (never truncation).
    pub fn from_table(
        origin: ArtifactOrigin,
        table: AnalyticalTable,
    ) -> Result<Self, SpreadsheetError> {
        origin.validate()?;
        table.validate()?;
        if table.rows.len() > MAX_INLINE_ROWS {
            return Err(SpreadsheetError::TooLargeForInline {
                detail: format!(
                    "{} rows exceed the {MAX_INLINE_ROWS}-row inline block cap",
                    table.rows.len()
                ),
            });
        }
        if table.columns.len() > MAX_INLINE_COLS {
            return Err(SpreadsheetError::TooLargeForInline {
                detail: format!(
                    "{} columns exceed the {MAX_INLINE_COLS}-column inline block cap",
                    table.columns.len()
                ),
            });
        }
        let cell_count = table.rows.len().saturating_mul(table.columns.len());
        if cell_count > MAX_INLINE_CELLS {
            return Err(SpreadsheetError::TooLargeForInline {
                detail: format!(
                    "{cell_count} cells exceed the {MAX_INLINE_CELLS}-cell inline block cap"
                ),
            });
        }
        Ok(Self {
            viz: SPREADSHEET_VIZ.to_string(),
            schema_version: SPREADSHEET_BLOCK_SCHEMA_VERSION,
            title: table.title.clone(),
            access: SpreadsheetAccess::InlineTable,
            origin,
            table,
        })
    }

    pub fn validate(&self) -> Result<(), SpreadsheetError> {
        if self.viz != SPREADSHEET_VIZ {
            return Err(SpreadsheetError::InvalidBlock {
                detail: format!(
                    "viz discriminator is {:?}, expected {SPREADSHEET_VIZ:?}",
                    self.viz
                ),
            });
        }
        if self.schema_version != SPREADSHEET_BLOCK_SCHEMA_VERSION {
            return Err(SpreadsheetError::InvalidBlock {
                detail: format!(
                    "schema version {} is not {SPREADSHEET_BLOCK_SCHEMA_VERSION}",
                    self.schema_version
                ),
            });
        }
        if self.access != SpreadsheetAccess::InlineTable {
            return Err(SpreadsheetError::InvalidBlock {
                detail: "inline table block must carry the InlineTable access mode".into(),
            });
        }
        self.origin.validate()?;
        self.table.validate()?;
        Ok(())
    }
}

/// The `WorkbookWhatIf` presentation block (plan §6): opaque artifact and
/// revision identity, content digest, a bounded initial viewport, the
/// analytical origin, and the server-authored mutation endpoint. Never
/// carries absolute paths, complete workbook bytes, unbounded row arrays, or
/// model-authored mutation authority.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpreadsheetBlock {
    pub viz: String,
    pub schema_version: u32,
    pub title: String,
    pub active_sheet: String,
    pub access: SpreadsheetAccess,
    pub artifact: SpreadsheetArtifactRef,
    pub viewport: SpreadsheetViewport,
    pub origin: ArtifactOrigin,
    /// The dispatch contract for persisted mutation (`spreadsheet_apply` via
    /// the spreadsheet MCP server). Incomplete provenance is rejected here —
    /// the widget must never render a Save affordance it cannot dispatch.
    pub mutation: BlockProvenance,
}

impl SpreadsheetBlock {
    pub fn new(
        title: String,
        active_sheet: String,
        artifact: SpreadsheetArtifactRef,
        viewport: SpreadsheetViewport,
        origin: ArtifactOrigin,
        mutation: BlockProvenance,
    ) -> Result<Self, SpreadsheetError> {
        let block = Self {
            viz: SPREADSHEET_VIZ.to_string(),
            schema_version: SPREADSHEET_BLOCK_SCHEMA_VERSION,
            title,
            active_sheet,
            access: SpreadsheetAccess::WorkbookWhatIf,
            artifact,
            viewport,
            origin,
            mutation,
        };
        block.validate()?;
        Ok(block)
    }

    pub fn validate(&self) -> Result<(), SpreadsheetError> {
        if self.viz != SPREADSHEET_VIZ {
            return Err(SpreadsheetError::InvalidBlock {
                detail: format!(
                    "viz discriminator is {:?}, expected {SPREADSHEET_VIZ:?}",
                    self.viz
                ),
            });
        }
        if self.schema_version != SPREADSHEET_BLOCK_SCHEMA_VERSION {
            return Err(SpreadsheetError::InvalidBlock {
                detail: format!(
                    "schema version {} is not {SPREADSHEET_BLOCK_SCHEMA_VERSION}",
                    self.schema_version
                ),
            });
        }
        if self.title.trim().is_empty() || self.active_sheet.trim().is_empty() {
            return Err(SpreadsheetError::InvalidBlock {
                detail: "block requires a title and an active sheet".into(),
            });
        }
        if self.access != SpreadsheetAccess::WorkbookWhatIf {
            return Err(SpreadsheetError::InvalidBlock {
                detail: "workbook block must carry the WorkbookWhatIf access mode".into(),
            });
        }
        self.artifact.validate()?;
        self.viewport.validate()?;
        if self.viewport.sheet != self.active_sheet {
            return Err(SpreadsheetError::InvalidCoordinate {
                detail: format!(
                    "viewport addresses sheet {:?} but the active sheet is {:?}",
                    self.viewport.sheet, self.active_sheet
                ),
            });
        }
        self.origin.validate()?;
        if !self.mutation.is_dispatchable() {
            return Err(SpreadsheetError::IncompleteProvenance {
                detail: "mutation endpoint needs both a tool and a server".into(),
            });
        }
        Ok(())
    }
}

// ── Errors ──────────────────────────────────────────────────────────────────

/// The typed error vocabulary of the spreadsheet capability. Every variant is
/// a distinct recovery category (per the `error.rs` rule: no catch-alls).
#[derive(Debug, Clone, PartialEq, Error, Serialize, Deserialize)]
#[non_exhaustive]
pub enum SpreadsheetError {
    #[error("invalid analytical table: {detail}")]
    InvalidTable { detail: String },

    #[error("nonrectangular table: {detail}")]
    NonRectangular { detail: String },

    #[error("duplicate column identity: {id}")]
    DuplicateColumn { id: String },

    #[error("invalid coordinate: {detail}")]
    InvalidCoordinate { detail: String },

    #[error("invalid artifact reference: {detail}")]
    InvalidArtifactRef { detail: String },

    #[error("artifact identity would escape the artifact root: {id}")]
    PathEscape { id: String },

    #[error("unknown spreadsheet artifact: {artifact_id}")]
    UnknownArtifact { artifact_id: String },

    #[error("inline table exceeds the bounded transport limit: {detail}")]
    TooLargeForInline { detail: String },

    #[error("digest mismatch for {artifact_id}: expected {expected}, found {found}")]
    Conflict {
        artifact_id: String,
        expected: String,
        found: String,
    },

    #[error("incomplete provenance: {detail}")]
    IncompleteProvenance { detail: String },

    #[error("invalid formula: {detail}")]
    FormulaInvalid { detail: String },

    #[error("invalid edit transaction: {detail}")]
    InvalidTransaction { detail: String },

    #[error("access mode mismatch: {detail}")]
    AccessMismatch { detail: String },

    #[error("invalid spreadsheet block: {detail}")]
    InvalidBlock { detail: String },

    #[error("spreadsheet engine failure: {detail}")]
    Engine { detail: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exactness oracle: value identity AND byte identity must survive
    /// serialize → deserialize → reserialize (plan Phase 1: "exact
    /// serialization round-trip tests").
    fn assert_round_trips<
        T: Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
    >(
        value: &T,
    ) {
        let json = serde_json::to_string(value).expect("serialize");
        let back: T = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(&back, value, "round trip changed the value");
        let again = serde_json::to_string(&back).expect("reserialize");
        assert_eq!(json, again, "round trip changed the bytes");
    }

    fn sample_table() -> AnalyticalTable {
        AnalyticalTable::new(
            "Characteristics".into(),
            "Main".into(),
            vec![
                TableColumn {
                    id: "symbol".into(),
                    label: "Symbol".into(),
                    kind: ColumnKind::Text,
                },
                TableColumn {
                    id: "weight".into(),
                    label: "Weight".into(),
                    kind: ColumnKind::Percent,
                },
                TableColumn {
                    id: "covered".into(),
                    label: "Covered".into(),
                    kind: ColumnKind::Boolean,
                },
            ],
            vec![
                vec![
                    TableValue::Text("AAPL".into()),
                    TableValue::Number(0.42),
                    TableValue::Boolean(true),
                ],
                vec![
                    TableValue::Text("MSFT".into()),
                    TableValue::Number(0.31),
                    TableValue::Empty,
                ],
            ],
        )
        .expect("sample table is valid")
    }

    fn sample_origin() -> ArtifactOrigin {
        ArtifactOrigin::new(
            "hkask-mcp-portfolio".into(),
            "portfolio_characteristics".into(),
            serde_json::json!({"portfolio": "main"}),
        )
        .expect("sample origin is valid")
    }

    fn sample_artifact_ref() -> SpreadsheetArtifactRef {
        SpreadsheetArtifactRef::new("art-1".into(), "rev-7".into(), "a".repeat(64))
            .expect("sample artifact ref is valid")
    }

    fn sample_mutation() -> BlockProvenance {
        BlockProvenance {
            tool: Some("spreadsheet_apply".into()),
            server: Some("spreadsheet".into()),
            args: serde_json::json!({}),
            span_id: None,
        }
    }

    fn sample_block() -> SpreadsheetBlock {
        SpreadsheetBlock::new(
            "What-if staging".into(),
            "Main".into(),
            sample_artifact_ref(),
            SpreadsheetViewport::new("Main".into(), 0, 0, 50, 8).expect("viewport"),
            sample_origin(),
            sample_mutation(),
        )
        .expect("sample block is valid")
    }

    // ── exact serialization round-trips ─────────────────────────────

    #[test]
    fn all_table_value_variants_round_trip_exactly() {
        assert_round_trips(&TableValue::Number(-0.125));
        assert_round_trips(&TableValue::Text("héllo \"quoted\"".into()));
        assert_round_trips(&TableValue::Boolean(false));
        assert_round_trips(&TableValue::Empty);
    }

    #[test]
    fn access_modes_round_trip_exactly() {
        assert_round_trips(&SpreadsheetAccess::DataOnly);
        assert_round_trips(&SpreadsheetAccess::InlineTable);
        assert_round_trips(&SpreadsheetAccess::WorkbookWhatIf);
    }

    #[test]
    fn analytical_table_round_trips_exactly() {
        assert_round_trips(&sample_table());
    }

    #[test]
    fn cell_coordinate_round_trips_exactly() {
        assert_round_trips(&CellCoordinate::new("Main".into(), 5, 9).expect("coordinate"));
    }

    #[test]
    fn viewport_round_trips_exactly() {
        assert_round_trips(
            &SpreadsheetViewport::new("Main".into(), 4, 2, 100, 16).expect("viewport"),
        );
    }

    #[test]
    fn artifact_ref_round_trips_exactly() {
        assert_round_trips(&sample_artifact_ref());
    }

    #[test]
    fn edit_transaction_round_trips_exactly() {
        let transaction = EditTransaction::new(
            sample_artifact_ref(),
            "idem-1".into(),
            SpreadsheetAccess::WorkbookWhatIf,
            vec![
                CellEdit::SetCell {
                    coordinate: CellCoordinate::new("Main".into(), 1, 1).expect("coordinate"),
                    value: TableValue::Number(30000.0),
                },
                CellEdit::SetFormula {
                    coordinate: CellCoordinate::new("Main".into(), 3, 1).expect("coordinate"),
                    formula: "=SUM(B2:B3)".into(),
                },
                CellEdit::ClearCell {
                    coordinate: CellCoordinate::new("Main".into(), 4, 1).expect("coordinate"),
                },
            ],
        )
        .expect("transaction is valid");
        assert_round_trips(&transaction);
    }

    #[test]
    fn inline_table_block_round_trips_exactly() {
        let block =
            InlineTableBlock::from_table(sample_origin(), sample_table()).expect("fits inline");
        assert_round_trips(&block);
        assert_eq!(block.access, SpreadsheetAccess::InlineTable);
        assert_eq!(block.viz, SPREADSHEET_VIZ);
        assert_eq!(block.schema_version, SPREADSHEET_BLOCK_SCHEMA_VERSION);
    }

    #[test]
    fn spreadsheet_block_round_trips_exactly() {
        assert_round_trips(&sample_block());
    }

    #[test]
    fn spreadsheet_errors_round_trip_exactly() {
        assert_round_trips(&SpreadsheetError::Conflict {
            artifact_id: "art-1".into(),
            expected: "a".repeat(64),
            found: "b".repeat(64),
        });
        assert_round_trips(&SpreadsheetError::TooLargeForInline {
            detail: "1001 rows".into(),
        });
        assert_round_trips(&SpreadsheetError::DuplicateColumn { id: "sym".into() });
    }

    // ── rejections (plan Phase 1: "Reject nonrectangular rows, duplicate
    //    column identities, invalid coordinates, and oversized inline blocks")

    #[test]
    fn rejects_nonrectangular_rows() {
        let columns = sample_table().columns;
        let error = AnalyticalTable::new(
            "Broken".into(),
            "Main".into(),
            columns,
            vec![
                vec![TableValue::Empty, TableValue::Empty, TableValue::Empty],
                vec![TableValue::Empty],
            ],
        )
        .expect_err("ragged row must be rejected");
        assert!(
            matches!(error, SpreadsheetError::NonRectangular { .. }),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn rejects_duplicate_column_identities() {
        let error = AnalyticalTable::new(
            "Broken".into(),
            "Main".into(),
            vec![
                TableColumn {
                    id: "sym".into(),
                    label: "A".into(),
                    kind: ColumnKind::Text,
                },
                TableColumn {
                    id: "sym".into(),
                    label: "B".into(),
                    kind: ColumnKind::Number,
                },
            ],
            vec![],
        )
        .expect_err("duplicate ids must be rejected");
        assert!(
            matches!(error, SpreadsheetError::DuplicateColumn { .. }),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn rejects_invalid_coordinates() {
        // At or beyond the sheet ceiling.
        assert!(matches!(
            CellCoordinate::new("Main".into(), MAX_SHEET_ROWS, 0),
            Err(SpreadsheetError::InvalidCoordinate { .. })
        ));
        assert!(matches!(
            CellCoordinate::new("Main".into(), 0, MAX_SHEET_COLS),
            Err(SpreadsheetError::InvalidCoordinate { .. })
        ));
        // Empty sheet name.
        assert!(matches!(
            CellCoordinate::new("  ".into(), 0, 0),
            Err(SpreadsheetError::InvalidCoordinate { .. })
        ));
        // Zero-dimension viewport.
        assert!(matches!(
            SpreadsheetViewport::new("Main".into(), 0, 0, 0, 8),
            Err(SpreadsheetError::InvalidCoordinate { .. })
        ));
        // Viewport extending past the sheet bound.
        assert!(matches!(
            SpreadsheetViewport::new(
                "Main".into(),
                MAX_SHEET_ROWS - 10,
                0,
                MAX_VIEWPORT_ROWS,
                MAX_VIEWPORT_COLS
            ),
            Err(SpreadsheetError::InvalidCoordinate { .. })
        ));
    }

    #[test]
    fn rejects_oversized_inline_blocks() {
        let wide_enough_cols = 1usize;
        let over_rows = MAX_INLINE_ROWS + 1;
        let big_table = AnalyticalTable::new(
            "Big".into(),
            "Main".into(),
            vec![TableColumn {
                id: "n".into(),
                label: "N".into(),
                kind: ColumnKind::Number,
            }],
            (0..over_rows)
                .map(|_| vec![TableValue::Number(1.0)])
                .collect(),
        )
        .expect("table is valid at the table caps");
        let error = InlineTableBlock::from_table(sample_origin(), big_table)
            .expect_err("over-cap inline table must be rejected, not truncated");
        assert!(
            matches!(error, SpreadsheetError::TooLargeForInline { .. }),
            "unexpected error: {error}"
        );
        assert_eq!(wide_enough_cols, 1); // keeps the column-count intent explicit
    }

    #[test]
    fn rejects_oversized_inline_column_count() {
        let over_cols = MAX_INLINE_COLS + 1;
        let error = AnalyticalTable::new(
            "Wide".into(),
            "Main".into(),
            (0..over_cols)
                .map(|i| TableColumn {
                    id: format!("c{i}"),
                    label: format!("C{i}"),
                    kind: ColumnKind::Text,
                })
                .collect(),
            vec![],
        )
        .and_then(|table| InlineTableBlock::from_table(sample_origin(), table))
        .expect_err("over-cap inline columns must be rejected");
        assert!(
            matches!(error, SpreadsheetError::TooLargeForInline { .. }),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn rejects_path_escape_in_artifact_identity() {
        for bad in ["a/b", "..", ".", "a\\b"] {
            let error = SpreadsheetArtifactRef::new(bad.into(), "rev".into(), "a".repeat(64))
                .expect_err("multi-segment ids must be rejected");
            assert!(
                matches!(error, SpreadsheetError::PathEscape { .. }),
                "unexpected error for {bad:?}: {error}"
            );
        }
    }

    #[test]
    fn rejects_malformed_digest() {
        let error = SpreadsheetArtifactRef::new("art".into(), "rev".into(), "not-hex".into())
            .expect_err("malformed digests must be rejected");
        assert!(
            matches!(error, SpreadsheetError::InvalidArtifactRef { .. }),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn rejects_incomplete_mutation_provenance() {
        let error = SpreadsheetBlock::new(
            "What-if".into(),
            "Main".into(),
            sample_artifact_ref(),
            SpreadsheetViewport::new("Main".into(), 0, 0, 50, 8).expect("viewport"),
            sample_origin(),
            BlockProvenance::default(),
        )
        .expect_err("non-dispatchable mutation must be rejected");
        assert!(
            matches!(error, SpreadsheetError::IncompleteProvenance { .. }),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn rejects_empty_edit_transaction() {
        let error = EditTransaction::new(
            sample_artifact_ref(),
            "  ".into(),
            SpreadsheetAccess::WorkbookWhatIf,
            vec![],
        )
        .expect_err("empty/blank transactions must be rejected");
        assert!(
            matches!(error, SpreadsheetError::InvalidTransaction { .. }),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn rejects_non_formula_strings_in_set_formula() {
        let error = CellEdit::SetFormula {
            coordinate: CellCoordinate::new("Main".into(), 0, 0).expect("coordinate"),
            formula: "SUM(B2:B3)".into(),
        }
        .validate()
        .expect_err("formulas must start with '='");
        assert!(
            matches!(error, SpreadsheetError::FormulaInvalid { .. }),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn rejects_non_finite_numbers() {
        let error = AnalyticalTable::new(
            "Nan".into(),
            "Main".into(),
            vec![TableColumn {
                id: "n".into(),
                label: "N".into(),
                kind: ColumnKind::Number,
            }],
            vec![vec![TableValue::Number(f64::NAN)]],
        )
        .expect_err("NaN cells must be rejected at validation");
        assert!(
            matches!(error, SpreadsheetError::InvalidTable { .. }),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn validated_wire_shapes_pass_validate() {
        // A deserialized block re-validates cleanly — the post-wire contract.
        let block = sample_block();
        let json = serde_json::to_string(&block).expect("serialize");
        let back: SpreadsheetBlock = serde_json::from_str(&json).expect("deserialize");
        back.validate().expect("wire shape revalidates");
    }
}
