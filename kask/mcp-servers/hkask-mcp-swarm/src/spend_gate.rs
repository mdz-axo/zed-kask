//! Consent-gated spend gate — the single enforcement surface for the four
//! spend-mutating ABW tools (`swarm_hire`, `swarm_delegate`,
//! `swarm_create_swarm`, `swarm_xaman`).
//!
//! Each spend follows a two-phase shape: `authorize_*` consumes the consent
//! token, re-verifies the cost against ABW, and enforces the per-dispatch
//! ceiling — returning an `Authorization` carrying the refund grant. Then
//! `complete_*` executes the spend (HTTP POST), refunding the authorization
//! on transient failure. On success the authorization is dropped (the token
//! stays consumed). The refund invariant is structural: `complete_*` owns the
//! authorization by value and refunds on every `Err` path; the caller never
//! touches the refund grant directly except via `Authorization::refund` (used
//! by `swarm_xaman`, whose two-step session lifecycle has custom error
//! mapping and cannot be wrapped in a single `complete_*`).
//!
//! This consolidation eliminates the verbatim duplication of the
//! consume→re-verify→ceiling→refund sequence across the four tools. In
//! particular `swarm_create_swarm` previously copy-pasted `swarm_hire`'s
//! entire re-verify + `/hire`→`/add` fallback body inside a per-agent loop;
//! both now route through `authorize_hire` + `complete_hire`, so a behavior
//! change in the gate changes both (the desync hazard is structurally
//! impossible).

use hkask_mcp_server::server::McpToolError;

use crate::abw_client::SwarmClient;
use crate::abw_util::{effective_hire_cost, url_encode_segment};
use crate::consent::{ConsentGrant, ConsentStore};
use crate::error::SwarmError;

/// The authorization source chosen for a spend: a single-use consent token or a
/// reusable pre-authorized session token. The two are mutually exclusive - a
/// caller provides exactly one.
pub(crate) enum SpendAuth<'a> {
    SingleUse(&'a str),
    Session(&'a str),
}

/// Resolve the caller-supplied auth tokens to exactly one `SpendAuth`. Errors
/// if both are set (ambiguous authorization source) or neither is set. Empty
/// strings are treated as "not provided" so callers can send an empty
/// `consent_token` when using a session.
pub(crate) fn resolve_auth(
    consent_token: Option<&str>,
    session_token: Option<&str>,
) -> Result<SpendAuth<'_>, McpToolError> {
    let consent = consent_token.filter(|s| !s.is_empty());
    let session = session_token.filter(|s| !s.is_empty());
    match (consent, session) {
        (Some(_), Some(_)) => Err(McpToolError::invalid_argument(
            "provide either consent_token or session_token, not both".to_string(),
        )),
        (Some(token), None) => Ok(SpendAuth::SingleUse(token)),
        (None, Some(token)) => Ok(SpendAuth::Session(token)),
        (None, None) => Err(McpToolError::invalid_argument(
            "consent_token or session_token is required".to_string(),
        )),
    }
}

/// A carried, refundable authorization to hire an agent. Created by
/// `authorize_hire`; consumed by `complete_hire` (which refunds on failure)
/// or refunded explicitly via `refund` (used by `swarm_create_swarm`'s
/// per-hire error-collection loop, which continues on failure rather than
/// early-returning). The re-verified `actual_cost` is enforced inside the
/// gate and not exposed — the caller has no need for it.
pub(crate) struct HireAuthorization {
    refund_grant: ConsentGrant,
}

impl HireAuthorization {
    /// Refund the consumed consent token so the operator can retry without
    /// re-minting. Idempotent (`ConsentStore::refund` is `INSERT OR REPLACE`).
    pub(crate) fn refund(self, consent: &ConsentStore) {
        consent.refund(self.refund_grant);
    }
}

/// A carried, refundable authorization to delegate to an agent. Created by
/// `authorize_delegate`; consumed by `complete_delegate` or refunded via
/// `refund` (used by `swarm_xaman`'s two-step session lifecycle).
pub(crate) struct DelegateAuthorization {
    refund_grant: ConsentGrant,
}

impl DelegateAuthorization {
    /// Refund the consumed consent token. Idempotent.
    pub(crate) fn refund(self, consent: &ConsentStore) {
        consent.refund(self.refund_grant);
    }
}

/// Gate a single hire: consume the consent token, re-verify the actual hire
/// cost against ABW, and enforce the per-dispatch ceiling.
///
/// `consume_cost` is the cost passed to `ConsentStore::consume` (the store's
/// over-spend guard rejects `consume_cost > grant.credits_authorized`).
/// `swarm_hire` passes the caller's `credits_authorized` (so the guard
/// rejects a caller that under-declares the budget); `swarm_create_swarm`
/// passes `0` (the actual cost is unknown until the re-verify below — the
/// two-phase consume pattern, documented at the original call site).
///
/// `budget` is the ceiling the re-verified `actual_cost` is checked against.
/// `Some(v)` uses the caller-supplied budget (`swarm_hire` passes its
/// `credits_authorized`); `None` uses the grant's own `credits_authorized`
/// (`swarm_create_swarm`, which has no per-agent caller budget and relies on
/// the token's embedded ceiling). Both paths are clamped by `consume` so the
/// budget never exceeds the minted ceiling.
///
/// On any gate failure the token is refunded and an `McpToolError` returned.
/// On success a `HireAuthorization` is returned for the subsequent
/// `complete_hire` call.
pub(crate) async fn authorize_hire(
    client: &SwarmClient,
    consent: &ConsentStore,
    token: &str,
    agent_name: &str,
    consume_cost: u32,
    budget: Option<u32>,
    include_optional: bool,
) -> Result<HireAuthorization, McpToolError> {
    // Consume (validates scope + single-use + the over-spend guard). A failed
    // consume does NOT refund — the token was never consumed (unknown / scope
    // mismatch / replay / over-spend). Mirrors the original tools.
    let grant = consent
        .consume(token, "hire", agent_name, consume_cost)
        .map_err(SwarmError::into_tool_error)?;
    let refund_grant = ConsentGrant {
        action: "hire".to_string(),
        target: agent_name.to_string(),
        credits_authorized: grant,
        token: token.to_string(),
    };

    // Re-verify the hire cost against ABW immediately before spending. A
    // malicious client could mint a consent for 1 credit while the actual
    // hire charges 20 — the gate must validate the *spend*, not just the
    // *token*.
    let deps = client
        .get(&format!(
            "/agents/{}/dependencies",
            url_encode_segment(agent_name)
        ))
        .await
        .map_err(|e| {
            // Refund before propagating: the spend never happened.
            consent.refund(refund_grant.clone());
            SwarmError::into_tool_error(e)
        })?;
    // Do not fabricate cost = 0 on a missing field — a missing
    // `total_hire_cost` means ABW changed its response shape or the agent
    // doesn't exist (the `.rules` trap: a failed measurement must be
    // distinguishable from a measured zero).
    if deps
        .get("total_hire_cost")
        .and_then(|c| c.as_u64())
        .is_none()
    {
        tracing::warn!(
            target: "hkask.mcp.swarm",
            agent = %agent_name,
            "spend_gate::authorize_hire: ABW re-verify response missing total_hire_cost — cost unknown"
        );
        consent.refund(refund_grant.clone());
        return Err(McpToolError::internal(
            "hire cost unknown — ABW re-verify response missing total_hire_cost field".to_string(),
        ));
    }
    // Conservative cost re-verification: the effective cost is the dependency
    // quote floored at the owned-add flat fee for dependency-less agents, and
    // when the caller requests optional dependencies, use
    // `max(total, required + optional)` so the gate never under-estimates.
    let base_cost = effective_hire_cost(&deps);
    let actual_cost = if include_optional {
        let required = deps
            .get("required_cost")
            .and_then(|c| c.as_u64())
            .unwrap_or(base_cost);
        let optional = deps
            .get("optional_cost")
            .and_then(|c| c.as_u64())
            .unwrap_or(0);
        std::cmp::max(base_cost, required.saturating_add(optional))
    } else {
        base_cost
    };
    let budget = budget.unwrap_or(grant);
    if actual_cost > u64::from(budget) {
        consent.refund(refund_grant.clone());
        return Err(SwarmError::PaymentRequired(format!(
            "actual hire cost {actual_cost} exceeds authorized {budget} — \
             re-request consent with the updated cost"
        ))
        .into_tool_error());
    }
    // The operator-configured per-dispatch ceiling
    // (`max_credits_per_dispatch`, env `HKASK_ABW_MAX_CREDITS`, default 50) is
    // a hard gate, not advisory. There is no per-call override path by design
    // (a per-call override would let a prompt-injected agent talk the operator
    // into raising it mid-session). To raise it, the operator sets
    // `HKASK_ABW_MAX_CREDITS`.
    let ceiling = client.config().max_credits_per_dispatch;
    if actual_cost > u64::from(ceiling) {
        consent.refund(refund_grant.clone());
        tracing::warn!(
            target: "hkask.mcp.swarm",
            agent = %agent_name,
            cost = actual_cost,
            ceiling,
            "spend_gate::authorize_hire: hire cost exceeds per-dispatch ceiling — refused"
        );
        return Err(SwarmError::PaymentRequired(format!(
            "hire cost {actual_cost} exceeds per-dispatch ceiling {ceiling} \
             (raise HKASK_ABW_MAX_CREDITS to authorize)"
        ))
        .into_tool_error());
    }

    Ok(HireAuthorization { refund_grant })
}

/// Execute the hire POST with the `/hire`→`/add` fallback, refunding the
/// authorization on transient failure. Other authors' catalogue agents use
/// `/hire`; the operator's OWN agents return 400 "Use /add for your own
/// agents" and must use `/add` (verified live). Returns the raw ABW response
/// value; the caller wraps it (with the wallet signal, hire-specific fields).
pub(crate) async fn complete_hire(
    client: &SwarmClient,
    consent: &ConsentStore,
    auth: HireAuthorization,
    workspace_id: &str,
    agent_name: &str,
    include_optional: bool,
) -> Result<serde_json::Value, McpToolError> {
    // Wrap in Option so the failure closure can refund via `.take()`
    // (capturing `&mut`) without moving `auth` out of the function scope.
    // On success `auth` is still `Some(_)`; dropping it leaves the token
    // consumed (single-use per *successful* spend).
    let mut auth = Some(auth);
    let data = match client
        .post(
            &format!("/workspaces/{}/hire", url_encode_segment(workspace_id)),
            &serde_json::json!({
                "agent_id": agent_name,
                "include_optional": include_optional,
            }),
        )
        .await
    {
        Ok(d) => Ok(d),
        Err(SwarmError::Unavailable(m)) if m.contains("Use /add for your own agents") => {
            tracing::info!(
                target: "hkask.mcp.swarm",
                agent = %agent_name,
                "own agent — falling back to /workspaces/{{id}}/add"
            );
            client
                .post(
                    &format!("/workspaces/{}/add", url_encode_segment(workspace_id)),
                    &serde_json::json!({ "agent_id": agent_name }),
                )
                .await
        }
        Err(e) => Err(e),
    }
    .map_err(|e| {
        // Refund before propagating: the spend never happened. The auth
        // owns the refund grant; take it and refund.
        if let Some(a) = auth.take() {
            a.refund(consent);
        }
        SwarmError::into_tool_error(e)
    })?;
    // Success: drop the auth (token stays consumed).
    drop(auth);
    Ok(data)
}

/// Gate a delegate: consume the consent token and enforce the per-dispatch
/// ceiling. Delegation cost is `1 cr + tokens` and not pre-quoted by ABW, so
/// the consent token's `credits_authorized` is the only cost signal — the
/// ceiling gates the declared authorization directly.
pub(crate) fn authorize_delegate(
    client: &SwarmClient,
    consent: &ConsentStore,
    token: &str,
    workspace_id: &str,
    credits_authorized: u32,
) -> Result<DelegateAuthorization, McpToolError> {
    let grant = consent
        .consume(token, "delegate", workspace_id, credits_authorized)
        .map_err(SwarmError::into_tool_error)?;
    let refund_grant = ConsentGrant {
        action: "delegate".to_string(),
        target: workspace_id.to_string(),
        credits_authorized: grant,
        token: token.to_string(),
    };
    // Per-dispatch ceiling enforcement (mirrors `authorize_hire`). Without
    // this, an operator (or a prompt-injected agent in Steer mode) could mint
    // a delegate consent for 1000 credits and bypass the dispatch limit.
    let ceiling = client.config().max_credits_per_dispatch;
    if u64::from(grant) > u64::from(ceiling) {
        // Refund the grant (moving it) and refuse. The grant was consumed
        // above; refunding restores it for retry.
        consent.refund(refund_grant);
        tracing::warn!(
            target: "hkask.mcp.swarm",
            workspace = %workspace_id,
            authorized = grant,
            ceiling,
            "spend_gate::authorize_delegate: authorized ceiling exceeds per-dispatch limit — refused"
        );
        return Err(SwarmError::PaymentRequired(format!(
            "authorized credits {grant} exceed per-dispatch ceiling {ceiling} \
             (raise HKASK_ABW_MAX_CREDITS to authorize)"
        ))
        .into_tool_error());
    }
    Ok(DelegateAuthorization { refund_grant })
}

/// Execute the delegate @mention POST, refunding the authorization on
/// transient failure. Returns the raw ABW response value; the caller wraps
/// it (with the wallet signal, delegate-specific fields).
pub(crate) async fn complete_delegate(
    client: &SwarmClient,
    consent: &ConsentStore,
    auth: DelegateAuthorization,
    workspace_id: &str,
    agent_name: &str,
    task: &str,
) -> Result<serde_json::Value, McpToolError> {
    let mut auth = Some(auth);
    // Strip leading @mentions (KA-06): a task starting with `@other_agent`
    // would mention a different agent in the workspace chat, a semantic
    // injection at the ABW chat layer.
    let task_clean = crate::sanitize::strip_leading_mentions(task);
    let data = client
        .post(
            &format!("/workspaces/{}/messages", url_encode_segment(workspace_id)),
            &serde_json::json!({ "content": format!("@{} {}", agent_name, task_clean) }),
        )
        .await
        .map_err(|e| {
            if let Some(a) = auth.take() {
                a.refund(consent);
            }
            SwarmError::into_tool_error(e)
        })?;
    drop(auth);
    Ok(data)
}

/// Gate a curate (Xaman Ek) call. Returns `Ok(None)` when
/// `curator_consent_default` is true (the operator has globally opted in —
/// no per-call token needed). Otherwise consumes the consent token
/// (action "curate", fixed target "xaman") and returns `Ok(Some(auth))`.
///
/// The caller (`swarm_xaman`) holds the `Option<DelegateAuthorization>` and
/// refunds it on every failure path of its two-step session lifecycle
/// (session create + message send), which has custom error mapping and
/// cannot be wrapped in a single `complete_*`.
pub(crate) fn authorize_curate(
    client: &SwarmClient,
    consent: &ConsentStore,
    token: Option<&str>,
) -> Result<Option<DelegateAuthorization>, McpToolError> {
    if client.config().curator_consent_default {
        return Ok(None);
    }
    let Some(token) = token else {
        return Err(SwarmError::ConsentDenied(
            "Xaman Ek curator call requires a consent token (action 'curate') — \
             set kask.swarm.curator_consent_default true to opt in globally"
                .to_string(),
        )
        .into_tool_error());
    };
    let grant = consent
        .consume(token, "curate", "xaman", 0)
        .map_err(SwarmError::into_tool_error)?;
    let refund_grant = ConsentGrant {
        action: "curate".to_string(),
        target: "xaman".to_string(),
        credits_authorized: grant,
        token: token.to_string(),
    };
    Ok(Some(DelegateAuthorization { refund_grant }))
}

// ── Session-aware spend (NOT YET WIRED) ────────────────────────────────────
//
// `swarm_authorize_session` mints a pre-authorized session token (headless
// pipelines), and `ConsentStore::consume_session` / `session_balance` implement
// the session store. But the SpendGate consume path — `authorize_*_with_session`
// / `complete_*_with_session` — was never called by `swarm_hire` / `swarm_delegate`
// (they use the single-use `authorize_*` / `complete_*` path). The dead
// session wrappers were removed 2026-08-03 (clippy: never constructed/used).
// To wire headless sessions: when `swarm_hire` / `swarm_delegate` receive a
// session token, branch to the session consume path (re-add the two-phase
// `authorize_*_with_session` / `complete_*_with_session` wrappers) or extend the
// single-use path to accept either token kind. Until then a minted session is
// not consumed by any spend tool — an advertised feature with no enforcement
// point (the `.rules` trap).
