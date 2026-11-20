use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Host-owned tool identities, never model-facing aliases.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum DelegatedToolIdentity {
    Builtin(String),
    Mcp { server: String, tool: String },
}

/// A persisted, monotonically narrowing tool ceiling, independent of profiles.
/// This does not turn natural-language instructions into an OS sandbox.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelegationAuthority {
    tools: BTreeSet<DelegatedToolIdentity>,
}

impl DelegationAuthority {
    /// The bridge supplies exact qualified MCP names. Bare names and wildcards
    /// grant nothing, and can never authorize a builtin with the same name.
    pub fn from_mcp_tools(names: &[String]) -> Self {
        Self::from_identities(names.iter().filter_map(|name| {
            let (server, tool) = name.split_once('/')?;
            if server.is_empty() || tool.is_empty() || tool.contains('/') || name.contains('*') {
                return None;
            }
            Some(DelegatedToolIdentity::Mcp {
                server: server.to_owned(),
                tool: tool.to_owned(),
            })
        }))
    }

    pub(crate) fn from_identities(tools: impl IntoIterator<Item = DelegatedToolIdentity>) -> Self {
        Self {
            tools: tools.into_iter().collect(),
        }
    }

    pub(crate) fn allows(&self, identity: &DelegatedToolIdentity) -> bool {
        // Match the native depth-one delegation limit for siblings as well.
        // The kanban route has only server-scoped IPC identity until P2; a
        // restricted thread cannot use it to exchange its ceiling for that
        // server's broader grant.
        let delegates = match identity {
            DelegatedToolIdentity::Builtin(name) => {
                matches!(name.as_str(), "create_thread" | "spawn_agent")
            }
            DelegatedToolIdentity::Mcp { server, tool } => {
                server == "kata-kanban" && tool == "kanban_task_spawn"
            }
        };
        !delegates && self.tools.contains(identity)
    }

    pub(crate) fn intersect(&mut self, other: &Self) {
        self.tools.retain(|tool| other.tools.contains(tool));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// expect: [P1] host MCP grants cannot authorize builtins, aliases, or other servers.
    #[test]
    fn delegation_exact_mcp_identity() {
        let authority = DelegationAuthority::from_mcp_tools(&[
            "server/read-file".into(),
            "terminal".into(),
            "*/tool".into(),
        ]);
        assert!(authority.allows(&DelegatedToolIdentity::Mcp {
            server: "server".into(),
            tool: "read-file".into(),
        }));
        for identity in [
            DelegatedToolIdentity::Builtin("read-file".into()),
            DelegatedToolIdentity::Builtin("terminal".into()),
            DelegatedToolIdentity::Mcp {
                server: "other".into(),
                tool: "read-file".into(),
            },
            DelegatedToolIdentity::Mcp {
                server: "server".into(),
                tool: "read_file".into(),
            },
        ] {
            assert!(!authority.allows(&identity));
        }
    }

    /// expect: [P1] persistence and repeated delegation cannot enlarge authority.
    #[test]
    fn delegation_roundtrip_and_attenuation() -> anyhow::Result<()> {
        let mut authority =
            DelegationAuthority::from_mcp_tools(&["a/read".into(), "b/read".into()]);
        authority.intersect(&DelegationAuthority::from_mcp_tools(&[
            "a/read".into(),
            "c/write".into(),
        ]));
        let restored: DelegationAuthority =
            serde_json::from_slice(&serde_json::to_vec(&authority)?)?;
        assert_eq!(
            restored,
            DelegationAuthority::from_mcp_tools(&["a/read".into()])
        );
        authority.intersect(&DelegationAuthority::default());
        assert_eq!(authority, DelegationAuthority::default());
        Ok(())
    }

    /// expect: [P1] children cannot amplify depth or exchange their ceiling for a server grant.
    #[test]
    fn delegation_cannot_spawn_through_another_route() {
        let identities = [
            DelegatedToolIdentity::Builtin("create_thread".into()),
            DelegatedToolIdentity::Builtin("spawn_agent".into()),
            DelegatedToolIdentity::Mcp {
                server: "kata-kanban".into(),
                tool: "kanban_task_spawn".into(),
            },
        ];
        let authority = DelegationAuthority::from_identities(identities.clone());
        assert!(
            identities
                .iter()
                .all(|identity| !authority.allows(identity))
        );
    }
}
