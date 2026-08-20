//! Seed accessors and compiled-in template lookups. The in-memory `Registry`
//! struct was removed — production resolves manifests from disk via
//! `BridgeManifestExecutor::load_cached_manifest`.

// Auto-generated per-skill template manifests (from build.rs).
include!(concat!(env!("OUT_DIR"), "/manifest_skills.rs"));

// Auto-generated MCP tool names from #[tool] annotations (from build.rs).
include!(concat!(env!("OUT_DIR"), "/known_mcp_tools.rs"));

/// Look up the compiled-in process manifest (FlowDef cascade) for a skill.
///
/// Process manifests are authored at `registry/manifests/<skill>.yaml` and
/// compiled in via `include_str!` as a **seed payload**. At startup
/// [`process_manifest_seed`] materialises them to disk; the runtime reads
/// exclusively from disk (via `BridgeManifestExecutor::manifest_yaml`). This
/// accessor remains available for tests and the seeding path.
/// The full compiled-in process-manifest seed payload as `(skill_name, yaml)`
/// pairs. Seed-only: used by the registry seeding path to write the shipped
/// manifests to disk. Not read at runtime — the runtime resolves manifests
/// from disk.
pub fn process_manifest_seed() -> &'static [(&'static str, &'static str)] {
    PROCESS_MANIFEST_YAMLS
}

/// The full compiled-in Jinja2 template seed payload as `(rel_path, content)`
/// pairs, where `rel_path` is `<skill>/<file>.j2`. Seed-only.
pub fn template_file_seed() -> &'static [(&'static str, &'static str)] {
    TEMPLATE_FILES
}

/// The full compiled-in YAML template seed payload as `(rel_path, content)`
/// pairs, where `rel_path` is `<skill>/<file>.yaml` (excluding `manifest.yaml`).
/// Seed-only.
pub fn template_yaml_file_seed() -> &'static [(&'static str, &'static str)] {
    TEMPLATE_YAML_FILES
}

/// The full compiled-in per-skill template-manifest seed payload as
/// `(skill_name, manifest_yaml)` pairs (`registry/templates/<skill>/manifest.yaml`).
/// Seed-only.
pub fn template_manifest_seed() -> &'static [(&'static str, &'static str)] {
    MANIFEST_YAMLS
}

/// The full compiled-in company-source manifest seed payload as
/// `(symbol, yaml)` pairs (`registry/company-sources/<symbol>.yaml`).
/// Seed-only: used by the registry seeding path to write the shipped
/// company-source manifests to disk under `company-sources/` in the data
/// directory. The corpus MCP server's `corpus_discover_company` tool reads
/// them from disk at runtime.
pub fn company_source_seed() -> &'static [(&'static str, &'static str)] {
    COMPANY_SOURCE_YAMLS
}

/// Look up an embedded Jinja2 template file by its `template_ref`.
///
/// Template refs in manifests omit the `.j2` extension (e.g.
/// `grill-me/grill-me-assess`), but the embedded files are keyed with
/// the extension (e.g. `grill-me/grill-me-assess.j2`). This function
/// handles both forms: it first tries the ref as-is, then appends `.j2`
/// if the ref doesn't already end with it.
///
/// Returns the raw template content, or `None` if no embedded template
/// matches. Callers that need to fall back to the filesystem (dev
/// workflows where a template has been edited but not yet rebuilt)
/// should do so after this returns `None`.
pub fn template_file(template_ref: &str) -> Option<&'static str> {
    // Try the ref as-is first (handles refs that already include .j2).
    if let Some((_, content)) = TEMPLATE_FILES.iter().find(|(key, _)| *key == template_ref) {
        return Some(*content);
    }
    // If the ref doesn't end with .j2, try appending it.
    if !template_ref.ends_with(".j2") {
        let with_ext = format!("{template_ref}.j2");
        if let Some((_, content)) = TEMPLATE_FILES.iter().find(|(key, _)| *key == with_ext) {
            return Some(*content);
        }
    }
    None
}

/// Look up an embedded YAML template file by its `template_ref`.
///
/// YAML template files are FlowDef sub-manifests (composable `.yaml` pipelines)
/// and RenderAct `.yaml` reference docs. Like `.j2` templates, template refs
/// often omit the extension (e.g. `media/logo-discovery`), but the embedded
/// files are keyed with it (e.g. `media/logo-discovery.yaml`). This function
/// handles both forms.
///
/// Returns the raw YAML content, or `None` if no embedded YAML template
/// matches. Callers that need to fall back to the filesystem should do so
/// after this returns `None`.
pub fn template_yaml_file(template_ref: &str) -> Option<&'static str> {
    // Try the ref as-is first (handles refs that already include .yaml).
    if let Some((_, content)) = TEMPLATE_YAML_FILES
        .iter()
        .find(|(key, _)| *key == template_ref)
    {
        return Some(*content);
    }
    // If the ref doesn't end with .yaml, try appending it.
    if !template_ref.ends_with(".yaml") {
        let with_ext = format!("{template_ref}.yaml");
        if let Some((_, content)) = TEMPLATE_YAML_FILES.iter().find(|(key, _)| *key == with_ext) {
            return Some(*content);
        }
    }
    None
}

