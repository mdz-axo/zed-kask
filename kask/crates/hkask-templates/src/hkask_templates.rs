#![forbid(unsafe_code)]
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
pub mod crate_loader;
pub mod executor;
pub mod input_mapping;
pub mod inputs;
pub mod manifest_loader;
pub mod ports;
pub mod prompt_strategy;
pub mod registry;
pub mod registry_sqlite;
pub mod skill_loader;
pub mod taint_context;
pub mod template_renderer;

pub use bundle::BundleManifest;
pub use bundle::BundleRegistryIndex;
pub use crate_loader::TemplateCrateLoader;
pub use executor::ManifestExecutor;
pub use hkask_types::InferencePort;
pub use hkask_types::Skill;
pub use hkask_types::SkillPolarity;
pub use hkask_types::SkillZone;

pub use inputs::{render_input_param_spec, validate_inputs};
pub use manifest_loader::{
    ManifestLoadError, load_manifest_from_file, load_manifest_from_yaml, resolve_manifest,
};
pub use ports::{FsSkillReader, ManifestResolveError, Result, SkillFinding, TemplateError};
pub use prompt_strategy::PromptStrategy;

pub use registry::{Registry, process_manifest_yaml, template_file, template_yaml_file};
pub use registry_sqlite::SqliteRegistry;
pub use skill_loader::{SkillFrontMatter, SkillLoadResult, SkillLoader};
