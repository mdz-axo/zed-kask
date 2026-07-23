//! Manifest YAML loader — parse process manifest files into BundleManifest
//!
//! The YAML files in `registry/manifests/` use a top-level structure where
//! the `manifest:` key contains identity fields (id, name, description, etc.)
//! while `steps:`, `gas:`, `error_handling:`, etc. are top-level peers.
//! This module provides a deserialization wrapper that flattens this structure
//! into the canonical `BundleManifest` type.

use crate::bundle::{
    BundleAuditConfig, BundleComplementarity, BundleConflict, BundleGasConfig, BundleLedgerConfig,
    BundleManifest, BundleManifestStep, BundleSkill, ConvergenceConfig, ErrorHandlingConfig,
    OcapConfig, RjouleConfig,
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
/// gas:
///   ...
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
    gas: Option<BundleGasConfig>,
    #[serde(default)]
    rjoule: Option<RjouleConfig>,
    #[serde(default)]
    error_handling: Option<ErrorHandlingConfig>,
    #[serde(default)]
    ocap: Option<OcapConfig>,
    #[serde(default)]
    ledger: Option<BundleLedgerConfig>,
    #[serde(default)]
    audit: Option<BundleAuditConfig>,
    #[serde(default)]
    inputs: Option<serde_json::Value>,
    #[serde(default)]
    principles: Option<serde_json::Value>,
    #[serde(default)]
    fusion: Option<hkask_types::fusion::FusionConfig>,
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
pub(crate) fn load_manifest_from_file(
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

    let manifest = BundleManifest {
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
        gas: file.gas.unwrap_or_default(),
        rjoule: file.rjoule.unwrap_or_default(),
        error_handling: file.error_handling.unwrap_or_default(),
        ocap: file.ocap.unwrap_or_default(),
        ledger: file.ledger.unwrap_or_default(),
        audit: file.audit.unwrap_or_default(),
        functional_role: file.manifest.functional_role,
        category: file.manifest.category,
        inputs: file.inputs,
        principles: file.principles,
        fusion: file.fusion,
    };

    info!(
        target: "hkask.manifest_loader",
        id = %manifest.id,
        steps = manifest.steps.len(),
        "Loaded manifest from YAML"
    );

    Ok(manifest)
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
/// \[P8\] Constraining: Semantic Grounding — manifest terms validated against lexicon
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

    // Try as a relative path from CWD
    let cwd_path = std::path::PathBuf::from(reference);
    if cwd_path.exists() {
        match load_manifest_from_file(&cwd_path) {
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
                    "Loaded manifest from relative path"
                );
                return Ok(manifest);
            }
            Err(e) => {
                tracing::warn!(
                    target: "hkask.manifest_loader",
                    path = reference,
                    error = %e,
                    "Failed to load manifest from relative path"
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
