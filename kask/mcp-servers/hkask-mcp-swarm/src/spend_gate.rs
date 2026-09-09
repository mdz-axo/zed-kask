//! Consent-gated spend gate — the single enforcement surface for the four
//! spend-mutating ABW tools (`swarm_hire`, `swarm_delegate`,
//! `swarm_create_swarm`, `swarm_xaman`).
//!
//! Each spend follows a two-phase shape. `authorize_*` takes the RESERVATION
//! — a single-use consent token is consumed (the consumed token IS the
//! reservation), or the authorized cost is ATOMICALLY DEDUCTED from the
//! session (the store's conditional-UPDATE: two processes racing on one
//! session cannot both reserve the same credits) — and enforces the
//! per-dispatch ceiling. Then `complete_*` executes the spend (HTTP POST)
//! and settles by the operator-ratified T05 policy (2026-09-08):
//!
//! - **Proven pre-dispatch rejection** (the request never left — connection
//!   phase/construction failure — or ABW answered with an error): `release`.
//!   Single-use refunds the token; session credits the reserved cost back.
//! - **Ambiguous outcome** (sent, no definitive answer — timeout, reset,
//!   lost response): `hold`. The reservation stays exactly as it is (token
//!   consumed / credits deducted), the uncertainty is surfaced to the
//!   caller, and a retry cannot reuse the held capacity. Never auto-released.
//! - **Success**: nothing further — the reservation IS the spend.
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
pub enum SpendAuth<'a> {
    SingleUse(&'a str),
    Session(&'a str),
}

/// Resolve the caller-supplied auth tokens to exactly one `SpendAuth`. Errors
/// if both are set (ambiguous authorization source) or neither is set. Empty
/// strings are treated as "not provided" so callers can send an empty
/// `consent_token` when using a session.
pub fn resolve_auth<'a>(
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
/// The reservation is taken at `authorize_*` time:
/// - `SingleUse`: the consent token is consumed upfront — the consumed
///   token IS the reservation.
/// - `Session`: the authorized cost is ATOMICALLY DEDUCTED from the session
///   at `authorize_*` time (the store's conditional-UPDATE — two processes
///   racing on one session cannot both reserve). The deduction is the
///   reservation; a successful dispatch needs no further settlement.
///
/// Settlement follows the operator-ratified T05 policy (2026-09-08):
/// release on proven pre-dispatch rejection, hold on ambiguity, nothing on
/// success.
pub enum Settlement {
    SingleUse { refund_grant: ConsentGrant },
    Session { token: String, cost: u32 },
}

impl Settlement {
    /// Release the reservation on a PROVEN pre-dispatch rejection. Single-use
    /// refunds the consumed token; session credits the reserved cost back
    /// (best-effort with a loud warn on store failure — the dispatch already
    /// failed).
    pub(crate) fn release(self, consent: &ConsentStore) {
        match self {
            Self::SingleUse { refund_grant } => consent.refund(refund_grant),
            Self::Session { token, cost } => consent.release_session(&token, cost),
        }
    }
    /// Hold the reservation on an AMBIGUOUS dispatch outcome. Deliberately
    /// consumes the authorization without touching the store: the
    /// single-use token stays consumed and the session credits stay
    /// deducted — a retry cannot reuse the held capacity. The caller
    /// surfaces the uncertainty.
    pub(crate) fn hold(self) {
        drop(self);
    }
    /// What is held, for the uncertainty message surfaced to the operator.
    pub(crate) fn held_description(&self) -> String {
        match self {
            Self::SingleUse { .. } => "the consent token stays consumed".to_string(),
            Self::Session { cost, .. } => format!("{cost} session credits stay held"),
        }
    }
}

/// Gate a single hire: take the reservation, re-verify the actual hire cost
/// against ABW, and enforce the per-dispatch ceiling. Accepts either a
/// single-use consent token or a session token (`SpendAuth`).
///
/// Single-use: the token is consumed upfront (the consumed token IS the
/// reservation); every gate failure after that releases it. Session: NOTHING
/// is held until the final ATOMIC RESERVE — every intermediate failure
/// (re-verify GET, cost unknown, budget, ceiling) is pre-dispatch by
/// construction and needs no release. The reserve is the store's
/// conditional-UPDATE deduction with the re-verified `actual_cost` — it
/// checks the balance and reserves it in one step, so two processes racing
/// on one session cannot both dispatch against the same credits.
///
/// On any gate failure the reservation is released and an `McpToolError`
/// returned. On success the `Settlement` (the reservation) is returned for
/// the subsequent `complete_hire` call.
pub(crate) async fn authorize_hire(
    client: &SwarmClient,
    consent: &ConsentStore,
    auth: SpendAuth<'_>,
    agent_name: &str,
    consume_cost: u32,
    budget: Option<u32>,
    include_optional: bool,
) -> Result<Settlement, McpToolError> {
    // The in-flight authorization state: a single-use token is consumed
    // (reserved) here; a session token is only reserved at the end, after
    // every check has passed.
    enum Pending {
        SingleUse(ConsentGrant),
        Session(String),
    }
    let pending = match auth {
        SpendAuth::SingleUse(token) => {
            // A failed consume does NOT release — the token was never
            // consumed (unknown / scope mismatch / replay / over-spend).
            let grant = consent
                .consume(token, "hire", agent_name, consume_cost)
                .map_err(SwarmError::into_tool_error)?;
            Pending::SingleUse(ConsentGrant {
                action: "hire".to_string(),
                target: agent_name.to_string(),
                credits_authorized: grant,
                token: token.to_string(),
            })
        }
        SpendAuth::Session(token) => Pending::Session(token.to_string()),
    };
    // Every failure from here to the session reserve is pre-dispatch:
    // release the single-use reservation; a session has nothing held yet.
    let release = |pending: &Pending| match pending {
        Pending::SingleUse(grant) => consent.refund(grant.clone()),
        Pending::Session(_) => {}
    };

    // Re-verify the hire cost against ABW (shared by both token kinds).
    let deps = match client
        .get(&format!(
            "/agents/{}/dependencies",
            url_encode_segment(agent_name)
        ))
        .await
    {
        Ok(deps) => deps,
        Err(e) => {
            release(&pending);
            return Err(SwarmError::into_tool_error(e));
        }
    };
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
        release(&pending);
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

    // Budget check for single-use: the caller/grant budget bounds the
    // re-verified cost. (Sessions are bounded by the atomic reserve below.)
    if let Pending::SingleUse(grant) = &pending {
        let budget = budget.unwrap_or(grant.credits_authorized);
        if actual_cost > u64::from(budget) {
            release(&pending);
            return Err(SwarmError::PaymentRequired(format!(
                "actual hire cost {actual_cost} exceeds authorized {budget} — \
                 re-request consent with the updated cost"
            ))
            .into_tool_error());
        }
    }

    // Per-dispatch ceiling (shared). No per-call override by design — a
    // per-call override would let a prompt-injected agent talk the operator into
    // raising it mid-session. To raise it, set `HKASK_ABW_MAX_CREDITS`.
    let ceiling = client.config().max_credits_per_dispatch;
    if actual_cost > u64::from(ceiling) {
        release(&pending);
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

    let settlement = match pending {
        Pending::SingleUse(refund_grant) => Settlement::SingleUse { refund_grant },
        Pending::Session(token) => {
            // ATOMIC RESERVE: the conditional-UPDATE deduction both checks
            // the session balance and reserves the credits — the former
            // non-atomic `session_balance` read plus post-dispatch deduction
            // admitted two overlapping authorizations against one session.
            let cost = u32::try_from(actual_cost).unwrap_or(u32::MAX);
            consent
                .consume_session(&token, "hire", cost)
                .map_err(SwarmError::into_tool_error)?;
            Settlement::Session { token, cost }
        }
    };
    Ok(settlement)
}

/// Execute the hire POST with the `/hire`→`/add` fallback, settling the
/// reservation by the T05 policy: proven pre-dispatch rejection releases,
/// ambiguity holds and surfaces the uncertainty, success needs nothing
/// further (the reservation IS the spend). Returns the raw ABW response
/// value; the caller wraps it.
pub(crate) async fn complete_hire(
    client: &SwarmClient,
    consent: &ConsentStore,
    reservation: Settlement,
    workspace_id: &str,
    agent_name: &str,
    include_optional: bool,
) -> Result<serde_json::Value, McpToolError> {
    let mut reservation = Some(reservation);
    let result = match client
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
            // (That 500 is a PROVEN rejection of `/hire`; the `/add` POST is
            // the dispatch whose outcome drives settlement below.)
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
    };
    match result {
        Ok(data) => {
            // Success: the reservation IS the spend — the single-use token
            // stays consumed and the session credits stay deducted.
            drop(reservation.take());
            Ok(data)
        }
        Err(e) => {
            let held = reservation.take();
            settle_dispatch_failure(consent, held, e).map_err(SwarmError::into_tool_error)
        }
    }
}

/// Settle a failed dispatch by the operator-ratified T05 policy
/// (2026-09-08): a PROVEN pre-dispatch rejection releases the reservation
/// and propagates the error; an AMBIGUOUS outcome holds the reservation and
/// surfaces the uncertainty (never auto-released). `None` carries no
/// reservation (the curate gate when `curator_consent_default` opted the
/// operator in globally).
fn settle_dispatch_failure(
    consent: &ConsentStore,
    reservation: Option<Settlement>,
    error: SwarmError,
) -> Result<serde_json::Value, SwarmError> {
    match reservation {
        Some(settlement) => {
            if error.is_proven_rejection() {
                settlement.release(consent);
                Err(error)
            } else {
                let held = settlement.held_description();
                settlement.hold();
                Err(SwarmError::DispatchAmbiguous(format!(
                    "{error}; {held} — inspect the ABW workspace before \
                     retrying; a retry cannot reuse this authorization"
                )))
            }
        }
        None => Err(error),
    }
}

/// Gate a delegate: take the reservation and enforce the per-dispatch
/// ceiling. Accepts either a single-use consent token or a session token.
/// Delegation cost is `1 cr + tokens` and not pre-quoted by ABW, so the declared
/// `credits_authorized` is the cost signal — the ceiling gates it directly.
pub fn authorize_delegate(
    client: &SwarmClient,
    consent: &ConsentStore,
    auth: SpendAuth<'_>,
    workspace_id: &str,
    credits_authorized: u32,
) -> Result<Settlement, McpToolError> {
    let settlement = match auth {
        SpendAuth::SingleUse(token) => {
            // The consumed token IS the reservation.
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
            // Ceiling first (no store mutation), then the ATOMIC RESERVE: the
            // conditional-UPDATE deduction checks the balance and reserves
            // the credits in one step — two processes racing on one session
            // cannot both reserve the same credits.
            let ceiling = client.config().max_credits_per_dispatch;
            if u64::from(credits_authorized) > u64::from(ceiling) {
                tracing::warn!(
                    target: "hkask.mcp.swarm",
                    workspace = %workspace_id,
                    authorized = credits_authorized,
                    ceiling,
                    "spend_gate::authorize_delegate: authorized ceiling exceeds per-dispatch limit — refused"
                );
                return Err(SwarmError::PaymentRequired(format!(
                    "authorized credits {credits_authorized} exceed per-dispatch ceiling {ceiling} \
                     (raise HKASK_ABW_MAX_CREDITS to authorize)"
                ))
                .into_tool_error());
            }
            consent
                .consume_session(token, "delegate", credits_authorized)
                .map_err(SwarmError::into_tool_error)?;
            Settlement::Session {
                token: token.to_string(),
                cost: credits_authorized,
            }
        }
    };
    // Single-use ceiling check: the grant's own ceiling may exceed the
    // requested cost — gate on the grant ceiling (the operator's
    // authorization), releasing the reservation on refusal.
    let single_use_over_ceiling = match &settlement {
        Settlement::SingleUse { refund_grant } => {
            u64::from(refund_grant.credits_authorized)
                > u64::from(client.config().max_credits_per_dispatch)
        }
        Settlement::Session { .. } => false,
    };
    if single_use_over_ceiling {
        let authorized = match &settlement {
            Settlement::SingleUse { refund_grant } => refund_grant.credits_authorized,
            Settlement::Session { .. } => unreachable!("checked above"),
        };
        let ceiling = client.config().max_credits_per_dispatch;
        settlement.release(consent);
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
    Ok(settlement)
}

/// Execute the delegate @mention POST, settling the reservation by the T05
/// policy: proven pre-dispatch rejection releases, ambiguity holds and
/// surfaces the uncertainty, success needs nothing further (the reservation
/// IS the spend). Returns the raw ABW response value; the caller wraps it.
pub(crate) async fn complete_delegate(
    client: &SwarmClient,
    consent: &ConsentStore,
    reservation: Settlement,
    workspace_id: &str,
    agent_name: &str,
    task: &str,
) -> Result<serde_json::Value, McpToolError> {
    let mut reservation = Some(reservation);
    // Strip leading @mentions (KA-06): a task starting with `@other_agent`
    // would mention a different agent in the workspace chat.
    let task_clean = crate::sanitize::strip_leading_mentions(task);
    let result = client
        .post(
            &format!("/workspaces/{}/messages", url_encode_segment(workspace_id)),
            &serde_json::json!({ "content": format!("@{} {}", agent_name, task_clean) }),
        )
        .await;
    match result {
        Ok(data) => {
            // Success: the reservation IS the spend.
            drop(reservation.take());
            Ok(data)
        }
        Err(e) => {
            let held = reservation.take();
            settle_dispatch_failure(consent, held, e).map_err(SwarmError::into_tool_error)
        }
    }
}

/// Gate a curate (Xaman Ek) call. Returns `Ok(None)` when
/// `curator_consent_default` is true (the operator has globally opted in —
/// no per-call token needed). Otherwise consumes the consent token
/// (action "curate", fixed target "xaman") and returns `Ok(Some(auth))`.
/// Curate is single-use only — sessions do not cover the curate action.
///
/// The caller (`swarm_xaman`) holds the `Option<Settlement>` and
/// refunds it on every failure path of its two-step session lifecycle
/// (session create + message send), which has custom error mapping and
/// cannot be wrapped in a single `complete_*`.
pub fn authorize_curate(
    client: &SwarmClient,
    consent: &ConsentStore,
    token: Option<&str>,
) -> Result<Option<Settlement>, McpToolError> {
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
    Ok(Some(Settlement::SingleUse { refund_grant }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abw_client::test_http::{Behavior, FixtureServer};
    use crate::config::SwarmConfig;

    fn test_client(base_url: &str) -> SwarmClient {
        SwarmClient::new(
            reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(2))
                .build()
                .expect("test client"),
            SwarmConfig {
                api_base_url: base_url.to_string(),
                api_key: None,
                ..SwarmConfig::default()
            },
        )
    }

    /// Two stores on one SQLite file model two server instances sharing the
    /// consent DB (the panel flow and the tool flow).
    fn sqlite_store(dir: &tempfile::TempDir) -> ConsentStore {
        ConsentStore::open_sqlite(dir.path().join("consent.db").to_str().expect("path"))
            .expect("consent store")
    }

    /// Read a live session's remaining balance without deducting anything:
    /// `consume_session` with cost 0 validates and returns the balance.
    fn remaining(consent: &ConsentStore, session: &str) -> u32 {
        consent
            .consume_session(session, "delegate", 0)
            .expect("session alive")
    }

    /// T05: two overlapping 10-credit authorizations against one 10-credit
    /// session admit at most one POST — including across separate server
    /// instances sharing the consent DB. Pre-fix, both authorized
    /// (validate-only, non-atomic balance read) and both POSTed.
    #[tokio::test]
    async fn overlapping_session_authorizations_admit_at_most_one_dispatch() {
        let dir = tempfile::tempdir().expect("dir");
        let store_a = sqlite_store(&dir);
        let store_b = sqlite_store(&dir);
        let session = store_a.open_session(10, &[]).expect("session");
        let server =
            FixtureServer::start(vec![Behavior::Respond(200, "{\"ok\":true}".to_string())]);
        let client = test_client(&server.base_url());
        let first = authorize_delegate(&client, &store_a, SpendAuth::Session(&session), "ws", 10);
        let second = authorize_delegate(&client, &store_b, SpendAuth::Session(&session), "ws", 10);
        let winner = match (first, second) {
            (Ok(auth), Err(_)) => auth,
            (Err(_), Ok(auth)) => auth,
            (Ok(_), Ok(_)) => {
                panic!("both authorizations succeeded — the session was oversubscribed")
            }
            (Err(first), Err(second)) => panic!("both authorizations failed: {first}; {second}"),
        };
        let data = complete_delegate(&client, &store_a, winner, "ws", "agent", "task")
            .await
            .expect("the single authorized dispatch succeeds");
        assert_eq!(data.get("ok").and_then(|value| value.as_bool()), Some(true));
        assert_eq!(remaining(&store_a, &session), 0);
        assert_eq!(
            server.requests_served(),
            1,
            "exactly one POST may reach ABW"
        );
    }

    /// T05 (operator-ratified option A): an ambiguous dispatch outcome HOLDS
    /// the session reservation — the credits stay deducted, the uncertainty
    /// is surfaced, the hold survives reopen, and a retry cannot reserve the
    /// held capacity.
    #[tokio::test]
    async fn ambiguous_dispatch_holds_session_reservation_across_reopen() {
        let dir = tempfile::tempdir().expect("dir");
        let store = sqlite_store(&dir);
        let session = store.open_session(10, &[]).expect("session");
        let server = FixtureServer::start(vec![Behavior::Ambiguous]);
        let client = test_client(&server.base_url());
        let auth = authorize_delegate(&client, &store, SpendAuth::Session(&session), "ws", 10)
            .expect("reserved");
        let error = complete_delegate(&client, &store, auth, "ws", "agent", "task")
            .await
            .expect_err("ambiguous outcome is an error");
        let message = error.to_string();
        assert!(
            message.contains("uncertain"),
            "the uncertainty must be surfaced: {message}"
        );
        assert!(
            message.contains("held"),
            "the held credits must be named: {message}"
        );
        assert_eq!(remaining(&store, &session), 0, "credits stay held");
        // The hold is durable: a fresh store instance (server restart) sees it.
        let reopened = sqlite_store(&dir);
        assert_eq!(
            remaining(&reopened, &session),
            0,
            "the hold survives reopen"
        );
        assert!(
            authorize_delegate(&client, &reopened, SpendAuth::Session(&session), "ws", 10).is_err(),
            "a retry cannot reserve the held credits"
        );
    }

    /// T05 (Q4): an ambiguous dispatch outcome keeps a single-use token
    /// consumed — unreusable by a retry, across reopen.
    #[tokio::test]
    async fn ambiguous_dispatch_keeps_single_use_token_consumed() {
        let dir = tempfile::tempdir().expect("dir");
        let store = sqlite_store(&dir);
        let token = store.mint("delegate", "ws", 10).expect("mint");
        let server = FixtureServer::start(vec![Behavior::Ambiguous]);
        let client = test_client(&server.base_url());
        let auth = authorize_delegate(&client, &store, SpendAuth::SingleUse(&token), "ws", 10)
            .expect("authorized");
        let error = complete_delegate(&client, &store, auth, "ws", "agent", "task")
            .await
            .expect_err("ambiguous outcome is an error");
        assert!(
            error.to_string().contains("uncertain"),
            "the uncertainty must be surfaced: {error}"
        );
        assert!(
            store.consume(&token, "delegate", "ws", 10).is_err(),
            "the token must stay consumed — a refunded token would be reusable"
        );
        let reopened = sqlite_store(&dir);
        assert!(reopened.consume(&token, "delegate", "ws", 10).is_err());
    }

    /// T05 control: a connection-phase failure (nothing listening) proves
    /// the request never left — the reservation is released.
    #[tokio::test]
    async fn connection_refused_releases_session_reservation() {
        let dir = tempfile::tempdir().expect("dir");
        let store = sqlite_store(&dir);
        let session = store.open_session(10, &[]).expect("session");
        let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = probe.local_addr().expect("addr").port();
        drop(probe);
        let client = test_client(&format!("http://127.0.0.1:{port}"));
        let auth = authorize_delegate(&client, &store, SpendAuth::Session(&session), "ws", 10)
            .expect("reserved");
        let _ = complete_delegate(&client, &store, auth, "ws", "agent", "task")
            .await
            .expect_err("connection refused is an error");
        assert_eq!(
            remaining(&store, &session),
            10,
            "proven never-sent releases the reservation"
        );
    }

    /// T05 control: ABW answering with an HTTP error proves the dispatch did
    /// not land — the reservation is released.
    #[tokio::test]
    async fn http_rejection_releases_session_reservation() {
        let dir = tempfile::tempdir().expect("dir");
        let store = sqlite_store(&dir);
        let session = store.open_session(10, &[]).expect("session");
        let server = FixtureServer::start(vec![Behavior::Respond(
            402,
            "insufficient credits".to_string(),
        )]);
        let client = test_client(&server.base_url());
        let auth = authorize_delegate(&client, &store, SpendAuth::Session(&session), "ws", 10)
            .expect("reserved");
        let _ = complete_delegate(&client, &store, auth, "ws", "agent", "task")
            .await
            .expect_err("rejection is an error");
        assert_eq!(
            remaining(&store, &session),
            10,
            "ABW's rejection proves the dispatch did not land"
        );
    }

    /// T05 control: a successful dispatch settles exactly the reserved cost —
    /// the reservation IS the spend; no second deduction.
    #[tokio::test]
    async fn successful_dispatch_reserves_exactly_once() {
        let dir = tempfile::tempdir().expect("dir");
        let store = sqlite_store(&dir);
        let session = store.open_session(20, &[]).expect("session");
        let server =
            FixtureServer::start(vec![Behavior::Respond(200, "{\"ok\":true}".to_string())]);
        let client = test_client(&server.base_url());
        let auth = authorize_delegate(&client, &store, SpendAuth::Session(&session), "ws", 10)
            .expect("reserved");
        complete_delegate(&client, &store, auth, "ws", "agent", "task")
            .await
            .expect("dispatch succeeds");
        assert_eq!(
            remaining(&store, &session),
            10,
            "exactly the reserved cost is spent — no double deduction"
        );
    }

    /// T05: the hire path reserves the re-verified cost atomically — two
    /// overlapping hires against one session admit at most one POST.
    /// Pre-fix, both authorized (validate-only + non-atomic balance read).
    #[tokio::test]
    async fn overlapping_hire_authorizations_admit_at_most_one_dispatch() {
        let dir = tempfile::tempdir().expect("dir");
        let store_a = sqlite_store(&dir);
        let store_b = sqlite_store(&dir);
        let session = store_a.open_session(10, &[]).expect("session");
        // Two dependency re-verifies (one per authorize) + one hire POST.
        let server = FixtureServer::start(vec![
            Behavior::Respond(200, "{\"total_hire_cost\":10}".to_string()),
            Behavior::Respond(200, "{\"total_hire_cost\":10}".to_string()),
            Behavior::Respond(200, "{\"ok\":true}".to_string()),
        ]);
        let client = test_client(&server.base_url());
        let first = authorize_hire(
            &client,
            &store_a,
            SpendAuth::Session(&session),
            "agent",
            0,
            None,
            false,
        )
        .await;
        let second = authorize_hire(
            &client,
            &store_b,
            SpendAuth::Session(&session),
            "agent",
            0,
            None,
            false,
        )
        .await;
        let winner = match (first, second) {
            (Ok(auth), Err(_)) => auth,
            (Err(_), Ok(auth)) => auth,
            (Ok(_), Ok(_)) => {
                panic!("both hire authorizations succeeded — the session was oversubscribed")
            }
            (Err(first), Err(second)) => panic!("both hires failed: {first}; {second}"),
        };
        complete_hire(&client, &store_a, winner, "ws", "agent", false)
            .await
            .expect("the single authorized hire succeeds");
        assert_eq!(remaining(&store_a, &session), 0);
        assert_eq!(
            server.requests_served(),
            3,
            "two dependency re-verifies plus exactly one hire POST"
        );
    }
}
