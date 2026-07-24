use super::*;

// ── Spawn Specification ─────────────────────────────────────────────────

/// SpawnSpec — configuration for spawning a sub-userpod to execute a task.
///
/// Defines what capabilities (skills, memory scope, tool access) the parent
/// userpod delegates to the spawned sub-agent. Spawning is consent-mediated
/// (P1) — the parent chooses what to delegate.
///
/// Delegation levels:
/// - Minimal: read-only access to the task, no memory, restricted tools
/// - Standard: read-write task access, episodic memory, kanban tools
/// - Maximal: full userpod capabilities within the task scope
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpawnSpec {
    /// The task this spawn is for.
    pub task_id: TaskId,
    /// Delegation level: "minimal", "standard", or "maximal".
    pub(crate) delegation_level: String,
    /// Skills to delegate to the spawned userpod.
    pub delegated_skills: Vec<String>,
    /// Memory scope: "none", "episodic", or "full".
    pub(crate) memory_scope: String,
    /// Tool servers accessible to the spawned userpod.
    pub(crate) tool_servers: Vec<String>,
    /// Maximum gas/energy budget for the spawned userpod.
    pub(crate) gas_budget: Option<u64>,
    /// Maximum time the spawned userpod can run (seconds).
    pub(crate) timeout_seconds: Option<u64>,
    /// Template/skill registries accessible to the spawned userpod.
    pub(crate) registries: Vec<String>,
    /// File paths or artifact roots the agent can access.
    pub(crate) artifacts: Vec<String>,
    /// OCAP capability token specs (e.g. "tool:kanban:execute").
    /// These are validated against the parent userpod's tokens at spawn time.
    /// Each entry is a CapabilitySpec string: "resource:domain:action".
    pub(crate) capability_tokens: Vec<String>,
}

impl SpawnSpec {
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  task_id is valid
    /// post: returns a SpawnSpec with standard delegation defaults
    pub fn new(task_id: TaskId) -> Self {
        Self {
            task_id,
            delegation_level: "standard".into(),
            delegated_skills: vec!["kanban".into()],
            memory_scope: "episodic".into(),
            tool_servers: vec!["hkask-mcp-kata-kanban".into()],
            gas_budget: None,
            timeout_seconds: None,
            registries: Vec::new(),
            artifacts: Vec::new(),
            capability_tokens: Vec::new(),
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
