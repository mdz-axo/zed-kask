//! JSON extraction from LLM responses — brace-balanced parsing.
//!
//! Shared security-critical primitive for parsing JSON from LLM output.
//! Prevents injected JSON blocks in reasoning preambles from hijacking the
//! model's real answer (OWASP LLM02:2025, CWE-1336).
//!
//! Originally extracted from `hkask-mcp-corpus/src/json_extract.rs` (RR-0017)
//! so all LLM-output parsers use the same secure primitive instead of
//! duplicating the vulnerable `find('{')`…`rfind('}')` pattern.
//!
//! Contract: extraction returns the FIRST balanced top-level object — which,
//! in an injection attempt, may be the injected one. Callers must
//! schema-validate the result; this primitive guarantees single-object
//! extraction, not that the object is the intended one.

/// Strip markdown code fences from LLM JSON responses.
///
/// Models often wrap JSON in ```json ... ``` blocks. This also handles
/// fences without a language tag (``` ... ```).
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
pub fn extract_json_from_response(text: &str) -> String {
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
pub(crate) fn find_balanced_json_object(text: &str) -> Option<&str> {
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
