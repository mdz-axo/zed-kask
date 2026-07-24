//! Format detection, text extraction, and HTML/markdown preprocessing for docproc server.

/// Detect document format from file path/extension.
///
/// Returns `(format_name, supported, note)` where `supported` indicates whether
/// `docproc_convert` can extract text from this format.
///
/// Supported formats (text extraction works): pdf, markdown, html, plain,
/// docx, pptx, xlsx, csv (csv via xlsx backend)
pub fn detect_format(path: &str) -> (&'static str, bool, Option<&'static str>) {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "pdf" => ("pdf", true, None),
        "md" | "markdown" => ("markdown", true, None),
        "html" | "htm" => ("html", true, None),
        "txt" => ("plain", true, None),
        "docx" | "doc" => ("docx", true, None),
        "pptx" | "ppt" => ("pptx", true, None),
        "xlsx" | "xls" | "csv" => ("xlsx", true, None),
        "rtf" => (
            "rtf",
            false,
            Some("RTF support requires additional Rust crate"),
        ),
        _ => ("unknown", false, None),
    }
}

/// Whether a format is supported for text extraction by `docproc_convert`.
pub fn is_format_supported(format: &str) -> bool {
    matches!(
        format,
        "pdf" | "markdown" | "html" | "plain" | "docx" | "pptx" | "xlsx"
    )
}

/// Strip YAML frontmatter (delimited by `---`) from content.
pub fn strip_frontmatter(content: &str) -> String {
    if content.starts_with("---") {
        content
            .splitn(3, "---")
            .nth(2)
            .unwrap_or(content)
            .trim()
            .to_string()
    } else {
        content.to_string()
    }
}

/// Strip HTML tags and extract visible text content.
///
/// Removes script/style elements entirely, then strips all remaining
/// HTML tags to produce clean plain text. Collapses consecutive whitespace.
pub fn strip_html(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut in_strip_tag = false;
    let chars: Vec<char> = html.chars().collect();
    let len = chars.len();

    /// Block-level tags that should insert a space when stripped.
    const BLOCK_TAGS: &[&str] = &[
        "p",
        "div",
        "br",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "li",
        "tr",
        "table",
        "blockquote",
        "pre",
        "section",
        "article",
        "header",
        "footer",
        "main",
        "aside",
        "nav",
        "figure",
    ];

    let mut i = 0;
    while i < len {
        let ch = chars[i];

        if ch == '<' {
            let remaining: String = chars[i..].iter().collect();
            let lower_remaining = remaining.to_lowercase();

            // Check for closing script/style tags
            if lower_remaining.starts_with("</script") || lower_remaining.starts_with("</style") {
                // Insert space boundary when exiting a strip tag
                if in_strip_tag
                    && !result.is_empty()
                    && !result.chars().last().is_none_or(|c| c.is_whitespace())
                {
                    result.push(' ');
                }
                in_strip_tag = false;
                while i < len && chars[i] != '>' {
                    i += 1;
                }
                if i < len {
                    i += 1;
                }
                continue;
            }

            // Check for opening script/style tags
            if lower_remaining.starts_with("<script") || lower_remaining.starts_with("<style") {
                // Insert space boundary when entering a strip tag
                if !result.is_empty() && !result.chars().last().is_none_or(|c| c.is_whitespace()) {
                    result.push(' ');
                }
                in_strip_tag = true;
                while i < len && chars[i] != '>' {
                    i += 1;
                }
                if i < len {
                    i += 1;
                }
                continue;
            }

            // For regular tags, check if it's a block-level tag
            // and insert a space if needed (for word boundaries)
            let tag_name = remaining
                .trim_start_matches('<')
                .split(|c: char| c.is_whitespace() || c == '>' || c == '/')
                .next()
                .unwrap_or("")
                .to_lowercase();
            let is_block = BLOCK_TAGS.contains(&tag_name.as_str());

            if is_block
                && !result.is_empty()
                && !result.chars().last().is_none_or(|c| c.is_whitespace())
            {
                result.push(' ');
            }

            in_tag = true;
            i += 1;
            continue;
        }

        if ch == '>' {
            in_tag = false;
            i += 1;
            continue;
        }

        if !in_tag && !in_strip_tag {
            result.push(ch);
        }

        i += 1;
    }

    // Collapse whitespace
    let collapsed: String = result.split_whitespace().collect::<Vec<&str>>().join(" ");

    // Decode HTML entities after whitespace collapse
    decode_html_entities(&collapsed)
}

/// Decode common HTML entities in extracted text.
///
/// Handles named entities (`&amp;`, `&lt;`, `&gt;`, `&quot;`, `&apos;`,
/// `&nbsp;`, `&#39;`) and numeric entities (`&#NNN;`, `&#xNNN;`).
/// `&amp;` is decoded last to prevent double-decode of nested entities.
pub fn decode_html_entities(text: &str) -> String {
    let text = text.replace("&nbsp;", " ");
    let text = text.replace("&lt;", "<");
    let text = text.replace("&gt;", ">");
    let text = text.replace("&quot;", "\"");
    let text = text.replace("&apos;", "'");
    let text = text.replace("&#39;", "'");
    let text = text.replace("&amp;", "&");

    // Numeric decimal entities: &#NNN;
    let re_dec = regex::Regex::new(r"&#(\d+);").expect("decimal entity regex");
    let text = re_dec.replace_all(&text, |caps: &regex::Captures| {
        caps[1]
            .parse::<u32>()
            .ok()
            .and_then(char::from_u32)
            .map(|c| c.to_string())
            .unwrap_or_default()
    });

    // Numeric hex entities: &#xNNN;
    let re_hex = regex::Regex::new(r"&#x([0-9a-fA-F]+);").expect("hex entity regex");
    re_hex
        .replace_all(&text, |caps: &regex::Captures| {
            u32::from_str_radix(&caps[1], 16)
                .ok()
                .and_then(char::from_u32)
                .map(|c| c.to_string())
                .unwrap_or_default()
        })
        .into_owned()
}

/// Strip HTML comments (`<!-- ... -->`) from text.
///
/// Handles multi-line comments. Used for markdown files that contain
/// embedded HTML comments from OCR/Kindle conversion tools.
pub fn strip_html_comments(text: &str) -> String {
    let re = regex::Regex::new(r"(?s)<!--.*?-->").expect("html comment regex");
    re.replace_all(text, "").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_named_entities() {
        assert_eq!(
            decode_html_entities("a &amp; b &lt; c &gt; d &quot;e&quot; f&apos;g"),
            "a & b < c > d \"e\" f'g"
        );
    }

    #[test]
    fn decodes_nbsp_to_space() {
        assert_eq!(decode_html_entities("word1&nbsp;word2"), "word1 word2");
    }

    #[test]
    fn decodes_numeric_decimal_entities() {
        assert_eq!(decode_html_entities("&#8217;quote&#8217;"), "’quote’");
    }

    #[test]
    fn decodes_numeric_hex_entities() {
        assert_eq!(decode_html_entities("&#x2014;dash&#x2014;"), "—dash—");
    }

    #[test]
    fn amp_decoded_last_to_prevent_double_decode() {
        assert_eq!(decode_html_entities("&amp;lt;"), "&lt;");
    }

    #[test]
    fn strips_html_comments() {
        assert_eq!(strip_html_comments("text<!-- comment -->more"), "textmore");
    }

    #[test]
    fn strips_multiline_html_comments() {
        assert_eq!(
            strip_html_comments("before<!-- page 1 | Page 6 of 619 | 567ms -->after"),
            "beforeafter"
        );
    }

    #[test]
    fn strip_html_decodes_entities() {
        let html = "<p>A &amp; B</p>";
        assert_eq!(strip_html(html), "A & B");
    }
}
