//! Parent-owned IPC grants. Configuration selects permissions; child requests
//! only carry an opaque reference and may further restrict the selected grant.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

/// A host effect, explicitly selected in the same parent-owned grant registry.
pub(crate) const WORKTREE_SPAWN: &str = "host/create_worktree_thread";
struct Grant {
    token: String,
    tools: Vec<String>,
}
static GRANTS: LazyLock<Mutex<HashMap<String, Grant>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub(crate) fn grant_for_server(server: &str, tools: &[String]) -> Option<String> {
    let mut tools = tools.to_vec();
    tools.sort();
    tools.dedup();
    let valid = tools.iter().all(|tool| {
        tool.split_once('/').is_some_and(|(server, name)| {
            !server.is_empty() && !name.is_empty() && !tool.contains('*')
        })
    });
    let Ok(mut grants) = GRANTS.lock() else {
        tracing::warn!(target: "reg.inference", "Delegation registry poisoned; refusing grant");
        return None;
    };
    if tools.is_empty() || !valid {
        grants.remove(server);
        if !valid {
            tracing::warn!(target: "reg.inference", server, "Invalid delegated_tools entry; exact server/tool names required");
        }
        return None;
    }
    if let Some(existing) = grants
        .get(server)
        .filter(|existing| existing.tools == tools)
    {
        return Some(existing.token.clone());
    }
    let token = uuid::Uuid::new_v4().to_string();
    grants.insert(
        server.to_string(),
        Grant {
            token: token.clone(),
            tools,
        },
    );
    Some(token)
}

/// Revoke the child grant before an operator-requested unload.
pub fn revoke_delegation_grant(server: &str) {
    match GRANTS.lock() {
        Ok(mut grants) => {
            grants.remove(server);
        }
        Err(_) => {
            tracing::warn!(target: "reg.inference", "Delegation registry poisoned; all dispatch is denied")
        }
    }
}

/// Derive the child's ceiling from the parent grant and the requested narrowing.
/// Missing narrowing is not an implicit grant of the parent's whole tool set.
pub(crate) fn worktree_tools(
    token: Option<&str>,
    requested: Option<&[String]>,
) -> Option<Vec<String>> {
    let token = token?;
    let requested = requested?;
    let grants = GRANTS.lock().ok()?;
    let grant = grants.values().find(|grant| grant.token == token)?;
    if !grant.tools.iter().any(|tool| tool == WORKTREE_SPAWN) {
        return None;
    }
    let mut tools: Vec<_> = requested
        .iter()
        .filter(|tool| tool.as_str() != WORKTREE_SPAWN && grant.tools.contains(tool))
        .cloned()
        .collect();
    tools.sort();
    tools.dedup();
    Some(tools)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grants_are_stable_until_changed_and_revoked() {
        let server = format!("test-{}", uuid::Uuid::new_v4());
        let tools = vec![WORKTREE_SPAWN.to_string(), "research/search".to_string()];
        let token = grant_for_server(&server, &tools).expect("grant");
        assert_eq!(grant_for_server(&server, &tools), Some(token.clone()));
        // The requested narrowing is intersected with the grant; a
        // non-granted tool drops out, never widens.
        assert_eq!(
            worktree_tools(Some(&token), Some(&["research/search".to_string()])),
            Some(vec!["research/search".to_string()])
        );
        assert_eq!(
            worktree_tools(Some(&token), Some(&["ledger/debit".to_string()])),
            Some(Vec::new())
        );
        // No token or no narrowing request: no ceiling, never grant-all.
        assert_eq!(
            worktree_tools(None, Some(&["research/search".to_string()])),
            None
        );
        let replacement_tools = vec![WORKTREE_SPAWN.to_string(), "research/read".to_string()];
        let replacement = grant_for_server(&server, &replacement_tools).expect("replacement");
        // A changed tool list replaces the grant: the old token resolves to
        // no grant at all, so its ceiling is None.
        assert_eq!(
            worktree_tools(Some(&token), Some(&["research/search".to_string()])),
            None
        );
        assert_eq!(
            worktree_tools(Some(&replacement), Some(&["research/read".to_string()])),
            Some(vec!["research/read".to_string()])
        );
        revoke_delegation_grant(&server);
        assert_eq!(
            worktree_tools(Some(&replacement), Some(&["research/read".to_string()])),
            None
        );
    }
}
