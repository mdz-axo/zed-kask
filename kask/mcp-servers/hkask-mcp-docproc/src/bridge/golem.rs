//! GOLEM narrative/literary ontology bridge.
//!
//! Canonical predicate URIs for narrative concepts — characters, events,
//! themes, literary devices, and interpretive relationships. Used by
//! docproc extract_triples for narrative passages (prose, fiction, memoir,
//! biography, narrative nonfiction).
//!
//! Pattern: thin mapping layer — canonical URI constants, no dependencies,
//! no reasoners, no overhead. Mirrors hkask-bridge-dublincore and
//! hkask-bridge-pko.

/// A GOLEM concept URI.
pub type GolemConcept = &'static str;

// ── Characters and agents ─────────────────────────────────────────────────

/// A character or person in the narrative.
pub const HAS_CHARACTER: GolemConcept = "golem:hasCharacter";
/// The narrator or narrative voice.
pub const HAS_NARRATOR: GolemConcept = "golem:hasNarrator";
/// The narrative perspective or point of view.
pub const HAS_PERSPECTIVE: GolemConcept = "golem:hasPerspective";

// ── Plot and structure ────────────────────────────────────────────────────

/// An event or action in the story.
pub const HAS_EVENT: GolemConcept = "golem:hasEvent";
/// A plot element or development.
pub const HAS_PLOT: GolemConcept = "golem:hasPlot";
/// A conflict or tension in the narrative.
pub const HAS_CONFLICT: GolemConcept = "golem:hasConflict";
/// How a conflict is resolved.
pub const HAS_RESOLUTION: GolemConcept = "golem:hasResolution";

// ── Setting and atmosphere ────────────────────────────────────────────────

/// The setting or location of the narrative.
pub const HAS_SETTING: GolemConcept = "golem:hasSetting";
/// The tone or mood of the passage.
pub const HAS_TONE: GolemConcept = "golem:hasTone";

// ── Theme and meaning ─────────────────────────────────────────────────────

/// The central theme or idea.
pub const HAS_THEME: GolemConcept = "golem:hasTheme";
/// A recurring motif or pattern.
pub const HAS_MOTIF: GolemConcept = "golem:hasMotif";
/// A symbol or symbolic element.
pub const HAS_SYMBOL: GolemConcept = "golem:hasSymbol";

// ── Interpretive relationships ───────────────────────────────────────────

/// Allegorical meaning or representation.
pub const ALLEGORY_OF: GolemConcept = "golem:allegoryOf";
/// Metaphorical meaning.
pub const METAPHOR_FOR: GolemConcept = "golem:metaphorFor";
/// What concept or principle the narrative illustrates.
pub const ILLUSTRATES: GolemConcept = "golem:illustrates";
/// What emotion or idea the passage evokes.
pub const EVOKES: GolemConcept = "golem:evokes";

/// All GOLEM predicates, for validation or iteration.
pub const ALL_PREDICATES: &[GolemConcept] = &[
    HAS_CHARACTER,
    HAS_NARRATOR,
    HAS_PERSPECTIVE,
    HAS_EVENT,
    HAS_PLOT,
    HAS_CONFLICT,
    HAS_RESOLUTION,
    HAS_SETTING,
    HAS_TONE,
    HAS_THEME,
    HAS_MOTIF,
    HAS_SYMBOL,
    ALLEGORY_OF,
    METAPHOR_FOR,
    ILLUSTRATES,
    EVOKES,
];
