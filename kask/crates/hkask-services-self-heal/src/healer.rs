//! SelfHealer — two-stage autonomous error recovery engine.
//!
//! Every fallible operation in hKask can pass through a `SelfHealer`. The healer
//! maps error patterns to recovery strategies, executes healing actions, and
//! returns Healed (retry), Degraded (fallback), or Unhealable (escalate to Curator).
//!
//! **Stage 1 (always available):** Deterministic env/config healing — `RunCommand`,
//! `SetEnv`, `LoadDotEnv`, `CreateDefaultFile`, `RetryWithBackoff`, `ProposeCodeChange`.
//! No inference required.
//!
//! **Stage 2 (requires `with_inference()`):** LLM template-assisted healing via
//! `LlmAssisted`. Renders a Jinja2 template from `registry/templates/heal/`,
//! calls the classifier model, parses JSON instructions, executes sub-actions.
//!
//! ## Regulation spans emitted
//!
//! | Target | When |
//! |--------|------|
//! | `reg.heal.attempt` | Healing starts |
//! | `reg.heal.strategy` | Strategy selected |
//! | `reg.heal.dotenv` | .env loaded |
//! | `reg.heal.set_env` | Env var set |
//! | `reg.heal.file_created` | File created |
//! | `reg.heal.code_change_proposed` | Code change proposed |
//! | `reg.heal.llm_assisted` | LLM template rendered |
//! | `reg.heal.retry_loop` | Each retry iteration |
//! | `reg.heal.unmatched` | No strategy found |
//! | `reg.heal.escalated` | Exhausted, escalating to Curator |

use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

use super::helpers::{parse_llm_response, resolve_env_value};
use super::registry::HealRegistry;
use super::types::{
    ActionResult, DebugLogAction, EnvValueSource, HealAction, HealContext, HealError,
    HealInferenceFn, HealInstruction, HealOutcome, MiniDebugLog,
};

const MAX_RETRIES: u32 = 3;
const CLASSIFY_TEMPLATE: &str = "registry/templates/heal/classify-error.j2";
static CLASSIFY_TEMPLATE_CACHE: OnceLock<String> = OnceLock::new();

pub struct SelfHealer {
    registry: HealRegistry,
    inference: Option<HealInferenceFn>,
}

impl SelfHealer {
    #[must_use]
    pub fn new() -> Self {
        Self {
            registry: HealRegistry::with_defaults(),
            inference: None,
        }
    }

    #[must_use]
    pub fn with_registry(registry: HealRegistry) -> Self {
        Self {
            registry,
            inference: None,
        }
    }

    #[must_use]
    pub fn with_inference(mut self, f: HealInferenceFn) -> Self {
        self.inference = Some(f);
        self
    }

    pub fn registry(&self) -> &HealRegistry {
        &self.registry
    }
    pub fn registry_mut(&mut self) -> &mut HealRegistry {
        &mut self.registry
    }
    #[must_use]
    pub fn has_inference(&self) -> bool {
        self.inference.is_some()
    }

    // ── Bounded retry loop ──────────────────────────────────────────────

    #[must_use = "result must be used"]
    pub fn healable<T, E: fmt::Display>(
        &self,
        mut operation: impl FnMut() -> Result<T, E>,
        context: HealContext,
    ) -> Result<T, E> {
        let base_delay_ms: u64 = 1000;
        let mut debug_log = MiniDebugLog::default();

        for attempt in 1..=MAX_RETRIES {
            match operation() {
                Ok(v) => return Ok(v),
                Err(e) => {
                    let err = e.to_string();
                    tracing::info!(target: "reg.heal.retry_loop", attempt, max_retries = MAX_RETRIES, operation = %context.operation, error = %err);

                    match self.attempt(&err, &context) {
                        HealOutcome::Healed {
                            action_taken,
                            modifications,
                        } => {
                            debug_log.modifications.extend(modifications);
                            debug_log.actions_taken.push(DebugLogAction {
                                name: action_taken,
                                output: "Healed — retrying".into(),
                                success: true,
                            });
                            thread::sleep(Duration::from_millis(
                                base_delay_ms * (1u64 << (attempt - 1)),
                            ));
                            continue;
                        }
                        HealOutcome::Degraded { reason, .. } => {
                            debug_log.attempt_count = attempt;
                            if attempt < MAX_RETRIES {
                                thread::sleep(Duration::from_millis(
                                    base_delay_ms * (1u64 << (attempt - 1)),
                                ));
                                continue;
                            }
                            debug_log.modifications.push(reason);
                            break;
                        }
                        HealOutcome::Unhealable {
                            reason,
                            suggestion,
                            debug_log: attempt_log,
                            ..
                        } => {
                            debug_log.attempt_count = attempt;
                            debug_log.reg_spans.extend(attempt_log.reg_spans);
                            debug_log.modifications.extend(attempt_log.modifications);
                            debug_log.actions_taken.extend(attempt_log.actions_taken);
                            debug_log.suggestion = suggestion.clone();
                            if attempt < MAX_RETRIES {
                                thread::sleep(Duration::from_millis(
                                    base_delay_ms * (1u64 << (attempt - 1)),
                                ));
                                continue;
                            }
                            let json = serde_json::to_string(&debug_log).unwrap_or_default();
                            tracing::error!(target: "reg.heal.escalated", operation = %context.operation, reason = %reason, debug_log = %json, "Healing exhausted — escalating to Curator");
                            return Err(e);
                        }
                    }
                }
            }
        }

        match operation() {
            Ok(v) => {
                tracing::info!(target: "reg.heal.retry_loop", operation = %context.operation, "Operation succeeded after healing exhaustion");
                Ok(v)
            }
            Err(e) => {
                let json = serde_json::to_string(&debug_log).unwrap_or_default();
                tracing::error!(target: "reg.heal.escalated", operation = %context.operation, attempt_count = debug_log.attempt_count, debug_log = %json, "Healing exhausted — escalating to Curator");
                Err(e)
            }
        }
    }

    // ── 4-stage attempt pipeline ────────────────────────────────────────

    #[must_use]
    pub fn attempt(&self, error: &str, context: &HealContext) -> HealOutcome {
        tracing::info!(target: "reg.heal.attempt", operation = %context.operation, error = %error, reg_span = %hkask_types::regulation::RegulationSpan::SelfHeal);

        // Stage 1: KnowAct classification
        if let Some((strategy_name, confidence)) = self.classify_error(error, context)
            && confidence > 0.5
            && let Some(strategy) = self.registry.find_strategy_by_name(&strategy_name)
        {
            tracing::info!(target: "reg.heal.strategy", strategy = %strategy_name, confidence = confidence, source = "llm_classify");
            return self.execute_and_judge(strategy, context);
        }

        // Stage 2: String-matching fallback
        if let Some(strategy) = self.registry.find_strategy(error) {
            tracing::info!(target: "reg.heal.strategy", strategy = %strategy.name, source = "string_match");
            return self.execute_and_judge(strategy, context);
        }

        // Stage 3: LLM-assisted fallback
        if self.inference.is_some() {
            return self.llm_assisted_fallback(error, context);
        }

        // Stage 4: Unhealable
        tracing::warn!(target: "reg.heal.unmatched", operation = %context.operation, error = %error);
        HealOutcome::Unhealable {
            reason: format!("No healing strategy matches: {}", error),
            suggestion: "Add a strategy to HealRegistry or a template to registry/templates/heal/"
                .into(),
            requires_code_change: false,
            debug_log: MiniDebugLog {
                attempt_count: 1,
                reg_spans: vec!["reg.heal.unmatched".into()],
                suggestion: "Add strategy or template".into(),
                ..Default::default()
            },
        }
    }

    // ── Stage 1: classify via LLM ───────────────────────────────────────

    fn classify_error(&self, _error: &str, context: &HealContext) -> Option<(String, f64)> {
        let inference = self.inference.as_ref()?;
        let prompt = match self.render_classify_template(context) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(target: "reg.heal", error = %e, "Failed to render classify-error.j2 — falling back to string matching");
                return None;
            }
        };
        let result = inference(&prompt).ok()?;
        let parsed: serde_json::Value = serde_json::from_str(&result).ok()?;
        let strategy = parsed.get("strategy")?.as_str()?.to_string();
        let confidence = parsed.get("confidence")?.as_f64()?;
        if strategy == "none" || strategy.is_empty() {
            return None;
        }
        Some((strategy, confidence))
    }

    // ── Stage 3: LLM proposes fix actions ───────────────────────────────

    fn llm_assisted_fallback(&self, error: &str, context: &HealContext) -> HealOutcome {
        // Caller guarantees inference is Some via the is_some() guard in attempt()
        let inference = self.inference.as_ref().expect("caller guarded is_some");

        let prompt = format!(
            "Diagnose this hKask runtime error and propose concrete fix actions.\n\
             Return ONLY a JSON array. Each action is one of:\n\
             {{\"action\": \"RunCommand\", \"command\": \"...\"}}\n\
             {{\"action\": \"SetEnv\", \"key\": \"...\", \"value\": \"...\"}}\n\
             {{\"action\": \"CreateDefaultFile\", \"path\": \"...\", \"content\": \"...\"}}\n\n\
             Error: {error}\nOperation: {operation}\n\nJSON array:",
            operation = context.operation,
        );

        tracing::info!(target: "reg.heal.llm_assisted", operation = %context.operation);

        let result = match inference(&prompt) {
            Ok(r) => r,
            Err(e) => {
                return HealOutcome::Degraded {
                    reason: format!("LLM fallback failed: {e}"),
                    fallback_description: "LLM-assisted healing unavailable".into(),
                };
            }
        };

        let instructions: Vec<HealInstruction> = match parse_llm_response(&result) {
            Ok(i) => i,
            Err(e) => {
                return HealOutcome::Degraded {
                    reason: format!("Failed to parse: {e}"),
                    fallback_description: "LLM response was not valid JSON".into(),
                };
            }
        };

        if instructions.is_empty() {
            return HealOutcome::Degraded {
                reason: "LLM returned no instructions".into(),
                fallback_description: "Classifier could not determine recovery actions".into(),
            };
        }

        let ar = self.execute_llm_instructions(&instructions, context);
        if ar.success {
            HealOutcome::Healed {
                action_taken: ar.output,
                modifications: ar.modifications,
            }
        } else {
            HealOutcome::Degraded {
                reason: "All LLM-proposed actions failed".into(),
                fallback_description: ar.output,
            }
        }
    }

    // ── Internal helpers ────────────────────────────────────────────────

    /// Execute parsed LLM instructions — shared by `LlmAssisted` and `llm_assisted_fallback`.
    fn execute_llm_instructions(
        &self,
        instructions: &[HealInstruction],
        context: &HealContext,
    ) -> ActionResult {
        let mut mods = Vec::new();
        let mut last = String::new();
        let mut any = false;
        for instr in instructions {
            let action = match instr.action.as_str() {
                "RunCommand" if !instr.command.is_empty() => HealAction::RunCommand {
                    command: instr.command.clone(),
                    capture_output: true,
                },
                "SetEnv" if !instr.key.is_empty() => HealAction::SetEnv {
                    key: instr.key.clone(),
                    value_source: EnvValueSource::Literal(instr.value.clone()),
                },
                "CreateDefaultFile" if !instr.path.is_empty() => HealAction::CreateDefaultFile {
                    path: PathBuf::from(&instr.path),
                    content: instr.content.clone(),
                },
                _ => continue,
            };
            match self.execute_action(&action, context) {
                Ok(r) => {
                    mods.extend(r.modifications);
                    last = r.output;
                    if r.success {
                        any = true;
                    }
                }
                Err(e) => last = e.to_string(),
            }
        }
        ActionResult {
            success: any,
            output: last,
            modifications: mods,
        }
    }

    fn execute_and_judge(
        &self,
        strategy: &super::types::HealStrategy,
        context: &HealContext,
    ) -> HealOutcome {
        match self.execute_action(&strategy.action, context) {
            Ok(r) if r.success => HealOutcome::Healed {
                action_taken: strategy.name.clone(),
                modifications: r.modifications,
            },
            Ok(r) => HealOutcome::Degraded {
                reason: format!("Strategy '{}' did not resolve: {}", strategy.name, r.output),
                fallback_description: strategy.description.clone(),
            },
            Err(e) => HealOutcome::Unhealable {
                reason: format!("Strategy '{}' failed: {}", strategy.name, e),
                suggestion: strategy.description.clone(),
                requires_code_change: matches!(
                    strategy.action,
                    HealAction::ProposeCodeChange { .. }
                ),
                debug_log: MiniDebugLog {
                    attempt_count: 1,
                    reg_spans: vec!["reg.heal.strategy".into()],
                    actions_taken: vec![DebugLogAction {
                        name: strategy.name.clone(),
                        output: e.to_string(),
                        success: false,
                    }],
                    suggestion: strategy.description.clone(),
                    ..Default::default()
                },
            },
        }
    }

    fn render_classify_template(&self, ctx: &HealContext) -> Result<String, HealError> {
        let content = CLASSIFY_TEMPLATE_CACHE.get_or_init(|| {
            std::fs::read_to_string(CLASSIFY_TEMPLATE).unwrap_or_else(|_| String::new())
        });
        if content.is_empty() {
            return Err(HealError::TemplateNotFound);
        }
        self.render_template_content(content, ctx)
    }

    fn render_template_content(
        &self,
        content: &str,
        ctx: &HealContext,
    ) -> Result<String, HealError> {
        let mut vars: HashMap<String, String> = HashMap::new();
        vars.insert("operation".into(), ctx.operation.clone());
        vars.insert("error".into(), ctx.error_message.clone());
        vars.insert("error_message".into(), ctx.error_message.clone());
        vars.insert(
            "env_hints".into(),
            ctx.env_vars
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        vars.insert(
            "config_search_paths".into(),
            ctx.config_search_paths
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(":"),
        );

        let mut env = minijinja::Environment::new();
        env.set_undefined_behavior(minijinja::UndefinedBehavior::Lenient);
        env.add_template("tpl", content)
            .map_err(|e| HealError::TemplateRender(format!("Invalid template: {e}")))?;
        let cv = serde_json::to_value(&vars)
            .map_err(|e| HealError::TemplateRender(format!("Serialize: {e}")))?;
        let jc = minijinja::Value::from_serialize(&cv);
        env.get_template("tpl")
            .and_then(|t| t.render(jc))
            .map_err(|e| HealError::TemplateRender(format!("Render: {e}")))
    }

    #[allow(unsafe_code)]
    pub(crate) fn execute_action(
        &self,
        action: &HealAction,
        context: &HealContext,
    ) -> Result<ActionResult, HealError> {
        match action {
            HealAction::RunCommand {
                command,
                capture_output,
            } => {
                let out = Command::new("sh")
                    .arg("-c")
                    .arg(command)
                    .output()
                    .map_err(|e| HealError::Command(format!("{}: {}", command, e)))?;
                let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
                let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                Ok(ActionResult {
                    success: out.status.success(),
                    output: if *capture_output { stdout } else { stderr },
                    modifications: vec![format!("Ran: {}", command)],
                })
            }
            HealAction::SetEnv { key, value_source } => {
                let value = resolve_env_value(value_source)?;
                if value.is_empty() {
                    return Ok(ActionResult {
                        success: false,
                        output: format!("Could not resolve '{}'", key),
                        modifications: vec![],
                    });
                }
                // SAFETY: set_var is unsafe due to potential data races. This
                // runs during startup self-healing (single-threaded), before the
                // multi-threaded runtime and MCP servers begin. No concurrent env
                // access is possible at this point.
                unsafe { std::env::set_var(key, &value) };
                tracing::info!(target: "reg.heal.set_env", key = %key);
                Ok(ActionResult {
                    success: true,
                    output: format!("Set {}", key),
                    modifications: vec![format!("Set env: {}", key)],
                })
            }
            HealAction::LoadDotEnv { search_paths } => {
                let mut loaded = false;
                let mut mods = Vec::new();
                for path in search_paths {
                    if path.exists() && dotenvy::from_path(path).is_ok() {
                        loaded = true;
                        mods.push(format!("Loaded .env from: {}", path.display()));
                        tracing::info!(target: "reg.heal.dotenv", path = %path.display());
                    }
                }
                Ok(ActionResult {
                    success: loaded,
                    output: if loaded {
                        "Loaded .env".into()
                    } else {
                        "No .env found".into()
                    },
                    modifications: mods,
                })
            }
            HealAction::CreateDefaultFile { path, content } => {
                if path.exists() {
                    return Ok(ActionResult {
                        success: true,
                        output: format!("Already exists: {}", path.display()),
                        modifications: vec![],
                    });
                }
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| HealError::Io(format!("mkdir {}: {}", parent.display(), e)))?;
                }
                std::fs::write(path, content)
                    .map_err(|e| HealError::Io(format!("write {}: {}", path.display(), e)))?;
                tracing::info!(target: "reg.heal.file_created", path = %path.display());
                Ok(ActionResult {
                    success: true,
                    output: format!("Created: {}", path.display()),
                    modifications: vec![format!("Created: {}", path.display())],
                })
            }
            HealAction::RetryWithBackoff {
                max_attempts,
                delay_ms,
            } => Ok(ActionResult {
                success: true,
                output: format!("Retry: {} attempts, {}ms base", max_attempts, delay_ms),
                modifications: vec![],
            }),
            HealAction::ProposeCodeChange {
                file,
                description,
                diff_suggestion,
            } => {
                tracing::warn!(target: "reg.heal.code_change_proposed", file = %file.display(), description = %description, suggestion = %diff_suggestion);
                Ok(ActionResult {
                    success: false,
                    output: format!("Proposed: {} — {}", file.display(), description),
                    modifications: vec![format!("Proposed: {}", description)],
                })
            }
            HealAction::Sequence(actions) => {
                let mut mods = Vec::new();
                let mut last = String::new();
                let mut any = false;
                for a in actions {
                    match self.execute_action(a, context) {
                        Ok(r) => {
                            mods.extend(r.modifications);
                            last = r.output;
                            if r.success {
                                any = true;
                            }
                        }
                        Err(e) => last = e.to_string(),
                    }
                }
                Ok(ActionResult {
                    success: any,
                    output: last,
                    modifications: mods,
                })
            }
            HealAction::LlmAssisted { template_path } => {
                let inference = self.inference.as_ref().ok_or(HealError::NoInference)?;
                let content = std::fs::read_to_string(template_path).map_err(|e| {
                    HealError::Io(format!("Read {}: {}", template_path.display(), e))
                })?;
                let prompt = self.render_template_content(&content, context)?;
                tracing::info!(target: "reg.heal.llm_assisted", template = %template_path.display(), operation = %context.operation);
                let response = inference(&prompt)
                    .map_err(|e| HealError::Inference(format!("Inference: {}", e)))?;
                let instructions: Vec<HealInstruction> = parse_llm_response(&response)?;
                if instructions.is_empty() {
                    return Ok(ActionResult {
                        success: false,
                        output: "LLM returned no instructions".into(),
                        modifications: vec![],
                    });
                }
                Ok(self.execute_llm_instructions(&instructions, context))
            }
        }
    }
}

impl Default for SelfHealer {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for SelfHealer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SelfHealer")
            .field("strategies", &self.registry.len())
            .field("has_inference", &self.inference.is_some())
            .finish()
    }
}
