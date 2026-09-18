//! Pure interaction logic for the spreadsheet widget: navigation, window
//! math, clipboard (TSV) conversion, and editor text mapping. Everything
//! here is side-effect-free and unit-tested; `view.rs` supplies the GPUI
//! plumbing around it.
//!
//! ## Windowing (plan §8: "paint only visible row/column intersections")
//!
//! The widget paints at most
//! [`WIDGET_WINDOW_ROWS`] × [`WIDGET_WINDOW_COLS`] cells. Windows are
//! aligned to fixed blocks (a cell at row 65 lands in the window starting
//! at 64, always), so navigation never thrashes the window while a user
//! walks within a block — and the painted-element count is bounded by
//! construction, the viewport-performance invariant.

use hkask_types::spreadsheet::{
    CellCoordinate, CellEdit, MAX_SHEET_COLS, MAX_SHEET_ROWS, SpreadsheetViewport, TableValue,
};

use hkask_spreadsheet::ViewportContent;

/// The fetch/paint window height. 64 × 16 = 1,024 painted cells max.
pub const WIDGET_WINDOW_ROWS: usize = 64;
/// The fetch/paint window width.
pub const WIDGET_WINDOW_COLS: usize = 16;

/// A normalized rectangular selection in absolute sheet coordinates
/// (row_start ≤ row_end, col_start ≤ col_end).
pub type Rect = (usize, usize, usize, usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
}

/// Move the active cell, clamped to the sheet ceiling (the engine accepts
/// writes anywhere inside it; beyond it is not a coordinate).
pub fn move_active(active: (usize, usize), nav: Nav) -> (usize, usize) {
    let (row, col) = active;
    let (moved_row, moved_col) = match nav {
        Nav::Up => (row.saturating_sub(1), col),
        Nav::Down => ((row + 1).min(MAX_SHEET_ROWS.saturating_sub(1)), col),
        Nav::Left => (row, col.saturating_sub(1)),
        Nav::Right => (row, (col + 1).min(MAX_SHEET_COLS.saturating_sub(1))),
        Nav::Home => (row, 0),
        Nav::End => (row, WIDGET_WINDOW_COLS.saturating_sub(1)),
        Nav::PageUp => (row.saturating_sub(WIDGET_WINDOW_ROWS), col),
        Nav::PageDown => (
            (row + WIDGET_WINDOW_ROWS).min(MAX_SHEET_ROWS.saturating_sub(1)),
            col,
        ),
    };
    (moved_row, moved_col)
}

/// The aligned fetch window covering `cell` on `sheet`.
pub fn window_covering(cell: (usize, usize), sheet: &str) -> SpreadsheetViewport {
    let start_row = cell.0 - (cell.0 % WIDGET_WINDOW_ROWS);
    let start_col = cell.1 - (cell.1 % WIDGET_WINDOW_COLS);
    let row_count = WIDGET_WINDOW_ROWS.min(MAX_SHEET_ROWS - start_row);
    let col_count = WIDGET_WINDOW_COLS.min(MAX_SHEET_COLS - start_col);
    SpreadsheetViewport::new(
        sheet.to_string(),
        start_row,
        start_col,
        row_count,
        col_count,
    )
    .expect("aligned window is always within the sheet ceiling")
}

/// Whether `cell` lies inside `window` (no re-fetch needed).
pub fn window_contains(window: &SpreadsheetViewport, cell: (usize, usize)) -> bool {
    cell.0 >= window.start_row
        && cell.0 < window.start_row + window.row_count
        && cell.1 >= window.start_col
        && cell.1 < window.start_col + window.col_count
}

/// Normalize an anchor/extent pair into a rectangle.
pub fn selection_rect(anchor: (usize, usize), extent: (usize, usize)) -> Rect {
    (
        anchor.0.min(extent.0),
        anchor.1.min(extent.1),
        anchor.0.max(extent.0),
        anchor.1.max(extent.1),
    )
}

/// The TSV/display text of a cell value. Numbers use Rust's shortest
/// round-trip formatting; booleans use the spreadsheet convention.
pub fn value_to_text(value: &TableValue) -> String {
    match value {
        TableValue::Number(n) => format!("{n}"),
        TableValue::Text(s) => s.clone(),
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

/// Interpret committed/pasted text as a cell value, mirroring the engine's
/// auto-interpretation: empty → Empty, `TRUE`/`FALSE` → Boolean,
/// numeric → Number, everything else → Text. Formulas are handled by the
/// caller (`commit_to_edit` routes `=`-prefixed text as SetFormula).
pub fn text_to_value(text: &str) -> TableValue {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return TableValue::Empty;
    }
    if trimmed.eq_ignore_ascii_case("true") {
        return TableValue::Boolean(true);
    }
    if trimmed.eq_ignore_ascii_case("false") {
        return TableValue::Boolean(false);
    }
    if let Ok(number) = trimmed.parse::<f64>() {
        if number.is_finite() {
            return TableValue::Number(number);
        }
    }
    TableValue::Text(text.to_string())
}

/// Serialize the selection rectangle (intersected with the loaded window)
/// as rectangular tab/newline data for the clipboard (plan §8).
pub fn selection_to_tsv(content: &ViewportContent, rect: Rect) -> String {
    let window = &content.window;
    let row_lo = rect.0.max(window.start_row);
    let row_hi = rect.2.min(window.start_row + window.row_count - 1);
    let col_lo = rect.1.max(window.start_col);
    let col_hi = rect.3.min(window.start_col + window.col_count - 1);
    let mut lines = Vec::new();
    for row in row_lo..=row_hi {
        let mut cells = Vec::new();
        for col in col_lo..=col_hi {
            let r = row - window.start_row;
            let c = col - window.start_col;
            cells.push(value_to_text(&content.cells[r][c]));
        }
        lines.push(cells.join("\t"));
    }
    lines.join("\n")
}

/// Parse clipboard TSV into staged cell edits anchored at `at` (plan §8:
/// "Paste stages the intended transaction"). Nonrectangular clipboard data
/// is preserved row-by-row — trailing empty cells are dropped per row
/// (a short row clears fewer cells), matching how spreadsheets paste
/// ragged selections.
pub fn tsv_to_edits(tsv: &str, at: (usize, usize), sheet: &str) -> Vec<CellEdit> {
    let mut edits = Vec::new();
    for (row_offset, line) in tsv.lines().enumerate() {
        for (col_offset, cell_text) in line.split('\t').enumerate() {
            let row = at.0 + row_offset;
            let col = at.1 + col_offset;
            if row >= MAX_SHEET_ROWS || col >= MAX_SHEET_COLS {
                return edits;
            }
            let value = if cell_text.starts_with('=') {
                // A pasted formula is a formula, not text.
                None
            } else {
                Some(text_to_value(cell_text))
            };
            edits.push(match value {
                Some(value) => CellEdit::SetCell {
                    coordinate: CellCoordinate::new(sheet.to_string(), row, col)
                        .expect("coordinate within the sheet ceiling"),
                    value,
                },
                None => CellEdit::SetFormula {
                    coordinate: CellCoordinate::new(sheet.to_string(), row, col)
                        .expect("coordinate within the sheet ceiling"),
                    formula: cell_text.to_string(),
                },
            });
        }
    }
    edits
}

/// The editor text for a cell: the formula when one exists (editing
/// `=SUM(B2:B3)` as its evaluated number would silently destroy the
/// formula), otherwise the value's display text.
pub fn editor_text(formula: &str, value: &TableValue) -> String {
    if formula.is_empty() {
        value_to_text(value)
    } else {
        formula.to_string()
    }
}

/// Interpret committed editor text as the typed edit for a cell:
/// `=`-prefixed → SetFormula; otherwise the auto-interpreted value.
pub fn commit_to_edit(text: &str, at: (usize, usize), sheet: &str) -> CellEdit {
    let coordinate =
        CellCoordinate::new(sheet.to_string(), at.0, at.1).expect("active cell is a coordinate");
    if text.starts_with('=') {
        CellEdit::SetFormula {
            coordinate,
            formula: text.to_string(),
        }
    } else {
        CellEdit::SetCell {
            coordinate,
            value: text_to_value(text),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_types::spreadsheet::SpreadsheetViewport;

    fn content(window: SpreadsheetViewport, cells: Vec<Vec<TableValue>>) -> ViewportContent {
        let formulas = vec![vec![String::new(); window.col_count]; window.row_count];
        ViewportContent {
            window,
            cells,
            formulas,
        }
    }

    #[test]
    fn navigation_moves_and_clamps() {
        assert_eq!(move_active((5, 5), Nav::Up), (4, 5));
        assert_eq!(move_active((5, 5), Nav::Down), (6, 5));
        assert_eq!(move_active((5, 5), Nav::Left), (5, 4));
        assert_eq!(move_active((5, 5), Nav::Right), (5, 6));
        assert_eq!(move_active((0, 0), Nav::Up), (0, 0));
        assert_eq!(move_active((0, 0), Nav::Left), (0, 0));
        assert_eq!(
            move_active((MAX_SHEET_ROWS - 1, MAX_SHEET_COLS - 1), Nav::Down),
            (MAX_SHEET_ROWS - 1, MAX_SHEET_COLS - 1)
        );
        assert_eq!(move_active((5, 9), Nav::Home), (5, 0));
        assert_eq!(move_active((5, 9), Nav::End), (5, WIDGET_WINDOW_COLS - 1));
        assert_eq!(
            move_active((70, 3), Nav::PageUp),
            (70 - WIDGET_WINDOW_ROWS, 3)
        );
    }

    #[test]
    fn windows_are_block_aligned_covering_and_bounded() {
        let w = window_covering((0, 0), "Main");
        assert_eq!(
            (w.start_row, w.start_col, w.row_count, w.col_count),
            (0, 0, WIDGET_WINDOW_ROWS, WIDGET_WINDOW_COLS)
        );
        assert!(window_contains(&w, (0, 0)));
        assert!(window_contains(
            &w,
            (WIDGET_WINDOW_ROWS - 1, WIDGET_WINDOW_COLS - 1)
        ));

        let w = window_covering((65, 17), "Main");
        assert_eq!(
            (w.start_row, w.start_col),
            (WIDGET_WINDOW_ROWS, WIDGET_WINDOW_COLS)
        );
        assert!(window_contains(&w, (65, 17)));
        assert!(
            !window_contains(&w, (63, 15)),
            "the previous block is not covered"
        );

        // The viewport-performance invariant: painted cells are bounded for
        // ANY navigation target, including the sheet ceiling.
        let w = window_covering((MAX_SHEET_ROWS - 1, MAX_SHEET_COLS - 1), "Main");
        assert!(w.row_count <= WIDGET_WINDOW_ROWS);
        assert!(w.col_count <= WIDGET_WINDOW_COLS);
        assert!(window_contains(
            &w,
            (MAX_SHEET_ROWS - 1, MAX_SHEET_COLS - 1)
        ));
    }

    #[test]
    fn selection_rect_normalizes_both_orders() {
        assert_eq!(selection_rect((5, 9), (2, 3)), (2, 3, 5, 9));
        assert_eq!(selection_rect((2, 3), (5, 9)), (2, 3, 5, 9));
    }

    #[test]
    fn value_text_round_trips_through_interpretation() {
        // text → value → text preserves the canonical display form of each
        // interpreted kind (booleans canonicalize to the TRUE/FALSE
        // spreadsheet convention; free text is preserved verbatim).
        for (text, expected) in [
            ("123", "123"),
            ("12.5", "12.5"),
            ("hello", "hello"),
            ("TRUE", "TRUE"),
            ("false", "FALSE"),
            ("", ""),
        ] {
            assert_eq!(
                value_to_text(&text_to_value(text)),
                expected,
                "text {text:?}"
            );
        }
        // Numbers keep shortest round-trip formatting.
        assert_eq!(value_to_text(&text_to_value("0.1")), "0.1");
    }

    #[test]
    fn selection_serializes_rectangular_tsv() {
        let window = SpreadsheetViewport::new("Main".into(), 0, 0, 4, 4).expect("window");
        let cells = (0..4)
            .map(|r| {
                (0..4)
                    .map(|c| TableValue::Number((r * 4 + c) as f64))
                    .collect()
            })
            .collect();
        let tsv = selection_to_tsv(&content(window, cells), (1, 1, 2, 2));
        assert_eq!(tsv, "5\t6\n9\t10");
    }

    #[test]
    fn paste_parses_tsv_into_edits_and_formulas() {
        let edits = tsv_to_edits("1\thello\n=SUM(A1:A2)\tTRUE", (5, 2), "Main");
        assert_eq!(edits.len(), 4);
        assert!(matches!(
            &edits[0],
            CellEdit::SetCell { value: TableValue::Number(n), .. } if *n == 1.0
        ));
        assert!(matches!(
            &edits[1],
            CellEdit::SetCell { value: TableValue::Text(t), .. } if t == "hello"
        ));
        assert!(matches!(
            &edits[2],
            CellEdit::SetFormula { formula, .. } if formula == "=SUM(A1:A2)"
        ));
        assert!(matches!(
            &edits[3],
            CellEdit::SetCell { value: TableValue::Boolean(b), .. } if *b
        ));
        // Ragged and blank cells stage what they name: a pasted empty cell
        // clears its target (spreadsheet paste semantics), it is not skipped.
        let edits = tsv_to_edits("1\n\n2", (0, 0), "Main");
        assert_eq!(edits.len(), 3);
        assert!(matches!(
            &edits[1],
            CellEdit::SetCell {
                value: TableValue::Empty,
                ..
            }
        ));
        assert!(matches!(
            &edits[2],
            CellEdit::SetCell { value: TableValue::Number(n), .. } if *n == 2.0
        ));
    }

    #[test]
    fn editor_text_prefers_the_formula() {
        assert_eq!(
            editor_text("=SUM(B2:B3)", &TableValue::Number(40000.0)),
            "=SUM(B2:B3)"
        );
        assert_eq!(editor_text("", &TableValue::Number(40000.0)), "40000");
        assert_eq!(editor_text("", &TableValue::Empty), "");
    }

    #[test]
    fn commit_routes_formulas_and_values() {
        assert!(matches!(
            commit_to_edit("=SUM(1)", (1, 1), "Main"),
            CellEdit::SetFormula { .. }
        ));
        assert!(matches!(
            commit_to_edit("42", (1, 1), "Main"),
            CellEdit::SetCell {
                value: TableValue::Number(42.0),
                ..
            }
        ));
        assert!(matches!(
            commit_to_edit("", (1, 1), "Main"),
            CellEdit::SetCell {
                value: TableValue::Empty,
                ..
            }
        ));
    }
}
