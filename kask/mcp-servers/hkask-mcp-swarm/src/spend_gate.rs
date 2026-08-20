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
#[derive(Debug)]
pub(crate) enum SpendAuth<'a> {
    SingleUse(&'a str),
    Session(&'a str),
}

/// Resolve the caller-supplied auth tokens to exactly one `SpendAuth`. Errors
/// if both are set (ambiguous authorization source) or neither is set. Empty
/// strings are treated as "not provided" so callers can send an empty
/// `consent_token` when using a session.
pub(crate) fn resolve_auth<'a>(
    consent_token: Option<&'a str>,
    session_token: Option<&'a str>,
) -> Result<SpendAuth<'a>, McpToolError> {
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

/// How a carried authorization is reconciled when the spend completes.
///
/// The single spend path (`authorize_*` / `complete_*`) accepts EITHER a
/// single-use consent token OR a reusable pre-authorized session token
/// (selected by `SpendAuth`). The settlement differs only in how the credit
/// budget is touched:
/// - `SingleUse`: the consent token is consumed upfront in `authorize_*`; on
///   failure `complete_*` refunds it, on success it stays consumed (single-use
///   per successful spend).
/// - `Session`: `authorize_*` validates the session without deducting
///   (cost=0); `complete_*` deducts the authorized cost on success and does
///   nothing on failure (nothing was deducted to refund).
pub(crate) enum Settlement {
    SingleUse { refund_grant: ConsentGrant },
    Session { token: String, cost: u32 },
}

impl Settlement {
    /// Refund an unsuccessful spend. No-op for sessions (the validation consume
    /// deducted 0); refunds the consumed consent grant for single-use tokens.
    fn refund(self, consent: &ConsentStore) {
        if let Settlement::SingleUse { refund_grant } = self {
            consent.refund(refund_grant);
        }
    }
    /// Refund only if this is a single-use settlement. Used by the gate's
    /// failure paths, which must release a consumed single-use token but must
    /// not touch a session that only validated (nothing was deducted).
    fn refund_if_singleuse(&self, consent: &ConsentStore) {
        if let Settlement::SingleUse { refund_grant } = self {
            consent.refund(refund_grant.clone());
        }
    }
}

/// A carried, refundable authorization to hire an agent. Created by
/// `authorize_hire`; consumed by `complete_hire` (which refunds on failure or
/// deducts from the session on success) or refunded explicitly via `refund`
/// (used by `swarm_create_swarm`'s per-hire error-collection loop, which
/// continues on failure rather than early-returning).
pub(crate) struct HireAuthorization {
    settlement: Settlement,
}

impl HireAuthorization {
    /// Refund an unsuccessful spend (single-use refunds the consumed token;
    /// session is a no-op). Idempotent for single-use (`ConsentStore::refund`
    /// is `INSERT OR REPLACE`).
    pub(crate) fn refund(self, consent: &ConsentStore) {
        self.settlement.refund(consent);
    }
    /// Settle a successful spend: single-use is already consumed (no-op);
    /// session deducts the authorized cost.
    fn settle_success(&self, consent: &ConsentStore) {
        if let Settlement::Session { token, cost } = &self.settlement {
            if let Err(e) = consent.consume_session(token, "hire", *cost) {
                tracing::warn!(
                    target: "hkask.mcp.swarm",
                    error = %e,
                    "session deduction failed after successful hire — session balance may be stale"
                );
            }
        }
    }
}

/// A carried, refundable authorization to delegate to an agent. Created by
/// `authorize_delegate`; consumed by `complete_delegate` or refunded via
/// `refund` (used by `swarm_xaman`'s two-step session lifecycle).
pub(crate) struct DelegateAuthorization {
    settlement: Settlement,
}

impl DelegateAuthorization {
    pub(crate) fn refund(self, consent: &ConsentStore) {
        self.settlement.refund(consent);
    }
    fn settle_success(&self, consent: &ConsentStore) {
        if let Settlement::Session { token, cost } = &self.settlement {
            if let Err(e) = consent.consume_session(token, "delegate", *cost) {
                tracing::warn!(
                    target: "hkask.mcp.swarm",
                    error = %e,
                    "session deduction failed after successful delegate — session balance may be stale"
                );
            }
        }
    }
}

/// Gate a single hire: consume/validate the token, re-verify the actual hire
/// cost against ABW, and enforce the per-dispatch ceiling. Accepts either a
/// single-use consent token or a reusable session token (`SpendAuth`).
///
/// `consume_cost` is the cost passed to `ConsentStore::consume` for single-use
/// tokens (the store's over-spend guard rejects `consume_cost > grant`). It is
/// ignored for session tokens (the session is validated with cost=0 and the
/// re-verified `actual_cost` is deducted in `complete_hire` on success).
///
/// `budget` is the ceiling the re-verified `actual_cost` is checked against
/// for single-use tokens (`Some(v)` uses the caller-supplied budget; `None`
/// uses the grant's own `credits_authorized`). For session tokens the session's
/// remaining balance is the budget.
///
/// On any gate failure a single-use token is refunded (a session has nothing to
/// refund — it only validated) and an `McpToolError` returned. On success a
/// `HireAuthorization` is returned for the subsequent `complete_hire` call.
pub(crate) async fn authorize_hire(
    client: &SwarmClient,
    consent: &ConsentStore,
    auth: SpendAuth<'_>,
    agent_name: &str,
    consume_cost: u32,
    budget: Option<u32>,
    include_optional: bool,
) -> Result<HireAuthorization, McpToolError> {
    // Consume/validate the token upfront.
    let settlement = match auth {
        SpendAuth::SingleUse(token) => {
            // A failed consume does NOT refund — the token was never consumed
            // (unknown / scope mismatch / replay / over-spend).
            let grant = consent
                .consume(token, "hire", agent_name, consume_cost)
                .map_err(SwarmError::into_tool_error)?;
            Settlement::SingleUse {
                refund_grant: ConsentGrant {
                    action: "hire".to_string(),
                    target: agent_name.to_string(),
                    credits_authorized: grant,
                    token: token.to_string(),
                },
            }
        }
        SpendAuth::Session(token) => {
            // Validate the session (cost=0): existence, action scope, expiry —
            // nothing deducted. The actual deduction happens in complete_hire.
            consent
                .consume_session(token, "hire", 0)
                .map_err(SwarmError::into_tool_error)?;
            Settlement::Session {
                token: token.to_string(),
                cost: 0,
            }
        }
    };

    // Re-verify the hire cost against ABW (shared by both token kinds).
    let deps = client
        .get(&format!(
            "/agents/{}/dependencies",
            url_encode_segment(agent_name)
        ))
        .await
        .map_err(|e| {
            settlement.refund_if_singleuse(consent);
            SwarmError::into_tool_error(e)
        })?;
    // Do not fabricate cost = 0 on a missing field (the `.rules` trap: a failed
    // measurement must be distinguishable from a measured zero).
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
        settlement.refund_if_singleuse(consent);
        return Err(McpToolError::unavailable(
            "hire cost unknown — ABW re-verify response missing total_hire_cost field".to_string(),
        ));
    }
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

    // Budget check: single-use uses the caller/grant budget; session uses the
    // session's remaining balance.
    match &settlement {
        Settlement::SingleUse { refund_grant } => {
            let budget = budget.unwrap_or(refund_grant.credits_authorized);
            if actual_cost > u64::from(budget) {
                settlement.refund_if_singleuse(consent);
                return Err(SwarmError::PaymentRequired(format!(
                    "actual hire cost {actual_cost} exceeds authorized {budget} — \
                     re-request consent with the updated cost"
                ))
                .into_tool_error());
            }
        }
        Settlement::Session { token, .. } => {
            let remaining = consent.session_balance(token).ok_or_else(|| {
                McpToolError::unavailable(
                    "session balance query failed — cannot verify hire budget".to_string(),
                )
            })?;
            if actual_cost > u64::from(remaining) {
                return Err(SwarmError::PaymentRequired(format!(
                    "hire cost {actual_cost} exceeds session remaining {remaining} — \
                     open a new session with more credits"
                ))
                .into_tool_error());
            }
        }
    }

    // Per-dispatch ceiling (shared). No per-call override by design — a
    // per-call override would let a prompt-injected agent talk the operator into
    // raising it mid-session. To raise it, set `HKASK_ABW_MAX_CREDITS`.
    let ceiling = client.config().max_credits_per_dispatch;
    if actual_cost > u64::from(ceiling) {
        settlement.refund_if_singleuse(consent);
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

    // Set the session deduction cost (single-use is unchanged).
    let settlement = match settlement {
        Settlement::Session { token, .. } => Settlement::Session {
            token,
            cost: u32::try_from(actual_cost).unwrap_or(u32::MAX),
        },
        other => other,
    };
    Ok(HireAuthorization { settlement })
}

/// Execute the hire POST with the `/hire`→`/add` fallback. On transient failure
/// the authorization is refunded (single-use) or left untouched (session, which
/// only validated). On success a session authorization deducts the authorized
/// cost; a single-use authorization is dropped (token stays consumed). Returns
/// the raw ABW response value; the caller wraps it.
pub(crate) async fn complete_hire(
    client: &SwarmClient,
    consent: &ConsentStore,
    auth: HireAuthorization,
    workspace_id: &str,
    agent_name: &str,
    include_optional: bool,
) -> Result<serde_json::Value, McpToolError> {
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
            // fermi-contract: the `/hire`→`/add` fallback matches this exact
            // error string from fermi's `POST /workspaces/{id}/hire` handler
            // (verified live 2026-08-13). fermi returns it with HTTP 500 when
            // the caller owns the agent being hired — own-agent hires go via
            // `POST /workspaces/{id}/add` (flat 2 cr) instead of `/hire`
            // (third-party 5 cr base + dependencies). If fermi rewords this
            // string, the fallback silently breaks and own-agent hires 500.
            // A live probe (`swarm_hire` against an owned agent) is the
            // canary; run it with `--test-threads=1` per the `.rules` trap.
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
        if let Some(a) = auth.take() {
            a.refund(consent);
        }
        SwarmError::into_tool_error(e)
    })?;
    if let Some(a) = auth.take() {
        a.settle_success(consent);
    }
    Ok(data)
}

/// Gate a delegate: consume/validate the token and enforce the per-dispatch
/// ceiling. Accepts either a single-use consent token or a session token.
/// Delegation cost is `1 cr + tokens` and not pre-quoted by ABW, so the declared
/// `credits_authorized` is the cost signal — the ceiling gates it directly.
pub(crate) fn authorize_delegate(
    client: &SwarmClient,
    consent: &ConsentStore,
    auth: SpendAuth<'_>,
    workspace_id: &str,
    credits_authorized: u32,
) -> Result<DelegateAuthorization, McpToolError> {
    let settlement = match auth {
        SpendAuth::SingleUse(token) => {
            let grant = consent
                .consume(token, "delegate", workspace_id, credits_authorized)
                .map_err(SwarmError::into_tool_error)?;
            Settlement::SingleUse {
                refund_grant: ConsentGrant {
                    action: "delegate".to_string(),
                    target: workspace_id.to_string(),
                    credits_authorized: grant,
                    token: token.to_string(),
                },
            }
        }
        SpendAuth::Session(token) => {
            consent
                .consume_session(token, "delegate", 0)
                .map_err(SwarmError::into_tool_error)?;
            Settlement::Session {
                token: token.to_string(),
                cost: credits_authorized,
            }
        }
    };
    let authorized = match &settlement {
        Settlement::SingleUse { refund_grant } => refund_grant.credits_authorized,
        Settlement::Session { cost, .. } => *cost,
    };
    // Per-dispatch ceiling (shared). Without this, an operator (or a
    // prompt-injected agent in Steer mode) could mint a delegate consent for
    // 1000 credits and bypass the dispatch limit.
    let ceiling = client.config().max_credits_per_dispatch;
    if u64::from(authorized) > u64::from(ceiling) {
        settlement.refund_if_singleuse(consent);
        tracing::warn!(
            target: "hkask.mcp.swarm",
            workspace = %workspace_id,
            authorized,
            ceiling,
            "spend_gate::authorize_delegate: authorized ceiling exceeds per-dispatch limit — refused"
        );
        return Err(SwarmError::PaymentRequired(format!(
            "authorized credits {authorized} exceed per-dispatch ceiling {ceiling} \
             (raise HKASK_ABW_MAX_CREDITS to authorize)"
        ))
        .into_tool_error());
    }
    // Session: verify the session has enough remaining for the authorized cost.
    if let Settlement::Session { token, cost } = &settlement {
        let remaining = consent.session_balance(token).ok_or_else(|| {
            McpToolError::unavailable(
                "session balance query failed — cannot verify delegate budget".to_string(),
            )
        })?;
        if u64::from(*cost) > u64::from(remaining) {
            return Err(SwarmError::PaymentRequired(format!(
                "authorized credits {cost} exceed session remaining {remaining}"
            ))
            .into_tool_error());
        }
    }
    Ok(DelegateAuthorization { settlement })
}

/// Execute the delegate @mention POST. On transient failure the authorization
/// is refunded (single-use) or untouched (session); on success a session
/// authorization deducts the authorized cost. Returns the raw ABW response
/// value; the caller wraps it.
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
    // would mention a different agent in the workspace chat.
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
    if let Some(a) = auth.take() {
        a.settle_success(consent);
    }
    Ok(data)
}

/// Gate a curate (Xaman Ek) call. Returns `Ok(None)` when
/// `curator_consent_default` is true (the operator has globally opted in —
/// no per-call token needed). Otherwise consumes the consent token
/// (action "curate", fixed target "xaman") and returns `Ok(Some(auth))`.
/// Curate is single-use only — sessions do not cover the curate action.
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
    Ok(Some(DelegateAuthorization {
        settlement: Settlement::SingleUse { refund_grant },
    }))
}
