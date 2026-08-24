//! JSON extraction from LLM responses — brace/bracket-balanced parsing.
//!
//! Shared security-critical primitive for parsing JSON from LLM output.
//! Prevents injected JSON blocks in reasoning preambles from hijacking the
//! model's real answer (OWASP LLM02:2025, CWE-1336).
//!
//! Contract: extraction returns the FIRST balanced top-level JSON value
//! (object or array). Callers must schema-validate the result; this primitive
//! guarantees single-value extraction, not that the value is the intended one.

/// Strip markdown code fences from LLM JSON responses.
///
/// Models often wrap JSON in ```json ... ``` blocks. This also handles
/// fences without a language tag (``` ... ```).
pub(crate) fn strip_json_fences(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.starts_with("```") {
        if let Some(after_fence) = trimmed.find('\n') {
            let content = &trimmed[after_fence + 1..];
            if let Some(close_pos) = content.rfind("```") {
                content[..close_pos].trim().to_string()
            } else {
                content.trim().to_string()
            }
        } else {
            trimmed.to_string()
        }
    } else {
        trimmed.to_string()
    }
}

/// Extract a single JSON value (object or array) from an LLM response that
/// may contain thinking-mode reasoning.
///
/// Strips code fences, then scans for the first JSON container (`{` or `[`)
/// and uses balanced delimiter tracking to find its matching close —
/// discarding any reasoning preamble or trailing text.
///
/// Security: balanced extraction defeats the first-`{`-to-last-`}` substring
/// grab attack, where a poisoned chunk embeds a JSON-looking block in its
/// text and the LLM echoes it in its reasoning preamble. Balanced tracking
/// ensures we extract exactly one top-level JSON value.
///
/// Returns the matched value substring, or the de-fenced text if no balanced
/// value is found.
pub fn extract_json_from_response(text: &str) -> String {
    let de_fenced = strip_json_fences(text);
    match find_balanced_json(&de_fenced) {
        Some(slice) => slice.to_string(),
        None => de_fenced,
    }
}

/// Find the first balanced top-level JSON value (object or array) in `text`.
///
/// Scans from the first `{` or `[`, tracking a single combined depth counter
/// across both delimiter types. This is correct for valid JSON because objects
/// and arrays nest but never cross — `{]` and `[}` are not well-formed JSON.
/// When depth returns to 0, the outermost container is complete.
///
/// Respects string literals so braces/brackets inside strings don't affect
/// the count.
pub(crate) fn find_balanced_json(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    let start = bytes.iter().position(|&b| b == b'{' || b == b'[')?;
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut escape = false;
    let mut i = start;
    while i < bytes.len() {
        let b = bytes[i];
        if in_string {
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                in_string = false;
            }
        } else if b == b'"' {
            in_string = true;
        } else if b == b'{' || b == b'[' {
            depth += 1;
        } else if b == b'}' || b == b']' {
            depth -= 1;
            if depth == 0 {
                return Some(&text[start..=i]);
            }
            if depth < 0 {
                return None;
            }
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_single_object() {
        let input = r#"{"key": "value"}"#;
        assert_eq!(extract_json_from_response(input), input);
    }

    #[test]
    fn extracts_array_of_objects() {
        let input = r#"[{"a": 1}, {"b": 2}, {"c": 3}]"#;
        let result = extract_json_from_response(input);
        assert_eq!(
            result, input,
            "must return the full array, not just the first object inside it"
        );
    }

    #[test]
    fn extracts_object_from_reasoning_preamble() {
        let input = "Here are the tags:\n\n{\"dimensions\": [\"what\"], \"ontology_tags\": {}}";
        let result = extract_json_from_response(input);
        assert!(result.starts_with('{'));
        assert!(result.ends_with('}'));
    }

    #[test]
    fn extracts_array_from_reasoning_preamble() {
        let input = "Here are the tags:\n\n```json\n[{\"a\": 1}, {\"b\": 2}]\n```";
        let result = extract_json_from_response(input);
        assert!(
            result.starts_with('['),
            "must extract the array, not the first object inside it"
        );
        assert!(result.ends_with(']'));
    }

    #[test]
    fn strips_code_fences() {
        let input = "```json\n{\"key\": \"value\"}\n```";
        assert_eq!(extract_json_from_response(input), r#"{"key": "value"}"#);
    }

    #[test]
    fn handles_nested_objects_in_array() {
        let input = r#"[{"outer": {"inner": 1}}, {"x": 2}]"#;
        let result = extract_json_from_response(input);
        assert_eq!(result, input);
    }

    #[test]
    fn handles_braces_inside_strings() {
        let input = r#"[{"text": "a } b { c"}, {"text": "normal"}]"#;
        let result = extract_json_from_response(input);
        assert_eq!(
            result, input,
            "braces inside string literals must not affect depth tracking"
        );
    }

    #[test]
    fn returns_de_fenced_text_when_no_json_found() {
        let input = "no json here";
        assert_eq!(extract_json_from_response(input), "no json here");
    }

    #[test]
    fn handles_empty_array() {
        assert_eq!(extract_json_from_response("[]"), "[]");
    }

    #[test]
    fn handles_array_with_reasoning_and_no_fences() {
        let input = "Thinking about this...\n[{\"a\": 1}, {\"b\": 2}]\nDone.";
        let result = extract_json_from_response(input);
        assert!(result.starts_with('['));
        assert!(result.ends_with(']'));
    }
}
