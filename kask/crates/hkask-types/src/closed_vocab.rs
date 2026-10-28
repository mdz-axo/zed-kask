//! The canonical closed-vocabulary emission-noise normalizer.
//!
//! Hoisted to its own module when the spreadsheet capability (its original
//! home) was removed (2026-10-15) — the normalizer is capability-agnostic
//! and every server's closed-vocab fields parse through it.

/// Strip surrounding whitespace and one symmetric layer of quotes from a
/// closed-vocabulary field value — the canonical emission-noise normalizer
/// for every closed-vocab MCP field (operator ruling 2026-10-13; the
/// research server's original tolerance, hoisted here so each server
/// imports it rather than re-implementing — the `resolve_db_passphrase`
/// canonical-helper precedent). Interior quotes and asymmetric wrapping
/// are preserved — they are data, not emission noise, and stay visible to
/// validation. Byte slicing is safe because the strip guard matches ASCII
/// quote bytes, which are always char boundaries.
pub fn normalize_closed_vocab(raw: &str) -> &str {
    let s = raw.trim();
    if s.len() >= 2 {
        let first = s.as_bytes()[0];
        let last = s.as_bytes()[s.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return s[1..s.len() - 1].trim();
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::normalize_closed_vocab;

    #[test]
    fn strips_one_symmetric_quote_layer_and_padding() {
        assert_eq!(normalize_closed_vocab("\"quick\""), "quick");
        assert_eq!(normalize_closed_vocab("'quick'"), "quick");
        assert_eq!(normalize_closed_vocab("  \"quick\"  "), "quick");
        assert_eq!(normalize_closed_vocab("quick"), "quick");
    }

    #[test]
    fn preserves_interior_quotes_and_asymmetric_wrapping() {
        // Interior quotes are data, not emission noise.
        assert_eq!(normalize_closed_vocab("\"qu\"ick\""), "qu\"ick");
        // Asymmetric wrapping stays visible to validation.
        assert_eq!(normalize_closed_vocab("\"quick"), "\"quick");
        assert_eq!(normalize_closed_vocab("quick\""), "quick\"");
    }
}
