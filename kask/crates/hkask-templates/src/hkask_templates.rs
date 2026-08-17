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
pub mod concurrency;
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
pub use bundle::GoldenOutputFixture;
pub use executor::ManifestExecutor;
pub use executor::extract_final_step_result;
pub use step_graph::ExitKind;
pub use step_machine::CascadeOutcome;

pub use inputs::{
    InputValidationError, extract_contract_input_keys, render_input_param_spec, validate_inputs,
};
pub use manifest_loader::{
    ManifestLoadError, McpReferenceWarning, load_manifest_from_file, load_manifest_from_yaml,
    resolve_manifest, validate_mcp_references,
};
pub use ports::{FsSkillReader, ManifestResolveError, Result, SkillFinding, TemplateError};
pub use prompt_strategy::PromptStrategy;

pub use registry::{
    KNOWN_MCP_TOOLS, Registry, company_source_seed, process_manifest_seed, process_manifest_yaml,
    template_file, template_file_seed, template_manifest_seed, template_yaml_file,
    template_yaml_file_seed,
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

    /// Re-export the step-condition evaluator and choice-condition parser so
    /// external proptest tests can drive the pure string-parsing and
    /// boolean-evaluation surfaces directly. Both are total over arbitrary
    /// condition strings and arbitrary context — the property tests pin that.
    pub use crate::condition::{evaluate_step_condition, parse_choice_condition};

    /// Re-export the dot-path resolver so external proptest tests can drive the
    /// pure context-lookup surface directly. Total over arbitrary paths and
    /// context — never panics, returns `Option<Value>`.
    pub use crate::input_mapping::resolve_dot_path;

    /// Re-export the `[inference]` block parser and its parsed struct so
    /// external tests can audit token-budget adequacy across the template
    /// registry — verifying that every template with a complex output schema
    /// declares a `max_tokens` sufficient for the expected JSON output.
    pub use crate::template_renderer::{
        InferenceBlock, parse_and_strip_inference_block, strip_front_matter,
    };
}
