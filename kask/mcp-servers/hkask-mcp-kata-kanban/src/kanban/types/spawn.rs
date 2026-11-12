// ── Spawn Specification ─────────────────────────────────────────────────

/// SpawnSpec — what a spawn request asked for, recorded on the task as a
/// comment. The skills are named in the child's prompt; the level and the
/// memory scope are recorded only — nothing in this server or the spawn
/// port reads them to change what the child can do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SpawnSpec {
    /// Delegation level: "minimal", "standard", or "maximal".
    pub(crate) delegation_level: String,
    /// Skills named in the spawned agent's prompt.
    pub delegated_skills: Vec<String>,
    /// Memory scope: "none", "episodic", or "full".
    pub(crate) memory_scope: String,
}

impl SpawnSpec {
    /// post: returns a SpawnSpec with the standard defaults
    pub fn new() -> Self {
        Self {
            delegation_level: "standard".into(),
            delegated_skills: vec!["kanban".into()],
            memory_scope: "episodic".into(),
        }
    }

    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  value is a valid timeout
    /// post: returns Self with timeout set
    #[must_use = "builder methods must be chained or assigned"]
    pub fn with_level(mut self, level: &str) -> Self {
        self.delegation_level = level.into();
        self
    }

    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  value is valid for skills
    /// post: returns Self with skills set
    #[must_use = "builder methods must be chained or assigned"]
    pub fn with_skills(mut self, skills: Vec<String>) -> Self {
        self.delegated_skills = skills;
        self
    }

    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  value is valid for memory
    /// post: returns Self with memory set
    #[must_use = "builder methods must be chained or assigned"]
    pub fn with_memory(mut self, scope: &str) -> Self {
        self.memory_scope = scope.into();
        self
    }
}
