//! Consent gate — operator authorization for spend tools.
//!
//! Extracted from the swarm server root. `ConsentStore` holds single-use,
//! action-scoped spend grants keyed by an opaque token. Two backends:
//! in-memory (tests + fallback) and SQLite (production default — shared and
//! restart-durable across the governed and per-project swarm server
//! processes, with single-use enforced atomically via the DELETE-affected-rows
//! check). Grants expire after `CONSENT_TTL_SECS`.

use crate::error::LocalSwarmError;
use crate::error::SwarmError;
use hkask_storage::database::value::DbValue;
use std::sync::Arc;

/// An operator's authorization to spend credits on a specific action. Minted
/// by `swarm_request_consent` (which the panel calls after the operator
/// confirms), consumed by the spend tools. Single-use and action-scoped so a
/// consent for one hire cannot be replayed for a different agent or a second
/// spend — the enforcement point for the cost/consent gate.
#[derive(Debug, Clone)]
pub struct ConsentGrant {
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
pub const CONSENT_TTL_SECS: i64 = 3600;

/// Check whether a consent grant has expired. Shared by both backends so the
/// TTL logic doesn't drift between the Memory and Sqlite paths.
fn is_expired(created_at: chrono::DateTime<chrono::Utc>) -> bool {
    chrono::Utc::now()
        .signed_duration_since(created_at)
        .num_seconds()
        > CONSENT_TTL_SECS
}

/// Validate a consent grant against the requested action, target, and cost.
/// Returns the authorized ceiling on success. Shared by both backends so the
/// scope and over-spend checks don't drift between the Memory and Sqlite
/// paths.
///
/// This does NOT remove the grant — the caller (backend-specific) handles
/// single-use removal atomically.
fn validate_grant(
    grant: &ConsentGrant,
    created_at: chrono::DateTime<chrono::Utc>,
    action: &str,
    target: &str,
    cost: u32,
) -> Result<u32, SwarmError> {
    if is_expired(created_at) {
        return Err(SwarmError::ConsentDenied("consent token expired".into()));
    }
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
    Ok(grant.credits_authorized)
}

/// Validate a session grant against the requested action and cost. Returns
/// the remaining credits after deduction on success. Shared by both backends
/// so the session action and over-budget checks don't drift.
///
/// This does NOT deduct — the caller (backend-specific) handles the atomic
/// deduction.
fn validate_session(grant: &SessionGrant, action: &str, cost: u32) -> Result<u32, SwarmError> {
    if is_expired(grant.created_at) {
        return Err(SwarmError::ConsentDenied("session expired".into()));
    }
    let action_ok = grant.actions.is_empty() || grant.actions.iter().any(|a| a == action);
    if !action_ok {
        return Err(SwarmError::ConsentDenied(format!(
            "session does not authorize action '{action}'"
        )));
    }
    if cost > grant.remaining_credits {
        return Err(SwarmError::ConsentDenied(format!(
            "cost {cost} exceeds session remaining {}",
            grant.remaining_credits
        )));
    }
    Ok(grant.remaining_credits - cost)
}

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
    /// path (default `mcp/swarm/consent.db`), making consent tokens
    /// consumable across processes — the panel's hire flow and the Steer
    /// curator's spend flow compose.
    pub(crate) fn open_sqlite(path: &str) -> Result<Self, LocalSwarmError> {
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                LocalSwarmError::Io(format!(
                    "failed to create consent store dir {}: {e}",
                    parent.display()
                ))
            })?;
        }
        let manager = hkask_storage::SqliteConnectionManager::file(path)
            .with_init(|conn| conn.execute_batch(hkask_storage::WAL_PRAGMA_BATCH));
        let pool = r2d2::Pool::builder()
            .max_size(4)
            .build(manager)
            .map_err(|e| {
                LocalSwarmError::Database(format!("failed to create consent store pool: {e}"))
            })?;
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
            .map_err(|e| {
                LocalSwarmError::Database(format!("failed to init consent store schema: {e}"))
            })?;
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
            .map_err(|e| {
                LocalSwarmError::Database(format!("failed to init consent sessions schema: {e}"))
            })?;
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
                // The Memory backend doesn't persist `created_at` on the grant
                // (the grant is session-scoped). Use `now` as a non-expired
                // sentinel — the TTL check in `validate_grant` passes.
                let authorized = validate_grant(grant, chrono::Utc::now(), action, target, cost)?;
                // Remove only on success — the token is consumed.
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
                let new_remaining = validate_session(grant, action, cost)?;
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
                if is_expired(grant.created_at) {
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
        if let Err(e) = self.driver.execute(
            "DELETE FROM consent_grants WHERE created_at < ?1",
            &[DbValue::Text(cutoff)],
        ) {
            tracing::warn!(
                target: "hkask.mcp.swarm",
                error = %e,
                "consent grant TTL sweep failed — expired grants remain until consumed"
            );
        }
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
        // Expired — remove and treat as unknown (never spendable).
        if is_expired(created) {
            if let Err(e) = self.driver.execute(
                "DELETE FROM consent_grants WHERE token = ?1",
                &[DbValue::Text(token.to_string())],
            ) {
                tracing::warn!(
                    target: "hkask.mcp.swarm",
                    error = %e,
                    "failed to delete expired consent grant — it is already unusable"
                );
            }
            return Err(SwarmError::ConsentDenied("consent token expired".into()));
        }
        let grant_action = row.get_str(0).map_err(consent_store_err)?.to_string();
        let grant_target = row.get_str(1).map_err(consent_store_err)?.to_string();
        let grant_credits =
            u32::try_from(row.get_int(2).map_err(consent_store_err)?).map_err(|_| {
                SwarmError::Unavailable("consent store corrupt credits_authorized".into())
            })?;
        let grant = ConsentGrant {
            action: grant_action,
            target: grant_target,
            credits_authorized: grant_credits,
            token: token.to_string(),
        };
        // Validate scope and cost using the shared helper.
        let authorized = validate_grant(&grant, created, action, target, cost)?;
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
        Ok(authorized)
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
        if let Err(e) = self.driver.execute(
            "DELETE FROM consent_sessions WHERE created_at < ?1",
            &[DbValue::Text(cutoff)],
        ) {
            tracing::warn!(
                target: "hkask.mcp.swarm",
                error = %e,
                "consent session TTL sweep failed — expired sessions remain until consumed"
            );
        }
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
        if is_expired(created) {
            if let Err(e) = self.driver.execute(
                "DELETE FROM consent_sessions WHERE token = ?1",
                &[DbValue::Text(token.to_string())],
            ) {
                tracing::warn!(
                    target: "hkask.mcp.swarm",
                    error = %e,
                    "failed to delete expired consent session — it is already unusable"
                );
            }
            return Err(SwarmError::ConsentDenied("session expired".into()));
        }
        // Check action — empty actions string means all actions allowed.
        let actions_str = row.get_str(1).map_err(consent_store_err)?;
        if !actions_str.is_empty() {
            let allowed: Vec<&str> = actions_str.split(',').collect();
            if !allowed.contains(&action) {
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
        if is_expired(created) {
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
