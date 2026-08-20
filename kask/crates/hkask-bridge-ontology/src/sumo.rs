//! SUMO (Suggested Upper Merged Ontology) vocabulary bridge.
//!
//! SUMO is the upper ontology — the general-purpose fallback for domains
//! that don't map to a specific supplement (FIBO, ESO, GOLEM, ML-Schema).
//! It provides the foundational categories (Entity, Process, Object, Agent,
//! Relation) that all domain supplements ultimately specialize. Where Dublin
//! Core is the universal state axis and PKO is the universal process axis,
//! SUMO is the universal *upper* ontology that gives those axes a formal
//! grounding when no domain supplement applies.
//!
//! SUMO is the 9th ontology in the bridge and the universal fallback for
//! `select_ontology_anchor`: unknown domains that previously fell through to
//! `OntologyAnchor::Core` (bare 5W1H) now route to SUMO when the domain hint
//! suggests a real entity/process that deserves formal categorization beyond
//! the interrogative ground.
//!
//! Reference: <https://github.com/ontologyportal/sumo>
//! Pease, A. (2010). Ontology: A Practical Guide. Articulate Software Press.
//! SUMO is actively maintained by Adam Pease and the Ontology Portal project.
//!
//! This module holds the SUMO seed vocabulary only — the top-level categories
//! most directly entailed by hKask artifacts. SUMO is large (~20k terms); we
//! extract only the seed terms the tools actually produce.

/// A SUMO concept URI — the canonical identifier for an upper-ontology concept.
pub type SumoConcept = &'static str;

// ── Foundational categories (SUMO top-level) ────────────────────────────────

/// The root of the SUMO hierarchy — anything that exists.
/// SUMO: `sumo:Entity` (subclass of nothing; the root).
pub const ENTITY: SumoConcept = "sumo:Entity";

/// A physical or abstract object — something with identity that persists.
/// SUMO: `sumo:Object` (subclass of Entity).
pub const OBJECT: SumoConcept = "sumo:Object";

/// A process — a temporally extended event or action.
/// SUMO: `sumo:Process` (subclass of Entity; the dynamic counterpart to Object).
pub const PROCESS: SumoConcept = "sumo:Process";

/// An agent — something that can act intentionally.
/// SUMO: `sumo:Agent` (subclass of Object; has the capacity for intentional action).
pub const AGENT: SumoConcept = "sumo:Agent";

/// A relation between entities — the formal grounding for KG triples.
/// SUMO: `sumo:Relation` (subclass of Entity; the meta-level for predicates).
pub const RELATION: SumoConcept = "sumo:Relation";

// ── Cognitive / informational categories ────────────────────────────────────

/// A proposition — a declarative content that can be true or false.
/// SUMO: `sumo:Proposition` (subclass of Entity; the carrier of truth values).
pub const PROPOSITION: SumoConcept = "sumo:Proposition";

/// A representation — an artifact that encodes information about something.
/// SUMO: `sumo:Representation` (subclass of Object; a sign/symbol/record).
pub const REPRESENTATION: SumoConcept = "sumo:Representation";

/// A text — a linguistic representation.
/// SUMO: `sumo:Text` (subclass of Representation; a document/passage/utterance).
pub const TEXT: SumoConcept = "sumo:Text";

/// A quantity — a measurable amount.
/// SUMO: `sumo:Quantity` (subclass of Entity; the carrier of measurements).
pub const QUANTITY: SumoConcept = "sumo:Quantity";

/// A time measure — a temporal quantity or interval.
/// SUMO: `sumo:TimeMeasure` (subclass of Quantity; temporal ordering/duration).
pub const TIME_MEASURE: SumoConcept = "sumo:TimeMeasure";

// ── Relations (for KG triples) ──────────────────────────────────────────────

/// A subsumption / sub-class relation — A is-a B.
/// SUMO: `sumo:subclass` (the formal is-a relation).
pub const SUBCLASS: SumoConcept = "sumo:subclass";

/// A part-of relation — A is part of B.
/// SUMO: `sumo:part` (mereological containment).
pub const PART: SumoConcept = "sumo:part";

/// An attribute relation — A has-attribute B.
/// SUMO: `sumo:attribute` (property ascription).
pub const ATTRIBUTE: SumoConcept = "sumo:attribute";

/// A cause relation — A causes B.
/// SUMO: `sumo:causes` (causal dependency; the "why" grounding).
pub const CAUSES: SumoConcept = "sumo:causes";

/// All SUMO seed concepts, for validation or iteration.
pub const ALL_CONCEPTS: &[SumoConcept] = &[
    ENTITY,
    OBJECT,
    PROCESS,
    AGENT,
    RELATION,
    PROPOSITION,
    REPRESENTATION,
    TEXT,
    QUANTITY,
    TIME_MEASURE,
    SUBCLASS,
    PART,
    ATTRIBUTE,
    CAUSES,
];
