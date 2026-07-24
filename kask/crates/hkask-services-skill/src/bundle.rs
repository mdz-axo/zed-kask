//! BundleService — skill bundle composition and evolution.
//!
//! # REQ: P11 (Digital Public/Private Sphere) — compose skills into bundles
//! # expect: "The service layer exposes minimal, essential interfaces shared by all surfaces"
//!
//! Composes skills into `BundleManifest` via inference-driven analysis:
//! polarity classification, conflict detection, phase separation, and
//! cascade ordering. Uses `bundler-compose.j2` as the composition template.
//!
//! # Design decisions
//!
//! - **Constraint: Guideline** — BundleService lives in `hkask-services`
//!   because both CLI and API surfaces need composition. Duplication across
//!   surfaces is measurable waste.
//! - **Constraint: Hypothesis** — Composition is LLM-native (polarity
//!   classification, conflict detection). A deterministic fallback would be
//!   a different service. This hypothesis is verified by integration tests.
//! - **Depth test** — Deleting this module would cause composition logic
//!   to reappear in both CLI and API. Passes deletion test.
//! - **OCAP gates** — Stay in domain crates. BundleService does NOT mint
//!   delegation tokens; callers pass pre-resolved secrets.

use std::sync::Arc;

use hkask_templates::BundleManifest;
use hkask_templates::BundleRegistryIndex;
use hkask_types::Visibility;
use hkask_types::{InferencePort, SkillRegistryIndex};

use hkask_services_context::AgentService;
use hkask_services_core::{DomainKind, ErrorKind, ServiceError};

/// Result of composing a bundle from skill IDs.
#[derive(Debug)]
pub struct BundleComposeResult {
    /// The composed and validated bundle manifest.
    pub manifest: BundleManifest,
    /// Warnings from composition (e.g., zone-visibility mismatches).
    pub warnings: Vec<String>,
}

/// Service for skill bundle operations — compose, evolve, list, apply.
///
/// # Design (adversarial review F5/C6)
///
/// `BundleService` is a stateless unit struct used as a namespace grouping
/// bundle operations as associated functions (each takes `ctx: &AgentService`
/// as the shared context). This parallels the common Rust pattern of grouping
/// related operations on a unit struct for discoverable call syntax
/// (`BundleService::compose(...)`). Free functions in `mod bundle` were
/// considered and rejected to avoid a wide rename of call sites in the CLI and
/// API surfaces; the struct carries no invariants — it is not a type with
/// state, and that is intentional.
pub struct BundleService;

impl BundleService {
    /// Compose a bundle from a set of skill IDs using inference.
    ///
    /// Loads skill metadata from the registry, classifies polarities,
    /// detects conflicts and complementarities, determines cascade order,
    /// and produces a validated `BundleManifest`. The result is registered
    /// into the `BundleRegistryIndex` for persistence.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  skill_ids must have at least 2 entries; ctx.storage().registry must be initialized; inference_port must be valid
    /// post: returns BundleComposeResult with validated manifest and warnings; Err(Compose) if <2 skills, skills not found, or validation fails
    /// # Arguments
    /// - `ctx` — The shared AgentService context.
    /// - `skill_ids` — Skill IDs to bundle (at least 2).
    /// - `name` — Optional bundle name (auto-generated if None).
    /// - `visibility` — Bundle visibility (Private, Shared, Public).
    /// - `inference_port` — Inference port for composition template rendering.
    /// - `editor` — UserPod name for attribution.
    ///
    /// # Returns
    /// `BundleComposeResult` on success.
    /// `ServiceError::Compose` on failure (inference error, validation failure).
    /// `ServiceError::Skill` if a skill ID is not found in the registry.
    #[allow(clippy::too_many_arguments)]
    pub async fn compose(
        ctx: &AgentService,
        skill_ids: &[String],
        name: Option<&str>,
        visibility: Visibility,
        inference_port: Arc<dyn InferencePort>,
        editor: &str,
    ) -> Result<BundleComposeResult, ServiceError> {
        if skill_ids.len() < 2 {
            return Err(ServiceError::Domain {
                domain: DomainKind::Skill,
                kind: ErrorKind::ServiceUnavailable,
                source: None,
                message: "A bundle requires at least 2 skills".to_string(),
            });
        }

        // Resolve skill metadata from the registry.
        let registry = ctx.storage().registry.clone();
        let registry_guard = registry.lock().await;
        let skills: Vec<hkask_types::Skill> = skill_ids
            .iter()
            .filter_map(|id| registry_guard.get_skill(id))
            .collect();

        if skills.len() != skill_ids.len() {
            let found: std::collections::HashSet<&str> =
                skills.iter().map(|s| s.id.as_str()).collect();
            let missing: Vec<&str> = skill_ids
                .iter()
                .filter(|id| !found.contains(id.as_str()))
                .map(|s| s.as_str())
                .collect();
            return Err(ServiceError::Domain {
                domain: DomainKind::Skill,
                kind: ErrorKind::ServiceUnavailable,
                source: None,
                message: format!("Skills not found in registry: {}", missing.join(", ")),
            });
        }

        // Check for existing bundle with these skills (smart matching).
        let existing = registry_guard.find_bundle_by_skills(skill_ids);
        if let Some(existing_bundle) = existing {
            return Ok(BundleComposeResult {
                manifest: existing_bundle.clone(),
                warnings: vec![format!(
                    "An existing bundle '{}' already matches these skills. Use evolve to update.",
                    existing_bundle.id
                )],
            });
        }
        drop(registry_guard);

        // Build the composition prompt from skill metadata.
        let skill_descriptions: Vec<String> = skills
            .iter()
            .map(|s| {
                let polarity = s.polarity.map(|p| p.as_str()).unwrap_or("unknown");
                let domain = s.domain.as_str();
                format!(
                    "- {} (polarity: {}, domain: {}, visibility: {})",
                    s.id,
                    polarity,
                    domain,
                    s.visibility.as_str()
                )
            })
            .collect();

        let skill_list = skill_descriptions.join("\n");

        // When the caller supplies a name, instruct the LLM to use it verbatim.
        let name_line = match name {
            Some(n) => format!("- name: use exactly this name: {}\n", n),
            None => "- name: a descriptive name\n".to_string(),
        };

        // Render the bundler-compose prompt from skill metadata.
        let prompt = format!(
            "You are a skill composition orchestrator. Your job is to compose a bundle from the following skills.\n\
             \nSkills to bundle:\n{}\n\n\
             For each skill, classify its polarity (Generative, Evaluative, Regulative, Procedural).\n\
             Detect conflicts between skills and declare resolutions.\n\
             Identify complementarities that enhance the bundle.\n\
             Determine the cascade order with phase separation (Pre -> Core -> Post).\n\
             Never place divergent (Generative) and convergent (Evaluative) skills in the same phase.\n\
             Produce a valid BundleManifest JSON with:\n\
             - id: a unique kebab-case identifier\n\
             {}\n\
             - description: what this bundle does\n\
             - version: semantic version (start at 1.0.0)\n\
             - editor: {}\n\
             - visibility: {}\n\
             - skills: array of {{id, polarity, lexicon_terms, manifest_ref, content_hash}}\n\
             - conflicts: array of {{skills, domain, conflict_type, resolution, resolution_detail}}\n\
             - complementarities: array of {{skills, complementarity_type, detail}}\n\
             - steps: array of {{ordinal, action, description, phase, gas_cap, timeout_seconds}}\n\
             - Cascade depth must not exceed 7.\n\
             - Each skill must have <= 10 lexicon terms.\n\
             - Bundle must have at least one productive (Procedural) skill.\n\
             - Declare a convergence criterion.\n\
             Respond with ONLY the JSON object, no markdown fences, no commentary.",
            skill_list,
            name_line,
            editor,
            visibility.as_str()
        );

        let params = hkask_types::template::LLMParameters::default();
        let result = inference_port
            .generate(&prompt, &params, None)
            .await
            .map_err(|e| {
                let msg = format!("Inference failed: {}", e);
                ServiceError::Domain {
                    domain: DomainKind::Skill,
                    kind: ErrorKind::ServiceUnavailable,
                    source: Some(Box::new(e)),
                    message: msg,
                }
            })?;

        // Parse the JSON response into a BundleManifest.
        let manifest: BundleManifest = serde_json::from_str(&result.text).map_err(|e| {
            let msg = format!(
                "Failed to parse composition result as JSON: {}. Raw response: {}",
                e,
                &result.text[..result.text.len().min(200)]
            );
            ServiceError::Domain {
                domain: DomainKind::Skill,
                kind: ErrorKind::ServiceUnavailable,
                source: Some(Box::new(e)),
                message: msg,
            }
        })?;

        // Validate the manifest.
        let validation = manifest.validate();
        if !validation.is_valid() {
            return Err(ServiceError::Domain {
                domain: DomainKind::Skill,
                kind: ErrorKind::ServiceUnavailable,
                source: None,
                message: format!(
                    "Composed bundle failed validation: {}",
                    validation.errors.join("; ")
                ),
            });
        }

        // Register the bundle in the registry.
        {
            let mut registry_guard = registry.lock().await;
            if let Err(e) = registry_guard.register_bundle(manifest.clone()) {
                tracing::warn!(target: "hkask.bundle", error = %e, "Failed to register bundle in registry");
            }
        }

        let mut warnings = validation.warnings;
        warnings.push(format!(
            "Bundle '{}' composed with {} skills, {} steps",
            manifest.id,
            manifest.skills.len(),
            manifest.steps.len()
        ));

        Ok(BundleComposeResult { manifest, warnings })
    }

    /// List all bundles in the registry.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  ctx.storage().registry must be initialized
    /// post: returns `Vec<BundleManifest>` of all registered bundles; empty Vec if none
    pub async fn list(ctx: &AgentService) -> Result<Vec<BundleManifest>, ServiceError> {
        let registry = ctx.storage().registry.clone();
        let guard = registry.lock().await;
        Ok(guard.list_bundles())
    }

    /// Get a bundle by ID.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  ctx.storage().registry must be initialized; id must be non-empty
    /// post: returns Some(BundleManifest) if found; None if not found
    pub async fn get(ctx: &AgentService, id: &str) -> Result<Option<BundleManifest>, ServiceError> {
        let registry = ctx.storage().registry.clone();
        let guard = registry.lock().await;
        Ok(guard.get_bundle(id))
    }

    /// Apply a bundle to the current session.
    ///
    /// Returns the bundle manifest if found, or `ServiceError::Compose` if not.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  ctx.storage().registry must be initialized; id must be non-empty
    /// post: returns BundleManifest if found; Err(Compose) if bundle not found
    pub async fn apply(ctx: &AgentService, id: &str) -> Result<BundleManifest, ServiceError> {
        let registry = ctx.storage().registry.clone();
        let guard = registry.lock().await;
        guard.get_bundle(id).ok_or_else(|| ServiceError::Domain {
            domain: DomainKind::Skill,
            kind: ErrorKind::ServiceUnavailable,
            source: None,
            message: format!("Bundle '{}' not found", id),
        })
    }

    /// Evolve a bundle — re-compose when skills have changed.
    ///
    /// Re-loads skill metadata, re-runs composition, and updates the manifest.
    /// Returns the evolved manifest.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  ctx.storage().registry must be initialized; id must reference an existing bundle; inference_port must be valid
    /// post: returns BundleComposeResult with evolved manifest; old bundle removed, new one registered; Err(Compose) if bundle not found
    pub async fn evolve(
        ctx: &AgentService,
        id: &str,
        inference_port: Arc<dyn InferencePort>,
        editor: &str,
    ) -> Result<BundleComposeResult, ServiceError> {
        let existing = Self::get(ctx, id).await?;
        let existing = existing.ok_or_else(|| ServiceError::Domain {
            domain: DomainKind::Skill,
            kind: ErrorKind::ServiceUnavailable,
            source: None,
            message: format!("Bundle '{}' not found", id),
        })?;

        // Re-compose using the same skill IDs.
        let skill_ids = existing.skill_ids();
        let result = Self::compose(
            ctx,
            &skill_ids,
            Some(&existing.name),
            existing.visibility,
            inference_port,
            editor,
        )
        .await?;

        // Remove the old bundle and register the new one.
        {
            let registry = ctx.storage().registry.clone();
            let mut guard = registry.lock().await;
            if let Err(e) = guard.remove_bundle(id) {
                tracing::warn!(target: "hkask.bundle", error = %e, id = %id, "Failed to remove old bundle");
            }
            if let Err(e) = guard.register_bundle(result.manifest.clone()) {
                tracing::warn!(target: "hkask.bundle", error = %e, "Failed to register evolved bundle");
            }
        }

        Ok(result)
    }

    /// List available skills from the registry.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  ctx.storage().registry must be initialized
    /// post: returns `Vec<Skill>` of all registered skills; empty Vec if none
    ///
    /// Returns `Vec<Skill>` directly (not `Result`) because the underlying
    /// `tokio::sync::Mutex` cannot poison — the previous `Result` wrapper had a
    /// dead error path (adversarial review F20/C5).
    pub async fn list_skills(ctx: &AgentService) -> Vec<hkask_types::Skill> {
        let registry = ctx.storage().registry.clone();
        let guard = registry.lock().await;
        hkask_types::SkillRegistryIndex::list_skills(&*guard)
    }
}
