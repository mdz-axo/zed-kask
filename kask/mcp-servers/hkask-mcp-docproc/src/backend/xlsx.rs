//! XLSX backend — uses `calamine` to read spreadsheet cells, then builds
//! `DocStructure` directly (each sheet becomes a `Table` block).

use super::{BackendError, DocumentBackend};
use calamine::{Data, Reader, open_workbook_auto};
use hkask_types::document::{Block, DocStructure, Page};

/// XLSX/XLS/ODS spreadsheet backend.
pub struct XlsxBackend;

impl DocumentBackend for XlsxBackend {
    fn format(&self) -> &'static str {
        "xlsx"
    }

    fn parse(&self, path: &str) -> Result<DocStructure, BackendError> {
        let mut workbook = open_workbook_auto(path).map_err(|e| BackendError::Parse {
            format: "xlsx",
            path: path.to_string(),
            message: e.to_string(),
        })?;

        let sheet_names = workbook.sheet_names().to_vec();
        let mut blocks = Vec::new();

        for sheet_name in &sheet_names {
            let range = match workbook.worksheet_range(sheet_name) {
                Ok(r) => r,
                Err(_) => continue,
            };
            let rows: Vec<Vec<String>> = range
                .rows()
                .map(|row| row.iter().map(cell_to_string).collect())
                .collect();
            if !rows.is_empty() {
                blocks.push(Block::Table { rows });
            }
        }

        if blocks.is_empty() {
            return Err(BackendError::Parse {
                format: "xlsx",
                path: path.to_string(),
                message: "Spreadsheet contained no data".to_string(),
            });
        }

        Ok(DocStructure {
            source_format: "xlsx".to_string(),
            pages: vec![Page {
                page_number: 1,
                blocks,
            }],
        })
    }
}

/// Convert a calamine `Data` cell to a display string.
fn cell_to_string(cell: &Data) -> String {
    match cell {
        Data::Empty => String::new(),
        Data::String(s) => s.clone(),
        Data::DateTime(dt) => dt.to_string(),
        Data::DateTimeIso(s) => s.clone(),
        Data::DurationIso(s) => s.clone(),
        Data::Int(i) => i.to_string(),
        Data::Float(f) => {
            // Avoid trailing ".0" for whole numbers
            if *f == f.trunc() && f.is_finite() {
                format!("{}", *f as i64)
            } else {
                format!("{}", f)
            }
        }
        Data::Bool(b) => b.to_string(),
        Data::Error(e) => format!("#ERR:{:?}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_name() {
        assert_eq!(XlsxBackend.format(), "xlsx");
    }

    #[test]
    fn cell_to_string_float_whole() {
        assert_eq!(cell_to_string(&Data::Float(42.0)), "42");
    }

    #[test]
    fn cell_to_string_float_fractional() {
        assert_eq!(cell_to_string(&Data::Float(3.15)), "3.15");
    }

    #[test]
    fn cell_to_string_int() {
        assert_eq!(cell_to_string(&Data::Int(7)), "7");
    }

    #[test]
    fn cell_to_string_empty() {
        assert_eq!(cell_to_string(&Data::Empty), "");
    }
}
