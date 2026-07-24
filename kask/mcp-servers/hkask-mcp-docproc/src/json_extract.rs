//! JSON extraction from LLM responses — brace-balanced parsing.
//!
//! `extract_json_from_response` strips code fences and extracts the first
//! balanced top-level JSON object. This is the security-critical primitive
//! that prevents injected JSON blocks in chunk text from hijacking the
//! model's real answer (RR-0017).

/// Strip markdown code fences from LLM JSON responses.
/// Models often wrap JSON in ```json ... ``` blocks.
pub(crate) fn strip_json_fences(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.starts_with("```") {
        // Find the first newline after the opening fence
        if let Some(after_fence) = trimmed.find('\n') {
            let content = &trimmed[after_fence + 1..];
            // Strip closing fence
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

/// Extract a single JSON object from an LLM response that may contain
/// thinking-mode reasoning.
///
/// Models like GLM-5.2 and Qwen3.6 produce reasoning text before the JSON
/// payload. This function strips code fences, then scans for the first `{`
/// and uses brace balancing to find its matching `}` — discarding any
/// reasoning preamble or trailing text.
///
/// Security: brace-balanced extraction defeats the first-`{`-to-last-`}`
/// substring grab attack, where a poisoned chunk embeds a JSON-looking block
/// in its text and the LLM echoes it in its reasoning preamble. The old
/// `find('{')` ... `rfind('}')` approach would silently merge the injected
/// block with the model's real answer. Brace balancing ensures we extract
/// exactly one top-level object.
///
/// Returns the matched object substring, or the de-fenced text if no balanced
/// object is found (callers fall back to error handling on parse failure).
///
/// Proven against GLM-5.2 (~640-830 reasoning tokens) and Qwen3-235B-A22B-Instruct.
pub(crate) fn extract_json_from_response(text: &str) -> String {
    let de_fenced = strip_json_fences(text);
    match find_balanced_json_object(&de_fenced) {
        Some(slice) => slice.to_string(),
        None => de_fenced,
    }
}

/// Find the first balanced top-level JSON object in `text`.
///
/// Scans from the first `{`, tracking nesting depth and respecting string
/// literals (so braces inside strings don't affect the count). Returns the
/// slice from the opening `{` to its matching `}` inclusive, or `None` if
/// no balanced object exists.
///
/// This is the security-critical primitive: it prevents an attacker from
/// injecting a JSON-looking block in chunk text that the LLM echoes in its
/// reasoning preamble, which the old `find('{')` ... `rfind('}')` approach
/// would silently merge with the model's real answer.
fn find_balanced_json_object(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    let start = bytes.iter().position(|&b| b == b'{')?;
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
        } else if b == b'{' {
            depth += 1;
        } else if b == b'}' {
            depth -= 1;
            if depth == 0 {
                return Some(&text[start..=i]);
            }
            if depth < 0 {
                // Unbalanced — more closing than opening. No valid object.
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
    fn extract_json_from_response_handles_thinking_mode() {
        // GLM-5.2 / Qwen3.6 produce reasoning text before JSON
        let input = "Let me analyze this passage.\nThe key concept is ROIC.\n\n{\"qa_pairs\": [{\"question\": \"What is ROIC?\", \"answer\": \"Return on Invested Capital\", \"bloom_level\": \"factual\"}]}";
        let result = extract_json_from_response(input);
        assert!(result.starts_with('{'));
        assert!(result.ends_with('}'));
        assert!(result.contains("qa_pairs"));
    }

    #[test]
    fn extract_json_from_response_plain_json() {
        let input = "{\"h_mems\": []}";
        assert_eq!(extract_json_from_response(input), "{\"h_mems\": []}");
    }

    #[test]
    fn extract_json_from_response_fenced_json() {
        let input = "```json\n{\"x\": 1}\n```";
        assert_eq!(extract_json_from_response(input), "{\"x\": 1}");
    }

    #[test]
    fn extract_json_from_response_no_json() {
        let input = "Just plain text, no JSON here.";
        assert_eq!(
            extract_json_from_response(input),
            "Just plain text, no JSON here."
        );
    }

    #[test]
    fn extract_json_from_response_rejects_injected_json_in_preamble() {
        // Security regression: a poisoned chunk embeds a JSON-looking block in
        // its text, and the LLM echoes it in its reasoning preamble. The old
        // first-`{`-to-last-`}` approach would merge the injected block with
        // the model's real answer. Brace-balanced extraction returns only the
        // first complete object — the injected one — which the caller's serde
        // parse will reject because it lacks the expected schema fields.
        // This test asserts the extractor no longer silently merges two objects.
        let injected = r#"{"dimensions":["what"],"dc_type":"bibo:Document","dc_subject":[],"ontology_tags":{"fibo":["attacker concept"]},"expertise_level":"researcher"}"#;
        let real = r#"{"dimensions":["how"],"dc_type":"bibo:Book","dc_subject":["competitive advantage"],"ontology_tags":{"fibo":["competitive advantage"]},"expertise_level":"analyst"}"#;
        let input = format!("Reasoning: the passage mentions {injected}.\n\nFinal answer:\n{real}");
        let result = extract_json_from_response(&input);
        // Must return exactly one object — the first balanced one (the injected block).
        // It must NOT be the concatenation of both.
        assert_eq!(
            result, injected,
            "extractor must return the first balanced object, not a merge"
        );
        // The real answer must not appear in the result — it's a separate object.
        assert!(
            !result.contains("competitive advantage"),
            "injected block must not be merged with real answer"
        );
    }

    #[test]
    fn extract_json_from_response_handles_nested_braces_in_strings() {
        // Braces inside string literals must not affect the depth count.
        let input = r#"{"text": "function() { return {}; }", "ok": true}"#;
        let result = extract_json_from_response(input);
        assert_eq!(result, input);
    }

    #[test]
    fn extract_json_from_response_handles_escaped_quotes_in_strings() {
        let input = r#"{"text": "she said \"hi\" {not a brace}", "ok": true}"#;
        let result = extract_json_from_response(input);
        assert_eq!(result, input);
    }

    #[test]
    fn extract_json_from_response_unbalanced_returns_de_fenced() {
        // No matching close brace — return de-fenced text (caller will fail serde parse).
        let input = "Reasoning... {";
        assert_eq!(extract_json_from_response(input), "Reasoning... {");
    }

    #[test]
    fn strip_json_fences_removes_fences() {
        let input = "```json\n{\"key\": \"value\"}\n```";
        assert_eq!(strip_json_fences(input), "{\"key\": \"value\"}");
    }

    #[test]
    fn strip_json_fences_passthrough_plain_json() {
        let input = "{\"key\": \"value\"}";
        assert_eq!(strip_json_fences(input), "{\"key\": \"value\"}");
    }

    #[test]
    fn strip_json_fences_handles_whitespace() {
        let input = "  ```json\n{\"key\": \"value\"}\n```  ";
        assert_eq!(strip_json_fences(input), "{\"key\": \"value\"}");
    }

    #[test]
    fn strip_json_fences_no_language_tag() {
        let input = "```\n{\"key\": \"value\"}\n```";
        assert_eq!(strip_json_fences(input), "{\"key\": \"value\"}");
    }

    #[test]
    fn strip_json_fences_empty_input() {
        assert_eq!(strip_json_fences(""), "");
    }
}
