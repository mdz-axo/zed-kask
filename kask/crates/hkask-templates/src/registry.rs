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
///     lexicon_terms: [...]
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
    lexicon_terms: Vec<String>,
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

    /// Register a template entry. Validates lexicon_terms against known vocabulary.
    ///
    /// Unknown terms are logged as warnings (Warn mode).
    /// The registry performs declaration-consistency checks at registration time;
    /// OCAP enforcement at runtime is handled by `GovernedTool` in `hkask-regulation`.
    ///
    /// expect: "The system manages a template registry for skill rendering"
    /// \[P3\] Motivating: Generative Space — registers a template in the registry
    /// pre:  entry.id is non-empty, entry.template_type is valid
    /// post: entry inserted into templates map
    /// post: returns Err(RegistryError) if lexicon_terms contain unknown terms
    pub fn register(
        &mut self,
        entry: RegistryEntry,
    ) -> std::result::Result<(), hkask_types::RegistryError> {
        // Validate entry consistency
        let warnings = entry.validate();
        for warning in &warnings {
            tracing::warn!(target: "hkask.templates", "Registration warning: {}", warning);
        }

        // Validate lexicon_terms against known vocabulary (F-07 fix:
        // unknown terms are now errors, not warnings — a typo like
        // `visibilty` should not silently register with a bad tag).
        let vocab_warnings = crate::vocabulary::validate_entry(&entry);
        if !vocab_warnings.is_empty() {
            return Err(hkask_types::RegistryError::Other(format!(
                "lexicon validation failed: {}",
                vocab_warnings.join("; ")
            )));
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
                            lexicon_terms: tmpl.lexicon_terms,
                            description: tmpl.description,
                            source_path: format!("registry/templates/{skill_name}/{}", tmpl.path),
                            required_capabilities: Vec::new(),
                            cascade_level: 0,
                            matroshka_limit: max_recursion,
                        };
                        if let Err(e) = registry.register(entry) {
                            tracing::warn!(
                                target: "hkask.templates",
                                error = %e,
                                "Failed to register template entry — lexicon validation error"
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

        // Register platform engineering FlowDef manifests.
        // These are the loyalty-anchored platform maintenance skills.
        let platform_manifests = [
            include_str!(
                "../../../registry/manifests/platform-governance-transparency-reporter.yaml"
            ),
            include_str!("../../../registry/manifests/platform-consent-auditor.yaml"),
            include_str!("../../../registry/manifests/platform-portability-verifier.yaml"),
            include_str!("../../../registry/manifests/platform-health-scorer.yaml"),
            include_str!("../../../registry/manifests/platform-loyalty-scorecard.yaml"),
            include_str!("../../../registry/manifests/platform-bulkhead-auditor.yaml"),
            include_str!("../../../registry/manifests/platform-wardley-mapper.yaml"),
        ];
        for manifest_yaml in platform_manifests {
            match crate::manifest_loader::load_manifest_from_yaml(manifest_yaml) {
                Ok(bundle) => {
                    tracing::info!(
                        target: "hkask.templates",
                        id = %bundle.id,
                        steps = bundle.steps.len(),
                        "Registered platform engineering manifest",
                    );
                    registry.register_bundle(bundle);
                }
                Err(e) => {
                    tracing::warn!(
                        target: "hkask.templates",
                        error = %e,
                        "Failed to load platform engineering manifest",
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
