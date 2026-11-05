//! The private local leg — llama.cpp as the engine, this crate as the
//! control plane.
//!
//! Four layers split by drift rate (plan §4): the manifest (L1, where most
//! llama.cpp churn lands as data), the constraint forms (L2, the in-tree
//! lisp escape hatch plus the CI fixture gates), the placement compiler
//! (L3, the stable Rust decision core), and the launch plan (the seam the
//! future supervisor L4 consumes). The privacy marking itself is
//! engine-independent and lives in the committed M4/M5/M6 surfaces.

pub mod fleet;
pub mod gguf;
pub mod launch;
pub mod manifest;
pub mod model_brief;
pub mod placement;
