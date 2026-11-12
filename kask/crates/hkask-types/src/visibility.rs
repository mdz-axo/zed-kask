//! Visibility types — Loop 6 (Cybernetics): access classification
//
//! Visibility is a Cybernetics concern — the Access Guard (6.1) enforces
//! visibility boundaries. Public/Private data categories determine
//! what each loop can read or write.
//
//! Defines a three-tier visibility model:
//! - Private: agent-specific access (episodic/experiential knowledge)
//! - Shared: consent-bound access (shared knowledge, not universally public)
//! - Public: universal access (no consent required)
//!
//! Enforcement is structural (doc re-anchored 2026-11-07, dual-axis
//! review E11 — the deleted `CapabilityToken`/`SovereigntyChecker`
//! machinery no longer exists to cite): the per-user data directory IS
//! the enforcement perimeter (P4.1 — one encrypted SQLCipher file per
//! user; no cross-user handle means no cross-user access is structurally
//! possible), the private store split (`curator-private.db`, D100), and
//! the M5 provenance partition (`call_admits` at the curator child's
//! recall and direct-ID surfaces).
//!
//! # Visibility Model
//!
//! | Level | Meaning | Enforcement |
//! |-------|---------|-------------|
//! | Private | Agent-specific access | Structural isolation (P4.1) + M5 provenance filters |
//! | Shared | Consent-bound shared access | P2.1 consent gates (OUGHT — not yet implemented) |
//! | Public | Universal access | No consent gate required |

use serde::{Deserialize, Serialize};

use crate::id::WebID;

/// Visibility level for artifacts
/// Loop: Cybernetics
///
/// Classification enum used by `Goal` and `HMem` to categorize data
/// accessibility (the deleted `SovereigntyChecker` no longer cites it).
/// Enforcement is structural — see the module doc (the P4.1 perimeter
/// plus the M5 provenance partition).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    #[default]
    Private,
    Shared,
    Public,
}

impl Visibility {
    /// Get string representation of visibility.
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// post: returns "private", "shared", or "public"
    pub fn as_str(&self) -> &'static str {
        match self {
            Visibility::Private => "private",
            Visibility::Shared => "shared",
            Visibility::Public => "public",
        }
    }

    /// Parse visibility from string.
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// post: returns Some(Visibility) if valid, None otherwise
    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "private" | "Private" => Some(Visibility::Private),
            "shared" | "Shared" => Some(Visibility::Shared),
            "public" | "Public" => Some(Visibility::Public),
            _ => None,
        }
    }

    // F-SYN-003: the three `is_*` predicates were dead code
    // (F-L1-005). Removed entirely. Use
    // `match self { Visibility::Private => ..., ... }` at the
    // call site.
}

impl std::fmt::Display for Visibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which inference leg produced this content — FR-4's provenance mark
/// (the private-inference marking predicate, D-2 2026-10-30).
///
/// `Visibility` answers WHO may see a record; this attribute answers
/// WHERE it was produced — a privacy-marked thread (one that runs only
/// local Ollama or llama.cpp models) marks its content so the
/// per-surface filters (FR-4's recall/export blocks) and FR-3's routing
/// enforcement can hold the line. The two axes compose; no parallel
/// predicate exists (P5.5). Engine-independent: the mark travels with
/// the content, never with a particular inference engine.
///
/// Ontology: `dcterms:provenance` — a statement of custody significant
/// for the record's authenticity and interpretation: its production
/// channel.
///
/// Principle ties: P11 (Digital Public/Private Sphere — users explicitly
/// control what is private), P2.1 (Shared vs Public Visibility — the FR-4
/// filters are that OUGHT's first enforcement leg).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InferenceProvenance {
    /// Produced on public (non-private) providers — the unmarked default.
    #[default]
    PublicProvider,
    /// Produced by a thread routed to private inference: this content
    /// must never flow to a public provider (FR-3's global falsifier) and
    /// stays inside private recall and export paths (FR-4).
    PrivateInference,
}

/// The MCP call-envelope `_meta` key carrying the calling context's
/// provenance from the zed-side dispatch to a kask server's per-call
/// provenance reader (SEP-1319; the private-inference M5 design, FR-4's
/// falsifier (b) tool legs). The parent (`hkask-mcp`'s
/// `invoke_with_call_provenance`) stamps
/// `InferenceProvenance::PrivateInference.as_str()` under this key for a
/// private-marked thread's calls — and nothing for a public call — and
/// `hkask-mcp-server`'s `request_meta_provenance` parses it (only the
/// exact private string claims private; absent/unparsable reads public,
/// fail-closed for recall). One constant for both ends of the wire: the
/// key must never fork between the writer and the reader.
pub const META_INFERENCE_PROVENANCE: &str = "inference_provenance";

impl InferenceProvenance {
    pub fn as_str(&self) -> &'static str {
        match self {
            InferenceProvenance::PublicProvider => "public_provider",
            InferenceProvenance::PrivateInference => "private_inference",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "public_provider" | "PublicProvider" => Some(InferenceProvenance::PublicProvider),
            "private_inference" | "PrivateInference" => Some(InferenceProvenance::PrivateInference),
            _ => None,
        }
    }

    /// Map the zed side's thread privacy flag (the D100 mark on
    /// `KaskThreadState`) to this enum — the one conversion home for the
    /// bool→mark mapping (the bridge's ingest seam and the agent's tool-call
    /// stamp both route through it; a second inline mapping is a fork of the
    /// same decision).
    pub fn from_private_flag(private_inference: bool) -> Self {
        if private_inference {
            InferenceProvenance::PrivateInference
        } else {
            InferenceProvenance::PublicProvider
        }
    }

    /// Parse a stored mark. A value outside the closed set reads as
    /// `PrivateInference` with a warn — privacy fails CLOSED (V7, the limit
    /// as positive): a corrupted mark must never read as public and flow
    /// into a public recall or export path. This is the deliberate mirror
    /// of the extraction `Provenance`'s fail-safe-down direction
    /// (`hkask-storage`): there an unknown tier lowers reliance, here an
    /// unknown mark raises exclusion. The direction differs because the
    /// harm differs — a false tool-observed elevates trust, a false
    /// public_provider breaches privacy.
    ///
    /// The legacy default is a different question and lives in the schema,
    /// not here: rows written before this mark existed were all produced on
    /// public providers (private inference did not exist yet), so the
    /// column's `DEFAULT 'public_provider'` backfill is honest history, not
    /// a fail-open. Only a value that is present-but-unknown (corruption,
    /// future enum drift) takes the fail-closed branch.
    pub fn parse_stored(text: &str) -> Self {
        match Self::parse_str(text) {
            Some(provenance) => provenance,
            None => {
                tracing::warn!(
                    target: "reg.storage",
                    mark = text,
                    "unknown inference-provenance mark — reading as private_inference (privacy fails closed)"
                );
                InferenceProvenance::PrivateInference
            }
        }
    }
}

impl std::fmt::Display for InferenceProvenance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_the_unmarked_public_provider_leg() {
        assert_eq!(
            InferenceProvenance::default(),
            InferenceProvenance::PublicProvider
        );
    }

    #[test]
    fn string_forms_round_trip() {
        for provenance in [
            InferenceProvenance::PublicProvider,
            InferenceProvenance::PrivateInference,
        ] {
            assert_eq!(
                InferenceProvenance::parse_str(provenance.as_str()),
                Some(provenance)
            );
            assert_eq!(provenance.to_string(), provenance.as_str());
        }
        // The provenance mark is not the visibility tier: "private"
        // belongs to Visibility and must not parse here.
        assert_eq!(InferenceProvenance::parse_str("private"), None);
    }

    #[test]
    fn serde_round_trips_snake_case() {
        let marked = InferenceProvenance::PrivateInference;
        let json = serde_json::to_string(&marked).expect("serialize");
        assert_eq!(json, r#""private_inference""#);
        let parsed: InferenceProvenance = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, marked);
    }

    #[test]
    fn from_private_flag_maps_the_d100_flag_both_ways() {
        assert_eq!(
            InferenceProvenance::from_private_flag(true),
            InferenceProvenance::PrivateInference
        );
        assert_eq!(
            InferenceProvenance::from_private_flag(false),
            InferenceProvenance::PublicProvider
        );
    }

    #[test]
    fn the_meta_key_is_the_exact_wire_spelling() {
        // The `_meta` wire key both the parent write side (hkask-mcp) and the
        // child read side (hkask-mcp-server's call_provenance.rs) consume —
        // pinned here so a drift from either end fails at the shared home,
        // not as a silent public read in production.
        assert_eq!(META_INFERENCE_PROVENANCE, "inference_provenance");
    }

    #[test]
    fn unknown_stored_marks_read_private_not_public() {
        // A corrupted or future mark never reads as public — privacy
        // fails closed (the mirror of the extraction provenance's
        // fail-safe-down, and the reason this lives on the type: every
        // store that reads the mark must share one fail direction).
        assert_eq!(
            InferenceProvenance::parse_stored(""),
            InferenceProvenance::PrivateInference
        );
        assert_eq!(
            InferenceProvenance::parse_stored("future_variant"),
            InferenceProvenance::PrivateInference
        );
        // Known marks parse exactly — the warn branch never fires on them.
        assert_eq!(
            InferenceProvenance::parse_stored("public_provider"),
            InferenceProvenance::PublicProvider
        );
        assert_eq!(
            InferenceProvenance::parse_stored("private_inference"),
            InferenceProvenance::PrivateInference
        );
    }
}

/// Access control grouping for h_mems and other stored artifacts.
///
/// \[DECLARATIVE\] Bundles the three fields that always appear together: (P5 — Essentialism).
/// `perspective`, `visibility`, and `owner_webid`. This value object
/// replaces the repeated pattern of passing these three as separate
/// parameters (Fowler H10: Group Data Clump Into Object).
///
/// Canonical constructor:
/// - `AccessControl::new(owner)` — default: private, no perspective
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct AccessControl {
    pub perspective: Option<WebID>,
    pub visibility: Visibility,
    pub owner_webid: WebID,
}

impl AccessControl {
    /// Create a default access control: private, no perspective, owned by `owner`.
    /// Create a new AccessControl with owner.
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  owner is valid
    /// post: returns AccessControl with Private visibility, no perspective
    pub fn new(owner: WebID) -> Self {
        Self {
            perspective: None,
            visibility: Visibility::Private,
            owner_webid: owner,
        }
    }

    #[must_use = "builder methods must be chained or assigned"]
    /// Set perspective (builder).
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// post: returns Self with perspective set
    pub fn with_perspective(mut self, perspective: WebID) -> Self {
        self.perspective = Some(perspective);
        self
    }

    /// Set the visibility mode.
    ///
    /// F-SYN-004: this method refuses the visibility *flip* from a
    /// private/perspective-bound (episodic) value to a shared/public
    /// value. Flipping would expose the perspective to a wider
    /// audience without removing the perspective, which is the
    /// Set visibility (builder).
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// post: returns Self with visibility set
    pub fn with_visibility(mut self, visibility: Visibility) -> Self {
        self.visibility = visibility;
        self
    }
}

/// Confidence value for h_mems, clamped to [0.0, 1.0].
///
/// Replaces bare `f64` confidence values with a type-safe newtype
/// (Fowler: Replace Primitive with Object). Confidence is always
/// within [0.0, 1.0] — values outside this range are clamped on
/// construction.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, serde::Serialize, serde::Deserialize)]
pub struct Confidence(f64);

impl Confidence {
    /// Create a confidence value, clamping to [0.0, 1.0].
    /// Create a new Confidence value.
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  value in [0.0, 1.0]
    /// post: returns Confidence
    pub fn new(value: f64) -> Self {
        Self(value.clamp(0.0, 1.0))
    }

    /// Full confidence (1.0) — the default for new h_mems.
    /// Full confidence (1.0).
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// post: returns Confidence(1.0)
    pub fn full() -> Self {
        Self(1.0)
    }

    /// Get the raw confidence value.
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// post: returns f64 value
    pub fn value(&self) -> f64 {
        self.0
    }

    /// Apply Wozniak-Gorzelanczyk forgetting curve: `confidence * exp(-t / S)`.
    ///
    /// Based on the two-component model of long-term memory (Wozniak & Gorzelanczyk,
    /// 1995, Acta Neurobiologiae Experimentalis, 55, 301-305), equation (3):
    ///
    /// ```text
    /// R(t) = exp(-t / S)
    /// ```rust,no_run
    ///
    /// Where:
    /// - `t` is days since most recent recall
    /// - `S` is memory life in days (configurable, default 180 = 6×30 days)
    ///
    /// At `t = S`: `R = exp(-1) ≈ 0.368`. At the halflife `H = S·ln(2)`:
    /// `R(H) = exp(-ln(2)) = 0.5`.
    ///
    /// # Edge cases
    ///
    /// - `memory_life_days ≤ 0`: infinite decay — confidence saturates to 0.0
    ///   for any elapsed time, except at t=0 which preserves original confidence.
    /// - `days_since_recall ≤ 0`: no decay — returns original confidence.
    /// - Both zero: original confidence preserved (no time has passed).
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  days_since_recall is any finite f64
    /// pre:  memory_life_days is any finite f64
    /// post: returns decayed Confidence in [0, 1], never NaN
    pub fn memory_decay(&self, days_since_recall: f64, memory_life_days: f64) -> Self {
        // Guard: no time has passed — no decay regardless of S.
        if days_since_recall <= 0.0 {
            return *self;
        }
        // Guard: infinite decay — confidence saturates to zero.
        if memory_life_days <= 0.0 {
            return Self(0.0);
        }
        Self((self.0 * (-days_since_recall / memory_life_days).exp()).clamp(0.0, 1.0))
    }
}

impl From<f64> for Confidence {
    fn from(value: f64) -> Self {
        Self::new(value)
    }
}

impl From<Confidence> for f64 {
    fn from(c: Confidence) -> Self {
        c.0
    }
}

impl std::fmt::Display for Confidence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:.4}", self.0)
    }
}

// ── 5W1H Dimension ──────────────────────────────────────────────────────────

/// The 5W1H dimension of a h_mem — which curator ontology category it belongs to.
///
/// Maps to the `OntologyAnchor::Core` tier (5W1H universal ground). Every h_mem
/// is classified into exactly one dimension.
///
/// # Serde
///
/// Serializes as lowercase snake_case strings: "who", "what", "where", etc.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dimension {
    /// An agent, persona, actor, or entity identity
    Who,
    /// An event, action, occurrence, or state change
    What,
    /// A location, path, address, or spatial context
    Where,
    /// A temporal fact, timestamp, duration, or ordering
    When,
    /// A reason, cause, motivation, or dependency
    Why,
    /// A method, technique, mechanism, or procedure
    How,
}

impl Dimension {
    /// Return the lowercase string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Dimension::Who => "who",
            Dimension::What => "what",
            Dimension::Where => "where",
            Dimension::When => "when",
            Dimension::Why => "why",
            Dimension::How => "how",
        }
    }
}

impl std::fmt::Display for Dimension {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for Dimension {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "who" => Ok(Dimension::Who),
            "what" => Ok(Dimension::What),
            "where" => Ok(Dimension::Where),
            "when" => Ok(Dimension::When),
            "why" => Ok(Dimension::Why),
            "how" => Ok(Dimension::How),
            other => Err(format!("unknown dimension: {other}")),
        }
    }
}

// ── 5W1H Dimension ──────────────────────────────────────────────────────────
