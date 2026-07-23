//! Bundle composition types — conflicts and complementarities between skills

use serde::{Deserialize, Serialize};

use hkask_types::TemplateType;

/// What kind of conflict exists between two skills
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ConflictType {
    CancelOut,
    ContradictoryDirective,
    OrderingCollision,
    ResourceContention,
}

// as_str pre:  self is a valid ConflictType variant
// as_str post: returns PascalCase string ("CancelOut", "ContradictoryDirective", "OrderingCollision", "ResourceContention")
// parse_str pre:  s is PascalCase or snake_case
// parse_str post: returns Some(ConflictType) if s matches; None otherwise
hkask_types::enum_str_ops!(ConflictType, {
    CancelOut => ("CancelOut", "cancel_out"),
    ContradictoryDirective => ("ContradictoryDirective", "contradictory_directive"),
    OrderingCollision => ("OrderingCollision", "ordering_collision"),
    ResourceContention => ("ResourceContention", "resource_contention"),
});

/// How to resolve a declared conflict
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ConflictResolution {
    DomainSeparation,
    PhaseSeparation,
    SpecificityOverride,
    ManifestOverride,
    UserIntent,
}

// as_str pre:  self is a valid ConflictResolution variant
// as_str post: returns PascalCase string ("DomainSeparation", "PhaseSeparation", "SpecificityOverride", "ManifestOverride", "UserIntent")
// parse_str pre:  s is PascalCase or snake_case
// parse_str post: returns Some(ConflictResolution) if s matches; None otherwise
hkask_types::enum_str_ops!(ConflictResolution, {
    DomainSeparation => ("DomainSeparation", "domain_separation"),
    PhaseSeparation => ("PhaseSeparation", "phase_separation"),
    SpecificityOverride => ("SpecificityOverride", "specificity_override"),
    ManifestOverride => ("ManifestOverride", "manifest_override"),
    UserIntent => ("UserIntent", "user_intent"),
});

/// How two skills enhance each other
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ComplementarityType {
    SequentialFeed,
    ParallelAmplify,
    CrossDomainEnhance,
}

// as_str pre:  self is a valid ComplementarityType variant
// as_str post: returns PascalCase string ("SequentialFeed", "ParallelAmplify", "CrossDomainEnhance")
// parse_str pre:  s is PascalCase or snake_case
// parse_str post: returns Some(ComplementarityType) if s matches; None otherwise
hkask_types::enum_str_ops!(ComplementarityType, {
    SequentialFeed => ("SequentialFeed", "sequential_feed"),
    ParallelAmplify => ("ParallelAmplify", "parallel_amplify"),
    CrossDomainEnhance => ("CrossDomainEnhance", "cross_domain_enhance"),
});

/// A declared conflict between exactly two skills in a bundle
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleConflict {
    pub skills: Vec<String>,
    pub domain: TemplateType,
    pub conflict_type: ConflictType,
    pub resolution: ConflictResolution,
    pub resolution_detail: String,
}

impl BundleConflict {
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self.conflict_type is a valid ConflictType variant
    /// post: returns the PascalCase string representation of the conflict type
    pub fn conflict_type_str(&self) -> &'static str {
        self.conflict_type.as_str()
    }
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self.resolution is a valid ConflictResolution variant
    /// post: returns the PascalCase string representation of the resolution strategy
    pub fn resolution_str(&self) -> &'static str {
        self.resolution.as_str()
    }
}

/// A declared complementarity between exactly two skills in a bundle
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleComplementarity {
    pub skills: Vec<String>,
    pub complementarity_type: ComplementarityType,
    pub detail: String,
}

impl BundleComplementarity {
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self.complementarity_type is a valid ComplementarityType variant
    /// post: returns the PascalCase string representation of the complementarity type
    pub fn complementarity_type_str(&self) -> &'static str {
        self.complementarity_type.as_str()
    }
}
