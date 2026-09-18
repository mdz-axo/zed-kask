//! LogiSheets-facing engine operations. Every function here runs on the
//! engine actor thread (the `!Send` `Workbook` never leaves it) and is
//! otherwise pure: it takes `&mut` / `&` workbook references and returns
//! contract types.
//!
//! Empirical conversion facts (probed against `logisheets-rs =1.15.1`,
//! admission-gate harness, 2026-09-18):
//!
//! - `CellInput.content` is auto-interpreted: `"123"` becomes a number,
//!   `"TRUE"` a boolean, `"=…"` a formula.
//! - A single leading apostrophe forces text and is stripped:
//!   `"'123"` → `Str("123")`, `"'abc"` → `Str("abc")`, `"''123"` →
//!   `Str("'123")`, `"'=SUM(1)"` → `Str("=SUM(1)")`. Always-prefixing
//!   therefore preserves text exactly, with no conditional logic.
//! - A default workbook owns one sheet named `"Sheet1"`; renaming by index
//!   0 works, and the engine's rename payload is a silent no-op on a miss —
//!   so the rename is verified by reading the name back.

use hkask_types::spreadsheet::{CellEdit, SpreadsheetError, SpreadsheetViewport, TableValue};

use crate::BULK_APPLY_CHUNK_CELLS;

/// The content string for one [`TableValue`] cell input.
fn cell_content(value: &TableValue) -> String {
    match value {
        TableValue::Number(n) => format!("{n}"),
        TableValue::Text(s) => format!("'{s}"),
        TableValue::Boolean(b) => {
            if *b {
                "TRUE".to_string()
            } else {
                "FALSE".to_string()
            }
        }
        TableValue::Empty => String::new(),
    }
}

/// Map a LogiSheets cell value back to a contract [`TableValue`]. An
/// engine-evaluated error (unsupported formula, bad reference) surfaces as
/// its display string — the spreadsheet convention — rather than being
/// silently dropped.
pub(crate) fn value_to_table_value(value: logisheets_rs::Value) -> TableValue {
    match value {
        logisheets_rs::Value::Number(n) => TableValue::Number(n),
        logisheets_rs::Value::Str(s) => TableValue::Text(s),
        logisheets_rs::Value::Bool(b) => TableValue::Boolean(b),
        logisheets_rs::Value::Empty => TableValue::Empty,
        logisheets_rs::Value::Error(e) => TableValue::Text(e),
    }
}

/// Fail a `handle_action` result: `StatusCode::Err` is a bare `u8` code, so
/// the code is surfaced verbatim — engine rejections are all-or-nothing per
/// transaction (LogiSheets doc: "if one of the payload is failed to be
/// executed, this EditAction will not do anything at all").
fn check_status(status: logisheets_rs::StatusCode, context: &str) -> Result<(), SpreadsheetError> {
    match status {
        logisheets_rs::StatusCode::Ok(_) => Ok(()),
        logisheets_rs::StatusCode::Err(code) => Err(SpreadsheetError::Engine {
            detail: format!("{context} rejected by the engine with code {code}"),
        }),
    }
}

/// Resolve a sheet name to its index; unknown sheets are invalid coordinates,
/// not engine failures.
fn sheet_index(workbook: &logisheets_rs::Workbook, sheet: &str) -> Result<usize, SpreadsheetError> {
    workbook
        .get_sheet_idx_by_name(sheet)
        .map_err(|error| SpreadsheetError::InvalidCoordinate {
            detail: format!("unknown sheet {sheet:?}: {error:?}"),
        })
}

/// Build a fresh workbook from an [`AnalyticalTable`]: rename sheet 0 to the
/// table's sheet name (verified by read-back), then write the header row of
/// column labels followed by the data rows — chunked at
/// [`BULK_APPLY_CHUNK_CELLS`] payloads per transaction.
pub(crate) fn build_workbook(
    table: &hkask_types::spreadsheet::AnalyticalTable,
) -> Result<logisheets_rs::Workbook, SpreadsheetError> {
    let mut workbook = logisheets_rs::Workbook::default();
    if workbook.get_sheet_count() != 1 {
        return Err(SpreadsheetError::Engine {
            detail: format!(
                "expected a fresh workbook with one sheet, found {}",
                workbook.get_sheet_count()
            ),
        });
    }

    // Rename by index; the engine's rename payload silently does nothing on
    // a miss, so verify the read-back.
    let rename = logisheets_rs::PayloadsAction::new()
        .add_payload(logisheets_rs::SheetRename {
            old_name: None,
            idx: Some(0),
            new_name: table.sheet_name.clone(),
        })
        .set_init(true);
    check_status(
        workbook
            .handle_action(logisheets_rs::EditAction::Payloads(rename))
            .status,
        "sheet rename",
    )?;
    let actual = workbook
        .get_sheet_name_by_idx(0)
        .map_err(|error| SpreadsheetError::Engine {
            detail: format!("cannot read sheet 0 name: {error:?}"),
        })?;
    if actual != table.sheet_name {
        return Err(SpreadsheetError::Engine {
            detail: format!(
                "sheet rename silently failed: sheet 0 is {actual:?}, expected {:?}",
                table.sheet_name
            ),
        });
    }

    // Header row (labels are text, always apostrophe-prefixed) then data rows.
    let mut pending: Vec<logisheets_rs::CellInput> = Vec::with_capacity(BULK_APPLY_CHUNK_CELLS);
    let flush = |pending: &mut Vec<logisheets_rs::CellInput>,
                 workbook: &mut logisheets_rs::Workbook|
     -> Result<(), SpreadsheetError> {
        if pending.is_empty() {
            return Ok(());
        }
        let mut action = logisheets_rs::PayloadsAction::new();
        for input in pending.drain(..) {
            action = action.add_payload(input);
        }
        let action = action.set_init(true);
        check_status(
            workbook
                .handle_action(logisheets_rs::EditAction::Payloads(action))
                .status,
            "bulk table fill",
        )
    };

    for (col, column) in table.columns.iter().enumerate() {
        pending.push(logisheets_rs::CellInput {
            sheet_idx: 0,
            row: 0,
            col,
            content: format!("'{}", column.label),
        });
        if pending.len() >= BULK_APPLY_CHUNK_CELLS {
            flush(&mut pending, &mut workbook)?;
        }
    }
    for (row_offset, row) in table.rows.iter().enumerate() {
        for (col, value) in row.iter().enumerate() {
            if matches!(value, TableValue::Empty) {
                continue;
            }
            pending.push(logisheets_rs::CellInput {
                sheet_idx: 0,
                row: row_offset + 1,
                col,
                content: cell_content(value),
            });
            if pending.len() >= BULK_APPLY_CHUNK_CELLS {
                flush(&mut pending, &mut workbook)?;
            }
        }
    }
    flush(&mut pending, &mut workbook)?;
    Ok(workbook)
}

/// Apply typed cell edits to an open workbook. The edits are chunked at
/// [`BULK_APPLY_CHUNK_CELLS`] payloads per transaction (superlinear engine
/// cost), each transaction all-or-nothing. `undoable` marks the transactions
/// as undo steps for staged (document) edits; persisted `apply` passes
/// `false` — its result is a new revision, not history.
///
/// Empty cells in `SetCell` clear the cell (an explicit value of
/// "nothing measured"), and `SetFormula` is parse-checked with the engine's
/// `check_formula` before application.
pub(crate) fn apply_edits(
    workbook: &mut logisheets_rs::Workbook,
    edits: &[CellEdit],
    undoable: bool,
) -> Result<(), SpreadsheetError> {
    struct Pending {
        sheet_idx: usize,
        row: usize,
        col: usize,
        content: Option<String>,
    }
    let mut pending: Vec<Pending> = Vec::with_capacity(BULK_APPLY_CHUNK_CELLS);
    let flush = |pending: &mut Vec<Pending>,
                 workbook: &mut logisheets_rs::Workbook|
     -> Result<(), SpreadsheetError> {
        if pending.is_empty() {
            return Ok(());
        }
        let mut action = logisheets_rs::PayloadsAction::new();
        for item in pending.drain(..) {
            // `CellClear` has no `Payload` impl (add_payload bound); the
            // payloads vec is public, so push the typed variant directly.
            action.payloads.push(match item.content {
                Some(content) => logisheets_rs::EditPayload::CellInput(logisheets_rs::CellInput {
                    sheet_idx: item.sheet_idx,
                    row: item.row,
                    col: item.col,
                    content,
                }),
                None => logisheets_rs::EditPayload::CellClear(logisheets_rs::CellClear {
                    sheet_idx: item.sheet_idx,
                    row: item.row,
                    col: item.col,
                }),
            });
        }
        let action = if undoable {
            action.set_undoable(true)
        } else {
            action.set_undoable(false)
        };
        check_status(
            workbook
                .handle_action(logisheets_rs::EditAction::Payloads(action))
                .status,
            "cell edit transaction",
        )
    };

    for edit in edits {
        match edit {
            CellEdit::SetCell { coordinate, value } => {
                let sheet_idx = sheet_index(workbook, &coordinate.sheet)?;
                coordinate.validate()?;
                pending.push(Pending {
                    sheet_idx,
                    row: coordinate.row,
                    col: coordinate.col,
                    content: match value {
                        TableValue::Empty => None,
                        other => Some(cell_content(other)),
                    },
                });
            }
            CellEdit::SetFormula {
                coordinate,
                formula,
            } => {
                if !workbook.check_formula(formula.clone()) {
                    return Err(SpreadsheetError::FormulaInvalid {
                        detail: format!("engine parse check rejected {formula:?}"),
                    });
                }
                let sheet_idx = sheet_index(workbook, &coordinate.sheet)?;
                coordinate.validate()?;
                pending.push(Pending {
                    sheet_idx,
                    row: coordinate.row,
                    col: coordinate.col,
                    content: Some(formula.clone()),
                });
            }
            CellEdit::ClearCell { coordinate } => {
                let sheet_idx = sheet_index(workbook, &coordinate.sheet)?;
                coordinate.validate()?;
                pending.push(Pending {
                    sheet_idx,
                    row: coordinate.row,
                    col: coordinate.col,
                    content: None,
                });
            }
        }
        if pending.len() >= BULK_APPLY_CHUNK_CELLS {
            flush(&mut pending, workbook)?;
        }
    }
    flush(&mut pending, workbook)
}

/// Extract one bounded viewport (row-major) from an open workbook.
pub(crate) fn extract_viewport(
    workbook: &logisheets_rs::Workbook,
    window: &SpreadsheetViewport,
) -> Result<crate::ViewportContent, SpreadsheetError> {
    window.validate()?;
    let sheet_idx = sheet_index(workbook, &window.sheet)?;
    let worksheet = workbook.get_sheet_by_idx(sheet_idx).map_err(|error| {
        SpreadsheetError::InvalidCoordinate {
            detail: format!("cannot open sheet {sheet_idx}: {error:?}"),
        }
    })?;
    let end_row = window.start_row + window.row_count - 1;
    let end_col = window.start_col + window.col_count - 1;
    let infos = worksheet
        .get_cell_infos(window.start_row, window.start_col, end_row, end_col)
        .map_err(|error| SpreadsheetError::Engine {
            detail: format!("viewport read failed: {error:?}"),
        })?;
    let mut cells: Vec<Vec<TableValue>> = Vec::with_capacity(window.row_count);
    for row in 0..window.row_count {
        let mut row_values = Vec::with_capacity(window.col_count);
        for col in 0..window.col_count {
            let index = row * window.col_count + col;
            row_values.push(value_to_table_value(infos[index].value.clone()));
        }
        cells.push(row_values);
    }
    Ok(crate::ViewportContent {
        window: window.clone(),
        cells,
    })
}
