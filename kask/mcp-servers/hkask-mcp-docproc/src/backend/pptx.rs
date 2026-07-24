//! PPTX backend — uses `pptx-to-md` (0.1.x API) to extract slide text as
//! markdown, then converts to `DocStructure` via the shared
//! `markdown_to_structure` parser.

use super::{BackendError, DocumentBackend, markdown_to_structure};
use hkask_types::document::DocStructure;
use pptx_to_md::PptxContainer;

/// PPTX presentation backend.
pub struct PptxBackend;

impl DocumentBackend for PptxBackend {
    fn format(&self) -> &'static str {
        "pptx"
    }

    fn parse(&self, path: &str) -> Result<DocStructure, BackendError> {
        let mut container =
            PptxContainer::open(std::path::Path::new(path)).map_err(|e| BackendError::Parse {
                format: "pptx",
                path: path.to_string(),
                message: e.to_string(),
            })?;
        let slides = container.parse_all().map_err(|e| BackendError::Parse {
            format: "pptx",
            path: path.to_string(),
            message: e.to_string(),
        })?;

        let mut markdown = String::new();
        for slide in slides {
            if let Some(md) = slide.convert_to_md() {
                if !markdown.is_empty() {
                    markdown.push_str("\n\n");
                }
                markdown.push_str(&md);
            }
        }

        if markdown.trim().is_empty() {
            return Err(BackendError::Parse {
                format: "pptx",
                path: path.to_string(),
                message: "Presentation contained no extractable text".to_string(),
            });
        }
        Ok(markdown_to_structure(&markdown, "pptx"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_name() {
        assert_eq!(PptxBackend.format(), "pptx");
    }
}
