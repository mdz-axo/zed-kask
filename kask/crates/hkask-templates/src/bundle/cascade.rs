//! Cascade phase types — where a step sits in the Pre/Core/Post pipeline

use serde::{Deserialize, Serialize};

/// Cascade phase — where a step sits in the Pre/Core/Post pipeline
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum CascadePhase {
    Pre,
    #[default]
    Core,
    Post,
}

// as_str pre:  self is a valid CascadePhase variant
// as_str post: returns PascalCase string ("Pre", "Core", "Post")
// parse_str pre:  s is PascalCase or lowercase ("Pre"/"pre", "Core"/"core", "Post"/"post")
// parse_str post: returns Some(CascadePhase) if s matches; None otherwise
hkask_types::enum_str_ops!(CascadePhase, {
    Pre => ("Pre", "pre"),
    Core => ("Core", "core"),
    Post => ("Post", "post"),
});
