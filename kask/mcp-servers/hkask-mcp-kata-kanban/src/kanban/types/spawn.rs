use super::*;

// ── Spawn Specification ─────────────────────────────────────────────────

/// SpawnSpec — configuration for spawning a sub-agent to execute a task.
///
/// Defines what capabilities (skills, memory scope, tool access) the parent
/// agent delegates to the spawned sub-agent. Spawning is consent-mediated
/// (P1) — the parent chooses what to delegate.
///
/// Delegation levels:
/// - Minimal: read-only access to the task, no memory, restricted tools
/// - Standard: read-write task access, episodic memory, kanban tools
/// - Maximal: full agent capabilities within the task scope
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpawnSpec {
    /// The task this spawn is for.
    pub task_id: TaskId,
    /// Delegation level: "minimal", "standard", or "maximal".
    pub(crate) delegation_level: String,
    /// Skills to delegate to the spawned agent.
    pub delegated_skills: Vec<String>,
    /// Memory scope: "none", "episodic", or "full".
    pub(crate) memory_scope: String,
    /// Tool servers accessible to the spawned agent.
    pub(crate) tool_servers: Vec<String>,
    /// Maximum time the spawned agent can run (seconds).
    pub(crate) timeout_seconds: Option<u64>,
    /// Template/skill registries accessible to the spawned agent.
    pub(crate) registries: Vec<String>,
    /// File paths or artifact roots the agent can access.
    pub(crate) artifacts: Vec<String>,
    /// The swarm this task belongs to, when the task is coordinated via a
    /// local swarm. Written to `Task.swarm_id` by `KanbanService::spawn_task`
    /// so `kanban_task_delegate_result` can return the durable link without
    /// depending on the runtime delegation path (worktree vs in-memory).
    /// `None` when the spawn is not scoped to a swarm.
    pub swarm_id: Option<String>,
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
            tool_servers: vec!["kata-kanban".into()],
            timeout_seconds: None,
            registries: Vec::new(),
            artifacts: Vec::new(),
            swarm_id: None,
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

    /// Set the swarm this task belongs to. Written to `Task.swarm_id` by
    /// `KanbanService::spawn_task` so the kanban board is the durable
    /// coordination source of truth linking a task to its swarm.
    #[must_use = "builder methods must be chained or assigned"]
    pub fn with_swarm(mut self, swarm_id: Option<String>) -> Self {
        self.swarm_id = swarm_id;
        self
    }
}
