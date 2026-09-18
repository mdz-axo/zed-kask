#![forbid(unsafe_code)]
//! `hkask-spreadsheet` — the LogiSheets-backed spreadsheet deep module
//! (plan `kask/docs/plans/logisheets-spreadsheet-capability-plan.md` §5.1).
//!
//! This crate is the one owner of spreadsheet complexity: typed-table
//! conversion, formula evaluation and recalculation, viewport extraction,
//! contained artifact resolution, immutable atomic revision publication,
//! digest and idempotency validation, and server-authoritative display-hint
//! blocks. Analytical servers, the spreadsheet MCP server, and the widget all
//! consume it without learning LogiSheets types.
//!
//! ## Threading contract (Phase 0 admission record)
//!
//! The LogiSheets `Workbook` is `!Send + !Sync` (live compile evidence:
//! `Workbook → Controller → Status → Navigator`, Rc/RefCell internals).
//! [`WorkbookService`] therefore runs the engine on one dedicated thread —
//! created and owned there, never crossing an await or a mutex — while
//! callers exchange `Send` commands and responses only. The async handle is
//! runtime-agnostic (futures oneshot), so the MCP server can await it under
//! tokio and the GPUI widget can await it on the foreground executor.
//!
//! ## Bulk-apply constraint (Phase 0 admission record)
//!
//! `handle_action` cost is superlinear in transaction size (20k payloads in
//! one transaction: 82.6s; 20 chunks of 1k: 17.2s). Every bulk conversion —
//! publish fills, applied edits, staged edits — therefore chunks at
//! [`BULK_APPLY_CHUNK_CELLS`].
//!
//! ## Boundary invariants (plan §2, §6, §7)
//!
//! - Spreadsheet edits modify derived workbook revisions only; the base
//!   revision file is never rewritten.
//! - Artifact identity is opaque and single-segment; every store resolution
//!   is contained beneath the canonical artifact root.
//! - A digest mismatch is a `Conflict`, never a silent overwrite of a stale
//!   base; a repeated idempotency identity returns the recorded result.

pub mod artifact_store;
pub mod engine;
pub mod service;

pub use hkask_types::spreadsheet::{
    AnalyticalTable, ArtifactOrigin, CellCoordinate, CellEdit, ColumnKind, SPREADSHEET_VIZ,
    SpreadsheetAccess, SpreadsheetArtifactRef, SpreadsheetBlock, SpreadsheetError,
    SpreadsheetViewport, TableColumn, TableValue,
};

pub use service::{
    PublishOptions, SpreadsheetPublication, ViewportContent, WorkbookDocument, WorkbookService,
};

pub use artifact_store::{ArtifactMeta, OperationRecord};

/// Maximum payloads applied to the engine in one transaction. Chunked because
/// `handle_action` cost is superlinear in transaction size (Phase 0 admission
/// record: 20,000 payloads in one transaction took 82.6s; 20 chunks of 1,000
/// took 17.2s).
pub const BULK_APPLY_CHUNK_CELLS: usize = 1_000;

/// The MCP server id of the central spreadsheet mutation owner (plan §4).
pub const SPREADSHEET_MCP_SERVER_ID: &str = "spreadsheet";

/// The persisted-mutation tool the widget dispatches through ToolInvoker
/// (plan §10 Phase 3).
pub const SPREADSHEET_APPLY_TOOL: &str = "spreadsheet_apply";
