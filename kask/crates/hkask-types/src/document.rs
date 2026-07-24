//! Structural document representation — a lightweight intermediate between
//! format-specific backends and the chunking/tagging/QA pipeline.
//!
//! Inspired by Docling's `DoclingDocument` but deliberately minimal: four block
//! types cover the vast majority of real-world documents without pulling in a
//! layout model. Add block variants only when a concrete corpus demands it.
//!
//! Design principles (P5 simplicity, P7 deep module):
//! - Backends produce `DocStructure`; downstream tools consume it.
//! - `text()` flattens to plain text for backward compatibility with callers
//!   that only need a string (e.g., `corpus_convert`'s legacy `text` field).
//! - Page provenance is optional — backends that don't have page boundaries
//!   (DOCX, XLSX, PPTX, plain text) emit a single page containing all blocks.

use serde::{Deserialize, Serialize};

/// A structured document: pages of blocks.
///
/// The unit of work for `corpus_chunk` when structure is available. When a
/// backend cannot produce structure (e.g., OCR-only PDF), callers fall back to
/// `DocStructure::from_plain_text`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DocStructure {
    /// Source format that produced this structure (e.g., "pdf", "docx").
    pub source_format: String,
    /// Pages in reading order. Single-page documents have one entry.
    pub pages: Vec<Page>,
}

impl DocStructure {
    /// Construct a single-page structure from plain text.
    ///
    /// Used by backends that only produce flat text (OCR, plain text, HTML
    /// after stripping). The text becomes one `Paragraph` block.
    pub fn from_plain_text(text: &str, source_format: &str) -> Self {
        let blocks = if text.trim().is_empty() {
            Vec::new()
        } else {
            vec![Block::Paragraph {
                text: text.to_string(),
            }]
        };
        Self {
            source_format: source_format.to_string(),
            pages: vec![Page {
                page_number: 1,
                blocks,
            }],
        }
    }

    /// Flatten the entire document to plain text, joining blocks with double
    /// newlines and pages with form feeds.
    ///
    /// Backward-compatibility path for callers that expect a `String` (e.g.,
    /// the `text` field in `corpus_convert`'s JSON response).
    pub fn text(&self) -> String {
        self.pages
            .iter()
            .map(|page| {
                page.blocks
                    .iter()
                    .map(|block| block.text())
                    .collect::<Vec<_>>()
                    .join("\n\n")
            })
            .collect::<Vec<_>>()
            .join("\n\x0c") // form feed between pages
    }

    /// Total word count across all blocks.
    pub fn word_count(&self) -> usize {
        self.pages
            .iter()
            .flat_map(|page| page.blocks.iter())
            .map(|block| block.text().split_whitespace().count())
            .sum()
    }

    /// Iterate over all blocks in reading order across all pages.
    pub fn iter_blocks(&self) -> impl Iterator<Item = &Block> {
        self.pages.iter().flat_map(|page| page.blocks.iter())
    }
}

/// A single page of a document.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Page {
    /// 1-based page number. For single-page documents, this is 1.
    pub page_number: usize,
    /// Blocks on this page in reading order.
    pub blocks: Vec<Block>,
}

/// A block-level element within a page.
///
/// Four variants cover paragraphs, headings, tables, and lists — the
/// structural elements that matter for chunking and QA generation. Formula,
/// figure, and caption variants are intentionally omitted until a corpus
/// demands them (P5: no speculative features).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Block {
    /// A paragraph of body text.
    Paragraph { text: String },
    /// A heading with a level (1 = top-level, 6 = deepest).
    Heading { level: u8, text: String },
    /// A table rendered as rows of cells. Each inner Vec is a row.
    /// Cell values are plain text (no nested structure).
    Table { rows: Vec<Vec<String>> },
    /// A list of items. `ordered` distinguishes `<ol>` from `<ul>`.
    List { ordered: bool, items: Vec<String> },
}

impl Block {
    /// Flatten the block to plain text.
    ///
    /// - `Paragraph` and `Heading` return their text.
    /// - `Heading` prepends `#` markers (matching heading level) for markdown
    ///   compatibility — downstream chunkers can use these as section boundaries.
    /// - `Table` renders as tab-separated rows with newlines.
    /// - `List` renders each item on its own line, prefixed with `- ` (unordered)
    ///   or `1. ` (ordered).
    pub fn text(&self) -> String {
        match self {
            Block::Paragraph { text } => text.clone(),
            Block::Heading { level, text } => {
                let hashes = "#".repeat((*level).clamp(1, 6) as usize);
                format!("{hashes} {text}")
            }
            Block::Table { rows } => rows
                .iter()
                .map(|row| row.join("\t"))
                .collect::<Vec<_>>()
                .join("\n"),
            Block::List { ordered, items } => items
                .iter()
                .enumerate()
                .map(|(i, item)| {
                    if *ordered {
                        format!("{}. {item}", i + 1)
                    } else {
                        format!("- {item}")
                    }
                })
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }

    /// Whether this block is a heading (a natural chunk boundary).
    pub fn is_heading(&self) -> bool {
        matches!(self, Block::Heading { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_plain_text_single_paragraph() {
        let doc = DocStructure::from_plain_text("Hello world.", "plain");
        assert_eq!(doc.source_format, "plain");
        assert_eq!(doc.pages.len(), 1);
        assert_eq!(doc.pages[0].page_number, 1);
        assert_eq!(doc.pages[0].blocks.len(), 1);
        assert_eq!(doc.text(), "Hello world.");
        assert_eq!(doc.word_count(), 2);
    }

    #[test]
    fn from_plain_text_empty() {
        let doc = DocStructure::from_plain_text("", "plain");
        assert!(doc.pages[0].blocks.is_empty());
        assert_eq!(doc.text(), "");
        assert_eq!(doc.word_count(), 0);
    }

    #[test]
    fn heading_text_prepends_hashes() {
        let block = Block::Heading {
            level: 2,
            text: "Section".to_string(),
        };
        assert_eq!(block.text(), "## Section");
        assert!(block.is_heading());
    }

    #[test]
    fn heading_level_clamped_to_six() {
        let block = Block::Heading {
            level: 99,
            text: "Deep".to_string(),
        };
        assert_eq!(block.text(), "###### Deep");
    }

    #[test]
    fn table_text_tab_separated() {
        let block = Block::Table {
            rows: vec![
                vec!["A".to_string(), "B".to_string()],
                vec!["1".to_string(), "2".to_string()],
            ],
        };
        assert_eq!(block.text(), "A\tB\n1\t2");
    }

    #[test]
    fn list_text_unordered() {
        let block = Block::List {
            ordered: false,
            items: vec!["one".to_string(), "two".to_string()],
        };
        assert_eq!(block.text(), "- one\n- two");
    }

    #[test]
    fn list_text_ordered() {
        let block = Block::List {
            ordered: true,
            items: vec!["first".to_string(), "second".to_string()],
        };
        assert_eq!(block.text(), "1. first\n2. second");
    }

    #[test]
    fn multi_page_text_joined_with_form_feed() {
        let doc = DocStructure {
            source_format: "pdf".to_string(),
            pages: vec![
                Page {
                    page_number: 1,
                    blocks: vec![Block::Paragraph {
                        text: "page one".to_string(),
                    }],
                },
                Page {
                    page_number: 2,
                    blocks: vec![Block::Paragraph {
                        text: "page two".to_string(),
                    }],
                },
            ],
        };
        assert_eq!(doc.text(), "page one\n\x0cpage two");
    }

    #[test]
    fn iter_blocks_flattens_pages() {
        let doc = DocStructure {
            source_format: "test".to_string(),
            pages: vec![
                Page {
                    page_number: 1,
                    blocks: vec![
                        Block::Paragraph {
                            text: "a".to_string(),
                        },
                        Block::Heading {
                            level: 1,
                            text: "H".to_string(),
                        },
                    ],
                },
                Page {
                    page_number: 2,
                    blocks: vec![Block::Paragraph {
                        text: "b".to_string(),
                    }],
                },
            ],
        };
        let texts: Vec<_> = doc.iter_blocks().map(|b| b.text()).collect();
        assert_eq!(texts, vec!["a", "# H", "b"]);
    }

    #[test]
    fn word_count_sums_across_blocks() {
        let doc = DocStructure {
            source_format: "test".to_string(),
            pages: vec![Page {
                page_number: 1,
                blocks: vec![
                    Block::Paragraph {
                        text: "one two".to_string(),
                    },
                    Block::Heading {
                        level: 1,
                        text: "three".to_string(),
                    },
                ],
            }],
        };
        assert_eq!(doc.word_count(), 4);
    }
}
