use super::*;

// ── Priority ────────────────────────────────────────────────────────────────

/// Priority level for kanban tasks.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    Low,
    Medium,
    High,
    Critical,
}

impl Priority {
    /// Every priority, lowest first.
    pub const ALL: [Priority; 4] = [
        Priority::Low,
        Priority::Medium,
        Priority::High,
        Priority::Critical,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Priority::Low => "low",
            Priority::Medium => "medium",
            Priority::High => "high",
            Priority::Critical => "critical",
        }
    }

    /// Parse a wire priority. Padding and wrapping quotes are tolerated,
    /// like every other closed vocabulary on this server (`TaskStatus`,
    /// verdict, kata stage).
    pub fn parse_str(s: &str) -> Option<Self> {
        match hkask_types::normalize_closed_vocab(s)
            .to_lowercase()
            .as_str()
        {
            "low" => Some(Priority::Low),
            "medium" | "med" => Some(Priority::Medium),
            "high" => Some(Priority::High),
            "critical" | "crit" => Some(Priority::Critical),
            _ => None,
        }
    }
}

impl std::fmt::Display for Priority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}
