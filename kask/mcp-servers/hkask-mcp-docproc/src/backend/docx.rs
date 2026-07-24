//! DOCX backend — uses `docx-rs` to read the document and traverse its
//! `Document → DocumentChild` tree into `DocStructure` blocks.
//!
//! Headings are detected from paragraph style IDs (`Heading1`–`Heading6`).
//! Paragraphs without a heading style become `Block::Paragraph`. Tables become
//! `Block::Table` with cell text extracted via `Paragraph::raw_text()`.

use super::{BackendError, DocumentBackend};
use docx_rs::{
    DocumentChild, Paragraph, ParagraphProperty, RunChild, TableCellContent, TableChild,
    TableRowChild, read_docx,
};
use hkask_types::document::{Block, DocStructure, Page};

/// DOCX document backend.
pub struct DocxBackend;

impl DocumentBackend for DocxBackend {
    fn format(&self) -> &'static str {
        "docx"
    }

    fn parse(&self, path: &str) -> Result<DocStructure, BackendError> {
        let bytes = std::fs::read(path).map_err(|source| BackendError::Read {
            path: path.to_string(),
            source,
        })?;
        let docx = read_docx(&bytes).map_err(|e| BackendError::Parse {
            format: "docx",
            path: path.to_string(),
            message: e.to_string(),
        })?;

        let blocks = traverse_document(&docx.document.children);

        if blocks.is_empty() {
            return Err(BackendError::Parse {
                format: "docx",
                path: path.to_string(),
                message: "DOCX contained no extractable text".to_string(),
            });
        }

        Ok(DocStructure {
            source_format: "docx".to_string(),
            pages: vec![Page {
                page_number: 1,
                blocks,
            }],
        })
    }
}

/// Traverse `DocumentChild` nodes, producing `Block`s in document order.
fn traverse_document(children: &[DocumentChild]) -> Vec<Block> {
    let mut blocks = Vec::new();
    for child in children {
        match child {
            DocumentChild::Paragraph(para) => {
                if let Some(block) = paragraph_to_block(para) {
                    blocks.push(block);
                }
            }
            DocumentChild::Table(table) => {
                let rows = table_to_rows(table.rows.as_slice());
                if !rows.is_empty() {
                    blocks.push(Block::Table { rows });
                }
            }
            // Bookmark/Comment/Section/TOC/StructuredDataTag — no text content
            _ => {}
        }
    }
    blocks
}

/// Convert a DOCX paragraph to a `Block`.
///
/// Returns `None` for empty paragraphs (no text). Heading detection uses the
/// paragraph's style ID — DOCX headings have style IDs like "Heading1",
/// "Heading2", etc. (case-insensitive match).
fn paragraph_to_block(para: &Paragraph) -> Option<Block> {
    let text = paragraph_text(para);
    if text.trim().is_empty() {
        return None;
    }
    let style = paragraph_style_id(&para.property);
    if let Some(level) = heading_level_from_style(style) {
        Some(Block::Heading { level, text })
    } else {
        Some(Block::Paragraph { text })
    }
}

/// Extract text from a paragraph by traversing its runs.
///
/// Uses `raw_text()` when available (concatenates all run text). For
/// paragraphs with hyperlinks (which `raw_text` may skip), we also traverse
/// `ParagraphChild` manually as a fallback.
fn paragraph_text(para: &Paragraph) -> String {
    // `raw_text()` handles the common case (runs with text).
    let raw = para.raw_text();
    if !raw.is_empty() {
        return raw;
    }
    // Fallback: traverse children manually for hyperlinks and other non-run text.
    let mut text = String::new();
    for child in &para.children {
        match child {
            docx_rs::ParagraphChild::Run(run) => {
                for rc in &run.children {
                    if let RunChild::Text(t) = rc {
                        text.push_str(&t.text);
                    }
                }
            }
            docx_rs::ParagraphChild::Hyperlink(hyper) => {
                for pc in &hyper.children {
                    if let docx_rs::ParagraphChild::Run(run) = pc {
                        for rc in &run.children {
                            if let RunChild::Text(t) = rc {
                                text.push_str(&t.text);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    text
}

/// Extract the style ID from a `ParagraphProperty`.
///
/// `ParagraphProperty` has a `style: Option<ParagraphStyle>` field;
/// `ParagraphStyle` has a `val: String` field.
fn paragraph_style_id(prop: &ParagraphProperty) -> Option<&str> {
    prop.style.as_ref().map(|s| s.val.as_str())
}

/// Map a DOCX style ID to a heading level (1–6).
///
/// Matches "Heading1"–"Heading6" (case-insensitive). Also matches "Title" as
/// level 1. Returns `None` for non-heading styles.
fn heading_level_from_style(style: Option<&str>) -> Option<u8> {
    let style = style?;
    let lower = style.to_ascii_lowercase();
    if lower == "title" {
        return Some(1);
    }
    lower
        .strip_prefix("heading")
        .and_then(|n| n.parse::<u8>().ok())
}

/// Convert a table's `TableChild` rows into `Vec<Vec<String>>`.
fn table_to_rows(rows: &[TableChild]) -> Vec<Vec<String>> {
    rows.iter()
        .filter_map(|child| match child {
            TableChild::TableRow(row) => {
                let cells: Vec<String> = row
                    .cells
                    .iter()
                    .map(|rc| match rc {
                        TableRowChild::TableCell(cell) => table_cell_text(cell),
                    })
                    .collect();
                if cells.is_empty() { None } else { Some(cells) }
            }
        })
        .collect()
}

/// Extract text from a table cell by concatenating all paragraph text.
fn table_cell_text(cell: &docx_rs::TableCell) -> String {
    let mut text = String::new();
    for content in &cell.children {
        match content {
            TableCellContent::Paragraph(para) => {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(&paragraph_text(para));
            }
            TableCellContent::Table(inner) => {
                // Nested table — flatten with a separator
                let inner_rows = table_to_rows(inner.rows.as_slice());
                for row in inner_rows {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(&row.join("\t"));
                }
            }
            _ => {}
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_name() {
        assert_eq!(DocxBackend.format(), "docx");
    }

    #[test]
    fn heading_level_from_title() {
        assert_eq!(heading_level_from_style(Some("Title")), Some(1));
    }

    #[test]
    fn heading_level_from_heading_n() {
        assert_eq!(heading_level_from_style(Some("Heading1")), Some(1));
        assert_eq!(heading_level_from_style(Some("Heading3")), Some(3));
        assert_eq!(heading_level_from_style(Some("heading6")), Some(6));
    }

    #[test]
    fn heading_level_none_for_body() {
        assert_eq!(heading_level_from_style(Some("Normal")), None);
        assert_eq!(heading_level_from_style(None), None);
    }
}
