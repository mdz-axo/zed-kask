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
