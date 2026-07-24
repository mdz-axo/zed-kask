#![forbid(unsafe_code)]
//! hKask MCP Skill — exposes registered skills as callable MCP tools.
//!
//! Each skill in the registry becomes available through the `skill_execute` tool.
//! When invoked, the server:
//! 1. Looks up the skill's template in the registry
//! 2. Renders the Jinja2 template with the provided context variables
//! 3. Runs inference on the rendered prompt via the centralized inference router
//! 4. Returns the inference result
//!
//! ## Architectural note: templates vs skills
//!
//! This server executes **templates** — Jinja2 prompt templates from the
//! template registry (`RegistryIndex`), rendered and sent through the inference
//! router. It is *not* a `Skill` (PDCA composition) executor: the `Skill` model
//! (`SkillRegistryIndex`) composes templates into FlowDef loops that require a
//! manifest executor, which is a separate concern. The two registry indexes are
//! therefore distinct, not redundant — this server reads `RegistryIndex` because
//! that is the layer it executes. `hkask-services-skill` provides skill
//! *management* (discovery, publishing, auditing, bundle composition); template
//! rendering + inference is an alternate surface concern that lives here.

#![allow(unused_crate_dependencies)] // tokio is used only by the bin (main.rs `#[tokio::main]`); the unused-crate-deps lint checks the lib target where tokio is unused. dotenvy was removed (no .rs references). Other deps are used in lib.rs.

// Bridge crates: shared ontological vocabulary (P5.4 dual-axis framework)

use hkask_inference::{InferenceConfig, InferenceRouter};
use hkask_mcp_server::server::{CapabilityTier, McpToolError, execute_tool};
use hkask_templates::Registry;
use hkask_types::template::LLMParameters;
use hkask_types::{InferencePort, RegistryEntry, RegistryIndex};
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;

const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Inference parameters for skill execution. Low temperature for deterministic
/// adherence to the skill template; capped tokens to bound cost. Intentionally
/// fixed across all skills — per-skill params would require a manifest field
/// (tracked as a deepening candidate; adversarial review F13).
const SKILL_TEMPERATURE: f32 = 0.3;
const SKILL_MAX_TOKENS: u32 = 2048;

hkask_mcp_server::mcp_server!(
    pub struct SkillServer {
        pub inference_port: Arc<dyn InferencePort>,
        /// Template entries available as tools, keyed by tool-facing ID
        /// (see `registry_entry_to_tool_id`). Template content is read lazily
        /// from `entry.source_path` on each `skill_execute`.
        pub skills: HashMap<String, RegistryEntry>,
        pub capability_tier: CapabilityTier,
    }
);

impl SkillServer {
    /// Load templates from the bootstrapped template registry as callable tools.
    ///
    /// Stores each `RegistryEntry` keyed by its tool-facing ID; template content
    /// is read lazily from `entry.source_path` on `skill_execute` (avoids N
    /// in-memory copies and stays current with disk). Unreadable templates are
    /// skipped with a `tracing::warn!` so the failure is observable —
    /// `skill_ping`'s `skills_loaded` reflects only successful loads
    /// (adversarial review F7: previously failures were silently dropped).
    pub fn load_skills(&mut self) {
        let registry = Registry::bootstrap();

        for entry in registry.list(None) {
            if std::fs::read_to_string(&entry.source_path).is_err() {
                tracing::warn!(
                    target: "hkask.mcp.skill",
                    id = %entry.id,
                    source = %entry.source_path,
                    "Template unreadable — skipping"
                );
                continue;
            }

            let tool_id = registry_entry_to_tool_id(&entry.id);
            self.skills.insert(tool_id, entry);
        }
    }
}

/// Convert a registry entry ID to a tool-facing skill ID.
///
/// Registry IDs use `/` as the namespace separator (e.g.
/// `coding-guidelines/guidelines-assess`). We map `/` → `.` for ergonomic display
/// in tool listings. This transform is **injective**: registry IDs never contain
/// `.`, so it is collision-free and reversible. The previous implementation
/// also mapped `_` → `-`, which was lossy (distinct IDs differing only by `_`
/// vs `-` would collide); that step was removed in the adversarial review (F11).
fn registry_entry_to_tool_id(id: &str) -> String {
    id.replace('/', ".")
}

/// Human-readable JSON type name for error messages (adversarial review F8).
fn json_type_name(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

/// Render a Jinja2 template with the given context map.
fn render_skill_template(
    template: &str,
    context: &HashMap<String, serde_json::Value>,
) -> anyhow::Result<String> {
    let mut env = minijinja::Environment::new();
    env.set_undefined_behavior(minijinja::UndefinedBehavior::Lenient);

    for (key, value) in context {
        let val = match value {
            serde_json::Value::String(s) => minijinja::Value::from(s.clone()),
            other => minijinja::Value::from(other.to_string()),
        };
        env.add_global(key, val);
    }

    env.render_str(template, minijinja::value::Value::UNDEFINED)
        .map_err(|e| anyhow::anyhow!("Template render error: {}", e))
}

// ── MCP Tools ─────────────────────────────────────────────────────────────────

/// Request parameters for `skill_execute`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SkillExecuteRequest {
    /// Skill identifier (e.g., "coding-guidelines.guidelines-assess")
    #[schemars(description = "Skill identifier (e.g., coding-guidelines.guidelines-assess)")]
    skill_id: String,
    /// Context variables to pass to the template as a JSON object
    #[schemars(description = "Context variables as JSON object to pass to the template")]
    context: serde_json::Value,
}

#[tool_router(server_handler)]
impl SkillServer {
    #[tool(description = "Liveness and profile info")]
    pub async fn skill_ping(&self) -> String {
        execute_tool(self, "skill_ping", async {
            Ok(serde_json::json!({
                "status": "ok",
                "version": SERVER_VERSION,
                "mode": if self.capability_tier.embedded { "embedded" } else { "standalone" },
                "skills_loaded": self.skills.len(),
            }))
        })
        .await
    }

    #[tool(description = "List available skill IDs with their descriptions")]
    pub async fn skill_list(&self) -> String {
        execute_tool(self, "skill_list", async {
            let skills: Vec<serde_json::Value> = self
                .skills
                .iter()
                .map(|(id, entry)| {
                    serde_json::json!({
                        "id": id,
                        "description": format!("[{}] {}", entry.template_type, entry.description),
                    })
                })
                .collect();
            Ok(serde_json::json!({ "skills": skills }))
        })
        .await
    }

    #[tool(
        description = "Execute a registered skill template with context variables. \
        Renders the skill as a Jinja2 template and runs inference. \
        Use skill_list first to discover available skill IDs."
    )]
    pub async fn skill_execute(
        &self,
        Parameters(SkillExecuteRequest { skill_id, context }): Parameters<SkillExecuteRequest>,
    ) -> String {
        execute_tool(self, "skill_execute", async {
            let entry = self.skills.get(&skill_id).ok_or_else(|| {
                let available: Vec<&str> = self.skills.keys().map(|s| s.as_str()).collect();
                McpToolError::new(
                    hkask_types::McpErrorKind::NotFound,
                    format!("Skill '{}' not found. Available: {:?}", skill_id, available),
                )
            })?;

            // Context must be a JSON object. Non-object values (array, string,
            // number, …) are rejected rather than silently rendered with an empty
            // context (adversarial review F8).
            let map = match &context {
                serde_json::Value::Object(map) => map,
                other => {
                    return Err(McpToolError::invalid_argument(format!(
                        "context must be a JSON object, got {}",
                        json_type_name(other)
                    )));
                }
            };

            let mut ctx = HashMap::new();
            for (k, v) in map {
                ctx.insert(k.clone(), v.clone());
            }

            // Read the template content fresh from disk (RegistryEntry.source_path).
            let template_content = std::fs::read_to_string(&entry.source_path).map_err(|e| {
                McpToolError::internal(format!(
                    "Failed to read template {}: {e}",
                    entry.source_path
                ))
            })?;

            let rendered = render_skill_template(&template_content, &ctx)
                .map_err(|e| McpToolError::invalid_argument(e.to_string()))?;

            // Minimal, honest preamble. The previous version asserted a specific
            // tool set (filesystem, memory, web search, …) that varies per
            // deployment — a hidden parameter (Magna Carta P3). We now ask the
            // agent to adapt to whatever capabilities it actually has (F12).
            let full_prompt = format!(
                "Follow the skill template's instructions precisely. If it references \
                 capabilities the calling agent lacks, adapt the approach or report \
                 the limitation rather than failing silently.\n\n{}",
                rendered
            );

            let params = LLMParameters {
                temperature: SKILL_TEMPERATURE,
                max_tokens: SKILL_MAX_TOKENS,
                ..Default::default()
            };

            let result = self
                .inference_port
                .generate(&full_prompt, &params, None)
                .await
                .map_err(|e| McpToolError::internal(format!("Inference failed: {}", e)))?;

            Ok(serde_json::json!({ "result": result.text }))
        })
        .await
    }
}

// ── Server runner ─────────────────────────────────────────────────────────────

pub async fn run(
    userpod: String,
    daemon_client: Option<hkask_mcp_server::DaemonClient>,
) -> Result<(), hkask_mcp_server::McpError> {
    let inference_config = InferenceConfig::from_env();
    let inference_router = InferenceRouter::new(inference_config);
    let inference_port: Arc<dyn InferencePort> = Arc::new(inference_router);

    hkask_mcp_server::run_server(
        "hkask-mcp-skill",
        env!("CARGO_PKG_VERSION"),
        |ctx: hkask_mcp_server::ServerContext| {
            {
                let webid = ctx.webid;
                let mut server = SkillServer::new(
                    webid,
                    userpod.clone(),
                    daemon_client.clone(),
                    inference_port.clone(),
                    HashMap::new(),
                    ctx.capability_tier,
                );
                server.load_skills();
                tracing::info!(
                    target: "hkask.mcp.skill",
                    skill_count = server.skills.len(),
                    "Skills loaded from registry"
                );
                Ok::<_, anyhow::Error>(server)
            }
            .map_err(|e| hkask_mcp_server::McpError::UnexpectedResponse {
                context: "skill server init".into(),
                detail: e.to_string(),
            })
        },
        vec![],
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_entry_to_tool_id_is_non_lossy() {
        // `/` → `.` (ergonomic display); injective because registry IDs never contain `.`.
        assert_eq!(
            registry_entry_to_tool_id("coding-guidelines/guidelines-assess"),
            "coding-guidelines.guidelines-assess"
        );
        // `_` is preserved (the previous `_` → `-` step was lossy — review F11).
        assert_eq!(
            registry_entry_to_tool_id("skill_maintenance/step"),
            "skill_maintenance.step"
        );
        // Distinct IDs differing only by `_` vs `-` must not collide.
        assert_ne!(
            registry_entry_to_tool_id("skill_maintenance/step"),
            registry_entry_to_tool_id("skill-maintenance/step"),
            "distinct IDs must not collide after transform"
        );
    }

    #[test]
    fn json_type_name_covers_all_variants() {
        assert_eq!(json_type_name(&serde_json::Value::Null), "null");
        assert_eq!(json_type_name(&serde_json::Value::Bool(true)), "bool");
        assert_eq!(json_type_name(&serde_json::json!(42)), "number");
        assert_eq!(json_type_name(&serde_json::json!("x")), "string");
        assert_eq!(json_type_name(&serde_json::json!([1])), "array");
        assert_eq!(json_type_name(&serde_json::json!({})), "object");
    }

    #[test]
    fn render_skill_template_passes_object_context() {
        let mut ctx = HashMap::new();
        ctx.insert("name".to_string(), serde_json::json!("world"));
        let rendered = render_skill_template("Hello {{ name }}!", &ctx).expect("renders");
        assert_eq!(rendered, "Hello world!");
    }
}
