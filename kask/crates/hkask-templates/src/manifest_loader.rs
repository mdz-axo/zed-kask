//! Manifest YAML loader — parse process manifest files into BundleManifest
//!
//! The YAML files in `registry/manifests/` use a top-level structure where
//! the `manifest:` key contains identity fields (id, name, description, etc.)
//! while `steps:`, `error_handling:`, etc. are top-level peers.
//! This module provides a deserialization wrapper that flattens this structure
//! into the canonical `BundleManifest` type.

use crate::bundle::manifest::default_concurrency;
use crate::bundle::{
    BundleAuditConfig, BundleComplementarity, BundleConflict, BundleLedgerConfig, BundleManifest,
    BundleManifestStep, BundleSkill, ConvergenceConfig, ErrorHandlingConfig, RjouleConfig,
};
use hkask_types::Visibility;
use serde::Deserialize;
use tracing::info;

use crate::ports::ManifestResolveError;

/// Wrapper struct for deserializing YAML manifest files.
///
/// YAML manifest files have this structure:
/// ```yaml
/// manifest:
///   id: ...
///   name: ...
///   ...
/// steps:
///   - ordinal: 1
///     ...
/// error_handling:
///   ...
/// ```rust,no_run
///
/// This wrapper flattens the `manifest:` inner fields with the top-level
/// config fields into a single `BundleManifest`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestFile {
    manifest: ManifestHeader,
    #[serde(default)]
    steps: Vec<BundleManifestStep>,
    #[serde(default)]
    skills: Vec<BundleSkill>,
    #[serde(default)]
    conflicts: Vec<BundleConflict>,
    #[serde(default)]
    complementarities: Vec<BundleComplementarity>,
    #[serde(default)]
    convergence: Option<ConvergenceConfig>,
    #[serde(default)]
    rjoule: Option<RjouleConfig>,
    #[serde(default)]
    error_handling: Option<ErrorHandlingConfig>,
    #[serde(default)]
    ledger: Option<BundleLedgerConfig>,
    #[serde(default)]
    audit: Option<BundleAuditConfig>,
    #[serde(default)]
    inputs: Option<serde_json::Value>,
    #[serde(default)]
    principles: Option<serde_json::Value>,
}

/// Inner header from the `manifest:` key in YAML files.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestHeader {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    editor: String,
    #[serde(default, deserialize_with = "deserialize_visibility_case_insensitive")]
    visibility: Option<Visibility>,
    #[serde(default)]
    functional_role: Option<String>,
    /// Manifest category — distinguishes agent skills from infrastructure
    /// that shares the FlowDef `.yaml` form. Values: `skill` (agent PDCA
    /// loop), `qa-script`, `runtime-config`, `daemon-process`, `pipeline`.
    /// Defaults to `skill` for back-compat with pre-category manifests.
    #[serde(default)]
    category: Option<String>,
    /// Opt-in to runtime validation of caller-supplied context against the
    /// manifest's declared `inputs` (see `crate::inputs::validate_inputs`).
    /// Defaults to `None` (no validation) for back-compat.
    #[serde(default)]
    enforce_inputs: Option<bool>,
    #[serde(default = "default_concurrency")]
    concurrency: u32,
}

/// Deserialize visibility in a case-insensitive manner.
///
/// YAML manifest files may use PascalCase (`Shared`) while the
/// `Visibility` enum serializes as lowercase (`shared`).
fn deserialize_visibility_case_insensitive<'de, D>(
    deserializer: D,
) -> Result<Option<Visibility>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;

    let opt: Option<String> = Option::deserialize(deserializer)?;
    match opt {
        Some(s) => Visibility::parse_str(&s)
            .map(Some)
            .ok_or_else(|| de::Error::custom(format!("unknown visibility variant: {s}"))),
        None => Ok(None),
    }
}

/// Load a BundleManifest from a YAML file at the given path.
///
/// Reads the file, parses it using the `ManifestFile` wrapper, and
/// flattens the structure into a canonical `BundleManifest`.
pub fn load_manifest_from_file(
    path: &std::path::Path,
) -> Result<BundleManifest, ManifestLoadError> {
    let content = std::fs::read_to_string(path).map_err(|e| ManifestLoadError::Io {
        path: path.display().to_string(),
        source: e,
    })?;

    load_manifest_from_yaml(&content)
}

/// Load a BundleManifest from a YAML string.
///
/// Parses the YAML using the `ManifestFile` wrapper and flattens
/// it into a canonical `BundleManifest`.
pub fn load_manifest_from_yaml(yaml: &str) -> Result<BundleManifest, ManifestLoadError> {
    let file: ManifestFile =
        serde_yaml_neo::from_str(yaml).map_err(|e| ManifestLoadError::Yaml { source: e })?;

    let mut manifest = BundleManifest {
        id: file.manifest.id,
        name: file.manifest.name,
        description: file.manifest.description,
        version: file.manifest.version,
        editor: file.manifest.editor,
        visibility: file.manifest.visibility.unwrap_or(Visibility::Public),
        skills: file.skills,
        conflicts: file.conflicts,
        complementarities: file.complementarities,
        steps: file.steps,
        convergence: file.convergence.unwrap_or_default(),
        rjoule: file.rjoule.unwrap_or_default(),
        error_handling: file.error_handling.unwrap_or_default(),
        ledger: file.ledger.unwrap_or_default(),
        audit: file.audit.unwrap_or_default(),
        functional_role: file.manifest.functional_role,
        category: file.manifest.category,
        inputs: file.inputs,
        enforce_inputs: file.manifest.enforce_inputs,
        principles: file.principles,
        concurrency: file.manifest.concurrency,
        golden_outputs: None,
    };

    // Sort steps by ordinal once at load time. The executor's `run_cascade`
    // previously cloned + sorted on every cascade entry (including recursive
    // flowdef sub-cascades); moving the sort here makes it a one-time cost.
    // Manifests are authored in ordinal order, so this is almost always a
    // no-op sort — but it guarantees the invariant for safety.
    manifest.steps.sort_by_key(|s| s.ordinal);

    info!(
        target: "hkask.manifest_loader",
        id = %manifest.id,
        steps = manifest.steps.len(),
        "Loaded manifest from YAML"
    );

    Ok(manifest)
}

/// Validate that every `mcp:` reference in a manifest's `execute` steps
/// exists in the provided set of known tool names. Returns a list of
/// warnings for tools that are not found — these are not errors (the tool
/// may be registered later at runtime), but they indicate a manifest-vs-
/// registry drift that will cause a runtime failure if the tool is not
/// registered by the time the step executes.
///
/// Callers should pass the set of tool names registered across all MCP
/// servers (e.g. from `ToolPort::get_tool_info`). At test time, this can
/// be a static list compiled from the known MCP server tool registrations.
pub fn validate_mcp_references(
    manifest: &BundleManifest,
    known_tools: &std::collections::HashSet<&str>,
) -> Vec<McpReferenceWarning> {
    let mut warnings = Vec::new();
    for step in &manifest.steps {
        if step.action != "execute" {
            continue;
        }
        if let Some(ref mcp_ref) = step.mcp {
            // Strip ${variable} references — these are resolved at runtime
            // and cannot be validated at load time.
            if mcp_ref.contains("${") {
                continue;
            }
            if !known_tools.contains(mcp_ref.as_str()) {
                warnings.push(McpReferenceWarning {
                    manifest_id: manifest.id.clone(),
                    step_ordinal: step.ordinal,
                    mcp_ref: mcp_ref.clone(),
                });
            }
        }
        // `mcp_batch` entries carry the same class of reference as `mcp` —
        // validate them too. Without this, a batch entry with a typo'd tool
        // name passes load-time validation and fails only at runtime (or
        // worse, silently lands in the `errors` sidecar under allSettled).
        if let Some(ref batch) = step.mcp_batch {
            for entry in batch {
                if entry.mcp.contains("${") {
                    continue;
                }
                if !known_tools.contains(entry.mcp.as_str()) {
                    warnings.push(McpReferenceWarning {
                        manifest_id: manifest.id.clone(),
                        step_ordinal: step.ordinal,
                        mcp_ref: entry.mcp.clone(),
                    });
                }
            }
        }
    }
    if !warnings.is_empty() {
        tracing::warn!(
            target: "hkask.manifest_loader.mcp_validation",
            manifest_id = %manifest.id,
            warning_count = warnings.len(),
            "Manifest references MCP tools not in the known set — these will fail at runtime if not registered"
        );
    }
    warnings
}

/// A warning about an MCP tool reference that is not in the known tool set.
#[derive(Debug, Clone)]
pub struct McpReferenceWarning {
    pub manifest_id: String,
    pub step_ordinal: u32,
    pub mcp_ref: String,
}

impl std::fmt::Display for McpReferenceWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "manifest '{}' step {} references mcp tool '{}' not in the known set",
            self.manifest_id, self.step_ordinal, self.mcp_ref
        )
    }
}

/// Resolve a process_manifest reference to a BundleManifest.
///
/// The `process_manifest` field on an agent definition can be:
/// - A file path (contains '/' or '.'): loaded from disk
/// - A manifest ID: looked up from the registry
///
/// # Errors
///
/// Returns `ManifestResolveError::NotFound` if the reference matches no
/// registry entry and no file path. Returns `ManifestResolveError::LoadFailed`
/// if a file path matches but the manifest fails to load. Returns
/// `ManifestResolveError::NotASkill` if the manifest loads but is not a
/// `skill` category.
///
/// expect: "The system resolves and executes template manifest cascades"
/// \[P3\] Motivating: Generative Space — resolves template manifest references
/// pre:  reference is non-empty, registry is initialized
/// post: returns Ok(BundleManifest) if found via registry or file path
/// post: returns Err(ManifestResolveError) with typed failure mode
pub fn resolve_manifest(
    reference: &str,
    registry: &dyn crate::BundleRegistryIndex,
) -> std::result::Result<BundleManifest, ManifestResolveError> {
    // Try as a registry ID first
    if let Some(bundle) = registry.get_bundle(reference) {
        if bundle.is_skill() {
            return Ok(bundle);
        }
        tracing::warn!(
            target: "hkask.manifest_loader",
            reference = reference,
            id = %bundle.id,
            category = ?bundle.category,
            "resolve_manifest: '{reference}' is not a skill (category={:?}); \
             only `skill` manifests may bind as agent process_manifests",
            bundle.category
        );
        return Err(ManifestResolveError::NotASkill {
            reference: reference.to_owned(),
            category: format!("{:?}", bundle.category),
        });
    }

    // Try as a file path
    let path = std::path::Path::new(reference);
    if path.exists() {
        match load_manifest_from_file(path) {
            Ok(manifest) => {
                if !manifest.is_skill() {
                    tracing::warn!(
                        target: "hkask.manifest_loader",
                        path = reference,
                        id = %manifest.id,
                        category = ?manifest.category,
                        "resolve_manifest: '{reference}' is not a skill (category={:?}); \
                         only `skill` manifests may bind as agent process_manifests",
                        manifest.category
                    );
                    return Err(ManifestResolveError::NotASkill {
                        reference: reference.to_owned(),
                        category: format!("{:?}", manifest.category),
                    });
                }
                info!(
                    target: "hkask.manifest_loader",
                    id = %manifest.id,
                    path = reference,
                    "Loaded manifest from file"
                );
                return Ok(manifest);
            }
            Err(e) => {
                tracing::warn!(
                    target: "hkask.manifest_loader",
                    path = reference,
                    error = %e,
                    "Failed to load manifest from file"
                );
                return Err(ManifestResolveError::LoadFailed {
                    reference: reference.to_owned(),
                    source: e,
                });
            }
        }
    }

    tracing::warn!(
        target: "hkask.manifest_loader",
        reference = reference,
        "Manifest not found in registry or filesystem"
    );
    Err(ManifestResolveError::NotFound {
        reference: reference.to_owned(),
    })
}

/// Errors that can occur when loading a manifest from YAML.
#[derive(Debug, thiserror::Error)]
pub enum ManifestLoadError {
    #[error("IO error reading {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("YAML parse error: {source}")]
    Yaml { source: serde_yaml_neo::Error },
}
