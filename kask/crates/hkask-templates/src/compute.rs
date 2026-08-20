//! Deterministic compute primitives for `action: compute` steps.
//!
//! Only generic primitives live here. Domain-specific computation belongs in
//! MCP servers — use `action: execute` with the appropriate MCP tool.

use crate::ports::TemplateError;
use serde_json::Value;

type Result<T> = std::result::Result<T, TemplateError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComputeRef {
    LispEval,
    ShellExec,
}

impl ComputeRef {
    pub fn parse(s: &str) -> Result<Self> {
        match s.trim() {
            "lisp.eval" => Ok(ComputeRef::LispEval),
            "shell.exec" => Ok(ComputeRef::ShellExec),
            other => Err(TemplateError::Manifest(format!(
                "Unknown compute_ref: '{other}'. Supported: lisp.eval, shell.exec."
            ))),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ComputeRef::LispEval => "lisp.eval",
            ComputeRef::ShellExec => "shell.exec",
        }
    }
}

pub async fn dispatch_compute(compute_ref: &str, input: &Value) -> Result<Value> {
    match ComputeRef::parse(compute_ref)? {
        ComputeRef::LispEval => {
            let form = input
                .get("form")
                .and_then(|v| v.as_str())
                .ok_or_else(|| TemplateError::Manifest("lisp.eval: missing 'form'".into()))?;
            let env = input.get("env").cloned().unwrap_or(Value::Null);
            let max_steps = input
                .get("max_steps")
                .and_then(|v| v.as_u64())
                .unwrap_or(100000);
            let max_depth = input
                .get("max_depth")
                .and_then(|v| v.as_u64())
                .unwrap_or(64);
            let result = hkask_lisp::eval_sandboxed_with_budget(form, &env, max_steps, max_depth)
                .map_err(|e| TemplateError::Manifest(format!("lisp.eval: {e}")))?;
            Ok(result)
        }
        ComputeRef::ShellExec => {
            let command = input
                .get("command")
                .and_then(|v| v.as_str())
                .ok_or_else(|| TemplateError::Manifest("shell.exec: missing 'command'".into()))?;
            let cwd = input.get("cwd").and_then(|v| v.as_str()).unwrap_or(".");
            let output = tokio::process::Command::new("sh")
                .arg("-c")
                .arg(command)
                .current_dir(cwd)
                .output()
                .await
                .map_err(|e| TemplateError::Manifest(format!("shell.exec: {e}")))?;
            Ok(serde_json::json!({
                "stdout": String::from_utf8_lossy(&output.stdout),
                "stderr": String::from_utf8_lossy(&output.stderr),
                "exit_code": output.status.code(),
            }))
        }
    }
}
