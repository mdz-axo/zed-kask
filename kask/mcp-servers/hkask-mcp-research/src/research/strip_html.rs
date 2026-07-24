//! hKask MCP Web — HTML to plain-text conversion

pub fn strip_html(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut in_script = false;
    let mut in_comment = false;
    // Three-char sliding window for detecting '-->' comment end.
    let mut comment_tail: [char; 3] = ['\0', '\0', '\0'];
    let mut tag_name = String::new();
    let mut collecting_tag = false;

    for ch in html.chars() {
        if in_comment {
            // Skip everything until we see '-->'. Track the last three chars
            // in a small buffer since comment content is not pushed to `result`.
            comment_tail[0] = comment_tail[1];
            comment_tail[1] = comment_tail[2];
            comment_tail[2] = ch;
            if comment_tail == ['-', '-', '>'] {
                in_comment = false;
            }
            continue;
        }
        if in_tag {
            // Detect comment start: tag_name is empty and we see '!'
            // immediately after '<'. This catches '<!-- ... -->'.
            if tag_name.is_empty() && !collecting_tag && ch == '!' {
                in_comment = true;
                in_tag = false;
                continue;
            }
            if ch == '>' {
                let tag_lower = tag_name.to_lowercase();
                if tag_lower == "script" || tag_lower == "style" {
                    in_script = true;
                } else if tag_lower == "/script" || tag_lower == "/style" {
                    in_script = false;
                } else if tag_lower == "br"
                    || tag_lower.starts_with("br ")
                    || tag_lower == "p"
                    || tag_lower.starts_with("p ")
                    || tag_lower == "/p"
                {
                    result.push('\n');
                } else if tag_lower == "h1"
                    || tag_lower.starts_with("h1 ")
                    || tag_lower == "h2"
                    || tag_lower.starts_with("h2 ")
                    || tag_lower == "h3"
                    || tag_lower.starts_with("h3 ")
                {
                    result.push_str("\n## ");
                } else if tag_lower == "/h1" || tag_lower == "/h2" || tag_lower == "/h3" {
                    result.push('\n');
                } else if tag_lower == "li" || tag_lower.starts_with("li ") {
                    // Insert newline before list items unless we're already
                    // at the start of a line. Fixes the concatenation bug
                    // where consecutive <li> elements produced "- item1- item2".
                    if !result.is_empty() && !result.ends_with('\n') {
                        result.push('\n');
                    }
                    result.push_str("- ");
                }
                in_tag = false;
                collecting_tag = false;
                tag_name.clear();
            } else if collecting_tag {
                if ch == ' ' || ch == '\n' || ch == '\r' || ch == '\t' {
                    collecting_tag = false;
                } else {
                    tag_name.push(ch);
                }
            } else if tag_name.is_empty() && (ch == '/' || ch.is_alphabetic()) {
                collecting_tag = true;
                tag_name.push(ch);
            }
            continue;
        }
        if in_script {
            if ch == '<' {
                in_tag = true;
                tag_name.clear();
            }
            continue;
        }
        if ch == '<' {
            in_tag = true;
            tag_name.clear();
            continue;
        }
        result.push(ch);
    }

    let lines: Vec<&str> = result
        .lines()
        .map(|l| l.trim_end())
        .filter(|l| !l.is_empty())
        .collect();
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_html_removes_tags() {
        let input = "<p>Hello</p>";
        assert_eq!(strip_html(input), "Hello");
    }

    #[test]
    fn strip_html_headings_to_markdown() {
        assert_eq!(strip_html("<h1>Title</h1>"), "## Title");
        assert_eq!(strip_html("<h2>Subtitle</h2>"), "## Subtitle");
    }

    #[test]
    fn strip_html_list_items() {
        // Consecutive <li> elements now produce separate list items on
        // their own lines (previously concatenated as "- item1- item2").
        assert_eq!(
            strip_html("<li>item1</li><li>item2</li>"),
            "- item1\n- item2"
        );
    }

    #[test]
    fn strip_html_removes_html_comments() {
        // HTML comments <!-- ... --> are stripped entirely, including
        // any nested content. Previously, comment text leaked into output.
        let input = "<p>before</p><!-- a comment --><p>after</p>";
        assert_eq!(strip_html(input), "before\nafter");
    }

    #[test]
    fn strip_html_removes_conditional_comments() {
        // Downlevel-revealed conditional comments like <!--[if IE]>...<![endif]-->
        // should also be stripped.
        let input = "<p>x</p><!--[if IE]><p>ie-only</p><![endif]--><p>y</p>";
        assert_eq!(strip_html(input), "x\ny");
    }

    #[test]
    fn strip_html_removes_empty_comment() {
        // Empty comment <!--> should be stripped entirely (HTML5 allows it).
        let input = "<p>before</p><!--><p>after</p>";
        assert_eq!(strip_html(input), "before\nafter");
    }

    #[test]
    fn strip_html_removes_empty_double_dash_comment() {
        // The minimal valid comment is <!-- --> (with space). Verify it works.
        let input = "<p>a</p><!-- --><p>b</p>";
        assert_eq!(strip_html(input), "a\nb");
    }

    #[test]
    fn strip_html_removes_script_content() {
        let input = "<script>alert('hi')</script><p>visible</p>";
        assert_eq!(strip_html(input), "visible");
    }

    #[test]
    fn strip_html_removes_style_content() {
        let input = "<style>body{color:red}</style><p>text</p>";
        assert_eq!(strip_html(input), "text");
    }

    #[test]
    fn strip_html_br_to_newline() {
        assert_eq!(strip_html("line1<br>line2"), "line1\nline2");
    }

    #[test]
    fn strip_html_collapses_blank_lines() {
        let input = "<p>a</p>\n\n\n<p>b</p>";
        assert_eq!(strip_html(input), "a\nb");
    }

    #[test]
    fn strip_html_trims_trailing_whitespace() {
        let input = "<p>text   </p>";
        assert_eq!(strip_html(input), "text");
    }
}
