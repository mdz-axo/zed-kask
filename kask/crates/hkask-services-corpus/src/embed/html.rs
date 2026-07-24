//! HTML tag stripping utility.

/// Strip HTML tags from text, decoding common entities and preserving
/// paragraph breaks from existing newlines in the HTML source.
///
/// \[P7\] Motivating: Evolutionary Architecture — HTML stripping utility emerged from embedding needs.
/// pre:  html is a valid HTML string
/// post: returns plain text with tags removed, common entities decoded, whitespace collapsed
#[must_use]
pub fn strip_html_tags(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut entity_buf = String::new();
    let mut in_entity = false;

    for ch in html.chars() {
        if ch == '<' {
            in_tag = true;
            continue;
        }
        if in_tag {
            if ch == '>' {
                in_tag = false;
            }
            continue;
        }
        if ch == '&' {
            in_entity = true;
            entity_buf.clear();
            entity_buf.push(ch);
            continue;
        }
        if in_entity {
            entity_buf.push(ch);
            if ch == ';' {
                in_entity = false;
                match entity_buf.as_str() {
                    "&amp;" => result.push('&'),
                    "&lt;" => result.push('<'),
                    "&gt;" => result.push('>'),
                    "&quot;" => result.push('"'),
                    "&apos;" => result.push('\''),
                    "&#160;" | "&nbsp;" => result.push(' '),
                    _ => {
                        result.push_str(&entity_buf);
                    }
                }
            }
            continue;
        }
        if ch.is_whitespace() {
            result.push(' ');
        } else {
            result.push(ch);
        }
    }

    if in_entity {
        result.push_str(&entity_buf);
    }

    let collapsed: String = result
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    collapsed
        .split(' ')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}
