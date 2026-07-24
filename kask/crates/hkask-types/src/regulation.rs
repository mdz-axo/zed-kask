//! Core Regulation (Cybernetic Nervous System) types for hKask
//!
//! Core spans: reg.tool.*, reg.inference.*, reg.fusion.*, reg.agent_pod.*,
//! reg.gas.*, reg.curation.*, reg.heal.*, reg.memory.encode.*
//!
//! Domain-specific spans have moved to their respective domain crates.
//!
//! `CANONICAL_NAMESPACES` (in `event.rs`) is the single source of truth for
//! **canonical** Regulation spans — the essential, regulation record-eligible spans that are
//! `SpanNamespace`-validated, `SpanCategory`-categorized, and loop-connected.
//! The `reg.*` prefix is reserved for canonical spans: every `reg.*` tracing
//! target MUST be registered in `CANONICAL_NAMESPACES`. **Performative**
//! telemetry (per PRINCIPLES §9.1) uses `hkask.*` tracing targets (e.g.
//! `hkask.cli`, `hkask.training.job.submit`), NOT `reg.*`; those are observability
//! logs, not loop variables, and `SpanNamespace::new` rejects them.

use serde::{Deserialize, Serialize};

// ── Domain newtypes (P2.3) ──────────────────────────────────────────────────

/// Communication queue depth for backpressure regulation.
///
/// Newtype wrapper that prevents accidental confusion with other numeric
/// thresholds in `SetPoints` (gas, variety deficit, error rate).
///
/// Defined in hkask-types (substrate crate) because it is shared across
/// hkask-regulation (SetPoints, cybernetics loop) and hkask-agents
/// (communication loop).
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct QueueDepth(pub f64);

impl QueueDepth {
    /// Create a queue depth threshold.
    pub fn new(value: f64) -> Self {
        QueueDepth(value.max(0.0))
    }

    /// Default backpressure threshold: 100 messages.
    pub const DEFAULT_BACKPRESSURE: QueueDepth = QueueDepth(100.0);

    /// Return the raw `f64` value.
    pub fn as_raw(self) -> f64 {
        self.0
    }
}

// Circuit Breaker — States

/// Circuit breaker states
///
/// Defined here (not in hkask-regulation) so the `CircuitBreakerPort` trait can
/// reference it without creating an upward dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

// Regulation Health — Observability data struct

/// Regulation health status
///
/// Pure data struct — construction logic (`reg_health_check`) lives in
/// hkask-regulation where it has access to `AlgedonicManager`.
#[derive(Debug, Clone)]
pub struct LedgerHealth {
    pub overall_deficit: u64,
    pub critical_count: usize,
    pub warning_count: usize,
    pub healthy: bool,
    /// Session-level EMA of domain variety (survives window resets).
    /// 0.0 when no domains have been tracked.
    pub variety_ema: f64,
}

/// Regulation loop health — the Curator's window into regulatory effectiveness.
///
/// Aggregated from `ImpactReport` decisions across regulation cycles.
/// Enables the metacognition loop to answer: "are our regulatory actions working?"
#[derive(Debug, Clone, Default)]
pub struct RegulationHealth {
    /// Total regulation cycles recorded.
    pub total_cycles: u64,
    /// Actions accepted (improved or within noise tolerance).
    pub accepted: u64,
    /// Actions staged for review (moderately ineffective).
    pub staged: u64,
    /// Actions blocked (severely counterproductive).
    pub blocked: u64,
}

impl RegulationHealth {
    /// Ratio of accepted actions to total (0.0–1.0). 1.0 if no actions recorded.
    pub fn effectiveness(&self) -> f64 {
        let total = self.accepted + self.staged + self.blocked;
        if total == 0 {
            1.0
        } else {
            self.accepted as f64 / total as f64
        }
    }
}

// ── RegulationSpan — Core Regulation Span Identifiers ────────────────────────────────────

/// Core Regulation span identifiers — spans that are constructed in 2+ crates from
/// different dependency domains (the "cross-cutting concern" test).
///
/// backup, ACP, curator, etc.) have moved to their respective domain crates
/// as enums implementing [`ObservableSpan`](crate::ObservableSpan).
///
/// `CANONICAL_NAMESPACES` (in `event.rs`) is the single source of truth for
/// **canonical** Regulation spans — essential spans that are `SpanNamespace`-validated,
/// `SpanCategory`-categorized, and connected to a cybernetic loop. The `reg.*`
/// prefix is reserved for these canonical spans: every `reg.*` tracing target
/// MUST be registered. Per PRINCIPLES §9.1, performative telemetry uses
/// `hkask.*` tracing targets (not `reg.*`); those are observability logs, not
/// loop variables, and `SpanNamespace::new` rejects them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RegulationSpan {
    /// Tool invocation span. Subsystem tracks which MCP server emitted the span.
    Tool { subsystem: ToolSubsystem },
    /// LLM inference request/response.
    Inference,
    /// Multi-model fusion deliberation (panel dispatch + judge orchestration).
    /// Distinct from `Inference` so fusion rounds, convergence, and panel/judge
    /// cost are independently observable (PRINCIPLES.md §9.1).
    Fusion,
    /// Agent pod lifecycle events.
    AgentPod,
    /// Gas (energy) consumption tracking.
    Gas,
    /// Curation loop operations — registry sync, pod sync, directive issuance.
    Curation,
    /// Self-healing operation span. Canonical string: `"reg.heal"`.
    SelfHeal,
    /// Memory encoding operations.
    MemoryEncode,
}

/// Subsystem identifier for `RegulationSpan::Tool` — which MCP server emitted the span.
///
/// Derived from the `hkask-mcp-*` server naming convention.
/// Unknown or future servers use `Other`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ToolSubsystem {
    WebSearch,
    Condenser,
    Training,
    Corpus,
    Research,
    Communication,
    Registry,
    Wallet,
    Media,
    Kanban,
    Memory,
    Companies,
    Filesystem,
    Curator,
    /// Catch-all for unknown or future MCP servers.
    Other,
}

impl ToolSubsystem {
    /// Map an MCP server name (e.g., "memory", "hkask-mcp-corpus") to a ToolSubsystem.
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  server_name is a non-empty string
    /// post: returns the corresponding ToolSubsystem variant; Other if unknown
    pub fn from_server_name(server_name: &str) -> Self {
        let name = server_name
            .strip_prefix("hkask-mcp-")
            .unwrap_or(server_name);
        match name {
            "memory" => ToolSubsystem::Memory,
            "condenser" => ToolSubsystem::Condenser,
            "research" => ToolSubsystem::Research,
            "companies" => ToolSubsystem::Companies,
            "communication" => ToolSubsystem::Communication,
            "fal" | "media" => ToolSubsystem::Media,
            "corpus" => ToolSubsystem::Corpus,
            "training" => ToolSubsystem::Training,
            "kanban" => ToolSubsystem::Kanban,
            "curator" => ToolSubsystem::Curator,
            _ => ToolSubsystem::Other,
        }
    }

    /// Canonical string suffix for the subsystem (e.g., `"web_search"`).
    pub fn as_str(self) -> &'static str {
        match self {
            ToolSubsystem::WebSearch => "web_search",
            ToolSubsystem::Condenser => "condenser",
            ToolSubsystem::Training => "training",
            ToolSubsystem::Corpus => "corpus",
            ToolSubsystem::Research => "research",
            ToolSubsystem::Communication => "communication",
            ToolSubsystem::Registry => "registry",
            ToolSubsystem::Wallet => "wallet",
            ToolSubsystem::Media => "media",
            ToolSubsystem::Kanban => "kanban",
            ToolSubsystem::Memory => "memory",
            ToolSubsystem::Companies => "companies",
            ToolSubsystem::Filesystem => "filesystem",
            ToolSubsystem::Curator => "curator",
            ToolSubsystem::Other => "other",
        }
    }
}

impl RegulationSpan {
    /// Emit a typed Regulation span event through the `tracing` infrastructure.
    ///
    /// Enforces the canonical Regulation emission convention (PRINCIPLES.md §9.2):
    /// - `target` = `"reg"` root namespace (full domain in `reg_domain` field)
    /// - `reg_domain` = `self.as_str()` (e.g. `"reg.tool.media"`)
    /// - `operation` = the verb describing what occurred (e.g. `"invoked"`)
    /// - message = `"REG"` (required for downstream regulation record parsing)
    ///
    /// Callers that need additional structured fields can attach them by
    /// entering a child [`mod@tracing::span`] before calling `emit()`.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use hkask_types::regulation::{RegulationSpan, ToolSubsystem};
    ///
    /// RegulationSpan::Tool { subsystem: ToolSubsystem::Media }
    ///     .emit("invoked");
    /// ```
    pub fn emit(&self, operation: &str) {
        tracing::info!(
            target: "reg",
            reg_domain = %self.as_str(),
            operation = %operation,
            "REG",
        );
    }

    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is a valid RegulationSpan variant
    /// post: returns the canonical namespace string (e.g. "reg.tool.web_search"); output matches CANONICAL_NAMESPACES byte-for-byte
    ///
    /// This output must match regulation record serialization strings byte-for-byte
    /// (P8 — Semantic Grounding).
    pub fn as_str(&self) -> &'static str {
        match self {
            RegulationSpan::Tool { subsystem } => match subsystem {
                ToolSubsystem::WebSearch => "reg.tool.web_search",
                ToolSubsystem::Condenser => "reg.tool.condenser",
                ToolSubsystem::Training => "reg.tool.training",
                ToolSubsystem::Corpus => "reg.tool.corpus",
                ToolSubsystem::Research => "reg.tool.research",
                ToolSubsystem::Communication => "reg.tool.communication",
                ToolSubsystem::Registry => "reg.tool.registry",
                ToolSubsystem::Wallet => "reg.tool.wallet",
                ToolSubsystem::Media => "reg.tool.media",
                ToolSubsystem::Kanban => "reg.tool.kanban",
                ToolSubsystem::Memory => "reg.tool.memory",
                ToolSubsystem::Companies => "reg.tool.companies",
                ToolSubsystem::Filesystem => "reg.tool.filesystem",
                ToolSubsystem::Curator => "reg.tool.curator",
                ToolSubsystem::Other => "reg.tool",
            },
            RegulationSpan::Inference => "reg.inference",
            RegulationSpan::Fusion => "reg.fusion",
            RegulationSpan::AgentPod => "reg.pod",
            RegulationSpan::Gas => "reg.gas",
            RegulationSpan::Curation => "reg.curation",
            RegulationSpan::SelfHeal => "reg.heal",
            RegulationSpan::MemoryEncode => "reg.memory.encode",
        }
    }
}

impl std::fmt::Display for RegulationSpan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl crate::observable_span::ObservableSpan for RegulationSpan {
    fn as_str(&self) -> &'static str {
        RegulationSpan::as_str(self)
    }

    fn emit(&self, operation: &str) {
        RegulationSpan::emit(self, operation);
    }
}

impl std::str::FromStr for RegulationSpan {
    type Err = ();

    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  s is a string matching a canonical RegulationSpan namespace
    /// post: returns Ok(RegulationSpan) for canonical strings; Err(()) for unknown strings
    ///
    /// Only strings matching canonical `RegulationSpan` namespaces parse
    /// successfully. Unknown strings return `Err(())`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "reg.tool" => Ok(RegulationSpan::Tool {
                subsystem: ToolSubsystem::Other,
            }),
            "reg.tool.web_search" => Ok(RegulationSpan::Tool {
                subsystem: ToolSubsystem::WebSearch,
            }),
            "reg.tool.condenser" => Ok(RegulationSpan::Tool {
                subsystem: ToolSubsystem::Condenser,
            }),
            "reg.tool.training" => Ok(RegulationSpan::Tool {
                subsystem: ToolSubsystem::Training,
            }),
            "reg.tool.corpus" => Ok(RegulationSpan::Tool {
                subsystem: ToolSubsystem::Corpus,
            }),
            "reg.tool.research" => Ok(RegulationSpan::Tool {
                subsystem: ToolSubsystem::Research,
            }),
            "reg.tool.communication" => Ok(RegulationSpan::Tool {
                subsystem: ToolSubsystem::Communication,
            }),
            "reg.tool.registry" => Ok(RegulationSpan::Tool {
                subsystem: ToolSubsystem::Registry,
            }),
            "reg.tool.wallet" => Ok(RegulationSpan::Tool {
                subsystem: ToolSubsystem::Wallet,
            }),
            "reg.tool.media" => Ok(RegulationSpan::Tool {
                subsystem: ToolSubsystem::Media,
            }),
            "reg.tool.kanban" => Ok(RegulationSpan::Tool {
                subsystem: ToolSubsystem::Kanban,
            }),
            "reg.tool.memory" => Ok(RegulationSpan::Tool {
                subsystem: ToolSubsystem::Memory,
            }),
            "reg.tool.companies" => Ok(RegulationSpan::Tool {
                subsystem: ToolSubsystem::Companies,
            }),
            "reg.tool.filesystem" => Ok(RegulationSpan::Tool {
                subsystem: ToolSubsystem::Filesystem,
            }),
            "reg.tool.curator" => Ok(RegulationSpan::Tool {
                subsystem: ToolSubsystem::Curator,
            }),
            "reg.inference" => Ok(RegulationSpan::Inference),
            "reg.fusion" => Ok(RegulationSpan::Fusion),
            "reg.pod" => Ok(RegulationSpan::AgentPod),
            "reg.gas" => Ok(RegulationSpan::Gas),
            "reg.curation" => Ok(RegulationSpan::Curation),
            "reg.heal" => Ok(RegulationSpan::SelfHeal),
            "reg.memory.encode" => Ok(RegulationSpan::MemoryEncode),
            _ => Err(()),
        }
    }
}

#[cfg(test)]
mod reg_span_tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn regspan_display_produces_canonical_strings() {
        assert_eq!(
            RegulationSpan::Tool {
                subsystem: ToolSubsystem::Other
            }
            .to_string(),
            "reg.tool"
        );
        assert_eq!(RegulationSpan::Inference.to_string(), "reg.inference");
        assert_eq!(RegulationSpan::AgentPod.to_string(), "reg.pod");
        assert_eq!(RegulationSpan::Gas.to_string(), "reg.gas");
        assert_eq!(RegulationSpan::Curation.to_string(), "reg.curation");
        assert_eq!(RegulationSpan::SelfHeal.to_string(), "reg.heal");
        assert_eq!(
            RegulationSpan::MemoryEncode.to_string(),
            "reg.memory.encode"
        );
    }

    #[test]
    fn regspan_from_str_rejects_invalid() {
        assert!(RegulationSpan::from_str("reg.nonexistent").is_err());
        assert!(RegulationSpan::from_str("invalid").is_err());
        assert!(RegulationSpan::from_str("").is_err());
        assert!(RegulationSpan::from_str("tool").is_err()); // short form not supported
    }

    #[test]
    fn regspan_from_str_round_trips() {
        let variants = vec![
            "reg.tool",
            "reg.tool.web_search",
            "reg.tool.condenser",
            "reg.tool.training",
            "reg.tool.corpus",
            "reg.tool.research",
            "reg.tool.communication",
            "reg.tool.registry",
            "reg.tool.wallet",
            "reg.tool.media",
            "reg.tool.kanban",
            "reg.tool.memory",
            "reg.tool.companies",
            "reg.tool.filesystem",
            "reg.tool.curator",
            "reg.inference",
            "reg.pod",
            "reg.gas",
            "reg.curation",
            "reg.heal",
            "reg.memory.encode",
        ];
        for s in variants {
            let span: RegulationSpan = s.parse().expect("should parse");
            assert_eq!(span.to_string(), s, "Display should match input");
        }
    }

    #[test]
    fn regspan_tool_subsystem_produces_correct_string() {
        assert_eq!(
            RegulationSpan::Tool {
                subsystem: ToolSubsystem::WebSearch
            }
            .to_string(),
            "reg.tool.web_search"
        );
        assert_eq!(
            RegulationSpan::Tool {
                subsystem: ToolSubsystem::Other
            }
            .to_string(),
            "reg.tool"
        );
    }

    #[test]
    fn regspan_exhaustive_match_covers_all_canonical() {
        let all_variants = vec![
            RegulationSpan::Tool {
                subsystem: ToolSubsystem::Other,
            },
            RegulationSpan::Tool {
                subsystem: ToolSubsystem::WebSearch,
            },
            RegulationSpan::Tool {
                subsystem: ToolSubsystem::Condenser,
            },
            RegulationSpan::Tool {
                subsystem: ToolSubsystem::Training,
            },
            RegulationSpan::Tool {
                subsystem: ToolSubsystem::Corpus,
            },
            RegulationSpan::Tool {
                subsystem: ToolSubsystem::Research,
            },
            RegulationSpan::Tool {
                subsystem: ToolSubsystem::Communication,
            },
            RegulationSpan::Tool {
                subsystem: ToolSubsystem::Registry,
            },
            RegulationSpan::Tool {
                subsystem: ToolSubsystem::Wallet,
            },
            RegulationSpan::Tool {
                subsystem: ToolSubsystem::Media,
            },
            RegulationSpan::Tool {
                subsystem: ToolSubsystem::Kanban,
            },
            RegulationSpan::Tool {
                subsystem: ToolSubsystem::Memory,
            },
            RegulationSpan::Tool {
                subsystem: ToolSubsystem::Companies,
            },
            RegulationSpan::Tool {
                subsystem: ToolSubsystem::Filesystem,
            },
            RegulationSpan::Tool {
                subsystem: ToolSubsystem::Curator,
            },
            RegulationSpan::Inference,
            RegulationSpan::Fusion,
            RegulationSpan::AgentPod,
            RegulationSpan::Gas,
            RegulationSpan::Curation,
            RegulationSpan::SelfHeal,
            RegulationSpan::MemoryEncode,
        ];
        // Round-trip test: Display → FromStr → Display must be identity
        for variant in &all_variants {
            let s = variant.to_string();
            assert!(
                !s.is_empty(),
                "{:?} should produce non-empty Display",
                variant
            );
            assert!(
                s.starts_with("reg."),
                "{:?} should start with reg.",
                variant
            );
            let parsed: RegulationSpan = s
                .parse()
                .expect("Display output must round-trip via FromStr");
            assert_eq!(
                variant, &parsed,
                "{:?} round-trip mismatch: {} -> {:?}",
                variant, s, parsed
            );
        }
        // Assert count matches enum variant count (8 core + 15 specific ToolSubsystem = 23).
        // If this fails, a new RegulationSpan variant was added without updating this test.
        assert!(
            all_variants.len() == 23,
            "Regulation span exhaustive test should cover all RegulationSpan variants, found {} (expected 23)",
            all_variants.len()
        );
    }

    #[test]
    fn tool_subsystem_display_produces_valid_suffix() {
        assert_eq!(ToolSubsystem::WebSearch.as_str(), "web_search");
        assert_eq!(ToolSubsystem::Other.as_str(), "other");
    }
}
