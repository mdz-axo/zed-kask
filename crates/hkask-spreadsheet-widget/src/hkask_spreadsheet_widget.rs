#![forbid(unsafe_code)]
#![warn(clippy::let_underscore_future)]
//! GPUI spreadsheet widget — the native presentation surface of the
//! LogiSheets-backed spreadsheet capability (plan §8).
//!
//! Renders ```` ```spreadsheet ```` display-hint blocks (the
//! [`hkask_types::spreadsheet::SpreadsheetBlock`] wire contract). Local
//! staging and viewport reads go through `hkask-spreadsheet`'s engine actor
//! in this process (off the MCP wire, off the foreground thread); persisted
//! mutations dispatch through the governed `ToolInvoker` seam to the
//! spreadsheet MCP server — never direct.
//!
//! ## Authoritative-state boundary (plan §2)
//!
//! Staged edits modify the open workbook revision's in-memory copy only.
//! Save publishes a new immutable revision through `spreadsheet_apply`; the
//! base revision and every domain store are untouched. The widget never
//! replays an operation whose outcome is unknown (§7): an `Interrupted`
//! dispatch surfaces the reconciliation instruction instead.

pub mod block;
pub mod logic;
pub mod view;

pub use block::{SpreadsheetBlockBody, parse_spreadsheet_body};
pub use view::{SpreadsheetWidget, shared_spreadsheet_service};
