//! The LaunchPlan — the placement compiler's output and the future
//! supervisor's input (L4). The plan carries concrete flag spellings
//! resolved through the manifest vocabulary; `Display` renders the
//! command line. Emission stays typed Rust in v1 — the jinja
//! unification seam (cloud-leg deploy templates) is the named promotion
//! trigger, not a day-one layer (plan §10, delta 4).

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchPlan {
    pub surface_id: String,
    pub binary: String,
    pub host: String,
    pub port: u16,
    pub args: Vec<String>,
}

impl LaunchPlan {
    /// The loopback endpoint the plan serves — the supervisor's health
    /// probe and the future `openai_compatible` settings entry both use
    /// it.
    pub fn endpoint(&self) -> String {
        format!("http://{}:{}/v1", self.host, self.port)
    }
}

impl fmt::Display for LaunchPlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.binary)?;
        for arg in &self.args {
            write!(f, " {}", shell_quote(arg))?;
        }
        Ok(())
    }
}

/// Minimal shell quoting: wrap in single quotes when the argument carries
/// anything the shell would interpret.
fn shell_quote(arg: &str) -> String {
    if arg.is_empty() {
        return "''".into();
    }
    let safe = arg
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '-' | '_' | '=' | ':'));
    if safe {
        arg.to_string()
    } else {
        format!("'{}'", arg.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_the_command_line_and_quotes_paths() {
        let plan = LaunchPlan {
            surface_id: "test-surface".into(),
            binary: "llama-server".into(),
            host: "127.0.0.1".into(),
            port: 8010,
            args: vec![
                "--model".into(),
                "/models/my model (qwen).gguf".into(),
                "--ctx-size".into(),
                "65536".into(),
            ],
        };
        let rendered = plan.to_string();
        assert!(rendered.starts_with("llama-server --model"));
        assert!(rendered.contains("'/models/my model (qwen).gguf'"));
        assert_eq!(plan.endpoint(), "http://127.0.0.1:8010/v1");
    }
}
