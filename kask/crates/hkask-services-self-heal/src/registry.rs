//! HealRegistry — strategy catalog and matching.

use std::path::PathBuf;

use super::types::{HealAction, HealStrategy};

#[derive(Debug, Clone, Default)]
pub struct HealRegistry {
    pub(crate) strategies: Vec<HealStrategy>,
}

impl HealRegistry {
    #[must_use]
    pub fn with_defaults() -> Self {
        let mut r = Self::default();
        r.add(HealStrategy {
            name: "missing-api-key".into(),
            error_pattern: "No API key".into(),
            description: "API key not found — try loading from .env files".into(),
            action: HealAction::LoadDotEnv {
                search_paths: vec![
                    PathBuf::from(".env"),
                    PathBuf::from("../.env"),
                    PathBuf::from("../../.env"),
                    dirs::home_dir()
                        .unwrap_or_default()
                        .join(".config/hkask/.env"),
                ],
            },
        });
        r.add(HealStrategy {
            name: "permission-denied".into(),
            error_pattern: "Permission denied".into(),
            description: "Permission denied — check filesystem".into(),
            action: HealAction::ProposeCodeChange {
                file: PathBuf::from("(runtime)"),
                description: "Permission denied".into(),
                diff_suggestion: "Check with `ls -la` and `chmod` or `sudo` as needed.".into(),
            },
        });
        r.add(HealStrategy {
            name: "command-not-found".into(),
            error_pattern: "command not found".into(),
            description: "Required binary not installed".into(),
            action: HealAction::ProposeCodeChange {
                file: PathBuf::from("(environment)"),
                description: "Command not found in PATH".into(),
                diff_suggestion: "Install via apt-get, brew, or cargo install.".into(),
            },
        });
        r.add(HealStrategy {
            name: "config-file-not-found".into(),
            error_pattern: "Failed to read classifier config".into(),
            description: "Classifier config missing".into(),
            action: HealAction::ProposeCodeChange {
                file: PathBuf::from("registry/classify/"),
                description: "Classifier config YAML not found".into(),
                diff_suggestion: "Create registry/classify/<name>.yaml".into(),
            },
        });
        r.add(HealStrategy {
            name: "network-error".into(),
            error_pattern: "connection refused".into(),
            description: "Network error — retry with backoff".into(),
            action: HealAction::RetryWithBackoff {
                max_attempts: 3,
                delay_ms: 2000,
            },
        });
        r.add(HealStrategy {
            name: "transient-retry".into(),
            error_pattern: "timeout|timed out|temporary failure|500|502|503|rate limit".into(),
            description: "Transient failure — retry with backoff".into(),
            action: HealAction::RetryWithBackoff {
                max_attempts: 3,
                delay_ms: 1000,
            },
        });
        r
    }

    pub fn add(&mut self, s: HealStrategy) {
        self.strategies.push(s);
    }

    #[must_use]
    pub fn find_strategy(&self, error: &str) -> Option<&HealStrategy> {
        let lower = error.to_lowercase();
        self.strategies.iter().find(|s| {
            s.error_pattern
                .to_lowercase()
                .split('|')
                .any(|w| lower.contains(w.trim()))
        })
    }

    #[must_use]
    pub fn find_strategy_by_name(&self, name: &str) -> Option<&HealStrategy> {
        self.strategies.iter().find(|s| s.name == name)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.strategies.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.strategies.is_empty()
    }
}
