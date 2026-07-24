//! Format-specific document backends.
//!
//! Each backend reads a specific file format and produces a `DocStructure`.
//! Backends that emit markdown (DOCX, PPTX) share `markdown_to_structure`;
//! XLSX builds `DocStructure` directly from cell data.
//!
//! Design (P5 simplicity, P7 deep module):
//! - One trait, three impls. No registry, no format-option god-object.
//! - Backends take a file path (the parsers are file-based).
//! - `DocStructure` is the single output type — downstream tools don't care
//!   which backend produced it.

pub mod docx;
pub mod pptx;
pub mod xlsx;

pub use docx::DocxBackend;
pub use pptx::PptxBackend;
pub use xlsx::XlsxBackend;

use hkask_types::document::{Block, DocStructure, Page};

/// A document backend: reads a file and produces a `DocStructure`.
///
/// Implementations are format-specific. The caller selects the backend based
/// on file extension (see `convert::detect_format`).
pub trait DocumentBackend {
    /// Format name this backend handles (e.g., "docx", "xlsx", "pptx").
    #[allow(dead_code)]
    fn format(&self) -> &'static str;

    /// Parse the file at `path` into a `DocStructure`.
    fn parse(&self, path: &str) -> Result<DocStructure, BackendError>;
}

/// Error from a document backend.
#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("Failed to read file '{path}': {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("Failed to parse {format} file '{path}': {message}")]
    Parse {
        format: &'static str,
        path: String,
        message: String,
    },
}

/// Convert a markdown string into a `DocStructure` with a single page.
///
/// Parses headings (`#`-`######`), tables (pipe-delimited `|` rows), unordered
/// lists (`- ` / `* `), ordered lists (`1. `), and paragraphs (everything else).
/// This is a lightweight parser — not a full markdown engine — sufficient for
/// the structured output produced by `docx-parser` and `pptx-to-md`.
///
/// Shared by backends that emit markdown (DOCX, PPTX).
pub(crate) fn markdown_to_structure(markdown: &str, source_format: &str) -> DocStructure {
    let blocks = parse_markdown_blocks(markdown);
    DocStructure {
        source_format: source_format.to_string(),
        pages: vec![Page {
            page_number: 1,
            blocks,
        }],
    }
}

/// Convert per-page markdown into a `DocStructure` with one `Page` per entry.
///
/// Used by the OCR pipeline path: vision OCR models (OLMOCR-2 et al.) emit
/// per-page markdown (headings, tables, LaTeX), so each `OcrResult.text`
/// becomes one page of blocks — enabling heading-aware `chunk_structure` for
/// OCR'd PDFs, the same way office-format backends enable it for DOCX/PPTX.
///
/// `pages` yields `(page_number, markdown_text)` in document order.
pub(crate) fn markdown_pages_to_structure(
    pages: impl IntoIterator<Item = (usize, String)>,
    source_format: &str,
) -> DocStructure {
    DocStructure {
        source_format: source_format.to_string(),
        pages: pages
            .into_iter()
            .map(|(page_number, md)| Page {
                page_number,
                blocks: parse_markdown_blocks(&md),
            })
            .collect(),
    }
}

/// Parse markdown text into a sequence of `Block`s.
fn parse_markdown_blocks(markdown: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut paragraph_lines: Vec<String> = Vec::new();
    let mut table_rows: Vec<Vec<String>> = Vec::new();

    // Flush accumulated paragraph lines as a single Paragraph block.
    let flush_paragraph = |lines: &mut Vec<String>, blocks: &mut Vec<Block>| {
        if !lines.is_empty() {
            let text = lines.join("\n");
            lines.clear();
            if !text.trim().is_empty() {
                blocks.push(Block::Paragraph { text });
            }
        }
    };

    // Flush accumulated table rows as a Table block.
    let flush_table = |rows: &mut Vec<Vec<String>>, blocks: &mut Vec<Block>| {
        if !rows.is_empty() {
            let finalized = std::mem::take(rows);
            // Drop separator rows (e.g., |---|---|) — rows where every cell
            // is only dashes/colons/spaces.
            let real_rows: Vec<_> = finalized
                .into_iter()
                .filter(|row| {
                    !row.iter().all(|cell| {
                        cell.trim()
                            .chars()
                            .all(|c| c == '-' || c == ':' || c.is_whitespace())
                    })
                })
                .collect();
            if !real_rows.is_empty() {
                blocks.push(Block::Table { rows: real_rows });
            }
        }
    };

    for line in markdown.lines() {
        let trimmed = line.trim();

        // Heading: 1-6 '#' followed by space
        if let Some(rest) = trimmed.strip_prefix('#') {
            let level = rest.chars().take_while(|c| *c == '#').count().min(5) as u8 + 1;
            let text = rest.trim_start_matches('#').trim().to_string();
            if !text.is_empty() {
                flush_paragraph(&mut paragraph_lines, &mut blocks);
                flush_table(&mut table_rows, &mut blocks);
                blocks.push(Block::Heading { level, text });
                continue;
            }
        }

        // Table row: starts and ends with '|'
        if trimmed.starts_with('|') && trimmed.ends_with('|') && trimmed.len() > 1 {
            flush_paragraph(&mut paragraph_lines, &mut blocks);
            let cells: Vec<String> = trimmed
                .trim_start_matches('|')
                .trim_end_matches('|')
                .split('|')
                .map(|c| c.trim().to_string())
                .collect();
            table_rows.push(cells);
            continue;
        } else {
            flush_table(&mut table_rows, &mut blocks);
        }

        // Unordered list: "- " or "* " (but not horizontal rule "---")
        if (trimmed.starts_with("- ") || trimmed.starts_with("* ")) && !trimmed.starts_with("---") {
            flush_paragraph(&mut paragraph_lines, &mut blocks);
            blocks.push(Block::List {
                ordered: false,
                items: vec![trimmed[2..].trim().to_string()],
            });
            continue;
        }

        // Ordered list: "1. ", "2. ", etc. — one or more digits followed by ". "
        let digits_end = trimmed
            .char_indices()
            .take_while(|(_, c)| c.is_ascii_digit())
            .last()
            .map(|(i, _)| i + 1)
            .unwrap_or(0);
        if digits_end > 0
            && let Some(text) = trimmed[digits_end..].strip_prefix(". ")
        {
            flush_paragraph(&mut paragraph_lines, &mut blocks);
            blocks.push(Block::List {
                ordered: true,
                items: vec![text.trim().to_string()],
            });
            continue;
        }

        // Empty line — paragraph boundary
        if trimmed.is_empty() {
            flush_paragraph(&mut paragraph_lines, &mut blocks);
            continue;
        }

        // Default: accumulate as paragraph text
        paragraph_lines.push(line.to_string());
    }

    flush_paragraph(&mut paragraph_lines, &mut blocks);
    flush_table(&mut table_rows, &mut blocks);
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_heading_levels() {
        let blocks = parse_markdown_blocks("# Title\n## Sub\n### Deep");
        assert_eq!(blocks.len(), 3);
        assert!(matches!(
            &blocks[0],
            Block::Heading { level: 1, text } if text == "Title"
        ));
        assert!(matches!(
            &blocks[1],
            Block::Heading { level: 2, text } if text == "Sub"
        ));
        assert!(matches!(
            &blocks[2],
            Block::Heading { level: 3, text } if text == "Deep"
        ));
    }

    #[test]
    fn parse_table_with_separator() {
        let md = "| A | B |\n|---|---|\n| 1 | 2 |";
        let blocks = parse_markdown_blocks(md);
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            Block::Table { rows } => {
                assert_eq!(rows.len(), 2);
                assert_eq!(rows[0], vec!["A", "B"]);
                assert_eq!(rows[1], vec!["1", "2"]);
            }
            _ => panic!("expected Table"),
        }
    }

    #[test]
    fn parse_unordered_list() {
        let blocks = parse_markdown_blocks("- one\n- two");
        assert_eq!(blocks.len(), 2);
        assert!(matches!(
            &blocks[0],
            Block::List { ordered: false, items } if items == &vec!["one".to_string()]
        ));
    }

    #[test]
    fn parse_ordered_list() {
        let blocks = parse_markdown_blocks("1. first\n2. second");
        assert_eq!(blocks.len(), 2);
        assert!(matches!(
            &blocks[0],
            Block::List { ordered: true, items } if items == &vec!["first".to_string()]
        ));
    }

    #[test]
    fn parse_paragraphs_separated_by_blank_lines() {
        let blocks = parse_markdown_blocks("First paragraph.\n\nSecond paragraph.");
        assert_eq!(blocks.len(), 2);
        assert!(matches!(
            &blocks[0],
            Block::Paragraph { text } if text == "First paragraph."
        ));
        assert!(matches!(
            &blocks[1],
            Block::Paragraph { text } if text == "Second paragraph."
        ));
    }

    #[test]
    fn markdown_to_structure_wraps_in_single_page() {
        let doc = markdown_to_structure("# H\nbody", "docx");
        assert_eq!(doc.source_format, "docx");
        assert_eq!(doc.pages.len(), 1);
        assert_eq!(doc.pages[0].page_number, 1);
        assert_eq!(doc.pages[0].blocks.len(), 2);
    }

    #[test]
    fn markdown_pages_to_structure_one_page_per_entry() {
        // Simulates OLMOCR-2 per-page markdown output: page 1 has a heading +
        // paragraph; page 2 has a table. Each entry becomes one Page with
        // parsed blocks (the payoff of wiring OCR markdown -> DocStructure).
        let pages = vec![
            (1, "# Introduction\nThis is the body text.".to_string()),
            (2, "| A | B |\n| 1 | 2 |".to_string()),
        ];
        let doc = markdown_pages_to_structure(pages, "pdf");
        assert_eq!(doc.source_format, "pdf");
        assert_eq!(doc.pages.len(), 2);
        assert_eq!(doc.pages[0].page_number, 1);
        assert_eq!(doc.pages[1].page_number, 2);
        // Page 1: heading + paragraph
        assert_eq!(doc.pages[0].blocks.len(), 2);
        assert!(doc.pages[0].blocks[0].is_heading());
        // Page 2: a table block
        assert_eq!(doc.pages[1].blocks.len(), 1);
        assert!(matches!(doc.pages[1].blocks[0], Block::Table { .. }));
    }
}
