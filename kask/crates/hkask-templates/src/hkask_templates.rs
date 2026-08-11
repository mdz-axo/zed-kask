#![forbid(unsafe_code)]
#![warn(clippy::let_underscore_future)]
//! hKask Templates — registry and template execution
//!
//! Unified registry with template_type discriminator per architecture v0.22.0.
//! Rust is the loom. YAML/Jinja2 is the thread.
//!
//! Inference (L1) has been extracted to `hkask-inference`.
//! Template types: Prompt (WordAct), Process (FlowDef), Cognition (KnowAct).
//! Registry adapters: `Registry` (in-memory), `SqliteRegistry` (SQLite).

pub mod budget;
pub mod bundle;
pub mod compute;
pub mod condition;
pub mod convergence;
pub mod executor;
mod input_mapping;
pub mod inputs;
pub mod manifest_loader;
mod output_schema;
pub mod ports;
pub mod prompt_strategy;
pub mod registry;
pub mod registry_sqlite;
pub mod skill_loader;
pub mod step_actions;
pub mod step_context;
pub mod step_graph;
pub mod step_machine;
pub mod template_renderer;

pub use bundle::BundleManifest;
pub use bundle::BundleRegistryIndex;
pub use executor::CascadeEvent;
pub use executor::ManifestExecutor;
pub use executor::extract_final_step_result;

pub use inputs::{InputValidationError, render_input_param_spec, validate_inputs};
pub use manifest_loader::{
    ManifestLoadError, load_manifest_from_file, load_manifest_from_yaml, resolve_manifest,
};
pub use ports::{FsSkillReader, ManifestResolveError, Result, SkillFinding, TemplateError};
pub use prompt_strategy::PromptStrategy;

pub use registry::{
    Registry, company_source_seed, process_manifest_seed, process_manifest_yaml, template_file,
    template_file_seed, template_manifest_seed, template_yaml_file, template_yaml_file_seed,
};
pub use registry_sqlite::SqliteRegistry;
pub use skill_loader::{SkillFrontMatter, SkillLoadResult, SkillLoader};

#[cfg(any(test, feature = "test-utils"))]
pub mod test_utils {
    /// Re-export the compute dispatch table so external proptest tests under
    /// the `test-utils` feature can drive the deterministic compute primitives
    /// (swarm accumulators, second-order monitor, kata convergence, forecast
    /// primitives) directly without an `InferencePort`.
    pub use crate::compute::dispatch_compute;
}
