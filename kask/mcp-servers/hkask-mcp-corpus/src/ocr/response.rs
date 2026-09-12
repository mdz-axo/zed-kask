//! The publisher's page-response contract, not a second OCR backend.
//! Reference: allenai/olmocr, prompts.py, build_no_anchoring_v4_yaml_prompt
//! and PageResponse. Figure markers are model annotations, never file paths.
use pulldown_cmark::{Event, LinkType, Parser, Tag, TagEnd};
use serde::{Deserialize, Serialize};

use super::pipeline::OcrError;

pub(crate) const OCR_PROTOCOL: &str = "olmocr-page-yaml-v4";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PageMetadata {
    #[serde(deserialize_with = "required_language")]
    pub primary_language: Option<String>,
    pub is_rotation_valid: bool,
    pub rotation_correction: i16,
    pub is_table: bool,
    pub is_diagram: bool,
}

// Nullable does not mean optional: omitted metadata must remain a failure.
fn required_language<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Option::<String>::deserialize(d)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct FigureAnnotation {
    pub description: String,
    /// A protocol marker only. No image artifact or valid crop is asserted.
    pub marker: String,
}

pub(crate) struct PageResponse {
    pub text: String,
    pub metadata: PageMetadata,
    pub figures: Vec<FigureAnnotation>,
}

fn invalid(message: impl Into<String>) -> OcrError {
    OcrError::InvalidResponse(message.into())
}

fn is_figure_marker(marker: &str) -> bool {
    let Some(coords) = marker
        .strip_prefix("page_")
        .and_then(|s| s.strip_suffix(".png"))
    else {
        return false;
    };
    let parts: Vec<_> = coords.split('_').collect();
    parts.len() == 4
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// expect: [P4] Only a complete page response can supply corpus text; annotations cannot masquerade as quotations or fetched images.
/// pre: raw is the configured model's page response.
/// post: metadata and inline figure annotations are separate from unchanged body text; unsupported links and rotation requests fail.
/// inv: literal Markdown inside code remains text; no legacy plain-text fallback or remote URL stripping.
pub(crate) fn parse_page_response(raw: &str) -> Result<PageResponse, OcrError> {
    let normalized = raw.replace("\r\n", "\n");
    let rest = normalized
        .strip_prefix("---\n")
        .ok_or_else(|| invalid("missing YAML page header"))?;
    let (header, body) = rest
        .split_once("\n---")
        .ok_or_else(|| invalid("unterminated YAML page header"))?;
    let body = if body.is_empty() {
        body
    } else {
        body.strip_prefix('\n')
            .ok_or_else(|| invalid("page-header delimiter must occupy its own line"))?
    };
    let metadata: PageMetadata = serde_yaml_neo::from_str(header)
        .map_err(|e| invalid(format!("invalid page metadata: {e}")))?;
    if ![0, 90, 180, 270].contains(&metadata.rotation_correction) {
        return Err(invalid("rotation_correction must be 0, 90, 180 or 270"));
    }
    if !metadata.is_rotation_valid || metadata.rotation_correction != 0 {
        return Err(invalid(format!(
            "page requests rotation correction {}; no corrected text admitted",
            metadata.rotation_correction
        )));
    }

    let mut figures = Vec::new();
    let mut removals = Vec::new();
    let mut events = Parser::new(body).into_offset_iter();
    while let Some((event, range)) = events.next() {
        match event {
            Event::Start(Tag::Image {
                link_type,
                dest_url,
                ..
            }) => {
                if link_type != LinkType::Inline || !is_figure_marker(&dest_url) {
                    return Err(invalid(format!(
                        "unsupported figure reference {dest_url:?}; expected an inline page_x_y_width_height.png marker"
                    )));
                }
                let mut description = String::new();
                let mut ended = false;
                for (part, _) in events.by_ref() {
                    match part {
                        Event::End(TagEnd::Image) => {
                            ended = true;
                            break;
                        }
                        Event::Text(text) | Event::Code(text) => description.push_str(&text),
                        Event::SoftBreak | Event::HardBreak => description.push(' '),
                        _ => {}
                    }
                }
                if !ended {
                    return Err(invalid("unterminated figure annotation"));
                }
                figures.push(FigureAnnotation {
                    description,
                    marker: dest_url.into_string(),
                });
                removals.push(range);
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                // The protocol specifies Markdown figure annotations, not HTML images.
                let lower = html.to_ascii_lowercase();
                if lower.contains("<img") {
                    return Err(invalid(
                        "HTML image annotations are not part of the page protocol",
                    ));
                }
            }
            _ => {}
        }
    }
    let mut text = String::new();
    let mut cursor = 0;
    for range in removals {
        text.push_str(
            body.get(cursor..range.start)
                .ok_or_else(|| invalid("invalid figure text range"))?,
        );
        cursor = range.end;
    }
    text.push_str(
        body.get(cursor..)
            .ok_or_else(|| invalid("invalid page text range"))?,
    );
    Ok(PageResponse {
        text: text.trim().to_string(),
        metadata,
        figures,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    const HEADER: &str = "---\nprimary_language: en\nis_rotation_valid: True\nrotation_correction: 0\nis_table: False\nis_diagram: False\n---\n";

    /// expect: [P4] Protocol markers are retained as annotations; source code and printed captions survive verbatim.
    #[test]
    fn separates_annotations_without_rewriting_literal_code() -> anyhow::Result<()> {
        let body = "Caption one.\n\n![**A diagram**](page_0_0_20_30.png)\n\n`![literal](https://example.com/a.png)`\n\n```md\n![literal](https://example.com/b.png)\n```\n\nCaption two.";
        let page = parse_page_response(&format!("{HEADER}{body}"))?;
        assert_eq!(page.figures.len(), 1);
        assert_eq!(
            page.figures.first().expect("figure").description,
            "A diagram"
        );
        assert!(page.text.starts_with("Caption one."));
        assert!(page.text.ends_with("Caption two."));
        assert!(
            page.text
                .contains("`![literal](https://example.com/a.png)`")
        );
        assert!(page.text.contains("![literal](https://example.com/b.png)"));
        assert!(!page.text.contains("page_0_0_20_30.png"));
        Ok(())
    }

    /// expect: [P4] Unsupported image destinations are rejected, never silently removed to make a response pass.
    #[test]
    fn rejects_untrusted_image_references() {
        for image in [
            "![x](https://example.com/x.png)",
            "![x](../page_0_0_1_1.png)",
            "![x](page_1_2.png)",
            "<img src=\"https://example.com/x.png\">",
            "![x][f]\n\n[f]: page_0_0_1_1.png",
        ] {
            assert!(
                parse_page_response(&format!("{HEADER}{image}")).is_err(),
                "{image}"
            );
        }
    }

    /// expect: [P4] Missing, mistyped and contradictory page metadata cannot become success through defaults.
    #[test]
    fn requires_complete_metadata_and_valid_orientation() {
        for raw in [
            "plain text".into(),
            HEADER.replace("primary_language: en\n", ""),
            HEADER.replace("is_table: False\n", ""),
            HEADER.replace("is_table: False", "is_table: unknown"),
            HEADER.replace("is_rotation_valid: True", "is_rotation_valid: False"),
            HEADER.replace("rotation_correction: 0", "rotation_correction: 90"),
            HEADER.replace("rotation_correction: 0", "rotation_correction: 13"),
            HEADER.replace("is_diagram: False", "is_diagram: False\nextra: true"),
            HEADER.replace("is_diagram: False", "is_diagram: False\nis_table: True"),
        ] {
            assert!(parse_page_response(&raw).is_err(), "{raw}");
        }
    }

    /// expect: [P4] A nullable language and empty body remain explicitly empty, not the fake word BLANK.
    #[test]
    fn preserves_empty_body_for_source_blank_review() -> anyhow::Result<()> {
        let page =
            parse_page_response(&HEADER.replace("primary_language: en", "primary_language: null"))?;
        assert!(page.metadata.primary_language.is_none());
        assert!(page.text.is_empty());
        Ok(())
    }
}
