//! Minting helpers for in-process delegation tokens.

use super::token_types::DelegationToken;

/// Mint a delegation token for in-process tool invocation.
///
/// Tokens are minted and consumed in-process (there is no untrusted transport
/// boundary), so the token carries no signing material — the enforced gate is
/// the capability match in `McpRuntime::invoke`, not cryptography.
///
/// Callers that need a distinct `delegated_from`/`delegated_to` WebID can pass
/// their own.
#[must_use]
pub fn panel_default_token(
    resource: super::resources::DelegationResource,
    resource_id: String,
    action: super::resources::DelegationAction,
    delegated_from: hkask_types::WebID,
    delegated_to: hkask_types::WebID,
) -> DelegationToken {
    DelegationToken::new(resource, resource_id, action, delegated_from, delegated_to)
}
