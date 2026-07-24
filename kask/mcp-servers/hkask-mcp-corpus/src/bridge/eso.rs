//! Epistemic Science Ontology (ESO) bridge.
//!
//! Canonical predicate URIs for epistemic and scientific reasoning concepts —
//! hypotheses, evidence, theories, models, falsification, uncertainty, and
//! inferential relationships. Used by docproc extract_triples for expository
//! passages on science, systems thinking, forecasting, complexity, and
//! research methodology.
//!
//! Relevant for corpus content: Tetlock (superforecasting), Meadows (systems),
//! Miller (complex adaptive systems), Popper (falsifiability), Ousterhout
//! (software design), Marletto (can and can't), Kauffman (complexity).
//!
//! Pattern: thin mapping layer — canonical URI constants, no dependencies,
//! no reasoners, no overhead. Mirrors hkask-bridge-dublincore and
//! hkask-bridge-pko.

/// An ESO concept URI.
pub type EsoConcept = &'static str;

// ── Epistemic entities ────────────────────────────────────────────────────

/// A hypothesis proposed in the text.
pub const HAS_HYPOTHESIS: EsoConcept = "eso:hasHypothesis";
/// A theory discussed or referenced.
pub const HAS_THEORY: EsoConcept = "eso:hasTheory";
/// A model or framework described.
pub const HAS_MODEL: EsoConcept = "eso:hasModel";
/// A factual claim made by the author.
pub const HAS_CLAIM: EsoConcept = "eso:hasClaim";
/// An assumption underlying an argument.
pub const HAS_ASSUMPTION: EsoConcept = "eso:hasAssumption";
/// Evidence supporting a claim or hypothesis.
pub const HAS_EVIDENCE: EsoConcept = "eso:hasEvidence";
/// A limitation or boundary condition of a theory or model.
pub const HAS_LIMITATION: EsoConcept = "eso:hasLimitation";

// ── Inferential relationships ─────────────────────────────────────────────

/// What follows from a claim or theory.
pub const IMPLIES: EsoConcept = "eso:implies";
/// What contradicts or challenges a claim.
pub const CONTRADICTS: EsoConcept = "eso:contradicts";
/// What would falsify a claim (Popper).
pub const FALSIFIED_BY: EsoConcept = "eso:falsifiedBy";
/// What corroborates or supports a claim.
pub const CORROBORATED_BY: EsoConcept = "eso:corroboratedBy";
/// What the claim or finding generalizes to.
pub const GENERALIZES_TO: EsoConcept = "eso:generalizesTo";

// ── Epistemic qualities ───────────────────────────────────────────────────

/// Uncertainty associated with a claim or forecast.
pub const HAS_UNCERTAINTY: EsoConcept = "eso:hasUncertainty";
/// Confidence level in a finding or prediction.
pub const HAS_CONFIDENCE: EsoConcept = "eso:hasConfidence";
/// The method or approach used to produce knowledge.
pub const USES_METHOD: EsoConcept = "eso:usesMethod";
/// A counterargument or alternative explanation.
pub const HAS_COUNTERARGUMENT: EsoConcept = "eso:hasCounterargument";

/// All ESO predicates, for validation or iteration.
pub const ALL_PREDICATES: &[EsoConcept] = &[
    HAS_HYPOTHESIS,
    HAS_THEORY,
    HAS_MODEL,
    HAS_CLAIM,
    HAS_ASSUMPTION,
    HAS_EVIDENCE,
    HAS_LIMITATION,
    IMPLIES,
    CONTRADICTS,
    FALSIFIED_BY,
    CORROBORATED_BY,
    GENERALIZES_TO,
    HAS_UNCERTAINTY,
    HAS_CONFIDENCE,
    USES_METHOD,
    HAS_COUNTERARGUMENT,
];
