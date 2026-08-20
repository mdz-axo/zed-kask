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
