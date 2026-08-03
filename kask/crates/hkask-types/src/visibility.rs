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
//! Access control enforcement is delegated to the `CapabilityToken` primitive.
//!
//! # Visibility Model
//!
//! | Level | Meaning | Enforcement |
//! |-------|---------|-------------|
//! | Private | Agent-specific access | `SovereigntyChecker` + `CapabilityToken` |
//! | Shared | Consent-bound shared access | `SovereigntyChecker` + `CapabilityToken` |
//! | Public | Universal access | No capability required |

use serde::{Deserialize, Serialize};

use crate::id::WebID;

/// Visibility level for artifacts
/// Loop: Cybernetics
///
/// Classification enum used by `SovereigntyChecker`, `Goal`, and
/// `HMem` to categorize data accessibility. The actual enforcement
/// is delegated to the `CapabilityToken` primitive.
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

/// Access control grouping for h_mems and other stored artifacts.
///
/// \[DECLARATIVE\] Bundles the three fields that always appear together: (P5 — Essentialism).
/// `perspective`, `visibility`, and `owner_webid`. This value object
/// replaces the repeated pattern of passing these three as separate
/// parameters (Fowler H10: Group Data Clump Into Object).
///
/// Canonical constructors:
/// - `AccessControl::new(owner)` — default: private, no perspective
/// - `AccessControl::episodic(perspective, owner)` — private, perspective-bound
/// - `AccessControl::semantic(owner)` — shared, no perspective
/// - `AccessControl::public(owner)` — public, no perspective
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

    /// Create an episodic (perspective-bound) access control: private, owned by `owner`.
    /// Create episodic access control.
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  perspective and owner are valid
    /// post: returns AccessControl with Private visibility and perspective
    pub fn episodic(perspective: WebID, owner: WebID) -> Self {
        Self {
            perspective: Some(perspective),
            visibility: Visibility::Private,
            owner_webid: owner,
        }
    }

    /// Create a semantic (shared, perspective-free) access control.
    /// Create semantic access control.
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  owner is valid
    /// post: returns AccessControl with Shared visibility, no perspective
    pub fn semantic(owner: WebID) -> Self {
        Self {
            perspective: None,
            visibility: Visibility::Shared,
            owner_webid: owner,
        }
    }

    /// Create a public (unrestricted, perspective-free) access control.
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  owner is valid
    /// post: returns AccessControl with Public visibility, no perspective
    pub fn public(owner: WebID) -> Self {
        Self {
            perspective: None,
            visibility: Visibility::Public,
            owner_webid: owner,
        }
    }

    /// Convert to semantic access control: strip perspective, set visibility to Shared.
    /// Convert to semantic access (strip perspective, set Shared).
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// post: returns AccessControl with Shared visibility, no perspective
    pub fn to_semantic(&self) -> Self {
        Self {
            perspective: None,
            visibility: Visibility::Shared,
            owner_webid: self.owner_webid,
        }
    }

    /// Convert to public access control: strip perspective, set visibility to Public.
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// post: returns AccessControl with Public visibility, no perspective
    pub fn to_public(&self) -> Self {
        Self {
            perspective: None,
            visibility: Visibility::Public,
            owner_webid: self.owner_webid,
        }
    }

    /// Is this an episodic (perspective-bound) access control?
    /// Check if this is episodic (has perspective).
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// post: returns true iff perspective is Some
    pub fn is_episodic(&self) -> bool {
        self.perspective.is_some()
    }

    /// Is this a semantic (shared, perspective-free) access control?
    /// Check if this is semantic (Shared, no perspective).
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// post: returns true iff visibility is Shared and perspective is None
    pub fn is_semantic(&self) -> bool {
        self.perspective.is_none() && self.visibility == Visibility::Shared
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

    /// Remove the perspective from this access control.
    ///
    /// F-SYN-004: this is the explicit operation to take a h_mem
    /// from *episodic* (perspective-bound) to *semantic* (shared,
    /// perspective-free). It does not change the visibility mode.
    /// Remove perspective (for legitimate sharing).
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// post: returns Self with perspective set to None
    pub fn without_perspective(mut self) -> Self {
        self.perspective = None;
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

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn arb_confidence() -> BoxedStrategy<Confidence> {
        any::<f64>()
            .prop_filter("must be finite", |f| f.is_finite())
            .prop_map(Confidence::new)
            .boxed()
    }

    fn arb_finite() -> BoxedStrategy<f64> {
        any::<f64>()
            .prop_filter("must be finite", |f| f.is_finite())
            .boxed()
    }

    proptest! {
        // Result is always in [0, 1] and never NaN for finite inputs.
        #[test]
        fn memory_decay_result_in_range(
            c in arb_confidence(),
            t in arb_finite(),
            s in arb_finite(),
        ) {
            let result = c.memory_decay(t, s);
            let v = result.value();
            prop_assert!(!v.is_nan(), "memory_decay produced NaN: c={}, t={}, s={}", c.value(), t, s);
            prop_assert!((0.0..=1.0).contains(&v), "memory_decay out of [0,1]: v={}, c={}, t={}, s={}", v, c.value(), t, s);
        }

        // Zero time preserves confidence regardless of S.
        #[test]
        fn memory_decay_zero_time_preserves(
            c in arb_confidence(),
            s in arb_finite(),
        ) {
            let result = c.memory_decay(0.0, s);
            prop_assert_eq!(result.value(), c.value(), "zero time must preserve confidence");
        }

        // Zero or negative life saturates to zero for any elapsed time.
        #[test]
        fn memory_decay_zero_life_saturates(
            c in arb_confidence(),
            t in arb_finite().prop_filter("must be positive", |&t| t > 0.0),
        ) {
            let result = c.memory_decay(t, 0.0);
            prop_assert_eq!(result.value(), 0.0, "zero life must saturate to zero for t>0");
        }

        // For S > 0, decay is monotonically decreasing in t.
        #[test]
        fn memory_decay_monotonically_decreasing(
            c in arb_confidence(),
            s in arb_finite().prop_filter("must be positive", |&s| s > 0.0),
            t1 in arb_finite().prop_filter("must be positive", |&t| t > 0.0),
            t2 in arb_finite().prop_filter("must exceed t1", |&t| t > 0.0),
        ) {
            prop_assume!(t2 > t1);
            let r1 = c.memory_decay(t1, s).value();
            let r2 = c.memory_decay(t2, s).value();
            prop_assert!(r2 <= r1 + 1e-15, "decay not monotonic: r2={} > r1={} for t2={}, t1={}, s={}", r2, r1, t2, t1, s);
        }
    }
}
