use hkask_types::registry::RegistryEntry;

/// Known vocabulary terms — bootstrapped from manifest `lexicon_terms` across the skill corpus.
///
/// Terms are sorted alphabetically for binary-search lookup.
/// New terms should be added in sorted order.
const KNOWN_TERMS: &[&str] = &[
    "abduct",
    "accept",
    "accommodate",
    "acknowledge",
    "act",
    "adapt",
    "adjust",
    "admit",
    "advise",
    "affirm",
    "agent",
    "aggregate",
    "agree",
    "alert",
    "align",
    "allostery",
    "amplify",
    "analogy",
    "analyze",
    "anchor",
    "anomaly",
    "answer",
    "apply",
    "assemble",
    "assert",
    "assess",
    "attenuate",
    "audit",
    "authorize",
    "backup",
    "balance",
    "base",
    "baseline",
    "batch",
    "bind",
    "blend",
    "block",
    "bound",
    "branch",
    "budget",
    "build",
    "calculate",
    "calibrate",
    "calibration",
    "canonicalize",
    "capability",
    "capture",
    "catalog",
    "categorize",
    "ceiling",
    "chain",
    "challenge",
    "charter",
    "check",
    "choice",
    "cite",
    "claim",
    "clarify",
    "classify",
    "cmmi",
    "coach",
    "coalesce",
    "collect",
    "combine",
    "command",
    "commit",
    "commoditize",
    "compact",
    "compare",
    "compatibility",
    "complete",
    "complexity",
    "compliance",
    "component",
    "compose",
    "compress",
    "compute",
    "condition",
    "confidence",
    "config",
    "configure",
    "confirm",
    "conform",
    "consent",
    "consistency",
    "consolidate",
    "constrain",
    "construct",
    "context",
    "contextualise",
    "contract",
    "contradiction",
    "convention",
    "converge",
    "coordinate",
    "correlate",
    "corroborate",
    "counterfactual",
    "coverage",
    "create",
    "critique",
    "crystallize",
    "cultivate",
    "curate",
    "cve",
    "dataset",
    "dead_code",
    "decide",
    "declare",
    "decompose",
    "deduce",
    "deepen",
    "defend",
    "defer",
    "definition",
    "delegate",
    "delegates",
    "deliver",
    "dependency",
    "deprecate",
    "derive",
    "describe",
    "design",
    "detach",
    "detect",
    "diagnose",
    "dialogue",
    "dimension",
    "discover",
    "discriminate",
    "dispatch",
    "distill",
    "divergence",
    "diversify",
    "divest",
    "document",
    "drift",
    "elicit",
    "eliminate",
    "embed",
    "emit",
    "encode",
    "encourage",
    "endure",
    "enforce",
    "engage",
    "enumerate",
    "epistemic",
    "escalate",
    "estimate",
    "evaluate",
    "evidence",
    "evolution",
    "evolve",
    "execute",
    "exemplify",
    "exercise",
    "expand",
    "expect",
    "expectation",
    "experiment",
    "explain",
    "explore",
    "export",
    "extract",
    "fact",
    "falsify",
    "feedback",
    "field",
    "filter",
    "finalize",
    "find",
    "finding",
    "fix",
    "flag",
    "floor",
    "flowdef",
    "focus",
    "forecast",
    "formalize",
    "format",
    "fractal",
    "frame",
    "gap",
    "gate",
    "gather",
    "generate",
    "goal",
    "gradient",
    "grasp",
    "ground",
    "guard",
    "guide",
    "habit",
    "handoff",
    "harness",
    "heal",
    "hunt",
    "hypothesise",
    "hypothesize",
    "identify",
    "identity",
    "impact",
    "import",
    "improve",
    "improvise",
    "index",
    "infer",
    "install",
    "instruct",
    "instrument",
    "integrate",
    "interpret",
    "interrogate",
    "intervene",
    "inventory",
    "invert",
    "invest",
    "invoke",
    "isolate",
    "iterate",
    "iteration",
    "justify",
    "label",
    "learn",
    "license",
    "link",
    "list",
    "locate",
    "loop",
    "manifest",
    "map",
    "mapping",
    "markdown",
    "mashup",
    "match",
    "maturity",
    "measure",
    "merge",
    "method",
    "metric",
    "migrate",
    "model",
    "monitor",
    "movement",
    "multi_step",
    "mutate",
    "name",
    "narrate",
    "normalize",
    "nussbaum",
    "object_capability",
    "observe",
    "obstacle",
    "ocr",
    "ontology",
    "organize",
    "orient",
    "osc_r",
    "oscr",
    "owasp",
    "package",
    "parse",
    "partition",
    "password_locked",
    "pattern",
    "perceive",
    "persona",
    "perspective",
    "phases",
    "pipeline",
    "plan",
    "populate",
    "position",
    "posture",
    "practice",
    "predict",
    "present",
    "preset",
    "principal",
    "principle",
    "principles",
    "prior",
    "prioritize",
    "probe",
    "produce",
    "profile",
    "prompt",
    "propose",
    "purpose",
    "quality",
    "query",
    "question",
    "rank",
    "reachability",
    "read",
    "readiness",
    "reason",
    "recall",
    "recognize",
    "recombine",
    "recommend",
    "reconcile",
    "record",
    "recover",
    "redact",
    "reduce",
    "reference",
    "refine",
    "reflect",
    "registry",
    "regression",
    "regulate",
    "reject",
    "relinquish",
    "remember",
    "remind",
    "render",
    "renounce",
    "repair",
    "report",
    "reproduce",
    "request",
    "require",
    "resolve",
    "respond",
    "restore",
    "restrict",
    "retrieve",
    "retry",
    "reverse",
    "review",
    "revise",
    "rewrite",
    "rotate",
    "route",
    "routine",
    "rubin",
    "rule_out",
    "runtime",
    "sample",
    "scaffold",
    "scale",
    "scan",
    "scenario",
    "score",
    "search",
    "select",
    "sequence",
    "serialize",
    "severity",
    "shape",
    "shared",
    "sibling",
    "signal",
    "signature",
    "simplify",
    "simulate",
    "slice",
    "span",
    "speak",
    "specify",
    "spoken",
    "standardize",
    "strategize",
    "structure",
    "suggest",
    "summarize",
    "supply",
    "surface",
    "survey",
    "switch",
    "synthesize",
    "tag",
    "target",
    "taxonomize",
    "taxonomy",
    "telemetry",
    "template",
    "tension",
    "test",
    "theme",
    "threat",
    "threshold",
    "topology",
    "trace",
    "track",
    "training",
    "transform",
    "transient",
    "transition",
    "translate",
    "trap",
    "traverse",
    "tts",
    "tune",
    "uncertainty",
    "undertake",
    "update",
    "validate",
    "value_chain",
    "variance",
    "vectorize",
    "verdict",
    "verify",
    "voice",
    "walk",
    "warrant",
    "weigh",
    "weight",
    "wire",
    "workflow",
    "write",
];

/// Is `term` a known vocabulary term?
///
/// expect: "The system validates template contracts against the lexicon"
/// pre:  term may be any string
/// post: returns true if term is in KNOWN_TERMS
pub fn is_known(term: &str) -> bool {
    KNOWN_TERMS.binary_search(&term).is_ok()
}

/// Returns unknown terms from `terms` that are not in the vocabulary.
///
/// expect: "The system validates template contracts against the lexicon"
/// pre:  terms is a slice of declared lexicon terms
/// post: returns Vec of terms not found in KNOWN_TERMS
pub fn unrecognized(terms: &[String]) -> Vec<String> {
    terms.iter().filter(|t| !is_known(t)).cloned().collect()
}

/// Validate an entry's `lexicon_terms` against the known vocabulary.
/// Returns warnings for any unrecognized or ill-formed terms.
///
/// Naming convention: terms must match `^[a-z][a-z0-9_]*$` (lowercase letters,
/// digits, underscores; must start with a letter). This catches casing
/// drift (`Multi-Step`), separator drift (`multi-step` vs `multi_step`),
/// and whitespace before they enter `KNOWN_TERMS`.
///
/// expect: "The system validates template contracts against the lexicon"
/// pre:  entry is a valid RegistryEntry
/// post: returns Vec of warning strings for unrecognized or ill-formed terms
#[allow(dead_code)]
pub(crate) fn validate_entry(entry: &RegistryEntry) -> Vec<String> {
    let mut warnings = Vec::new();
    let unknown = unrecognized(&entry.lexicon_terms);
    for term in &unknown {
        warnings.push(format!(
            "entry '{}' declares unknown lexicon term '{}'",
            entry.id, term
        ));
    }
    for term in &entry.lexicon_terms {
        if !is_well_formed(term) {
            warnings.push(format!(
                "entry '{}' declares ill-formed lexicon term '{}' (must match ^[a-z][a-z0-9_]*$)",
                entry.id, term
            ));
        }
    }
    warnings
}

/// Check that a term matches the lexicon naming convention.
///
/// Convention: lowercase letters, digits, and underscores; must start with
/// a letter. Rejects mixed case, hyphens, spaces, and leading digits/underscores.
///
/// expect: "The system validates template contracts against the lexicon"
/// pre:  term may be any string
/// post: returns true if term matches ^[a-z][a-z0-9_]*$
pub fn is_well_formed(term: &str) -> bool {
    let mut chars = term.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() => {
            chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_terms_are_sorted() {
        for w in KNOWN_TERMS.windows(2) {
            assert!(
                w[0] < w[1],
                "KNOWN_TERMS not sorted: '{}' >= '{}'",
                w[0],
                w[1]
            );
        }
    }

    #[test]
    fn known_terms_no_duplicates() {
        for w in KNOWN_TERMS.windows(2) {
            assert_ne!(w[0], w[1], "Duplicate term: '{}'", w[0]);
        }
    }

    #[test]
    fn validate_known_terms_passes() {
        let terms: Vec<String> = vec!["compose", "verify", "classify"]
            .into_iter()
            .map(String::from)
            .collect();
        let unknown = unrecognized(&terms);
        assert!(
            unknown.is_empty(),
            "Known terms should not be flagged: {:?}",
            unknown
        );
    }

    #[test]
    fn validate_unknown_terms_flags() {
        let terms: Vec<String> = vec![
            "compose".into(),
            "nonsense_term_xyz".into(),
            "verify".into(),
        ];
        let unknown = unrecognized(&terms);
        assert_eq!(unknown, vec!["nonsense_term_xyz".to_string()]);
    }

    #[test]
    fn all_bootstrapped_terms_are_known() {
        for term in KNOWN_TERMS {
            assert!(
                is_known(term),
                "Bootstrapped term '{}' not recognized",
                term
            );
        }
    }

    #[test]
    fn empty_terms_no_warnings() {
        let unknown = unrecognized(&[]);
        assert!(unknown.is_empty());
    }

    #[test]
    fn is_well_formed_accepts_lowercase_and_underscores() {
        assert!(is_well_formed("compose"));
        assert!(is_well_formed("rule_out"));
        assert!(is_well_formed("value_chain"));
        assert!(is_well_formed("dead_code"));
        assert!(is_well_formed("stage_3"));
        assert!(is_well_formed("a"));
    }

    #[test]
    fn is_well_formed_rejects_invalid_patterns() {
        // Hyphens rejected (use underscores)
        assert!(!is_well_formed("multi-step"));
        // Mixed case rejected
        assert!(!is_well_formed("MultiStep"));
        assert!(!is_well_formed("composeX"));
        // Leading underscore rejected
        assert!(!is_well_formed("_private"));
        // Leading digit rejected
        assert!(!is_well_formed("3stage"));
        // Empty rejected
        assert!(!is_well_formed(""));
        // Whitespace rejected
        assert!(!is_well_formed("multi step"));
        assert!(!is_well_formed("trailing "));
    }

    #[test]
    fn validate_entry_flags_ill_formed_terms() {
        use hkask_types::TemplateType;
        use hkask_types::registry::RegistryEntry;
        let entry = RegistryEntry {
            id: "test/ill-formed".into(),
            template_type: TemplateType::KnowAct,
            name: "Test".into(),
            lexicon_terms: vec!["compose".into(), "Multi-Step".into(), "multi-step".into()],
            description: String::new(),
            source_path: "test.j2".into(),
            required_capabilities: Vec::new(),
            cascade_level: 0,
            matroshka_limit: 0,
        };
        let warnings = validate_entry(&entry);
        // "compose" is known and well-formed — no warning.
        // "Multi-Step" is unknown AND ill-formed — two warnings (unknown + ill-formed).
        // "multi-step" is unknown AND ill-formed — two warnings (unknown + ill-formed).
        assert_eq!(warnings.len(), 4, "expected 4 warnings, got: {warnings:?}");
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("ill-formed") && w.contains("Multi-Step"))
        );
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("ill-formed") && w.contains("multi-step"))
        );
    }
}
