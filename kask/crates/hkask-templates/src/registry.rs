//! Template registry index
//!
//! Unified registry with template_type discriminator per architecture v0.22.0.
//! Template types align with hKask domains:
//! - **WordAct** — Jinja2 prompt templates ("what to say")
//! - **KnowAct** — Jinja2 cognition templates ("how to think")
//! - **FlowDef** — YAML pipeline manifests ("what to do", including specifications)
//!
//! Rust is the loom. YAML/Jinja2 is the thread.

use crate::bundle::BundleManifest;
use crate::bundle::BundleRegistryIndex;
use crate::ports::{Result, TemplateError};
use hkask_capability::SYSTEM_MAX_RECURSION;
use hkask_types::NotFound;
use hkask_types::Visibility;
use hkask_types::template_type::TemplateType;
use hkask_types::{RegistryEntry, RegistryIndex, Skill, SkillRegistryIndex};
use serde::Deserialize;
use std::collections::HashMap;

// Auto-generated per-skill template manifests (from build.rs).
include!(concat!(env!("OUT_DIR"), "/manifest_skills.rs"));

// Auto-generated MCP tool names from #[tool] annotations (from build.rs).
// Scans kask/mcp-servers/hkask-mcp-*/src/**/*.rs so the list refreshes
// automatically when a server adds a tool — no manual maintenance.
include!(concat!(env!("OUT_DIR"), "/known_mcp_tools.rs"));

/// Look up the compiled-in process manifest (FlowDef cascade) for a skill.
///
/// Process manifests are authored at `registry/manifests/<skill>.yaml` and
/// compiled in via `include_str!` as a **seed payload**. At startup
/// [`process_manifest_seed`] materialises them to disk; the runtime reads
/// exclusively from disk (via `BridgeManifestExecutor::manifest_yaml`). This
/// accessor remains available for tests and the seeding path.
///
/// Returns the raw YAML content for the skill, or `None` if no manifest
/// exists for that name.
pub fn process_manifest_yaml(skill_name: &str) -> Option<&'static str> {
    PROCESS_MANIFEST_YAMLS
        .iter()
        .find(|(name, _)| *name == skill_name)
        .map(|(_, yaml)| *yaml)
}

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

/// Per-skill template manifest deserialization shape.
///
/// Per-skill manifests (`registry/templates/<skill>/manifest.yaml`) use:
/// ```yaml
/// crate:
///   name: ...
///   version: ...
/// templates:
///   - id: <skill>/<template>
///     path: <file>.j2
///     type: WordAct|KnowAct|FlowDef|RenderAct
///     description: ...
/// ```
/// The `crate` section is ignored — only `templates` are extracted into
/// `RegistryEntry` objects.
#[derive(Deserialize)]
struct SkillTemplateManifest {
    #[serde(default)]
    templates: Vec<SkillTemplateEntry>,
}

#[derive(Deserialize)]
struct SkillTemplateEntry {
    id: String,
    #[serde(default)]
    name: String,
    path: String,
    #[serde(rename = "type")]
    template_type: TemplateType,
    #[serde(default)]
    description: String,
}

/// Unified template + skill registry
///
/// Thin in-memory wrapper (read-through cache) around `SqliteRegistry`.
/// Not a separate API surface — both `Registry` and `SqliteRegistry` implement
/// the same three index traits (`RegistryIndex`, `SkillRegistryIndex`,
/// `BundleRegistryIndex`). `Registry` loads from the filesystem on startup
/// and caches entries in HashMaps; `SqliteRegistry` provides the persistent
/// backing store. The two are always used in tandem: `Registry` for fast
/// lookups, `SqliteRegistry` for durability.
///
/// Templates are stored as `RegistryEntry` (the canonical type from `hkask_types::ports`).
/// Skills compose templates into coherent agent capabilities.
/// Bundles compose multiple skills into orchestrated process flows.
pub struct Registry {
    templates: HashMap<String, RegistryEntry>,
    skills: HashMap<String, Skill>,
    /// Bundle manifests — composed skill bundles
    bundles: HashMap<String, BundleManifest>,
}

impl Registry {
    /// Create an empty registry.
    ///
    /// expect: "The system manages a template registry for skill rendering"
    /// \[P3\] Motivating: Generative Space — in-memory template registry
    /// post: returns Registry with empty templates, skills, bundles
    pub fn new() -> Self {
        Self {
            templates: HashMap::new(),
            skills: HashMap::new(),
            bundles: HashMap::new(),
        }
    }

    /// Invalidate the registry cache (for hot-reload)
    pub(crate) fn invalidate_cache(&mut self) {
        self.templates.clear();
    }

    /// Reload registry from bootstrap (simulates reload from disk).
    ///
    /// expect: "The system manages a template registry for skill rendering"
    /// \[P3\] Motivating: Generative Space — refreshes registry from filesystem
    /// post: templates cache cleared and reloaded from bootstrap
    pub fn reload(&mut self) {
        self.invalidate_cache();
        let fresh = Self::bootstrap();
        self.templates = fresh.templates;
    }

    /// Validate that a template path is safe (no path traversal).
    ///
    /// Extended checks: component length ≤64 chars, Unicode NFC normalization.
    ///
    /// expect: "The system manages a template registry for skill rendering"
    /// \[P3\] Motivating: Generative Space — path safety for template discovery
    /// \[P4\] Constraining: Clear Boundaries — rejects paths outside template root
    /// pre:  template_id is non-empty
    /// post: returns Ok(()) if path is safe (no traversal, null bytes, non-ASCII)
    /// post: returns Err(PathTraversal) for unsafe paths
    pub fn validate_template_path(template_id: &str) -> Result<()> {
        // Reject absolute paths
        if template_id.starts_with('/') || template_id.starts_with('\\') {
            return Err(TemplateError::PathTraversal(format!(
                "Absolute path not allowed: {}",
                template_id
            )));
        }

        // Reject path traversal attempts
        if template_id.contains("..") {
            return Err(TemplateError::PathTraversal(format!(
                "Path traversal not allowed: {}",
                template_id
            )));
        }

        // Reject paths with null bytes
        if template_id.contains('\0') {
            return Err(TemplateError::PathTraversal(format!(
                "Null byte not allowed: {}",
                template_id
            )));
        }

        // Ensure path is normalized (no leading/trailing slashes)
        let normalized = template_id.trim_matches(|c| c == '/' || c == '\\');
        if normalized.is_empty() {
            return Err(TemplateError::PathTraversal(
                "Empty path not allowed".to_string(),
            ));
        }

        // Reject components exceeding 64 characters (resource-exhaustion hygiene)
        for component in normalized.split('/') {
            if component.len() > 64 {
                return Err(TemplateError::PathTraversal(format!(
                    "Path component exceeds 64 characters: {}",
                    component
                )));
            }
        }

        // Reject non-ASCII path components (homograph attack surface)
        // Template IDs must be ASCII: domain/name using lowercase a-z, digits, hyphens
        if !normalized.is_ascii() {
            return Err(TemplateError::PathTraversal(format!(
                "Non-ASCII path not allowed: {}",
                template_id
            )));
        }

        Ok(())
    }

    /// Register a template entry.
    ///
    /// The registry performs declaration-consistency checks at registration time;
    /// OCAP enforcement at runtime is handled by `McpRuntime::invoke` / `ToolGovernance`
    /// in `hkask-mcp`.
    ///
    /// expect: "The system manages a template registry for skill rendering"
    /// \[P3\] Motivating: Generative Space — registers a template in the registry
    /// pre:  entry.id is non-empty, entry.template_type is valid
    /// post: entry inserted into templates map
    pub fn register(
        &mut self,
        entry: RegistryEntry,
    ) -> std::result::Result<(), hkask_types::RegistryError> {
        // Validate entry consistency
        let warnings = entry.validate();
        for warning in &warnings {
            tracing::warn!(target: "hkask.templates", "Registration warning: {}", warning);
        }

        self.templates.insert(entry.id.clone(), entry);
        Ok(())
    }

    /// Get a template entry by ID.
    ///
    /// expect: "The system manages a template registry for skill rendering"
    /// \[P3\] Motivating: Generative Space — retrieves a registered template
    /// pre:  id is non-empty
    /// post: returns Some(&RegistryEntry) if found, None otherwise
    pub fn get(&self, id: &str) -> Option<&RegistryEntry> {
        self.templates.get(id)
    }

    pub(crate) fn by_type(&self, template_type: TemplateType) -> Vec<&RegistryEntry> {
        self.templates
            .values()
            .filter(|t| t.template_type == template_type)
            .collect()
    }

    /// Count registered templates.
    ///
    /// expect: "The system manages a template registry for skill rendering"
    /// \[P3\] Motivating: Generative Space — reports registry size
    /// post: returns count of templates in registry
    pub fn count(&self) -> usize {
        self.templates.len()
    }

    /// List all skills.
    ///
    /// expect: "The system manages a template registry for skill rendering"
    /// \[P3\] Motivating: Generative Space — lists registered skills
    /// post: returns `Vec<Skill>` with all registered skills
    pub fn list_skills(&self) -> Vec<Skill> {
        self.skills.values().cloned().collect()
    }

    /// List skills filtered by visibility.
    ///
    /// expect: "The system manages a template registry for skill rendering"
    /// \[P3\] Motivating: Generative Space — visibility-filtered skill listing
    /// pre:  visibility is a valid Visibility variant
    /// post: returns `Vec<Skill>` filtered by visibility
    pub fn list_skills_by_visibility(&self, visibility: Visibility) -> Vec<Skill> {
        self.skills
            .values()
            .filter(|s| s.visibility == visibility)
            .cloned()
            .collect()
    }

    /// Remove a skill by ID.
    ///
    /// expect: "The system manages a template registry for skill rendering"
    /// \[P3\] Motivating: Generative Space — removes a skill from registry
    /// pre:  id is non-empty
    /// post: returns Some(Skill) if removed, None if not found
    pub fn remove_skill(&mut self, id: &str) -> Option<Skill> {
        self.skills.remove(id)
    }

    /// Register a skill.
    ///
    /// expect: "The system manages a template registry for skill rendering"
    /// \[P3\] Motivating: Generative Space — registers a skill with metadata
    /// pre:  skill.id is non-empty
    /// post: skill inserted into skills map
    pub fn register_skill(&mut self, skill: Skill) {
        self.skills.insert(skill.id.clone(), skill);
    }

    /// Get a skill by ID.
    ///
    /// expect: "The system manages a template registry for skill rendering"
    /// \[P3\] Motivating: Generative Space — retrieves skill metadata
    /// pre:  id is non-empty
    /// post: returns Some(Skill) if found, None otherwise
    pub fn get_skill(&self, id: &str) -> Option<Skill> {
        self.skills.get(id).cloned()
    }

    /// List skills by domain.
    ///
    /// expect: "The system manages a template registry for skill rendering"
    /// \[P3\] Motivating: Generative Space — domain-filtered skill listing
    /// pre:  domain is a valid TemplateType
    /// post: returns `Vec<Skill>` filtered by domain
    pub fn skills_by_domain(&self, domain: TemplateType) -> Vec<Skill> {
        self.skills
            .values()
            .filter(|s| s.domain == domain)
            .cloned()
            .collect()
    }

    /// Find skills that reference a given template ID.
    ///
    /// expect: "The system manages a template registry for skill rendering"
    /// \[P3\] Motivating: Generative Space — reverse skill lookup by template
    /// pre:  template_id is non-empty
    /// post: returns `Vec<Skill>` referencing the given template
    pub fn skills_referencing_template(&self, template_id: &str) -> Vec<Skill> {
        self.skills
            .values()
            .filter(|s| {
                s.word_act.as_deref() == Some(template_id)
                    || s.flow_def.as_deref() == Some(template_id)
                    || s.know_act.as_deref() == Some(template_id)
            })
            .cloned()
            .collect()
    }

    /// Register a bundle manifest.
    ///
    /// expect: "The system manages a template registry for skill rendering"
    /// \[P3\] Motivating: Generative Space — registers a skill bundle
    /// pre:  bundle.id is non-empty
    /// post: bundle inserted into bundles map
    pub fn register_bundle(&mut self, bundle: BundleManifest) {
        self.bundles.insert(bundle.id.clone(), bundle);
    }

    /// Retrieve a bundle manifest by ID.
    ///
    /// expect: "The system manages a template registry for skill rendering"
    /// \[P3\] Motivating: Generative Space — retrieves a skill bundle
    /// pre:  id is non-empty
    /// post: returns Some(&BundleManifest) if found, None otherwise
    pub fn get_bundle(&self, id: &str) -> Option<&BundleManifest> {
        self.bundles.get(id)
    }

    /// List all bundle manifests.
    ///
    /// expect: "The system manages a template registry for skill rendering"
    /// \[P3\] Motivating: Generative Space — lists registered bundles
    /// post: returns `Vec<&BundleManifest>` with all registered bundles
    pub fn list_bundles(&self) -> Vec<&BundleManifest> {
        self.bundles.values().collect()
    }

    /// Remove a bundle manifest by ID.
    ///
    /// expect: "The system manages a template registry for skill rendering"
    /// \[P3\] Motivating: Generative Space — removes a bundle
    /// pre:  id is non-empty
    /// post: returns Some(BundleManifest) if removed, None if not found
    pub fn remove_bundle(&mut self, id: &str) -> Option<BundleManifest> {
        self.bundles.remove(id)
    }

    /// Find an existing bundle that contains exactly the given set of skills.
    /// Returns the first exact match, if any.
    ///
    /// expect: "The system manages a template registry for skill rendering"
    /// \[P3\] Motivating: Generative Space — finds bundle matching skill set
    /// pre:  skill_ids is non-empty
    /// post: returns Some(&BundleManifest) if exact skill set match found
    /// post: returns None if no exact match
    pub fn find_bundle_by_skills(&self, skill_ids: &[String]) -> Option<&BundleManifest> {
        let target: std::collections::HashSet<&str> =
            skill_ids.iter().map(|s| s.as_str()).collect();
        self.bundles.values().find(|b| {
            let bundle_skills: std::collections::HashSet<&str> =
                b.skills.iter().map(|s| s.id.as_str()).collect();
            bundle_skills == target
        })
    }

    /// Bootstrap registry from per-skill template manifests.
    ///
    /// Template definitions are auto-discovered from `registry/templates/*/manifest.yaml`
    /// at compile time via `build.rs`. Per-skill manifests are the canonical source
    /// of truth (AGENTS.md: "Registry crate (manifest.yaml + *.j2) is the canonical source").
    ///
    /// expect: "The system manages a template registry for skill rendering"
    /// \[P3\] Motivating: Generative Space — seeds registry from workspace templates
    /// post: returns Registry populated from per-skill manifests
    /// post: all entries have matroshka_limit set to SYSTEM_MAX_RECURSION
    pub fn bootstrap() -> Self {
        let mut registry = Self::new();
        let max_recursion = SYSTEM_MAX_RECURSION as u32;

        for (skill_name, manifest_yaml) in MANIFEST_YAMLS {
            match serde_yaml_neo::from_str::<SkillTemplateManifest>(manifest_yaml) {
                Ok(manifest) => {
                    for tmpl in manifest.templates {
                        let name = if tmpl.name.is_empty() {
                            tmpl.id
                                .split('/')
                                .next_back()
                                .unwrap_or(&tmpl.id)
                                .to_string()
                        } else {
                            tmpl.name
                        };
                        let entry = RegistryEntry {
                            id: tmpl.id,
                            template_type: tmpl.template_type,
                            name,
                            description: tmpl.description,
                            source_path: format!("registry/templates/{skill_name}/{}", tmpl.path),
                            cascade_level: 0,
                            matroshka_limit: max_recursion,
                        };
                        if let Err(e) = registry.register(entry) {
                            tracing::warn!(
                                target: "hkask.templates",
                                error = %e,
                                "Failed to register template entry"
                            );
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        target: "hkask.templates",
                        skill = %skill_name,
                        error = %e,
                        "Failed to parse skill manifest"
                    );
                }
            }
        }

        registry
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

impl RegistryIndex for Registry {
    fn list(&self, domain_hint: Option<TemplateType>) -> Vec<RegistryEntry> {
        match domain_hint {
            Some(t) => self.by_type(t).into_iter().cloned().collect(),
            None => self.templates.values().cloned().collect(),
        }
    }

    fn get(&self, id: &str) -> std::result::Result<RegistryEntry, hkask_types::RegistryError> {
        // Validate path first (security)
        if let Err(e) = Self::validate_template_path(id) {
            return Err(hkask_types::RegistryError::Other(e.to_string()));
        }
        // Delegate to inherent `get` (avoids trait method name collision)
        Registry::get(self, id).cloned().ok_or_else(|| {
            hkask_types::RegistryError::NotFound(NotFound {
                entity_type: "template".to_string(),
                id: format!("Template '{}' not found", id),
            })
        })
    }
}

impl SkillRegistryIndex for Registry {
    fn register_skill(
        &mut self,
        skill: Skill,
    ) -> std::result::Result<(), hkask_types::RegistryError> {
        Registry::register_skill(self, skill);
        Ok(())
    }

    fn get_skill(&self, id: &str) -> Option<Skill> {
        Registry::get_skill(self, id)
    }

    fn list_skills(&self) -> Vec<Skill> {
        Registry::list_skills(self)
    }

    fn list_skills_by_visibility(&self, visibility: hkask_types::Visibility) -> Vec<Skill> {
        Registry::list_skills_by_visibility(self, visibility)
    }

    fn skills_by_domain(&self, domain: TemplateType) -> Vec<Skill> {
        Registry::skills_by_domain(self, domain)
    }

    fn skills_referencing_template(&self, template_id: &str) -> Vec<Skill> {
        Registry::skills_referencing_template(self, template_id)
    }

    fn remove_skill(
        &mut self,
        id: &str,
    ) -> std::result::Result<Option<Skill>, hkask_types::RegistryError> {
        Ok(Registry::remove_skill(self, id))
    }
}

impl BundleRegistryIndex for Registry {
    fn register_bundle(&mut self, bundle: BundleManifest) -> Result<()> {
        Registry::register_bundle(self, bundle);
        Ok(())
    }

    fn get_bundle(&self, id: &str) -> Option<BundleManifest> {
        Registry::get_bundle(self, id).cloned()
    }

    fn list_bundles(&self) -> Vec<BundleManifest> {
        Registry::list_bundles(self).into_iter().cloned().collect()
    }

    fn remove_bundle(&mut self, id: &str) -> Result<Option<BundleManifest>> {
        Ok(Registry::remove_bundle(self, id))
    }

    fn find_bundle_by_skills(&self, skill_ids: &[String]) -> Option<BundleManifest> {
        Registry::find_bundle_by_skills(self, skill_ids).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::process_manifest_yaml;
    use crate::manifest_loader::load_manifest_from_yaml;

    // Cybernetic Swarm Plan C0: the `swarm-intelligence` manifest must declare
    // the optional deterministic `task_success` input and thread it to the
    // CHECK step's input_mapping, so a deterministic evaluator's verdict can
    // become a fourth axis of the convergence metric `d`. Pins the manifest
    // side of C0 (the template side is pinned by rendering, not here).
    #[test]
    fn swarm_intelligence_manifest_declares_task_success() {
        let yaml = process_manifest_yaml("swarm-intelligence")
            .expect("swarm-intelligence manifest must be embedded");
        let manifest =
            load_manifest_from_yaml(yaml).expect("swarm-intelligence manifest must parse");

        // The `task_success` input is declared (required: false).
        let inputs = manifest
            .inputs
            .as_ref()
            .and_then(|v| v.as_array())
            .expect("swarm-intelligence declares inputs");
        let has_task_success = inputs
            .iter()
            .any(|i| i.get("name").and_then(|v| v.as_str()) == Some("task_success"));
        assert!(
            has_task_success,
            "swarm-intelligence inputs must include `task_success` (C0)"
        );

        // CHECK (ordinal 11) threads `task_success` into its input_mapping.
        let check = manifest
            .steps
            .iter()
            .find(|s| s.ordinal == 11)
            .expect("swarm-intelligence has a CHECK step (ordinal 11)");
        let mapping = check
            .input_mapping
            .as_ref()
            .and_then(|v| v.as_object())
            .expect("CHECK step has an input_mapping");
        assert!(
            mapping.contains_key("task_success"),
            "CHECK step input_mapping must bind `task_success` (C0)"
        );
    }

    // Cybernetic Swarm Plan C1/C3/C5/C7: the `swarm-intelligence` manifest must
    // declare the two new CONVERGE compute steps (accumulate + second-order
    // monitor) and thread the deterministic accumulators through the loop step's
    // input_mapping so the next iteration's DECIDE/ORIENT/CHECK can read them.
    // Pins the manifest side (the compute primitives' math is pinned in
    // compute.rs unit tests; the template guards are pinned by rendering).
    #[test]
    fn swarm_intelligence_manifest_declares_converge_accumulators() {
        let yaml = process_manifest_yaml("swarm-intelligence")
            .expect("swarm-intelligence manifest must be embedded");
        let manifest =
            load_manifest_from_yaml(yaml).expect("swarm-intelligence manifest must parse");

        // Step 6 is the filter_proposed_moves compute primitive (C3/C7
        // deterministic enforcement between DECIDE and ACT).
        let filter = manifest
            .steps
            .iter()
            .find(|s| s.ordinal == 6)
            .expect("swarm-intelligence has a filter step (ordinal 6)");
        assert_eq!(
            filter.compute_ref.as_deref(),
            Some("swarm.filter_proposed_moves"),
            "step 6 compute_ref must be swarm.filter_proposed_moves (C3/C7 enforcement)"
        );

        // Step 12 is the converge_accumulate compute primitive.
        let accumulate = manifest
            .steps
            .iter()
            .find(|s| s.ordinal == 12)
            .expect("swarm-intelligence has a converge_accumulate step (ordinal 12)");
        assert_eq!(
            accumulate.action, "compute",
            "step 12 must be a compute step"
        );
        assert_eq!(
            accumulate.compute_ref.as_deref(),
            Some("swarm.converge_accumulate"),
            "step 12 compute_ref must be swarm.converge_accumulate (C1/C3/C7)"
        );
        let acc_mapping = accumulate
            .input_mapping
            .as_ref()
            .and_then(|v| v.as_object())
            .expect("step 12 has an input_mapping");
        for key in [
            "iteration_log",
            "failed_edits",
            "influence_scores",
            "d",
            "decisions",
            "agent_at_fault",
            "fault_count",
        ] {
            assert!(
                acc_mapping.contains_key(key),
                "converge_accumulate input_mapping must bind `{key}`"
            );
        }

        // Step 13 is the second_order_monitor compute primitive.
        let monitor = manifest
            .steps
            .iter()
            .find(|s| s.ordinal == 13)
            .expect("swarm-intelligence has a second_order_monitor step (ordinal 13)");
        assert_eq!(
            monitor.compute_ref.as_deref(),
            Some("swarm.second_order_monitor"),
            "step 13 compute_ref must be swarm.second_order_monitor (C1)"
        );

        // The loop step (ordinal 15) threads the accumulators + blame_count
        // back into context so the next iteration's DECIDE/ORIENT/CHECK/FILTER can
        // read them. A dropped binding silently disables a guard — this pins
        // the threading (the advertised-invariants trap).
        //
        // Ordinal shifted from 14 to 15 when a post-Act execute step was
        // inserted at ordinal 8 (Gap 4 fix: structural steering-mode loop
        // closure via swarm_execute_plan_local).
        let loop_step = manifest
            .steps
            .iter()
            .find(|s| s.ordinal == 15)
            .expect("swarm-intelligence has a loop step (ordinal 15)");
        let loop_mapping = loop_step
            .input_mapping
            .as_ref()
            .and_then(|v| v.as_object())
            .expect("loop step has an input_mapping");
        for key in [
            "iteration_log",
            "failed_edits",
            "influence_scores",
            "second_order",
            "fault_count",
        ] {
            assert!(
                loop_mapping.contains_key(key),
                "loop step input_mapping must thread `{key}` back (C1/C3/C5/C7)"
            );
        }
        // fault_count is now aggregated by the deterministic compute step
        // (swarm.converge_accumulate, ordinal 12), not the CHECK LLM template —
        // pin that the loop threads it from step_12_result, not step_11_result.
        let fc_binding = loop_mapping
            .get("fault_count")
            .and_then(|v| v.as_str())
            .expect("loop step binds fault_count");
        assert!(
            fc_binding.contains("step_12_result.fault_count"),
            "fault_count must thread from the compute step (step_12_result), not CHECK — got {fc_binding}"
        );

        // DECIDE (ordinal 5) binds the guards it consumes.
        let decide = manifest
            .steps
            .iter()
            .find(|s| s.ordinal == 5)
            .expect("swarm-intelligence has a DECIDE step (ordinal 5)");
        let decide_mapping = decide
            .input_mapping
            .as_ref()
            .and_then(|v| v.as_object())
            .expect("DECIDE step has an input_mapping");
        for key in [
            "failed_edits",
            "influence_scores",
            "second_order",
            "fault_count",
        ] {
            assert!(
                decide_mapping.contains_key(key),
                "DECIDE input_mapping must bind `{key}` (C3/C7/C1/C5 guards)"
            );
        }

        // ORIENT (ordinal 4) binds delegate_results for C5 fault attribution
        // (the execution telemetry feed, not the prior ACT plan).
        let orient = manifest
            .steps
            .iter()
            .find(|s| s.ordinal == 4)
            .expect("swarm-intelligence has an ORIENT step (ordinal 4)");
        let orient_mapping = orient
            .input_mapping
            .as_ref()
            .and_then(|v| v.as_object())
            .expect("ORIENT step has an input_mapping");
        assert!(
            orient_mapping.contains_key("delegate_results"),
            "ORIENT input_mapping must bind `delegate_results` for C5 fault attribution"
        );

        // The loop step must bind convergence_signal from a real field the
        // CHECK step (ordinal 11) actually produces — not a phantom
        // `hypotenuse` field on the converge_accumulate compute step (which
        // returns iteration_log/failed_edits/influence_scores/fault_count, not
        // hypotenuse). A stale binding leaves the convergence tracker's
        // signal_history at a constant default and causes premature Cauchy
        // convergence.
        //
        // The signal is now extracted by a lisp.eval compute step (ordinal 14)
        // that reads step_11_result.convergence_metric deterministically. The
        // loop step (ordinal 15) binds convergence_signal to step_14_result.
        // Pin both sides: (1) the loop reads from the compute step, and (2)
        // the compute step's env binds step_11_result.
        let conv_signal = loop_mapping
            .get("convergence_signal")
            .and_then(|v| v.as_str())
            .expect("loop step binds convergence_signal");
        assert!(
            conv_signal.contains("step_14_result"),
            "convergence_signal must read from the lisp.eval compute step (step_14_result) — got {conv_signal}"
        );
        let conv_compute = manifest
            .steps
            .iter()
            .find(|s| s.ordinal == 14)
            .expect("swarm-intelligence has a convergence-signal compute step (ordinal 14)");
        assert_eq!(
            conv_compute.action, "compute",
            "step 14 must be a compute step"
        );
        assert_eq!(
            conv_compute.compute_ref.as_deref(),
            Some("lisp.eval"),
            "step 14 compute_ref must be lisp.eval"
        );
        let conv_env = conv_compute
            .input_mapping
            .as_ref()
            .and_then(|v| v.as_object())
            .and_then(|m| m.get("env"))
            .and_then(|v| v.as_object())
            .expect("step 14 has an env block");
        assert!(
            conv_env.contains_key("step_11_result"),
            "step 14 env must bind step_11_result (the CHECK step)"
        );
    }

    // swarm-steering: the focused local-swarm steering skill (create-skill
    // artifact). Pins the manifest side: the emitted_calls input, the single
    // DIRECT step, and the process-manifest id. The template side is pinned by
    // rendering; the SKILL.md companion by the X4 invariant.
    #[test]
    fn swarm_steering_manifest_declares_directive_step() {
        let yaml = process_manifest_yaml("swarm-steering")
            .expect("swarm-steering manifest must be embedded");
        let manifest = load_manifest_from_yaml(yaml).expect("swarm-steering manifest must parse");

        // The required `emitted_calls` input is declared.
        let inputs = manifest
            .inputs
            .as_ref()
            .and_then(|v| v.as_array())
            .expect("swarm-steering declares inputs");
        let has_emitted_calls = inputs
            .iter()
            .any(|i| i.get("name").and_then(|v| v.as_str()) == Some("emitted_calls"));
        assert!(
            has_emitted_calls,
            "swarm-steering inputs must include `emitted_calls` (the plan to execute)"
        );

        // The single DIRECT step (ordinal 1) is a select that binds
        // emitted_calls + task + swarm_id + credits_authorized.
        let direct = manifest
            .steps
            .iter()
            .find(|s| s.ordinal == 1)
            .expect("swarm-steering has a DIRECT step (ordinal 1)");
        assert_eq!(
            direct.action, "select",
            "step 1 must be a select (directive producer, not executor)"
        );
        let mapping = direct
            .input_mapping
            .as_ref()
            .and_then(|v| v.as_object())
            .expect("DIRECT step has an input_mapping");
        for key in ["emitted_calls", "task", "swarm_id", "credits_authorized"] {
            assert!(
                mapping.contains_key(key),
                "DIRECT step input_mapping must bind `{key}`"
            );
        }

        // Single-pass (max_iterations 1) — a one-shot directive producer, not
        // a convergence loop.
        let max_iter = manifest.convergence.max_iterations;
        assert_eq!(
            max_iter, 1,
            "swarm-steering is single-pass (max_iterations 1) — it produces a directive, the Curator/human executes"
        );
    }

    // zed-kask: pins that skill-router is published as an installable skill
    // (process manifest embedded + parses). skill-router is the route half of
    // the route/discover loop with skill-discovery; task-breakdown emits
    // skill_match_query for it. Guards against regressions to the prior
    // "template crate only, no published manifest" state.
    #[test]
    fn skill_router_manifest_is_published_and_one_shot() {
        let yaml = process_manifest_yaml("skill-router")
            .expect("skill-router process manifest must be embedded (published)");
        let manifest = load_manifest_from_yaml(yaml).expect("skill-router manifest must parse");

        assert_eq!(manifest.id, "skill-router");
        // One-shot routing: a single match pass over the catalog (swarm-steering
        // precedent — ExitKind::MaxedOut is an Ok result, not an error).
        assert_eq!(
            manifest.convergence.max_iterations, 1,
            "skill-router is one-shot (max_iterations 1)"
        );
        let match_step = manifest
            .steps
            .iter()
            .find(|s| s.ordinal == 1)
            .expect("skill-router has a step 1 (the match)");
        assert_eq!(match_step.action, "select");
        assert_eq!(
            match_step.template_ref.as_deref(),
            Some("skill-router/skill-router-match"),
            "step 1 must render the skill-router-match template"
        );
    }
}
