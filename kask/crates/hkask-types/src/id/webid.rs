//! WebID — Unique identifier for agents.

use uuid::Uuid;

use super::core::BotID;

/// WebID — Unique identifier for agents
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct WebID(Uuid);

impl WebID {
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  (no inputs)
    /// post: returns a unique WebID wrapping a random UUID v4
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  uuid is any valid [`Uuid`]
    /// post: returns a [`WebID`] wrapping the given uuid unchanged
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is any valid [`WebID`]
    /// post: returns the inner [`Uuid`] unchanged
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }

    /// Convenience method: derive a WebID for an agent by name.
    ///
    /// Equivalent to `WebID::from_persona(agent_name.as_bytes())`.
    /// This is the canonical derivation for agent-name → WebID mapping
    /// used across CLI, API, REPL, and AgentService.
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  agent_name is non-empty
    /// post: returns a WebID deterministically derived from the agent name
    pub fn for_agent_name(agent_name: &str) -> Self {
        Self::from_persona(agent_name.as_bytes())
    }

    /// Derive WebID deterministically from persona using UUID v5
    ///
    /// Uses SHA-1 name-based UUID with a fixed namespace.
    /// Same persona bytes → same WebID.
    ///
    /// Note: This uses a default namespace. For namespace isolation,
    /// use `from_persona_with_namespace` instead.
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  persona_bytes is any non-empty byte slice (empty produces a deterministic but degenerate WebID)
    /// post: returns a [`WebID`] deterministically derived from persona_bytes using the default "hkask" namespace;
    ///       same persona_bytes → same WebID
    pub fn from_persona(persona_bytes: &[u8]) -> Self {
        Self::from_persona_with_namespace(persona_bytes, "hkask")
    }

    /// Derive WebID deterministically from persona with namespace isolation (R10)
    ///
    /// Uses SHA-1 name-based UUID with a fixed namespace.
    /// Combines namespace and persona bytes to prevent collisions across
    /// different agent registries.
    ///
    /// Same namespace + persona bytes → same WebID.
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  persona_bytes is any byte slice; namespace is any non-empty string
    /// post: returns a [`WebID`] deterministically derived; same (namespace, persona_bytes) → same WebID;
    ///       different namespace → different WebID (namespace isolation)
    pub fn from_persona_with_namespace(persona_bytes: &[u8], namespace: &str) -> Self {
        // Fixed namespace UUID for hKask personas
        // UUID: 686b6173-6b2d-7065-7273-6f6e612d6e73
        let base_namespace = Uuid::parse_str("686b6173-6b2d-7065-7273-6f6e612d6e73")
            .expect("Invalid namespace UUID");

        // Combine namespace and persona bytes to create isolated WebIDs
        let mut combined = Vec::with_capacity(namespace.len() + 1 + persona_bytes.len());
        combined.extend_from_slice(namespace.as_bytes());
        combined.push(b':');
        combined.extend_from_slice(persona_bytes);

        Self(Uuid::new_v5(&base_namespace, &combined))
    }

    /// Redacted display format — shows first 8 chars of UUID + "..."
    /// Use at INFO level and below to prevent full UUID leakage in logs.
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is any valid [`WebID`]
    /// post: returns a string of the form "XXXXXXXX..." where X are the first 8 hex characters of the inner UUID;
    ///       never reveals the full UUID
    pub fn redacted_display(&self) -> String {
        let hex = self.0.as_hyphenated().to_string();
        format!("{}...", &hex[..8])
    }
}

impl std::str::FromStr for WebID {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(s).map(WebID)
    }
}

impl Default for WebID {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for WebID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<BotID> for WebID {
    fn from(bot_id: BotID) -> Self {
        WebID(bot_id.as_uuid())
    }
}
