//! Parent-owned IPC grants. Configuration selects permissions; child requests
//! only carry an opaque reference and may further restrict the selected grant.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

pub(crate) use hkask_types::inference_ipc::TOOL_GRANT_ENV as GRANT_ENV;
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

pub(crate) fn parent_allows(token: Option<&str>, qualified: &str) -> bool {
    let Some(token) = token else {
        return false;
    };
    let Ok(grants) = GRANTS.lock() else {
        return false;
    };
    grants
        .values()
        .any(|grant| grant.token == token && grant.tools.iter().any(|tool| tool == qualified))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grants_are_stable_until_changed_and_revoked() {
        let server = format!("test-{}", uuid::Uuid::new_v4());
        let token = grant_for_server(&server, &["research/search".into()]).expect("grant");
        assert_eq!(
            grant_for_server(&server, &["research/search".into()]),
            Some(token.clone())
        );
        assert!(parent_allows(Some(&token), "research/search"));
        assert!(!parent_allows(Some(&token), "ledger/debit"));
        assert!(!parent_allows(None, "research/search"));
        let replacement =
            grant_for_server(&server, &["research/read".into()]).expect("replacement");
        assert!(!parent_allows(Some(&token), "research/search"));
        assert!(parent_allows(Some(&replacement), "research/read"));
        revoke_delegation_grant(&server);
        assert!(!parent_allows(Some(&replacement), "research/read"));
    }
}
