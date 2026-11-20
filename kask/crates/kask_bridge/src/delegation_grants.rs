//! Parent-owned IPC grants. Configuration selects permissions; child requests
//! only carry an opaque reference and may further restrict the selected grant.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

/// A host effect, explicitly selected in the same parent-owned grant registry.
pub(crate) const WORKTREE_SPAWN: &str = "host/create_worktree_thread";
/// The host effect that admits a Nostr sign request (D106). Same registry,
/// same token; named in `hkask-types` so the wire and the editor agree.
pub(crate) const NOSTR_SIGN: &str = hkask_types::inference_ipc::NOSTR_SIGN_GRANT;
/// Host effect admitting `InferenceMethod::WalletDbUnlock` (D110).
pub(crate) const WALLET_DB_UNLOCK: &str = hkask_types::inference_ipc::WALLET_DB_UNLOCK_GRANT;
/// Host effect admitting `InferenceMethod::WalletSpendApprove` (D110).
pub(crate) const WALLET_SPEND_APPROVE: &str =
    hkask_types::inference_ipc::WALLET_SPEND_APPROVE_GRANT;
/// Host effect admitting Nostr zap/nutzap signing (D110, separate from
/// `NOSTR_SIGN` so `autopublish` can never apply to zaps).
pub(crate) const NOSTR_ZAP_SIGN: &str = hkask_types::inference_ipc::NOSTR_ZAP_SIGN_GRANT;
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

/// Whether the grant behind `token` names the host effect `effect`. A
/// missing token, a revoked or replaced grant, or a grant without the effect
/// is `false`; nothing here widens.
pub(crate) fn grant_holds(token: Option<&str>, effect: &str) -> bool {
    let Some(token) = token else {
        return false;
    };
    let Ok(grants) = GRANTS.lock() else {
        return false;
    };
    grants
        .values()
        .any(|grant| grant.token == token && grant.tools.iter().any(|tool| tool == effect))
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

    /// expect: the sign effect is held only by a live grant that names it.
    /// A grant for other tools, a worktree grant, no token, a replaced token
    /// and a revoked grant all answer `false`.
    #[test]
    fn the_sign_effect_is_held_only_by_a_live_grant_that_names_it() {
        let server = format!("test-{}", uuid::Uuid::new_v4());
        assert!(!grant_holds(None, NOSTR_SIGN));

        let other = grant_for_server(&server, &[WORKTREE_SPAWN.to_string()]).expect("grant");
        assert!(!grant_holds(Some(&other), NOSTR_SIGN));
        assert!(grant_holds(Some(&other), WORKTREE_SPAWN));

        let signing = grant_for_server(&server, &[NOSTR_SIGN.to_string()]).expect("grant");
        assert!(grant_holds(Some(&signing), NOSTR_SIGN));
        assert!(!grant_holds(Some(&signing), WORKTREE_SPAWN));
        // The replaced token no longer holds anything.
        assert!(!grant_holds(Some(&other), WORKTREE_SPAWN));
        assert!(!grant_holds(Some("not-a-token"), NOSTR_SIGN));

        revoke_delegation_grant(&server);
        assert!(!grant_holds(Some(&signing), NOSTR_SIGN));
    }

    /// expect: D110 wallet and zap grants are isolated from `NOSTR_SIGN` and
    /// from each other; a Nostr sign grant never unlocks the wallet DB or
    /// admits a spend or zap.
    #[test]
    fn wallet_and_zap_grants_are_isolated_from_nostr_sign() {
        let server = format!("test-wallet-{}", uuid::Uuid::new_v4());
        let nostr_only = grant_for_server(&server, &[NOSTR_SIGN.to_string()]).expect("grant");
        assert!(!grant_holds(Some(&nostr_only), WALLET_DB_UNLOCK));
        assert!(!grant_holds(Some(&nostr_only), WALLET_SPEND_APPROVE));
        assert!(!grant_holds(Some(&nostr_only), NOSTR_ZAP_SIGN));

        let wallet = grant_for_server(
            &server,
            &[
                WALLET_DB_UNLOCK.to_string(),
                WALLET_SPEND_APPROVE.to_string(),
            ],
        )
        .expect("wallet grant");
        assert!(grant_holds(Some(&wallet), WALLET_DB_UNLOCK));
        assert!(grant_holds(Some(&wallet), WALLET_SPEND_APPROVE));
        assert!(!grant_holds(Some(&wallet), NOSTR_SIGN));
        assert!(!grant_holds(Some(&wallet), NOSTR_ZAP_SIGN));

        revoke_delegation_grant(&server);
        assert!(!grant_holds(Some(&wallet), WALLET_SPEND_APPROVE));
    }
}
