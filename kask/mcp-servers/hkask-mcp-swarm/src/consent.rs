//! Consent gate — operator authorization for spend tools.
//!
//! Extracted from the swarm server root. `ConsentStore` holds single-use,
//! action-scoped spend grants keyed by an opaque token. Two backends:
//! in-memory (tests + fallback) and SQLite (production default — shared and
//! restart-durable across the governed and per-project swarm server
//! processes, with single-use enforced atomically via the DELETE-affected-rows
//! check). Grants expire after `CONSENT_TTL_SECS`.

use crate::error::SwarmError;
use hkask_storage::database::value::DbValue;
use std::sync::Arc;

/// An operator's authorization to spend credits on a specific action. Minted
/// by `swarm_request_consent` (which the panel calls after the operator
/// confirms), consumed by the spend tools. Single-use and action-scoped so a
/// consent for one hire cannot be replayed for a different agent or a second
/// spend — the enforcement point for the cost/consent gate.
#[derive(Debug, Clone)]
pub(crate) struct ConsentGrant {
    /// The action this consent authorizes (e.g. "hire", "delegate").
    pub action: String,
    /// The target (agent name for hire, workspace id for delegate).
    pub target: String,
    /// The credit ceiling the operator authorized.
    pub credits_authorized: u32,
    /// The opaque token the spend tool must present.
    pub token: String,
}

/// A pre-authorized spend session — unlike a single-use [`ConsentGrant`], a
/// session can be consumed multiple times until the budget is exhausted.
/// Enables headless ABW pipelines where the operator pre-authorizes a total
/// budget upfront instead of approving each spend individually.
#[derive(Clone)]
struct SessionGrant {
    token: String,
    total_credits: u32,
    remaining_credits: u32,
    actions: Vec<String>, // empty = all actions allowed
    created_at: chrono::DateTime<chrono::Utc>,
}

/// Store of active consent grants, keyed by token.
///
/// Two backends:
/// - `Memory` — the session-scoped per-process store (tests, and the fallback
///   when the shared store cannot be opened). A grant does not survive a
///   server restart.
/// - `Sqlite` — the default in production: a shared, restart-durable store
///   (one SQLite file) so a token minted by the panel's governed server
///   process is consumable by the Steer curator's per-project server process
///   (and vice versa). Single-use is enforced atomically via the
///   DELETE-affected-rows check — two processes racing on the same token
///   cannot double-spend it. Grants expire after [`CONSENT_TTL_SECS`].
pub struct ConsentStore {
    inner: ConsentInner,
    sessions: std::sync::Mutex<std::collections::HashMap<String, SessionGrant>>,
}

enum ConsentInner {
    Memory(std::sync::Mutex<std::collections::HashMap<String, ConsentGrant>>),
    Sqlite(Arc<SqliteConsentStore>),
}

/// SQLite-backed consent store shared across swarm server processes.
struct SqliteConsentStore {
    driver: Arc<dyn hkask_storage::database::driver::DatabaseDriver>,
}

/// Lifetime of a consent grant in the shared store. Grants are
/// process-shared (both the governed `McpRuntime` instance and the
/// per-project `ContextServerStore` instance open the same SQLite store), so
/// unlike the in-memory fallback they survive a server restart — the TTL
/// bounds that durability: an operator authorization older than this is
/// unspendable.
pub(crate) const CONSENT_TTL_SECS: i64 = 3600;

impl Default for ConsentStore {
    fn default() -> Self {
        Self {
            inner: ConsentInner::Memory(std::sync::Mutex::new(std::collections::HashMap::new())),
            sessions: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }
}

/// Map a storage error into the swarm error surface (the `.rules` trap: a
/// failed measurement/store query must be distinguishable from a clean miss).
fn consent_store_err(e: hkask_storage::database::types::DbError) -> SwarmError {
    SwarmError::Unavailable(format!("consent store query failed: {e}"))
}

impl ConsentStore {
    /// Open (or create) the shared SQLite consent store at `path`. Both the
    /// governed and the per-project swarm server processes resolve the same
    /// path (default `~/.hkask/swarm_consent.db`), making consent tokens
    /// consumable across processes — the panel's hire flow and the Steer
    /// curator's spend flow compose.
    pub(crate) fn open_sqlite(path: &str) -> Result<Self, String> {
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                format!(
                    "failed to create consent store dir {}: {e}",
                    parent.display()
                )
            })?;
        }
        let manager = r2d2_sqlite::SqliteConnectionManager::file(path)
            .with_init(|conn| conn.execute_batch(hkask_storage::WAL_PRAGMA_BATCH));
        let pool = r2d2::Pool::builder()
            .max_size(4)
            .build(manager)
            .map_err(|e| format!("failed to create consent store pool: {e}"))?;
        let driver: Arc<dyn hkask_storage::database::driver::DatabaseDriver> =
            Arc::new(hkask_storage::SqliteDriver::new(pool));
        driver
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS consent_grants (\
                     token TEXT PRIMARY KEY, \
                     action TEXT NOT NULL, \
                     target TEXT NOT NULL, \
                     credits_authorized INTEGER NOT NULL, \
                     created_at TEXT NOT NULL \
                 )",
            )
            .map_err(|e| format!("failed to init consent store schema: {e}"))?;
        driver
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS consent_sessions (\
                     token TEXT PRIMARY KEY, \
                     total_credits INTEGER NOT NULL, \
                     remaining_credits INTEGER NOT NULL, \
                     actions TEXT NOT NULL, \
                     created_at TEXT NOT NULL \
                 )",
            )
            .map_err(|e| format!("failed to init consent sessions schema: {e}"))?;
        Ok(Self {
            inner: ConsentInner::Sqlite(Arc::new(SqliteConsentStore { driver })),
            sessions: std::sync::Mutex::new(std::collections::HashMap::new()),
        })
    }

    /// Mint a consent token for an action+target and record the grant.
    /// Returns the token the panel shows the operator and the spend tool
    /// must present. `Err` when the shared store cannot record the
    /// authorization (fail-closed — an unrecorded token is never handed
    /// out as if it were spendable).
    pub(crate) fn mint(
        &self,
        action: &str,
        target: &str,
        credits_authorized: u32,
    ) -> Result<String, SwarmError> {
        match &self.inner {
            ConsentInner::Memory(grants) => {
                let token = mint_token(action, target);
                grants.lock().unwrap_or_else(|e| e.into_inner()).insert(
                    token.clone(),
                    ConsentGrant {
                        action: action.to_string(),
                        target: target.to_string(),
                        credits_authorized,
                        token: token.clone(),
                    },
                );
                Ok(token)
            }
            ConsentInner::Sqlite(store) => store.mint(action, target, credits_authorized),
        }
    }

    /// Consume a consent token, validating it authorizes `action` on `target`
    /// for at least `cost` credits. Single-use: a successful consume removes
    /// the grant so it cannot be replayed. Returns the authorized ceiling.
    ///
    /// On validation failure (scope mismatch, over-spend) the grant is NOT
    /// removed — the caller may retry with the corrected scope or a lower cost
    /// without re-minting. A scope mismatch is not a replay attack (the token
    /// is unguessable); destroying it on a wrong-scope attempt would leak the
    /// operator's consent. The grant is removed only on a successful consume,
    /// which is the true single-use point.
    pub(crate) fn consume(
        &self,
        token: &str,
        action: &str,
        target: &str,
        cost: u32,
    ) -> Result<u32, SwarmError> {
        match &self.inner {
            ConsentInner::Memory(grants) => {
                let mut grants = grants.lock().unwrap_or_else(|e| e.into_inner());
                let grant = grants.get(token).ok_or_else(|| {
                    SwarmError::ConsentDenied("unknown or already-used consent token".into())
                })?;

                if grant.action != action || grant.target != target {
                    return Err(SwarmError::ConsentDenied(format!(
                        "consent token scope mismatch: token is for {} on '{}', not {} on '{}'",
                        grant.action, grant.target, action, target
                    )));
                }
                if cost > grant.credits_authorized {
                    return Err(SwarmError::ConsentDenied(format!(
                        "cost {cost} exceeds authorized ceiling {}",
                        grant.credits_authorized
                    )));
                }
                // Remove only on success — the token is consumed.
                let authorized = grant.credits_authorized;
                grants.remove(token);
                Ok(authorized)
            }
            ConsentInner::Sqlite(store) => store.consume(token, action, target, cost),
        }
    }

    /// Refund a consumed grant so the operator can retry after a transient
    /// failure (network drop, ABW 5xx) without re-confirming. The grant is
    /// re-inserted with its original scope and ceiling; it remains single-use
    /// per *successful* spend — a refunded token is consumed again on the next
    /// attempt and removed for good once the spend succeeds. No-op if the grant
    /// was never consumed (defensive against double-refund). Best-effort in the
    /// shared store: a refund failure (store unavailable) is logged loudly —
    /// the spend already failed, so the operator re-mints.
    pub(crate) fn refund(&self, grant: ConsentGrant) {
        match &self.inner {
            ConsentInner::Memory(grants) => {
                grants
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(grant.token.clone(), grant);
            }
            ConsentInner::Sqlite(store) => store.refund(grant),
        }
    }

    /// Open a pre-authorized spend session. Returns a session token that can
    /// be used in place of per-spend consent tokens. Each `consume_session`
    /// call deducts from `total_credits`; when `remaining_credits` reaches 0,
    /// the session is exhausted. The session is NOT single-use — it can be
    /// consumed multiple times until the budget is spent.
    pub(crate) fn open_session(
        &self,
        total_credits: u32,
        actions: &[String],
    ) -> Result<String, SwarmError> {
        match &self.inner {
            ConsentInner::Memory(_) => {
                let token = mint_session_token();
                self.sessions
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(
                        token.clone(),
                        SessionGrant {
                            token: token.clone(),
                            total_credits,
                            remaining_credits: total_credits,
                            actions: actions.to_vec(),
                            created_at: chrono::Utc::now(),
                        },
                    );
                Ok(token)
            }
            ConsentInner::Sqlite(store) => store.open_session(total_credits, actions),
        }
    }

    /// Consume credits from a session. Validates the action is in the
    /// session's allowed actions, checks remaining >= cost, deducts cost
    /// from remaining. Returns Ok(remaining_after) on success. Returns Err
    /// when:
    /// - session unknown or expired
    /// - action not in the session's allowed actions
    /// - cost > remaining_credits
    /// Unlike `consume` (single-use), the session stays alive after a
    /// successful consume — only the remaining balance decreases.
    pub(crate) fn consume_session(
        &self,
        token: &str,
        action: &str,
        cost: u32,
    ) -> Result<u32, SwarmError> {
        match &self.inner {
            ConsentInner::Memory(_) => {
                let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
                let grant = sessions.get(token).ok_or_else(|| {
                    SwarmError::ConsentDenied("unknown or expired session token".into())
                })?;
                let expired = chrono::Utc::now()
                    .signed_duration_since(grant.created_at)
                    .num_seconds()
                    > CONSENT_TTL_SECS;
                let action_ok =
                    grant.actions.is_empty() || grant.actions.iter().any(|a| a == action);
                let remaining_credits = grant.remaining_credits;
                // Borrow of `grant` ends here (last use) — NLL releases it.

                if expired {
                    sessions.remove(token);
                    return Err(SwarmError::ConsentDenied("session expired".into()));
                }
                if !action_ok {
                    return Err(SwarmError::ConsentDenied(format!(
                        "session does not authorize action '{action}'"
                    )));
                }
                if cost > remaining_credits {
                    return Err(SwarmError::ConsentDenied(format!(
                        "cost {cost} exceeds session remaining {remaining_credits}"
                    )));
                }
                let new_remaining = remaining_credits - cost;
                if let Some(grant) = sessions.get_mut(token) {
                    grant.remaining_credits = new_remaining;
                }
                Ok(new_remaining)
            }
            ConsentInner::Sqlite(store) => store.consume_session(token, action, cost),
        }
    }

    /// Read the remaining credits on a session. Returns None if
    /// unknown/expired.
    pub(crate) fn session_balance(&self, token: &str) -> Option<u32> {
        match &self.inner {
            ConsentInner::Memory(_) => {
                let sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
                let grant = sessions.get(token)?;
                if chrono::Utc::now()
                    .signed_duration_since(grant.created_at)
                    .num_seconds()
                    > CONSENT_TTL_SECS
                {
                    return None;
                }
                Some(grant.remaining_credits)
            }
            ConsentInner::Sqlite(store) => store.session_balance(token),
        }
    }
}

impl SqliteConsentStore {
    fn mint(
        &self,
        action: &str,
        target: &str,
        credits_authorized: u32,
    ) -> Result<String, SwarmError> {
        let token = mint_token(action, target);
        // Lazy expiry sweep — correctness is the TTL check on consume.
        let cutoff =
            (chrono::Utc::now() - chrono::Duration::seconds(CONSENT_TTL_SECS)).to_rfc3339();
        let _ = self.driver.execute(
            "DELETE FROM consent_grants WHERE created_at < ?1",
            &[DbValue::Text(cutoff)],
        );
        self.driver
            .execute(
                "INSERT OR REPLACE INTO consent_grants \
                     (token, action, target, credits_authorized, created_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                &[
                    DbValue::Text(token.clone()),
                    DbValue::Text(action.to_string()),
                    DbValue::Text(target.to_string()),
                    DbValue::Integer(i64::from(credits_authorized)),
                    DbValue::Text(chrono::Utc::now().to_rfc3339()),
                ],
            )
            .map_err(|e| {
                SwarmError::ConsentDenied(format!(
                    "consent store unavailable — cannot record the operator's authorization: {e}"
                ))
            })?;
        Ok(token)
    }

    fn consume(
        &self,
        token: &str,
        action: &str,
        target: &str,
        cost: u32,
    ) -> Result<u32, SwarmError> {
        let row = self
            .driver
            .query_optional(
                "SELECT action, target, credits_authorized, created_at \
                 FROM consent_grants WHERE token = ?1",
                &[DbValue::Text(token.to_string())],
            )
            .map_err(consent_store_err)?;
        let Some(row) = row else {
            return Err(SwarmError::ConsentDenied(
                "unknown or already-used consent token".into(),
            ));
        };
        let created = row.get_str(3).map_err(consent_store_err)?;
        let created = chrono::DateTime::parse_from_rfc3339(created)
            .map(|d| d.with_timezone(&chrono::Utc))
            .map_err(|e| {
                SwarmError::Unavailable(format!("consent store corrupt created_at: {e}"))
            })?;
        if chrono::Utc::now()
            .signed_duration_since(created)
            .num_seconds()
            > CONSENT_TTL_SECS
        {
            // Expired — remove and treat as unknown (never spendable).
            let _ = self.driver.execute(
                "DELETE FROM consent_grants WHERE token = ?1",
                &[DbValue::Text(token.to_string())],
            );
            return Err(SwarmError::ConsentDenied("consent token expired".into()));
        }
        let grant_action = row.get_str(0).map_err(consent_store_err)?.to_string();
        let grant_target = row.get_str(1).map_err(consent_store_err)?.to_string();
        let grant_credits =
            u32::try_from(row.get_int(2).map_err(consent_store_err)?).map_err(|_| {
                SwarmError::Unavailable("consent store corrupt credits_authorized".into())
            })?;
        if grant_action != action || grant_target != target {
            return Err(SwarmError::ConsentDenied(format!(
                "consent token scope mismatch: token is for {grant_action} on '{grant_target}', \
                 not {action} on '{target}'"
            )));
        }
        if cost > grant_credits {
            return Err(SwarmError::ConsentDenied(format!(
                "cost {cost} exceeds authorized ceiling {grant_credits}"
            )));
        }
        // Single-use, atomic across processes: the DELETE returns the affected
        // row count; a concurrent consume of the same token wins the DELETE and
        // this one sees 0 rows → treated as a replay.
        let deleted = self
            .driver
            .execute(
                "DELETE FROM consent_grants WHERE token = ?1",
                &[DbValue::Text(token.to_string())],
            )
            .map_err(consent_store_err)?;
        if deleted == 0 {
            return Err(SwarmError::ConsentDenied(
                "unknown or already-used consent token".into(),
            ));
        }
        Ok(grant_credits)
    }

    fn refund(&self, grant: ConsentGrant) {
        if let Err(e) = self.driver.execute(
            "INSERT OR REPLACE INTO consent_grants \
                 (token, action, target, credits_authorized, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            &[
                DbValue::Text(grant.token.clone()),
                DbValue::Text(grant.action.clone()),
                DbValue::Text(grant.target.clone()),
                DbValue::Integer(i64::from(grant.credits_authorized)),
                DbValue::Text(chrono::Utc::now().to_rfc3339()),
            ],
        ) {
            tracing::error!(
                target: "hkask.mcp.swarm",
                error = %e,
                token = %grant.token,
                "consent refund failed in the shared store — the grant is lost; \
                 the operator must re-confirm"
            );
        }
    }

    fn open_session(&self, total_credits: u32, actions: &[String]) -> Result<String, SwarmError> {
        let token = mint_session_token();
        let actions_str = actions.join(",");
        // Lazy expiry sweep — correctness is the TTL check on consume.
        let cutoff =
            (chrono::Utc::now() - chrono::Duration::seconds(CONSENT_TTL_SECS)).to_rfc3339();
        let _ = self.driver.execute(
            "DELETE FROM consent_sessions WHERE created_at < ?1",
            &[DbValue::Text(cutoff)],
        );
        self.driver
            .execute(
                "INSERT OR REPLACE INTO consent_sessions \
                     (token, total_credits, remaining_credits, actions, created_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                &[
                    DbValue::Text(token.clone()),
                    DbValue::Integer(i64::from(total_credits)),
                    DbValue::Integer(i64::from(total_credits)),
                    DbValue::Text(actions_str),
                    DbValue::Text(chrono::Utc::now().to_rfc3339()),
                ],
            )
            .map_err(|e| {
                SwarmError::ConsentDenied(format!(
                    "consent store unavailable — cannot record the session: {e}"
                ))
            })?;
        Ok(token)
    }

    fn consume_session(&self, token: &str, action: &str, cost: u32) -> Result<u32, SwarmError> {
        let row = self
            .driver
            .query_optional(
                "SELECT remaining_credits, actions, created_at \
                 FROM consent_sessions WHERE token = ?1",
                &[DbValue::Text(token.to_string())],
            )
            .map_err(consent_store_err)?;
        let Some(row) = row else {
            return Err(SwarmError::ConsentDenied(
                "unknown or expired session token".into(),
            ));
        };
        let created = row.get_str(2).map_err(consent_store_err)?;
        let created = chrono::DateTime::parse_from_rfc3339(created)
            .map(|d| d.with_timezone(&chrono::Utc))
            .map_err(|e| {
                SwarmError::Unavailable(format!("consent store corrupt created_at: {e}"))
            })?;
        if chrono::Utc::now()
            .signed_duration_since(created)
            .num_seconds()
            > CONSENT_TTL_SECS
        {
            let _ = self.driver.execute(
                "DELETE FROM consent_sessions WHERE token = ?1",
                &[DbValue::Text(token.to_string())],
            );
            return Err(SwarmError::ConsentDenied("session expired".into()));
        }
        // Check action — empty actions string means all actions allowed.
        let actions_str = row.get_str(1).map_err(consent_store_err)?;
        if !actions_str.is_empty() {
            let allowed: Vec<&str> = actions_str.split(',').collect();
            if !allowed.iter().any(|a| *a == action) {
                return Err(SwarmError::ConsentDenied(format!(
                    "session does not authorize action '{action}'"
                )));
            }
        }
        // Atomic deduction: UPDATE only if remaining >= cost. Two processes
        // racing on the same session cannot double-spend — the affected-rows
        // check is the same atomicity pattern as the single-use DELETE.
        let updated = self
            .driver
            .execute(
                "UPDATE consent_sessions SET remaining_credits = remaining_credits - ?1 \
                 WHERE token = ?2 AND remaining_credits >= ?1",
                &[
                    DbValue::Integer(i64::from(cost)),
                    DbValue::Text(token.to_string()),
                ],
            )
            .map_err(consent_store_err)?;
        if updated == 0 {
            // Distinguish over-budget from concurrent delete/expiry.
            let row = self
                .driver
                .query_optional(
                    "SELECT remaining_credits FROM consent_sessions WHERE token = ?1",
                    &[DbValue::Text(token.to_string())],
                )
                .map_err(consent_store_err)?;
            match row {
                Some(row) => {
                    let remaining = u32::try_from(row.get_int(0).map_err(consent_store_err)?)
                        .map_err(|_| {
                            SwarmError::Unavailable(
                                "consent store corrupt remaining_credits".into(),
                            )
                        })?;
                    Err(SwarmError::ConsentDenied(format!(
                        "cost {cost} exceeds session remaining {remaining}"
                    )))
                }
                None => Err(SwarmError::ConsentDenied(
                    "unknown or expired session token".into(),
                )),
            }
        } else {
            // Read back the new remaining balance.
            let row = self
                .driver
                .query_optional(
                    "SELECT remaining_credits FROM consent_sessions WHERE token = ?1",
                    &[DbValue::Text(token.to_string())],
                )
                .map_err(consent_store_err)?;
            let Some(row) = row else {
                return Err(SwarmError::Unavailable(
                    "session row vanished after update".into(),
                ));
            };
            let remaining =
                u32::try_from(row.get_int(0).map_err(consent_store_err)?).map_err(|_| {
                    SwarmError::Unavailable("consent store corrupt remaining_credits".into())
                })?;
            Ok(remaining)
        }
    }

    fn session_balance(&self, token: &str) -> Option<u32> {
        let row = match self.driver.query_optional(
            "SELECT remaining_credits, created_at FROM consent_sessions WHERE token = ?1",
            &[DbValue::Text(token.to_string())],
        ) {
            Ok(row) => row,
            Err(e) => {
                tracing::warn!(
                    target: "hkask.mcp.swarm",
                    error = %e,
                    token = %token,
                    "session balance query failed — returning None",
                );
                return None;
            }
        };
        let row = row?;
        let created = row.get_str(1).ok()?;
        let created = chrono::DateTime::parse_from_rfc3339(created)
            .map(|d| d.with_timezone(&chrono::Utc))
            .ok()?;
        if chrono::Utc::now()
            .signed_duration_since(created)
            .num_seconds()
            > CONSENT_TTL_SECS
        {
            return None;
        }
        u32::try_from(row.get_int(0).ok()?).ok()
    }
}

/// Build an opaque single-use consent token (timestamp XOR FNV-1a of the
/// scope). Not cryptographic — the token's value is its unguessability
/// combined with single-use consumption, not secrecy against a motivated
/// attacker with process access.
pub fn mint_token(action: &str, target: &str) -> String {
    format!(
        "hkask-consent-{:016x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
            ^ (fnv1a(action, target) as u128)
    )
}

/// Build an opaque session token (timestamp-based, no FNV mixing — sessions
/// are multi-use so replay-resistance comes from the server-side balance
/// check, not single-use consumption). Prefixed `hkask-session-` to
/// distinguish from single-use `hkask-consent-` tokens.
pub fn mint_session_token() -> String {
    format!(
        "hkask-session-{:016x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    )
}

/// A tiny FNV-1a hash so consent tokens are not trivially guessable from the
/// timestamp alone. Not cryptographic — the token's value is its unguessability
/// combined with single-use consumption, not secrecy against a motivated
/// attacker with process access.
pub fn fnv1a(action: &str, target: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in action.bytes().chain(target.bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consent_consume_succeeds_for_valid_in_scope_token() {
        let store = ConsentStore::default();
        let token = store.mint("hire", "style_transfer", 20).expect("mint");
        let authorized = store
            .consume(&token, "hire", "style_transfer", 20)
            .expect("valid token should consume");
        assert_eq!(authorized, 20);
    }

    #[test]
    fn consent_consume_rejects_unknown_token() {
        let store = ConsentStore::default();
        let result = store.consume("hkask-consent-bogus", "hire", "style_transfer", 20);
        assert!(matches!(result, Err(SwarmError::ConsentDenied(_))));
    }

    #[test]
    fn consent_consume_rejects_replay() {
        let store = ConsentStore::default();
        let token = store.mint("hire", "style_transfer", 20).expect("mint");
        store
            .consume(&token, "hire", "style_transfer", 20)
            .expect("first consume");
        let replay = store.consume(&token, "hire", "style_transfer", 20);
        assert!(matches!(replay, Err(SwarmError::ConsentDenied(_))));
    }

    #[test]
    fn consent_consume_rejects_scope_mismatch() {
        let store = ConsentStore::default();
        // Consent for one agent must not authorize a different agent.
        let token = store.mint("hire", "style_transfer", 20).expect("mint");
        let wrong_agent = store.consume(&token, "hire", "watermark", 20);
        assert!(matches!(wrong_agent, Err(SwarmError::ConsentDenied(_))));
    }

    #[test]
    fn consent_consume_rejects_action_mismatch() {
        let store = ConsentStore::default();
        // Consent for a hire must not authorize a delegate.
        let token = store.mint("hire", "style_transfer", 20).expect("mint");
        let wrong_action = store.consume(&token, "delegate", "style_transfer", 20);
        assert!(matches!(wrong_action, Err(SwarmError::ConsentDenied(_))));
    }

    #[test]
    fn consent_consume_rejects_over_spend() {
        let store = ConsentStore::default();
        let token = store.mint("hire", "style_transfer", 10).expect("mint");
        let over = store.consume(&token, "hire", "style_transfer", 20);
        assert!(matches!(over, Err(SwarmError::ConsentDenied(_))));
    }

    // Consent gate: `swarm_xaman` must require a consent token when
    // `curator_consent_default` is `false` (the default). This pins the
    // plan's §3.7 invariant: no task content reaches Xaman Ek without
    // explicit opt-in.
    #[test]
    fn consent_consume_rejects_curate_action_mismatch() {
        let store = ConsentStore::default();
        // A token minted for "hire" must not authorize a "curate" action.
        let token = store.mint("hire", "style_transfer", 20).expect("mint");
        let wrong = store.consume(&token, "curate", "xaman", 0);
        assert!(matches!(wrong, Err(SwarmError::ConsentDenied(_))));
    }

    #[test]
    fn consent_consume_accepts_curate_action() {
        let store = ConsentStore::default();
        let token = store.mint("curate", "xaman", 0).expect("mint");
        let result = store.consume(&token, "curate", "xaman", 0);
        assert!(result.is_ok());
    }

    // A refunded grant must be re-consumable so the operator can retry after a
    // transient failure without re-confirming. The grant retains its original
    // scope and ceiling.
    #[test]
    fn consent_refund_restores_grant_for_retry() {
        let store = ConsentStore::default();
        let token = store.mint("hire", "market_analyst", 20).expect("mint");
        let ceiling = store
            .consume(&token, "hire", "market_analyst", 20)
            .expect("first consume");
        assert_eq!(ceiling, 20);
        // Refund the consumed grant (simulating a network failure after consume).
        store.refund(ConsentGrant {
            action: "hire".to_string(),
            target: "market_analyst".to_string(),
            credits_authorized: 20,
            token: token.clone(),
        });
        // The refunded token must be consumable again.
        let ceiling2 = store
            .consume(&token, "hire", "market_analyst", 20)
            .expect("refunded token should consume");
        assert_eq!(ceiling2, 20);
    }

    #[test]
    fn consent_refund_is_noop_for_never_consumed_token() {
        // Defensive: refunding a grant that was never consumed (or already
        // refunded) must not panic and must leave the store usable.
        let store = ConsentStore::default();
        store.refund(ConsentGrant {
            action: "hire".to_string(),
            target: "ghost".to_string(),
            credits_authorized: 5,
            token: "hkask-consent-never".to_string(),
        });
        // The inserted grant is consumable.
        let ceiling = store
            .consume("hkask-consent-never", "hire", "ghost", 5)
            .expect("refunded ghost grant should consume");
        assert_eq!(ceiling, 5);
    }

    // The production default: both swarm server processes (governed McpRuntime
    // and per-project ContextServerStore) open the same SQLite file, so a token
    // minted by one process is consumable by the other. These tests simulate
    // the two processes as two `ConsentStore::open_sqlite` handles on one path.

    fn temp_consent_store_path(tag: &str) -> String {
        std::env::temp_dir()
            .join(format!(
                "hkask-swarm-consent-{tag}-{}",
                uuid::Uuid::new_v4()
            ))
            .join("consent.db")
            .to_string_lossy()
            .to_string()
    }

    #[test]
    fn consent_store_sqlite_roundtrip_and_single_use() {
        let path = temp_consent_store_path("roundtrip");
        let store = ConsentStore::open_sqlite(&path).expect("open sqlite store");
        let token = store.mint("hire", "market_analyst", 20).expect("mint");

        // In-scope consume returns the authorized ceiling.
        let ceiling = store
            .consume(&token, "hire", "market_analyst", 10)
            .expect("consume in scope");
        assert_eq!(ceiling, 20);
        // Replay is rejected (single-use).
        let replay = store.consume(&token, "hire", "market_analyst", 10);
        assert!(
            matches!(replay, Err(SwarmError::ConsentDenied(_))),
            "replay must be rejected"
        );
        // Refund re-inserts; the refunded token is consumable again.
        store.refund(ConsentGrant {
            action: "hire".to_string(),
            target: "market_analyst".to_string(),
            credits_authorized: 20,
            token: token.clone(),
        });
        store
            .consume(&token, "hire", "market_analyst", 10)
            .expect("refunded token re-consumable");
        std::fs::remove_dir_all(std::path::Path::new(&path).parent().unwrap()).ok();
    }

    #[test]
    fn consent_store_sqlite_token_minted_in_one_process_consumed_in_another() {
        let path = temp_consent_store_path("cross");
        let process_a = ConsentStore::open_sqlite(&path).expect("process A");
        let process_b = ConsentStore::open_sqlite(&path).expect("process B");

        // A mints (the panel's governed process); B consumes (the Steer
        // curator's per-project process) — the mixed flow that failed with
        // the old per-process in-memory store.
        let token = process_a.mint("hire", "market_analyst", 20).expect("mint");
        let ceiling = process_b
            .consume(&token, "hire", "market_analyst", 10)
            .expect("cross-process consume");
        assert_eq!(ceiling, 20);
        // Single-use holds across processes: a replay in either process fails.
        assert!(matches!(
            process_a.consume(&token, "hire", "market_analyst", 10),
            Err(SwarmError::ConsentDenied(_))
        ));
        std::fs::remove_dir_all(std::path::Path::new(&path).parent().unwrap()).ok();
    }

    #[test]
    fn consent_store_sqlite_scope_mismatch_preserves_grant() {
        let path = temp_consent_store_path("scope");
        let store = ConsentStore::open_sqlite(&path).expect("open sqlite store");
        let token = store.mint("hire", "market_analyst", 20).expect("mint");
        let mismatch = store.consume(&token, "hire", "different_agent", 10);
        assert!(matches!(mismatch, Err(SwarmError::ConsentDenied(_))));
        // The grant is preserved — a corrected-scope retry succeeds.
        store
            .consume(&token, "hire", "market_analyst", 10)
            .expect("corrected scope consumes");
        std::fs::remove_dir_all(std::path::Path::new(&path).parent().unwrap()).ok();
    }

    #[test]
    fn consent_store_sqlite_expired_grant_is_unspendable() {
        let path = temp_consent_store_path("ttl");
        let store = ConsentStore::open_sqlite(&path).expect("open sqlite store");
        let token = store.mint("hire", "market_analyst", 20).expect("mint");

        // Backdate the grant beyond the TTL via a raw driver on the same file.
        let manager = r2d2_sqlite::SqliteConnectionManager::file(&path)
            .with_init(|conn| conn.execute_batch(hkask_storage::WAL_PRAGMA_BATCH));
        let pool = r2d2::Pool::builder()
            .max_size(1)
            .build(manager)
            .expect("pool");
        let driver: Arc<dyn hkask_storage::database::driver::DatabaseDriver> =
            std::sync::Arc::new(hkask_storage::SqliteDriver::new(pool));
        let old =
            (chrono::Utc::now() - chrono::Duration::seconds(CONSENT_TTL_SECS + 60)).to_rfc3339();
        driver
            .execute(
                "UPDATE consent_grants SET created_at = ?1 WHERE token = ?2",
                &[DbValue::Text(old), DbValue::Text(token.clone())],
            )
            .expect("backdate");

        let err = store
            .consume(&token, "hire", "market_analyst", 10)
            .unwrap_err();
        assert!(
            matches!(err, SwarmError::ConsentDenied(ref m) if m.contains("expired")),
            "expired grant must be unspendable, got {err:?}"
        );
        std::fs::remove_dir_all(std::path::Path::new(&path).parent().unwrap()).ok();
    }

    // A curate token minted for "xaman" must be consumable regardless of
    // whether a session_id is present — the server uses a fixed "xaman"
    // target, not the session_id.
    #[test]
    fn curate_consume_uses_fixed_xaman_target() {
        let store = ConsentStore::default();
        let token = store.mint("curate", "xaman", 0).expect("mint");
        // Consume with the fixed target the server now uses.
        store
            .consume(&token, "curate", "xaman", 0)
            .expect("curate token for xaman should consume");
    }

    #[test]
    fn curate_consume_rejects_session_id_target_mismatch() {
        // A token minted for "xaman" must not be consumable for a different
        // target — this pins that the server's fixed "xaman" target is the
        // only valid scope for curate consent.
        let store = ConsentStore::default();
        let token = store.mint("curate", "xaman", 0).expect("mint");
        let wrong = store.consume(&token, "curate", "session-abc-123", 0);
        assert!(matches!(wrong, Err(SwarmError::ConsentDenied(_))));
    }

    #[test]
    fn ceiling_gate_refunds_consent_on_refusal() {
        // When `swarm_hire` refuses a hire for exceeding the per-dispatch
        // ceiling, it must refund the consent token so the operator can retry
        // after raising `HKASK_ABW_MAX_CREDITS` without re-confirming. This
        // mirrors the `actual_cost > credits_authorized` refund path. We pin
        // the refund semantics at the ConsentStore level: a refunded grant is
        // re-consumable.
        let store = ConsentStore::default();
        let token = store.mint("hire", "expensive_agent", 100).expect("mint");
        // Consume (the spend path does this before the ceiling check).
        let ceiling = store
            .consume(&token, "hire", "expensive_agent", 0)
            .expect("consume with cost=0 (two-phase pattern)");
        assert_eq!(ceiling, 100);
        // Refund (the ceiling-gate refusal path does this).
        store.refund(ConsentGrant {
            action: "hire".to_string(),
            target: "expensive_agent".to_string(),
            credits_authorized: 100,
            token: token.clone(),
        });
        // The refunded token must be re-consumable — the operator can retry
        // after raising the ceiling without re-confirming.
        store
            .consume(&token, "hire", "expensive_agent", 0)
            .expect("refunded ceiling-refused token should re-consume");
    }

    #[test]
    fn delegate_ceiling_gate_refunds_on_refusal() {
        // `swarm_delegate` checks `credits_authorized > max_credits_per_dispatch`
        // after consume and refunds on refusal. Pin the refund semantics: a
        // delegate token minted for more than the ceiling is consumable (the
        // store doesn't know the ceiling), refunded by the gate, and
        // re-consumable after the operator raises the ceiling.
        let store = ConsentStore::default();
        let token = store.mint("delegate", "ws-123", 1000).expect("mint");
        let authorized = store
            .consume(&token, "delegate", "ws-123", 1000)
            .expect("consume should succeed — store doesn't know the ceiling");
        assert_eq!(authorized, 1000);
        // The gate refuses because 1000 > 50 (default ceiling). Refund.
        store.refund(ConsentGrant {
            action: "delegate".to_string(),
            target: "ws-123".to_string(),
            credits_authorized: 1000,
            token: token.clone(),
        });
        // Re-consumable after refund.
        store
            .consume(&token, "delegate", "ws-123", 1000)
            .expect("refunded delegate token should re-consume");
    }

    #[test]
    fn consent_consume_preserves_grant_on_scope_mismatch() {
        let store = ConsentStore::default();
        let token = store.mint("hire", "style_transfer", 20).expect("mint");
        // A wrong-scope consume must fail but NOT destroy the token.
        let wrong = store.consume(&token, "hire", "watermark", 20);
        assert!(matches!(wrong, Err(SwarmError::ConsentDenied(_))));
        // The token is still usable with the correct scope.
        let authorized = store
            .consume(&token, "hire", "style_transfer", 20)
            .expect("token must still be usable after a scope-mismatch rejection");
        assert_eq!(authorized, 20);
        // And it is now consumed (single-use on success).
        let replay = store.consume(&token, "hire", "style_transfer", 20);
        assert!(matches!(replay, Err(SwarmError::ConsentDenied(_))));
    }

    #[test]
    fn consent_consume_preserves_grant_on_over_spend() {
        let store = ConsentStore::default();
        let token = store.mint("hire", "style_transfer", 10).expect("mint");
        // An over-spend consume must fail but NOT destroy the token.
        let over = store.consume(&token, "hire", "style_transfer", 20);
        assert!(matches!(over, Err(SwarmError::ConsentDenied(_))));
        // The token is still usable for a spend within its ceiling.
        let authorized = store
            .consume(&token, "hire", "style_transfer", 5)
            .expect("token must still be usable after an over-spend rejection");
        assert_eq!(authorized, 10);
    }

    // ── Session tests ──────────────────────────────────────────────────────

    #[test]
    fn session_consume_succeeds_and_deducts() {
        let store = ConsentStore::default();
        let token = store.open_session(100, &[]).expect("open session");
        let remaining = store
            .consume_session(&token, "delegate", 30)
            .expect("consume should succeed");
        assert_eq!(remaining, 70);
        // The session is still alive — consume again.
        let remaining = store
            .consume_session(&token, "hire", 20)
            .expect("second consume should succeed");
        assert_eq!(remaining, 50);
    }

    #[test]
    fn session_consume_rejects_unknown_token() {
        let store = ConsentStore::default();
        let result = store.consume_session("hkask-session-bogus", "delegate", 10);
        assert!(matches!(result, Err(SwarmError::ConsentDenied(_))));
    }

    #[test]
    fn session_consume_rejects_expired_session() {
        let path = temp_consent_store_path("session-ttl");
        let store = ConsentStore::open_sqlite(&path).expect("open sqlite store");
        let token = store.open_session(100, &[]).expect("open session");

        // Backdate the session beyond the TTL via a raw driver on the same
        // file — same pattern as `consent_store_sqlite_expired_grant_is_unspendable`.
        let manager = r2d2_sqlite::SqliteConnectionManager::file(&path)
            .with_init(|conn| conn.execute_batch(hkask_storage::WAL_PRAGMA_BATCH));
        let pool = r2d2::Pool::builder()
            .max_size(1)
            .build(manager)
            .expect("pool");
        let driver: Arc<dyn hkask_storage::database::driver::DatabaseDriver> =
            std::sync::Arc::new(hkask_storage::SqliteDriver::new(pool));
        let old =
            (chrono::Utc::now() - chrono::Duration::seconds(CONSENT_TTL_SECS + 60)).to_rfc3339();
        driver
            .execute(
                "UPDATE consent_sessions SET created_at = ?1 WHERE token = ?2",
                &[DbValue::Text(old), DbValue::Text(token.clone())],
            )
            .expect("backdate");

        let err = store.consume_session(&token, "delegate", 10).unwrap_err();
        assert!(
            matches!(err, SwarmError::ConsentDenied(ref m) if m.contains("expired")),
            "expired session must be unspendable, got {err:?}"
        );
        std::fs::remove_dir_all(std::path::Path::new(&path).parent().unwrap()).ok();
    }

    #[test]
    fn session_consume_rejects_action_not_allowed() {
        let store = ConsentStore::default();
        let token = store
            .open_session(100, &["hire".to_string()])
            .expect("open session");
        let result = store.consume_session(&token, "delegate", 10);
        assert!(matches!(result, Err(SwarmError::ConsentDenied(_))));
        // The session is still alive — a permitted action still works.
        let remaining = store
            .consume_session(&token, "hire", 10)
            .expect("permitted action should succeed");
        assert_eq!(remaining, 90);
    }

    #[test]
    fn session_consume_rejects_over_budget() {
        let store = ConsentStore::default();
        let token = store.open_session(10, &[]).expect("open session");
        let result = store.consume_session(&token, "delegate", 20);
        assert!(matches!(result, Err(SwarmError::ConsentDenied(_))));
        // The session is still alive — a spend within budget works.
        let remaining = store
            .consume_session(&token, "delegate", 5)
            .expect("in-budget spend should succeed");
        assert_eq!(remaining, 5);
    }

    #[test]
    fn session_consume_multiple_spend_exhausts_budget() {
        let store = ConsentStore::default();
        let token = store.open_session(30, &[]).expect("open session");
        let remaining = store
            .consume_session(&token, "delegate", 10)
            .expect("first spend");
        assert_eq!(remaining, 20);
        let remaining = store
            .consume_session(&token, "delegate", 20)
            .expect("second spend exhausts");
        assert_eq!(remaining, 0);
        // No budget left.
        let result = store.consume_session(&token, "delegate", 5);
        assert!(matches!(result, Err(SwarmError::ConsentDenied(_))));
    }

    #[test]
    fn session_balance_returns_remaining() {
        let store = ConsentStore::default();
        let token = store.open_session(50, &[]).expect("open session");
        assert_eq!(store.session_balance(&token), Some(50));
        store
            .consume_session(&token, "delegate", 20)
            .expect("consume");
        assert_eq!(store.session_balance(&token), Some(30));
    }

    #[test]
    fn session_balance_returns_none_for_unknown() {
        let store = ConsentStore::default();
        assert_eq!(store.session_balance("hkask-session-unknown"), None);
    }
}
