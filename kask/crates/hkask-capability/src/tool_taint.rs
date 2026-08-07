//! Taint labels for MCP tools, following the FIDES information flow control model.
//!
//! Source: Microsoft Research FIDES (arXiv:2505.23643)
//!
//! Every MCP tool is categorized by its data flow characteristics. The policy:
//! untrusted data (from Source tools) cannot reach Sink tools (state-changing)
//! without going through an Endorser (quarantined extraction).

use serde::{Deserialize, Serialize};
use std::fmt;

/// Taint label for MCP tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ToolTaint {
    /// Returns untrusted data from external sources. Output is tainted.
    Source,
    /// Performs state-changing actions. Inputs must not be tainted.
    Sink,
    /// No side effects, no external data. Pure transformation.
    Pure,
    /// Trusted extraction — endorses untrusted input via constrained processing.
    Endorser,
}

impl ToolTaint {
    /// Check if data can flow from this tool's output to the target tool's input.
    ///
    /// FIDES lattice rules:
    /// - Source → Sink: BLOCKED (must go through Endorser first)
    /// - All other flows: allowed
    ///
    /// expect: "The system enforces FIDES taint flow rules between MCP tools"
    /// pre:  self and target are valid ToolTaint labels
    /// post: returns false iff self=Source and target=Sink; true otherwise
    pub fn can_flow_to(&self, target: &ToolTaint) -> bool {
        !matches!((self, target), (ToolTaint::Source, ToolTaint::Sink))
    }
}

impl fmt::Display for ToolTaint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ToolTaint::Source => write!(f, "source"),
            ToolTaint::Sink => write!(f, "sink"),
            ToolTaint::Pure => write!(f, "pure"),
            ToolTaint::Endorser => write!(f, "endorser"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Full 4x4 can_flow_to matrix — the FIDES policy in one table.
    #[test]
    fn can_flow_to_matrix() {
        // (from, to, expected)
        let cases: [(ToolTaint, ToolTaint, bool); 16] = [
            (ToolTaint::Source, ToolTaint::Source, true),
            (ToolTaint::Source, ToolTaint::Sink, false),
            (ToolTaint::Source, ToolTaint::Pure, true),
            (ToolTaint::Source, ToolTaint::Endorser, true),
            (ToolTaint::Sink, ToolTaint::Source, true),
            (ToolTaint::Sink, ToolTaint::Sink, true),
            (ToolTaint::Sink, ToolTaint::Pure, true),
            (ToolTaint::Sink, ToolTaint::Endorser, true),
            (ToolTaint::Pure, ToolTaint::Source, true),
            (ToolTaint::Pure, ToolTaint::Sink, true),
            (ToolTaint::Pure, ToolTaint::Pure, true),
            (ToolTaint::Pure, ToolTaint::Endorser, true),
            (ToolTaint::Endorser, ToolTaint::Source, true),
            (ToolTaint::Endorser, ToolTaint::Sink, true),
            (ToolTaint::Endorser, ToolTaint::Pure, true),
            (ToolTaint::Endorser, ToolTaint::Endorser, true),
        ];
        for (from, to, expected) in cases {
            assert_eq!(
                from.can_flow_to(&to),
                expected,
                "{from} -> {to} should be {expected}",
            );
        }
    }

    #[test]
    fn display_lowercase() {
        assert_eq!(ToolTaint::Source.to_string(), "source");
        assert_eq!(ToolTaint::Sink.to_string(), "sink");
        assert_eq!(ToolTaint::Pure.to_string(), "pure");
        assert_eq!(ToolTaint::Endorser.to_string(), "endorser");
    }

    #[test]
    fn serde_roundtrip() {
        for taint in [
            ToolTaint::Source,
            ToolTaint::Sink,
            ToolTaint::Pure,
            ToolTaint::Endorser,
        ] {
            let json = serde_json::to_string(&taint).expect("serialize");
            let back: ToolTaint = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(taint, back, "roundtrip failed for {taint}");
        }
    }

    #[test]
    fn serde_serializes_as_variant_name() {
        // serde default: variant name as-is.
        assert_eq!(
            serde_json::to_string(&ToolTaint::Source).expect("serialize"),
            "\"Source\""
        );
        assert_eq!(
            serde_json::to_string(&ToolTaint::Endorser).expect("serialize"),
            "\"Endorser\""
        );
    }
}
