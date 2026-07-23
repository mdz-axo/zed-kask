//! Capability resources, actions, specs, and matching logic.

use serde::{Deserialize, Serialize};

/// Parsed colon-separated capability spec (e.g. `"tool:inference:call"`).
/// 2-part: `"resource:action"` → `resource_id = full string`. 3-part: `"resource:domain:action"` → `resource_id = domain`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilitySpec {
    pub resource: DelegationResource,
    pub resource_id: String,
    pub action: DelegationAction,
}

impl CapabilitySpec {
    /// Parse `"resource:action"` (2 parts) or `"resource:domain:action"` (3 parts).
    /// Unknown actions fall back to `Execute`. `"memory"` alias → `Registry`.
    pub fn parse(capability: &str) -> Result<Self, CapabilityParseError> {
        let parts: Vec<&str> = capability.split(':').collect();
        if parts.len() < 2 || parts.len() > 3 {
            return Err(CapabilityParseError::InvalidFormat(capability.to_string()));
        }
        let resource = DelegationResource::parse_str(parts[0])
            .ok_or_else(|| CapabilityParseError::UnknownResource(parts[0].to_string()))?;
        let resource_id = if parts.len() == 3 {
            parts[1].to_string()
        } else {
            capability.to_string()
        };
        let action =
            DelegationAction::parse_str(parts.last().expect("splitn always produces >=1 part"))
                .unwrap_or(DelegationAction::Execute);
        Ok(Self {
            resource,
            resource_id,
            action,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CapabilityParseError {
    #[error(
        "Invalid capability format: expected 'resource:action' or 'resource:domain:action', got '{0}'"
    )]
    InvalidFormat(String),
    #[error("Unknown resource type: '{0}'. Valid types: tool, template, registry, memory")]
    UnknownResource(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DelegationResource {
    Tool,
    Template,
    Registry,
    /// API key lifecycle management (issue, revoke, fund).
    Key,
}

impl DelegationResource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Tool => "tool",
            Self::Template => "template",
            Self::Registry => "registry",
            Self::Key => "key",
        }
    }
    pub fn parse_str(s: &str) -> Option<Self> {
        match s.split(':').next() {
            Some("tool") => Some(Self::Tool),
            Some("template") => Some(Self::Template),
            Some("registry") | Some("memory") => Some(Self::Registry),
            Some("key") => Some(Self::Key),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DelegationAction {
    Read,
    Write,
    Execute,
}

impl DelegationAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Execute => "execute",
        }
    }
    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "read" => Some(Self::Read),
            "write" => Some(Self::Write),
            "execute" => Some(Self::Execute),
            _ => None,
        }
    }
    /// `Write` and `Execute` grant write-level; `Read` is read-only.
    pub fn permits_write(&self) -> bool {
        !matches!(self, Self::Read)
    }
    /// All three actions grant read authority.
    pub fn permits_read(&self) -> bool {
        matches!(self, Self::Read | Self::Execute | Self::Write)
    }
}

/// Derive capability shorthand from MCP server ID.
///
/// Accepts both full binary-style IDs (`hkask-mcp-<domain>`) and short
/// BUILTIN_SERVERS IDs (`<domain>`). Non-mcp IDs (containing colons or
/// empty) return `None`.
pub fn capability_from_server_id(server_id: &str) -> Option<String> {
    if let Some(domain) = server_id.strip_prefix("hkask-mcp-") {
        return Some(format!("tool:{}:execute", domain));
    }
    // Short form from BUILTIN_SERVERS (e.g. "docproc", "memory")
    if !server_id.is_empty() && !server_id.contains(':') {
        return Some(format!("tool:{}:execute", server_id));
    }
    None
}

/// Check whether a token's capability covers a required capability.
/// Action hierarchy: Execute ≥ Write ≥ Read. Different domain → no match.
/// Unknown actions fall back to `Execute`. Falls back to exact string compare on parse failure.
pub fn capabilities_match(token_capability: &str, required_capability: &str) -> bool {
    let token_spec = match CapabilitySpec::parse(token_capability) {
        Ok(s) => s,
        Err(_) => return token_capability == required_capability,
    };
    let required_spec = match CapabilitySpec::parse(required_capability) {
        Ok(s) => s,
        Err(_) => return token_capability == required_capability,
    };

    // Different resource types never match (tool ≠ registry)
    if token_spec.resource != required_spec.resource {
        return false;
    }
    // Different domains never match (regulation ≠ semantic)
    if token_spec.resource_id != required_spec.resource_id {
        return false;
    }
    // Action hierarchy: Execute ≥ Write ≥ Read
    // Token's action must cover the required action
    match required_spec.action {
        DelegationAction::Read => token_spec.action.permits_read(),
        DelegationAction::Write => token_spec.action.permits_write(),
        DelegationAction::Execute => token_spec.action == DelegationAction::Execute,
    }
}
