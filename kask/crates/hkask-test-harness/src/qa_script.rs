//! QA Script Runner — parses and executes QA manifest YAML files.
//!
//! The runner is an S3* audit probe: it bypasses cached Regulation state to directly
//! verify system health through contract tests, classifier triage, and MCP
//! server dispatch.
//!
//! # Architecture
//! - Parse: deserialize YAML → `QaScriptManifest`
//! - Validate: check branch targets exist, no duplicate ordinals
//! - Execute: walk steps in ordinal order, follow branching
//!
//! # Principle grounding
//! - P5 (Essentialism): one module, one public function (`run_script`)
//! - P4 (Clear Boundaries): errors are values, not panics
//! - Hoare #1: `Step` enum makes invalid actions unrepresentable at parse time

use serde::Deserialize;
use std::collections::HashSet;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use thiserror::Error;
use tracing as _;

// ── Error type ──────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum RunnerError {
    #[error("failed to read manifest: {0}")]
    Io(#[from] std::io::Error),
    #[error("manifest parse error: {0}")]
    Parse(#[from] serde_yaml_neo::Error),
    #[error("step {ordinal}: branch target {target} not found (valid ordinals: {valid:?})")]
    BranchTargetNotFound {
        ordinal: u32,
        target: u32,
        valid: Vec<u32>,
    },
    #[error("step {ordinal}: duplicate ordinal")]
    DuplicateOrdinal { ordinal: u32 },
    #[error("step {ordinal}: command failed (exit {exit_code}): {stderr}")]
    CommandFailed {
        ordinal: u32,
        exit_code: i32,
        stderr: String,
    },
    #[error("step {ordinal}: classifier '{classifier}' unavailable: {reason}")]
    ClassifierUnavailable {
        ordinal: u32,
        classifier: String,
        reason: String,
    },
    #[error("classifier API error: {0}")]
    ClassifierApiError(String),
    #[error("step {ordinal}: loop exhausted after {iterations} iterations")]
    LoopExhausted { ordinal: u32, iterations: u32 },
    #[error("step {ordinal}: MCP dispatch not yet supported (tool: {tool})")]
    McpNotSupported { ordinal: u32, tool: String },
    #[error("gas cap {cap} exceeded (used: {used})")]
    GasExceeded { cap: u64, used: u64 },
}

// ── Public output type ──────────────────────────────────────────────────────

/// Terminal status of a QA script run. Centralizes the "FAIL"-in-message
/// convention so call sites use a typed status instead of substring-matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QaStatus {
    Pass,
    Fail,
}

/// Error returned by an `McpDispatchFn` callback (replaces `Result<_, String>`).
#[derive(Debug, thiserror::Error)]
pub enum QaDispatchError {
    #[error("tool '{tool}' not found in any registered MCP server")]
    ToolNotFound { tool: String },
    #[error("MCP dispatch error: {message}")]
    DispatchError { message: String },
}

#[derive(Debug)]
pub struct ScriptOutput {
    pub manifest_id: String,
    pub terminal_ordinal: u32,
    pub terminal_message: String,
    pub status: QaStatus,
    pub steps_executed: u32,
    pub gas_used: u64,
}

// ── Manifest types (deserialized from YAML) ─────────────────────────────────

#[derive(Debug, Deserialize)]
struct ManifestMeta {
    id: String,
}

#[derive(Debug, Deserialize)]
struct GasConfig {
    cap: u64,
    #[serde(default = "default_true")]
    hard_limit: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum BranchTarget {
    Single(u32),
    Map(BranchMap),
}

#[derive(Debug, Deserialize, Default)]
struct BranchMap {
    #[serde(default)]
    success: Option<u32>,
    #[serde(default)]
    failure: Option<u32>,
    #[serde(default)]
    high_confidence: Option<u32>,
    #[serde(default)]
    medium_confidence: Option<u32>,
    #[serde(default)]
    low_confidence: Option<u32>,
    #[serde(default)]
    flake: Option<u32>,
    #[serde(default)]
    unparseable: Option<u32>,
    #[serde(default)]
    loop_exhausted: Option<u32>,
    #[serde(default)]
    classifier_unavailable: Option<u32>,
}

/// A single step in the QA manifest.
///
/// Uses `#[serde(tag = "action")]` so invalid action strings fail at parse
/// time rather than producing a runtime error (Hoare #1: make invalid states
/// unrepresentable).
#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum StepDef {
    #[serde(rename = "run_command")]
    RunCommand {
        ordinal: u32,
        command: String,
        #[serde(default)]
        terminal: bool,
        #[serde(default)]
        branching: Option<BranchTarget>,
    },
    #[serde(rename = "classify")]
    Classify {
        ordinal: u32,
        classifier: String,
        description: String,
        #[serde(default)]
        terminal: bool,
        #[serde(default)]
        branching: Option<BranchTarget>,
    },
    #[serde(rename = "loop")]
    Loop {
        ordinal: u32,
        #[serde(default)]
        command: Option<String>,
        max_iterations: u32,
        #[serde(default)]
        iteration_delay_secs: u64,
        #[serde(default)]
        terminal: bool,
        #[serde(default)]
        branching: Option<BranchTarget>,
    },
    #[serde(rename = "mcp_tool")]
    McpTool {
        ordinal: u32,
        tool_name: String,
        #[serde(default)]
        tool_params: String,
        #[serde(default)]
        terminal: bool,
        #[serde(default)]
        branching: Option<BranchTarget>,
    },
}

impl StepDef {
    fn ordinal(&self) -> u32 {
        match self {
            StepDef::RunCommand { ordinal, .. }
            | StepDef::Classify { ordinal, .. }
            | StepDef::Loop { ordinal, .. }
            | StepDef::McpTool { ordinal, .. } => *ordinal,
        }
    }

    fn is_terminal(&self) -> bool {
        match self {
            StepDef::RunCommand { terminal, .. }
            | StepDef::Classify { terminal, .. }
            | StepDef::Loop { terminal, .. }
            | StepDef::McpTool { terminal, .. } => *terminal,
        }
    }

    fn branching(&self) -> Option<&BranchTarget> {
        match self {
            StepDef::RunCommand { branching, .. }
            | StepDef::Classify { branching, .. }
            | StepDef::Loop { branching, .. }
            | StepDef::McpTool { branching, .. } => branching.as_ref(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct QaScriptManifest {
    manifest: ManifestMeta,
    #[serde(default)]
    gas: Option<GasConfig>,
    steps: Vec<StepDef>,
}

// ── Classifier config (from registry/classify/*.yaml) ───────────────────────

#[derive(Debug, Deserialize)]
struct ClassifierConfigFile {
    classifier: ClassifierConfig,
}

#[derive(Debug, Deserialize, Clone)]
struct ClassifierConfig {
    /// Provider-native model id. When empty, `load_classifier_config` resolves
    /// the canonical classifier model from `HKASK_CLASSIFIER_MODEL` →
    /// `DEFAULT_CLASSIFIER_MODEL` and strips the router prefix, so
    /// `registry/classify/*.yaml` can leave `model:` empty to defer to the
    /// single canonical path.
    #[serde(default)]
    model: String,
    #[serde(default)]
    system_prompt: String,
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    api_key_env: Option<String>,
    #[serde(default)]
    temperature: Option<f64>,
    #[serde(default)]
    max_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct ClassifyResponse {
    failure_type: String,
    root_cause: String,
    confidence: f64,
    #[serde(default)]
    proposed_fix: String,
    #[serde(default)]
    is_flake: bool,
}

// ── Runner state ────────────────────────────────────────────────────────────

struct RunnerState {
    gas_used: u64,
    gas_cap: u64,
    hard_limit: bool,
    previous_stdout: String,
    steps_executed: u32,
}

impl RunnerState {
    fn new(gas: Option<&GasConfig>) -> Self {
        let cap = gas.map(|g| g.cap).unwrap_or(u64::MAX);
        let hard = gas.map(|g| g.hard_limit).unwrap_or(false);
        Self {
            gas_used: 0,
            gas_cap: cap,
            hard_limit: hard,
            previous_stdout: String::new(),
            steps_executed: 0,
        }
    }

    fn charge(&mut self, amount: u64) -> Result<(), RunnerError> {
        self.gas_used += amount;
        if self.hard_limit && self.gas_used > self.gas_cap {
            Err(RunnerError::GasExceeded {
                cap: self.gas_cap,
                used: self.gas_used,
            })
        } else {
            Ok(())
        }
    }
}

// ── Parsing ─────────────────────────────────────────────────────────────────

fn load_manifest(path: &Path) -> Result<QaScriptManifest, RunnerError> {
    let content = std::fs::read_to_string(path)?;
    let manifest: QaScriptManifest = serde_yaml_neo::from_str(&content)?;
    Ok(manifest)
}

fn load_classifier_config(
    workspace_root: &Path,
    classifier_name: &str,
) -> Result<ClassifierConfig, RunnerError> {
    let path = workspace_root.join(format!("registry/classify/{}.yaml", classifier_name));
    let content =
        std::fs::read_to_string(&path).map_err(|e| RunnerError::ClassifierUnavailable {
            ordinal: 0,
            classifier: classifier_name.to_string(),
            reason: format!("cannot read {}: {}", path.display(), e),
        })?;
    let config_file: ClassifierConfigFile =
        serde_yaml_neo::from_str(&content).map_err(|e| RunnerError::ClassifierUnavailable {
            ordinal: 0,
            classifier: classifier_name.to_string(),
            reason: format!("invalid classifier config: {}", e),
        })?;
    let mut config = config_file.classifier;
    // Canonical model resolution: an empty `model:` defers to the single
    // canonical path (HKASK_CLASSIFIER_MODEL → DEFAULT_CLASSIFIER_MODEL).
    // Strip the router prefix so the raw provider-native id is sent to the API.
    if config.model.is_empty() {
        let canonical = hkask_inference::model_constants::classifier_model();
        config.model = match hkask_inference::ProviderId::parse_from_model(&canonical) {
            Some((_, raw)) => raw.to_string(),
            None => canonical,
        };
    }
    Ok(config)
}

// ── Validation ──────────────────────────────────────────────────────────────

fn validate(manifest: &QaScriptManifest) -> Result<(), RunnerError> {
    let ordinals: Vec<u32> = manifest.steps.iter().map(|s| s.ordinal()).collect();

    // Check for duplicate ordinals
    let mut seen = HashSet::new();
    for &o in &ordinals {
        if !seen.insert(o) {
            return Err(RunnerError::DuplicateOrdinal { ordinal: o });
        }
    }

    // Check all branch targets exist
    for step in &manifest.steps {
        if let Some(branching) = step.branching() {
            let targets = match branching {
                BranchTarget::Single(t) => vec![*t],
                BranchTarget::Map(m) => {
                    let mut v = Vec::new();
                    if let Some(t) = m.success {
                        v.push(t);
                    }
                    if let Some(t) = m.failure {
                        v.push(t);
                    }
                    if let Some(t) = m.high_confidence {
                        v.push(t);
                    }
                    if let Some(t) = m.medium_confidence {
                        v.push(t);
                    }
                    if let Some(t) = m.low_confidence {
                        v.push(t);
                    }
                    if let Some(t) = m.flake {
                        v.push(t);
                    }
                    if let Some(t) = m.unparseable {
                        v.push(t);
                    }
                    if let Some(t) = m.loop_exhausted {
                        v.push(t);
                    }
                    if let Some(t) = m.classifier_unavailable {
                        v.push(t);
                    }
                    v
                }
            };
            for &target in &targets {
                if !ordinals.contains(&target) {
                    return Err(RunnerError::BranchTargetNotFound {
                        ordinal: step.ordinal(),
                        target,
                        valid: ordinals.clone(),
                    });
                }
            }
        }
    }

    Ok(())
}

// ── Step execution ──────────────────────────────────────────────────────────

const GAS_RUN_COMMAND: u64 = 100;
const GAS_CLASSIFY: u64 = 500;
const GAS_LOOP_ITERATION: u64 = 100;
const GAS_MCP_TOOL: u64 = 200;

/// Resolve a branch target based on a string key.
fn resolve_branch(branching: &BranchTarget, key: &str) -> Option<u32> {
    match branching {
        BranchTarget::Single(t) => {
            if key == "success" || key == "failure" {
                Some(*t)
            } else {
                None
            }
        }
        BranchTarget::Map(m) => match key {
            "success" => m.success,
            "failure" => m.failure,
            "high_confidence" => m.high_confidence,
            "medium_confidence" => m.medium_confidence,
            "low_confidence" => m.low_confidence,
            "flake" => m.flake,
            "unparseable" => m.unparseable,
            "loop_exhausted" => m.loop_exhausted,
            "classifier_unavailable" => m.classifier_unavailable,
            _ => None,
        },
    }
}

/// Default command timeout (5 minutes).
const COMMAND_TIMEOUT_SECS: u64 = 300;

/// Execute a shell command with timeout and null-byte guard.
///
/// Uses `std::process::Command` (not `tokio::process::Command`) because
/// `run_script` is always called via `block_on` from a dedicated thread —
/// blocking the current thread is safe in this context. If `run_script`
/// were ever spawned as a concurrent tokio task, this would need to switch
/// to `tokio::process::Command` + `tokio::time::sleep`.
fn run_shell(command: &str) -> Result<std::process::Output, RunnerError> {
    if command.contains('\0') {
        return Err(RunnerError::CommandFailed {
            ordinal: 0,
            exit_code: -1,
            stderr: "command contains null byte".into(),
        });
    }
    let mut child = std::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| RunnerError::CommandFailed {
            ordinal: 0,
            exit_code: -1,
            stderr: e.to_string(),
        })?;

    let start = std::time::Instant::now();
    loop {
        match child.try_wait().map_err(|e| RunnerError::CommandFailed {
            ordinal: 0,
            exit_code: -1,
            stderr: e.to_string(),
        })? {
            Some(status) => {
                let mut stdout = Vec::new();
                let mut stderr = Vec::new();
                if let Some(mut out) = child.stdout.take() {
                    let _ = std::io::Read::read_to_end(&mut out, &mut stdout);
                }
                if let Some(mut err) = child.stderr.take() {
                    let _ = std::io::Read::read_to_end(&mut err, &mut stderr);
                }
                return Ok(std::process::Output {
                    status,
                    stdout,
                    stderr,
                });
            }
            None => {
                if start.elapsed().as_secs() > COMMAND_TIMEOUT_SECS {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(RunnerError::CommandFailed {
                        ordinal: 0,
                        exit_code: -1,
                        stderr: format!("command timed out after {}s", COMMAND_TIMEOUT_SECS),
                    });
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    }
}

fn execute_run_command(
    state: &mut RunnerState,
    step: &StepDef,
) -> Result<(String, Option<u32>), RunnerError> {
    state.charge(GAS_RUN_COMMAND)?;

    let command = match step {
        StepDef::RunCommand { command, .. } => command,
        _ => unreachable!(),
    };

    let output = run_shell(command).map_err(|e| RunnerError::CommandFailed {
        ordinal: step.ordinal(),
        exit_code: -1,
        stderr: e.to_string(),
    })?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    state.previous_stdout = stdout.clone();
    state.steps_executed += 1;

    if output.status.success() {
        let next = resolve_branch(
            step.branching().unwrap_or(&BranchTarget::Single(0)),
            "success",
        );
        Ok((stdout, next))
    } else {
        let next = resolve_branch(
            step.branching().unwrap_or(&BranchTarget::Single(0)),
            "failure",
        );
        let err = RunnerError::CommandFailed {
            ordinal: step.ordinal(),
            exit_code: output.status.code().unwrap_or(-1),
            stderr: stderr.trim().to_string(),
        };
        if next.is_none() {
            return Err(err);
        }
        eprintln!("[QA:run_command] step {} FAILED: {}", step.ordinal(), err);
        Ok((stdout, next))
    }
}

async fn execute_classify(
    workspace_root: &Path,
    state: &mut RunnerState,
    step: &StepDef,
) -> Result<(String, Option<u32>), RunnerError> {
    state.charge(GAS_CLASSIFY)?;

    let (classifier_name, description) = match step {
        StepDef::Classify {
            classifier,
            description,
            ..
        } => (classifier, description),
        _ => unreachable!(),
    };

    let config = match load_classifier_config(workspace_root, classifier_name) {
        Ok(c) => c,
        Err(e) => {
            let fallback = step
                .branching()
                .and_then(|b| resolve_branch(b, "classifier_unavailable"));
            if let Some(next) = fallback {
                let msg = format!(
                    "[QA:classify] classifier '{}' unavailable: {}",
                    classifier_name, e
                );
                eprintln!("{}", msg);
                state.previous_stdout = msg.clone();
                state.steps_executed += 1;
                return Ok((msg, Some(next)));
            }
            return Err(RunnerError::ClassifierUnavailable {
                ordinal: step.ordinal(),
                classifier: classifier_name.clone(),
                reason: e.to_string(),
            });
        }
    };

    let api_key = match std::env::var(config.api_key_env.as_deref().unwrap_or("DI_API_KEY")) {
        Ok(k) => k,
        Err(_) => {
            let fallback = step
                .branching()
                .and_then(|b| resolve_branch(b, "classifier_unavailable"));
            if let Some(next) = fallback {
                let msg = format!(
                    "[QA:classify] classifier '{}' unavailable: API key not set",
                    classifier_name
                );
                eprintln!("{}", msg);
                state.previous_stdout = msg.clone();
                state.steps_executed += 1;
                return Ok((msg, Some(next)));
            }
            return Err(RunnerError::ClassifierUnavailable {
                ordinal: step.ordinal(),
                classifier: classifier_name.clone(),
                reason: format!(
                    "API key env var '{}' not set",
                    config.api_key_env.as_deref().unwrap_or("DI_API_KEY")
                ),
            });
        }
    };

    let base_url = config
        .base_url
        .as_deref()
        .unwrap_or("https://api.deepinfra.com/v1/openai/chat/completions");

    // Build prompt: system prompt + step description + previous step output
    let user_prompt = format!(
        "{}\n\nPrevious step output:\n{}",
        description, state.previous_stdout
    );

    let body = serde_json::json!({
        "model": config.model,
        "messages": [
            {"role": "system", "content": config.system_prompt},
            {"role": "user", "content": user_prompt}
        ],
        "temperature": config.temperature.unwrap_or(0.0),
        "max_tokens": config.max_tokens.unwrap_or(500)
    });

    let client = reqwest::Client::new();
    let response = client
        .post(base_url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| RunnerError::ClassifierApiError(e.to_string()))?;

    let status = response.status();
    let raw_text = response
        .text()
        .await
        .map_err(|e| RunnerError::ClassifierApiError(e.to_string()))?;

    if !status.is_success() {
        return Err(RunnerError::ClassifierApiError(format!(
            "HTTP {}: {}",
            status,
            &raw_text[..raw_text.len().min(500)]
        )));
    }

    // Extract content from OpenAI-compatible response, then parse as ClassifyResponse
    let raw_json: serde_json::Value =
        serde_json::from_str(&raw_text).unwrap_or(serde_json::Value::Null);
    let content = raw_json["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or(&raw_text)
        .to_string();

    let classify_result: Result<ClassifyResponse, _> = serde_json::from_str(content.trim());

    let (branch_key, summary) = match classify_result {
        Ok(cr) => {
            let key = if cr.is_flake {
                "flake"
            } else if cr.confidence >= 0.95 {
                "high_confidence"
            } else if cr.confidence >= 0.70 {
                "medium_confidence"
            } else {
                "low_confidence"
            };
            let summary = format!(
                "[{} confidence={:.2}] {}: {}",
                cr.failure_type, cr.confidence, cr.root_cause, cr.proposed_fix
            );
            (key, summary)
        }
        Err(_) => ("unparseable", format!("[unparseable] {}", content)),
    };

    state.previous_stdout = summary.clone();
    state.steps_executed += 1;

    let next = step.branching().and_then(|b| resolve_branch(b, branch_key));

    Ok((summary, next))
}

async fn execute_loop(
    state: &mut RunnerState,
    step: &StepDef,
) -> Result<(String, Option<u32>), RunnerError> {
    let (command, max_iterations, delay_secs) = match step {
        StepDef::Loop {
            command,
            max_iterations,
            iteration_delay_secs,
            ..
        } => (
            command.as_deref().unwrap_or("echo 'loop_iteration'"),
            *max_iterations,
            *iteration_delay_secs,
        ),
        _ => unreachable!(),
    };

    for iteration in 1..=max_iterations {
        state.charge(GAS_LOOP_ITERATION)?;

        let output = run_shell(command).map_err(|e| RunnerError::CommandFailed {
            ordinal: step.ordinal(),
            exit_code: -1,
            stderr: e.to_string(),
        })?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();

        if output.status.success() {
            let summary = format!("[loop] succeeded on iteration {}", iteration);
            state.previous_stdout = summary.clone();
            state.steps_executed += 1;

            let next = step.branching().and_then(|b| resolve_branch(b, "success"));
            return Ok((summary, next));
        }

        eprintln!(
            "[QA:loop] iteration {}/{} failed, retrying...",
            iteration, max_iterations
        );
        state.previous_stdout = stdout;

        if delay_secs > 0 && iteration < max_iterations {
            tokio::time::sleep(std::time::Duration::from_secs(delay_secs)).await;
        }
    }

    // All iterations exhausted
    let summary = format!("[loop] exhausted after {} iterations", max_iterations);
    state.steps_executed += 1;
    let next = step
        .branching()
        .and_then(|b| resolve_branch(b, "loop_exhausted"));

    if next.is_none() {
        return Err(RunnerError::LoopExhausted {
            ordinal: step.ordinal(),
            iterations: max_iterations,
        });
    }

    Ok((summary, next))
}

async fn execute_mcp_tool(
    state: &mut RunnerState,
    step: &StepDef,
    mcp_dispatch: Option<&McpDispatchFn>,
) -> Result<(String, Option<u32>), RunnerError> {
    state.charge(GAS_MCP_TOOL)?;

    let (tool_name, tool_params) = match step {
        StepDef::McpTool {
            tool_name,
            tool_params,
            ..
        } => (tool_name, tool_params),
        _ => unreachable!(),
    };

    // Try MCP dispatch via callback, fall back to stub
    if let Some(dispatch) = mcp_dispatch {
        match dispatch(tool_name.clone(), tool_params.clone()).await {
            Ok(result) => {
                let next = step.branching().and_then(|b| resolve_branch(b, "success"));
                state.previous_stdout = result.clone();
                state.steps_executed += 1;
                return Ok((result, next));
            }
            Err(e) => {
                let next = step.branching().and_then(|b| resolve_branch(b, "failure"));
                if next.is_none() {
                    return Err(RunnerError::McpNotSupported {
                        ordinal: step.ordinal(),
                        tool: tool_name.clone(),
                    });
                }
                let msg = format!(
                    "[QA:mcp_tool] dispatch failed — tool: {} error: {} — routed to failure branch",
                    tool_name, e
                );
                eprintln!("{}", msg);
                state.previous_stdout = msg.clone();
                state.steps_executed += 1;
                return Ok((msg, next));
            }
        }
    }

    // No dispatcher provided — stub
    let next = step.branching().and_then(|b| resolve_branch(b, "failure"));

    if next.is_none() {
        return Err(RunnerError::McpNotSupported {
            ordinal: step.ordinal(),
            tool: tool_name.clone(),
        });
    }

    let msg = format!(
        "[QA:mcp_tool] MCP dispatch not available — tool: {} params: {} — routed to failure branch",
        tool_name, tool_params
    );
    eprintln!("{}", msg);
    state.previous_stdout = msg.clone();
    state.steps_executed += 1;
    Ok((msg, next))
}

// ── MCP dispatch callback ───────────────────────────────────────────────────

/// Async callback for MCP tool dispatch.
/// Passed by the CLI (which owns McpRuntime) to avoid circular dependencies.
pub type McpDispatchFn = Arc<
    dyn Fn(String, String) -> Pin<Box<dyn Future<Output = Result<String, QaDispatchError>> + Send>>
        + Send
        + Sync,
>;

// ── Main entry point ────────────────────────────────────────────────────────

/// Run a QA script manifest.
///
/// Parses the YAML manifest at `path`, validates branch integrity, then
/// executes steps in ordinal order following branch targets until a
/// terminal step is reached or an unrouteable error occurs.
///
/// # Arguments
/// - `path` — path to a QA manifest YAML file (e.g., `registry/manifests/qa-comm-integration-gate.yaml`)
///
/// # Returns
/// `ScriptOutput` with the terminal step's message, or a `RunnerError`.
///
/// pre:  path points to a valid QA manifest YAML file
/// post: steps are executed in ordinal order; terminal step output is returned
pub async fn run_script(
    workspace_root: &Path,
    manifest_path: &Path,
    mcp_dispatch: Option<McpDispatchFn>,
) -> Result<ScriptOutput, RunnerError> {
    let full_path = workspace_root.join(manifest_path);
    let manifest = load_manifest(&full_path)?;
    validate(&manifest)?;

    let manifest_id = manifest.manifest.id.clone();
    let step_count = manifest.steps.len();

    tracing::info!(
        target: "reg",
        reg_domain = %hkask_regulation::qa_span::QaSpan::QaRepairAttempted.as_str(),
        operation = "started",
        manifest = %manifest_id,
        step_count = step_count,
        "REG"
    );

    let mut state = RunnerState::new(manifest.gas.as_ref());
    let steps = manifest.steps;
    let step_map: std::collections::HashMap<u32, &StepDef> =
        steps.iter().map(|s| (s.ordinal(), s)).collect();

    let start_ordinal = steps.iter().map(|s| s.ordinal()).min().unwrap_or(1);
    let mut current_ordinal = start_ordinal;

    let outcome = loop {
        let step =
            step_map
                .get(&current_ordinal)
                .ok_or_else(|| RunnerError::BranchTargetNotFound {
                    ordinal: current_ordinal,
                    target: current_ordinal,
                    valid: step_map.keys().copied().collect(),
                })?;

        let (output, next_ordinal) = match step {
            StepDef::RunCommand { .. } => execute_run_command(&mut state, step)?,
            StepDef::Classify { .. } => execute_classify(workspace_root, &mut state, step).await?,
            StepDef::Loop { .. } => execute_loop(&mut state, step).await?,
            StepDef::McpTool { .. } => {
                execute_mcp_tool(&mut state, step, mcp_dispatch.as_ref()).await?
            }
        };

        if step.is_terminal() {
            break Ok(ScriptOutput {
                manifest_id: manifest_id.clone(),
                terminal_ordinal: step.ordinal(),
                status: if output.contains("FAIL") {
                    QaStatus::Fail
                } else {
                    QaStatus::Pass
                },
                terminal_message: output,
                steps_executed: state.steps_executed,
                gas_used: state.gas_used,
            });
        }

        match next_ordinal {
            Some(next) => current_ordinal = next,
            None => {
                break Ok(ScriptOutput {
                    manifest_id: manifest_id.clone(),
                    terminal_ordinal: step.ordinal(),
                    status: if output.contains("FAIL") {
                        QaStatus::Fail
                    } else {
                        QaStatus::Pass
                    },
                    terminal_message: output,
                    steps_executed: state.steps_executed,
                    gas_used: state.gas_used,
                });
            }
        }
    };

    match &outcome {
        Ok(o) => {
            let passed = o.status == QaStatus::Pass;
            let span = if passed {
                hkask_regulation::qa_span::QaSpan::QaRepairVerified
            } else {
                hkask_regulation::qa_span::QaSpan::QaRepairExhausted
            };
            tracing::info!(
                target: "reg",
                reg_domain = %span.as_str(),
                operation = if passed { "completed" } else { "failed" },
                manifest = %manifest_id,
                terminal = o.terminal_ordinal,
                steps = o.steps_executed,
                gas = o.gas_used,
                "REG"
            );
        }
        Err(e) => {
            tracing::info!(
                target: "reg",
                reg_domain = %hkask_regulation::qa_span::QaSpan::QaRepairExhausted.as_str(),
                operation = "error",
                manifest = %manifest_id,
                error = %e,
                "REG"
            );
        }
    }

    outcome
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp_manifest(yaml: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test-manifest.yaml");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(yaml.as_bytes()).unwrap();
        dir
    }

    fn path_in(dir: &tempfile::TempDir, name: &str) -> std::path::PathBuf {
        dir.path().join(name)
    }

    // ── Parsing tests ───────────────────────────────────────────────────

    #[test]
    fn parse_minimal_manifest() {
        let yaml = r#"
manifest:
  id: test-minimal
  name: Test Minimal
  version: "0.31.0"
  description: "A minimal test manifest"
  visibility: Private
steps:
  - ordinal: 1
    action: run_command
    command: "echo hello"
    description: "Say hello"
    terminal: true
"#;
        let dir = write_temp_manifest(yaml);
        let manifest = load_manifest(&path_in(&dir, "test-manifest.yaml")).unwrap();
        assert_eq!(manifest.manifest.id, "test-minimal");
        assert_eq!(manifest.steps.len(), 1);
        assert!(manifest.steps[0].is_terminal());
    }

    #[test]
    fn parse_manifest_with_classify_step() {
        let yaml = r#"
manifest:
  id: test-classify
  name: Test Classify
  version: "0.31.0"
steps:
  - ordinal: 1
    action: run_command
    command: "cargo test"
    branching:
      success: 2
      failure: 3
  - ordinal: 2
    action: classify
    classifier: qa-triage
    description: "Classify test output"
    branching:
      high_confidence: 3
      low_confidence: 3
  - ordinal: 3
    action: run_command
    command: "echo done"
    terminal: true
"#;
        let dir = write_temp_manifest(yaml);
        let manifest = load_manifest(&path_in(&dir, "test-manifest.yaml")).unwrap();
        assert_eq!(manifest.steps.len(), 3);

        // Verify branching was parsed
        let step1 = &manifest.steps[0];
        assert!(step1.branching().is_some());

        let step2 = &manifest.steps[1];
        match step2 {
            StepDef::Classify { classifier, .. } => {
                assert_eq!(classifier, "qa-triage");
            }
            _ => panic!("expected Classify step"),
        }
    }

    #[test]
    fn parse_manifest_with_loop_step() {
        let yaml = r#"
manifest:
  id: test-loop
  name: Test Loop
  version: "0.31.0"
steps:
  - ordinal: 1
    action: run_command
    command: "echo try"
    branching:
      success: 2
      failure: 1
  - ordinal: 2
    action: loop
    command: "echo retry"
    description: "Retry loop"
    max_iterations: 3
    branching:
      success: 3
      loop_exhausted: 4
  - ordinal: 3
    action: run_command
    command: "echo pass"
    terminal: true
  - ordinal: 4
    action: run_command
    command: "echo fail"
    terminal: true
"#;
        let dir = write_temp_manifest(yaml);
        let manifest = load_manifest(&path_in(&dir, "test-manifest.yaml")).unwrap();
        assert_eq!(manifest.steps.len(), 4);

        match &manifest.steps[1] {
            StepDef::Loop { max_iterations, .. } => {
                assert_eq!(*max_iterations, 3);
            }
            _ => panic!("expected Loop step"),
        }
    }

    #[test]
    fn parse_invalid_action_fails() {
        let yaml = r#"
manifest:
  id: test-bad
  name: Test Bad
  version: "0.31.0"
steps:
  - ordinal: 1
    action: nonexistent_action
    command: "echo hi"
"#;
        let dir = write_temp_manifest(yaml);
        let result = load_manifest(&path_in(&dir, "test-manifest.yaml"));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("nonexistent_action") || err.contains("unknown variant"));
    }

    // ── Validation tests ────────────────────────────────────────────────

    #[test]
    fn validate_rejects_duplicate_ordinals() {
        let yaml = r#"
manifest:
  id: test-dup
  name: Test Dup
  version: "0.31.0"
steps:
  - ordinal: 1
    action: run_command
    command: "echo one"
  - ordinal: 1
    action: run_command
    command: "echo also one"
"#;
        let dir = write_temp_manifest(yaml);
        let manifest = load_manifest(&path_in(&dir, "test-manifest.yaml")).unwrap();
        let err = validate(&manifest).unwrap_err();
        assert!(matches!(err, RunnerError::DuplicateOrdinal { ordinal: 1 }));
    }

    #[test]
    fn validate_rejects_missing_branch_target() {
        let yaml = r#"
manifest:
  id: test-branch
  name: Test Branch
  version: "0.31.0"
steps:
  - ordinal: 1
    action: run_command
    command: "echo hi"
    branching:
      success: 99
"#;
        let dir = write_temp_manifest(yaml);
        let manifest = load_manifest(&path_in(&dir, "test-manifest.yaml")).unwrap();
        let err = validate(&manifest).unwrap_err();
        assert!(matches!(
            err,
            RunnerError::BranchTargetNotFound {
                ordinal: 1,
                target: 99,
                ..
            }
        ));
    }

    #[test]
    fn validate_accepts_valid_branches() {
        let yaml = r#"
manifest:
  id: test-valid-branch
  name: Test Valid Branch
  version: "0.31.0"
steps:
  - ordinal: 1
    action: run_command
    command: "echo hi"
    branching:
      success: 2
      failure: 3
  - ordinal: 2
    action: run_command
    command: "echo pass"
    terminal: true
  - ordinal: 3
    action: run_command
    command: "echo fail"
    terminal: true
"#;
        let dir = write_temp_manifest(yaml);
        let manifest = load_manifest(&path_in(&dir, "test-manifest.yaml")).unwrap();
        assert!(validate(&manifest).is_ok());
    }

    // ── Execution tests ─────────────────────────────────────────────────

    #[test]
    fn execute_run_command_success() {
        let yaml = r#"
manifest:
  id: test-exec
  name: Test Exec
  version: "0.31.0"
steps:
  - ordinal: 1
    action: run_command
    command: "echo hello world"
    terminal: true
"#;
        let dir = write_temp_manifest(yaml);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt
            .block_on(run_script(
                Path::new("."),
                &path_in(&dir, "test-manifest.yaml"),
                None,
            ))
            .unwrap();
        assert_eq!(result.manifest_id, "test-exec");
        assert_eq!(result.terminal_ordinal, 1);
        assert!(result.terminal_message.contains("hello world"));
        assert_eq!(result.steps_executed, 1);
    }

    #[test]
    fn execute_run_command_failure_routes_to_failure_branch() {
        let yaml = r#"
manifest:
  id: test-fail-route
  name: Test Fail Route
  version: "0.31.0"
steps:
  - ordinal: 1
    action: run_command
    command: "exit 1"
    branching:
      success: 2
      failure: 3
  - ordinal: 2
    action: run_command
    command: "echo 'should not reach'"
    terminal: true
  - ordinal: 3
    action: run_command
    command: "echo 'routed to failure'"
    terminal: true
"#;
        let dir = write_temp_manifest(yaml);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt
            .block_on(run_script(
                Path::new("."),
                &path_in(&dir, "test-manifest.yaml"),
                None,
            ))
            .unwrap();
        assert_eq!(result.terminal_ordinal, 3);
        assert!(result.terminal_message.contains("routed to failure"));
    }

    #[test]
    fn execute_multi_step_sequence() {
        let yaml = r#"
manifest:
  id: test-multi
  name: Test Multi
  version: "0.31.0"
steps:
  - ordinal: 1
    action: run_command
    command: "echo step1"
    branching:
      success: 2
  - ordinal: 2
    action: run_command
    command: "echo step2"
    branching:
      success: 3
  - ordinal: 3
    action: run_command
    command: "echo step3"
    terminal: true
"#;
        let dir = write_temp_manifest(yaml);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt
            .block_on(run_script(
                Path::new("."),
                &path_in(&dir, "test-manifest.yaml"),
                None,
            ))
            .unwrap();
        assert_eq!(result.terminal_ordinal, 3);
        assert!(result.terminal_message.contains("step3"));
        assert_eq!(result.steps_executed, 3);
    }

    #[test]
    fn execute_loop_exhausts() {
        let yaml = r#"
manifest:
  id: test-loop-exec
  name: Test Loop Exec
  version: "0.31.0"
steps:
  - ordinal: 1
    action: loop
    command: "exit 1"
    description: "Always fails"
    max_iterations: 2
    branching:
      success: 2
      loop_exhausted: 3
  - ordinal: 2
    action: run_command
    command: "echo 'should not reach'"
    terminal: true
  - ordinal: 3
    action: run_command
    command: "echo 'loop exhausted'"
    terminal: true
"#;
        let dir = write_temp_manifest(yaml);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt
            .block_on(run_script(
                Path::new("."),
                &path_in(&dir, "test-manifest.yaml"),
                None,
            ))
            .unwrap();
        assert_eq!(result.terminal_ordinal, 3);
        assert!(result.terminal_message.contains("loop exhausted"));
    }

    #[test]
    fn execute_loop_succeeds_on_retry() {
        // Uses a temp file to track attempts — succeeds on 2nd try
        let dir = write_temp_manifest("");
        let tmpfile = dir.path().join("counter");
        let yaml = format!(
            r#"
manifest:
  id: test-loop-retry
  name: Test Loop Retry
  version: "0.31.0"
steps:
  - ordinal: 1
    action: loop
    command: >
      if [ -f "{tmpfile}" ]; then
        echo 'success'; exit 0;
      else
        touch "{tmpfile}"; echo 'first try fails'; exit 1;
      fi
    description: "Succeeds on retry"
    max_iterations: 3
    branching:
      success: 2
      loop_exhausted: 3
  - ordinal: 2
    action: run_command
    command: "echo 'loop succeeded'"
    terminal: true
  - ordinal: 3
    action: run_command
    command: "echo 'loop exhausted'"
    terminal: true
"#,
            tmpfile = tmpfile.display()
        );
        std::fs::write(path_in(&dir, "test-manifest.yaml"), yaml).unwrap();

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt
            .block_on(run_script(
                Path::new("."),
                &path_in(&dir, "test-manifest.yaml"),
                None,
            ))
            .unwrap();
        assert_eq!(result.terminal_ordinal, 2);
        assert!(result.terminal_message.contains("loop succeeded"));
    }

    #[test]
    fn execute_gas_exceeded_hard_limit() {
        let yaml = r#"
manifest:
  id: test-gas
  name: Test Gas
  version: "0.31.0"
gas:
  cap: 50
  hard_limit: true
steps:
  - ordinal: 1
    action: run_command
    command: "echo hi"
    branching:
      success: 2
  - ordinal: 2
    action: run_command
    command: "echo should not run"
    terminal: true
"#;
        let dir = write_temp_manifest(yaml);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(run_script(
            Path::new("."),
            &path_in(&dir, "test-manifest.yaml"),
            None,
        ));
        match result {
            Err(RunnerError::GasExceeded { cap, used }) => {
                assert_eq!(cap, 50);
                assert!(used >= 100); // first step costs 100
            }
            other => panic!("expected GasExceeded, got {:?}", other),
        }
    }

    #[test]
    fn execute_mcp_tool_routes_to_failure_branch() {
        let yaml = r#"
manifest:
  id: test-mcp-not-ready
  name: Test MCP Not Ready
  version: "0.31.0"
steps:
  - ordinal: 1
    action: mcp_tool
    tool_name: skill_ping
    tool_params: "{}"
    description: "Ping skill server"
    branching:
      success: 2
      failure: 3
  - ordinal: 2
    action: run_command
    command: "echo 'should not reach'"
    terminal: true
  - ordinal: 3
    action: run_command
    command: "echo 'mcp not supported yet'"
    terminal: true
"#;
        let dir = write_temp_manifest(yaml);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt
            .block_on(run_script(
                Path::new("."),
                &path_in(&dir, "test-manifest.yaml"),
                None,
            ))
            .unwrap();
        assert_eq!(result.terminal_ordinal, 3);
        assert!(result.terminal_message.contains("mcp not supported"));
    }

    #[test]
    fn execute_mcp_tool_without_failure_branch_errors() {
        let yaml = r#"
manifest:
  id: test-mcp-no-branch
  name: Test MCP No Branch
  version: "0.31.0"
steps:
  - ordinal: 1
    action: mcp_tool
    tool_name: skill_ping
    tool_params: "{}"
    description: "Ping skill server"
    terminal: true
"#;
        let dir = write_temp_manifest(yaml);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(run_script(
            Path::new("."),
            &path_in(&dir, "test-manifest.yaml"),
            None,
        ));
        assert!(matches!(result, Err(RunnerError::McpNotSupported { .. })));
    }

    // ── Real manifest integration tests ────────────────────────────────

    fn workspace_root() -> &'static str {
        // CARGO_MANIFEST_DIR points to crates/hkask-test-harness;
        // trim the crate subpath to get the workspace root.
        env!("CARGO_MANIFEST_DIR").trim_end_matches("/crates/hkask-test-harness")
    }

    /// Verify all real QA manifests parse and validate.
    #[test]
    fn all_real_manifests_parse_and_validate() {
        let manifest_dir = std::path::Path::new(workspace_root()).join("registry/manifests");
        let entries: Vec<_> = std::fs::read_dir(&manifest_dir)
            .unwrap_or_else(|_| panic!("cannot read {:?}", manifest_dir))
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("qa-"))
            .collect();

        assert!(
            !entries.is_empty(),
            "no QA manifests found in {:?}",
            manifest_dir
        );

        for entry in entries {
            let path = entry.path();
            let name = path.file_name().unwrap().to_string_lossy();
            let manifest =
                load_manifest(&path).unwrap_or_else(|e| panic!("{}: parse failed: {}", name, e));
            validate(&manifest).unwrap_or_else(|e| panic!("{}: validation failed: {}", name, e));
        }
    }

    /// Run the communication contract gate manifest end-to-end.
    /// Requires live Matrix transport — ignored in standard CI.
    #[test]
    #[ignore]
    fn run_comm_integration_gate() {
        let path = std::path::Path::new(workspace_root())
            .join("registry/manifests/qa-comm-integration-gate.yaml");
        assert!(path.exists(), "manifest not found at {:?}", path);

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt
            .block_on(run_script(
                Path::new(workspace_root()),
                Path::new("registry/manifests/qa-comm-integration-gate.yaml"),
                None,
            ))
            .unwrap();

        assert_eq!(result.manifest_id, "qa-comm-integration-gate");
        assert!(
            result.terminal_message.contains("PASS")
                || result.terminal_message.contains("FAIL")
                || result.terminal_message.contains("WARN"),
            "unexpected terminal message: {}",
            result.terminal_message
        );
        assert!(result.steps_executed >= 1);
    }

    /// Run the condenser health check manifest end-to-end.
    /// Requires live Condenser MCP server — ignored in standard CI.
    #[test]
    #[ignore]
    fn run_condenser_health_check() {
        let path = std::path::Path::new(workspace_root())
            .join("registry/manifests/qa-condenser-health-check.yaml");
        assert!(path.exists(), "manifest not found at {:?}", path);

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt
            .block_on(run_script(
                Path::new(workspace_root()),
                Path::new("registry/manifests/qa-condenser-health-check.yaml"),
                None,
            ))
            .unwrap();

        assert_eq!(result.manifest_id, "qa-condenser-health-check");
        assert!(result.steps_executed >= 1);
    }

    /// Run the keystore security gate manifest end-to-end.
    /// Requires live keystore — ignored in standard CI.
    #[test]
    #[ignore]
    fn run_keystore_security_gate() {
        let path = std::path::Path::new(workspace_root())
            .join("registry/manifests/qa-keystore-security-gate.yaml");
        assert!(path.exists(), "manifest not found at {:?}", path);

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt
            .block_on(run_script(
                Path::new(workspace_root()),
                Path::new("registry/manifests/qa-keystore-security-gate.yaml"),
                None,
            ))
            .unwrap();

        assert_eq!(result.manifest_id, "qa-keystore-security-gate");
        assert!(result.steps_executed >= 1);
    }

    /// Run the memory privacy boundary manifest end-to-end.
    /// Requires live memory system — ignored in standard CI.
    #[test]
    #[ignore]
    fn run_memory_privacy_boundary() {
        let path = std::path::Path::new(workspace_root())
            .join("registry/manifests/qa-memory-privacy-boundary.yaml");
        assert!(path.exists(), "manifest not found at {:?}", path);

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt
            .block_on(run_script(
                Path::new(workspace_root()),
                Path::new("registry/manifests/qa-memory-privacy-boundary.yaml"),
                None,
            ))
            .unwrap();

        assert_eq!(result.manifest_id, "qa-memory-privacy-boundary");
        assert!(result.steps_executed >= 1);
    }
}
