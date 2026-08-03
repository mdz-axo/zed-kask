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
pub(crate) struct ConsentStore {
    inner: ConsentInner,
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
        Ok(Self {
            inner: ConsentInner::Sqlite(Arc::new(SqliteConsentStore { driver })),
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
}

/// Build an opaque single-use consent token (timestamp XOR FNV-1a of the
/// scope). Not cryptographic — the token's value is its unguessability
/// combined with single-use consumption, not secrecy against a motivated
/// attacker with process access.
fn mint_token(action: &str, target: &str) -> String {
    format!(
        "hkask-consent-{:016x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
            ^ (fnv1a(action, target) as u128)
    )
}

/// A tiny FNV-1a hash so consent tokens are not trivially guessable from the
/// timestamp alone. Not cryptographic — the token's value is its unguessability
/// combined with single-use consumption, not secrecy against a motivated
/// attacker with process access.
fn fnv1a(action: &str, target: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in action.bytes().chain(target.bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
