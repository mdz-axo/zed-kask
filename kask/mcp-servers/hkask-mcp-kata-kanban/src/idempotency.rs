//! Replay protection for state-changing kanban tools.
//!
//! # Why this exists
//!
//! An MCP client cannot always tell whether a tool call took effect. `rmcp`
//! reports both "the send failed" and "the response channel dropped" as the same
//! `ServiceError::TransportClosed`, so once a request reaches a live peer, a
//! transport loss is not proof of non-delivery (see
//! `hkask_capability::ToolPortError::Interrupted`). The client is therefore
//! forced to choose between never retrying (and stranding the operator) or
//! retrying blindly (and risking a duplicate).
//!
//! That ambiguity can only be *resolved* here, at the server: if a replayed call
//! returns the original response instead of performing the work again, the client
//! can retry an interrupted call safely.
//!
//! # Scope: only tools a replay would duplicate
//!
//! Most kanban mutations need nothing. `task_update`, `task_assign`,
//! `task_delete`, and `board_delete` are already idempotent — they converge on
//! the same state, and deleting an already-deleted task is a no-op. The unsafe
//! ones are those that **mint a fresh server-side identity**
//! (`Id::new()` → `Uuid::new_v4()`), because the client has no name for the thing
//! it asked to create and so cannot ask whether it landed:
//!
//! - `kanban_board_create` — a replay creates a second board
//! - `kanban_task_create` — a replay creates a second task
//! - `kanban_task_spawn` — a replay burns rJoules and starts a second subagent
//!
//! # Design
//!
//! Keyed on `(tool, key)`, storing the first response verbatim. `reserve` is a
//! single `INSERT` whose `UNIQUE` violation *is* the "already seen" signal, so
//! two concurrent replays cannot both win — no read-then-write race, and no
//! transaction (which `dyn DatabaseDriver` does not expose anyway).
//!
//! The reservation is two-phase because the work happens between the phases:
//! `reserve` claims the key, then `record` attaches the response. A key reserved
//! but never recorded (the server died mid-work) is `Pending` — reported as such
//! rather than silently re-run, because whether the work landed is exactly what
//! is unknown.
//!
//! Mirrors `hkask_mcp_swarm::consent`'s store shape deliberately: that code
//! already proved the cross-process single-use pattern against SQLite.

use std::sync::Arc;

use hkask_storage::database::driver::DatabaseDriver;
use hkask_storage::database::types::DbError;
use hkask_storage::database::value::DbValue;

/// Validation failure for a client-supplied idempotency key.
#[derive(Debug, Clone, thiserror::Error)]
pub enum IdempotencyKeyError {
    #[error("idempotency_key must not be empty or whitespace")]
    Empty,
    #[error("idempotency_key exceeds {max_len} bytes (got {actual_len})")]
    TooLong { max_len: usize, actual_len: usize },
}

/// How long a recorded response stays replayable.
///
/// Bounds durability rather than correctness: a retry follows its original call
/// within seconds (the client's mutation budget is ~2s), so an hour is generous.
/// Past this, a replay re-runs the tool — acceptable because no operator gesture
/// is still in flight an hour later.
const IDEMPOTENCY_TTL_SECS: i64 = 3600;

/// Maximum accepted key length. Keys are client-generated opaque strings (the
/// panel sends a UUID); this bounds a malformed or hostile client's write.
const MAX_KEY_LEN: usize = 200;

/// The outcome of claiming an idempotency key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reservation {
    /// The key is new and now claimed — perform the work, then call
    /// [`IdempotencyStore::record`].
    Fresh,
    /// This key already completed. Return `response` instead of re-running.
    Replay { response: String },
    /// The key was claimed but never completed: a previous attempt died between
    /// `reserve` and `record`. Whether the work landed is unknown, so the caller
    /// must report that rather than re-running or claiming success.
    Pending,
}

/// Replay-protection store for state-changing kanban tools.
///
/// Two backends, mirroring the kanban DB's own fallback: SQLite when a
/// passphrase is configured, in-memory otherwise. [`Self::is_durable`] reports
/// which, because an in-memory store cannot dedupe across a restart and callers
/// must not advertise protection it does not have.
pub struct IdempotencyStore {
    inner: Inner,
}

enum Inner {
    /// Process-local fallback. Correct within one process; lost on restart.
    Memory(std::sync::Mutex<std::collections::HashMap<(String, String), Option<String>>>),
    Sqlite(Arc<dyn DatabaseDriver>),
}

impl Default for IdempotencyStore {
    fn default() -> Self {
        Self {
            inner: Inner::Memory(std::sync::Mutex::new(std::collections::HashMap::new())),
        }
    }
}

impl IdempotencyStore {
    /// Build a store over an existing driver, creating the table if needed.
    ///
    /// Takes a driver rather than a path so it shares the kanban database (and
    /// its encryption): replay protection for kanban writes belongs in the same
    /// durability domain as the writes themselves.
    pub fn with_driver(driver: Arc<dyn DatabaseDriver>) -> Result<Self, DbError> {
        driver.execute_batch(
            "CREATE TABLE IF NOT EXISTS idempotency_keys (\
                 tool TEXT NOT NULL, \
                 key TEXT NOT NULL, \
                 response TEXT, \
                 created_at TEXT NOT NULL, \
                 PRIMARY KEY (tool, key) \
             )",
        )?;
        Ok(Self {
            inner: Inner::Sqlite(driver),
        })
    }

    /// Whether a recorded key survives a server restart.
    ///
    /// `false` for the in-memory fallback. Callers must surface this so an
    /// operator is not told a call was replay-protected when it was not (the
    /// repo's advertised-invariant rule: a claimed guarantee must point at its
    /// enforcement, or say it is absent).
    pub fn is_durable(&self) -> bool {
        matches!(self.inner, Inner::Sqlite(_))
    }

    /// Validate a client-supplied key.
    ///
    /// Rejects empty and over-long keys. A whitespace-only key is a client bug
    /// that would otherwise collapse distinct gestures onto one reservation.
    pub fn validate_key(key: &str) -> Result<(), IdempotencyKeyError> {
        if key.trim().is_empty() {
            return Err(IdempotencyKeyError::Empty);
        }
        if key.len() > MAX_KEY_LEN {
            return Err(IdempotencyKeyError::TooLong {
                max_len: MAX_KEY_LEN,
                actual_len: key.len(),
            });
        }
        Ok(())
    }

    /// Claim `key` for `tool`.
    ///
    /// Atomic: the claim is a single `INSERT` whose primary-key violation means
    /// another attempt already claimed it. Two concurrent replays therefore
    /// cannot both receive [`Reservation::Fresh`].
    ///
    /// A store failure returns `Err` — fail closed. Proceeding on an unrecorded
    /// reservation would silently drop the very protection the caller asked for.
    pub fn reserve(&self, tool: &str, key: &str) -> Result<Reservation, DbError> {
        match &self.inner {
            Inner::Memory(map) => {
                let mut map = map.lock().unwrap_or_else(|e| e.into_inner());
                match map.get(&(tool.to_string(), key.to_string())) {
                    Some(Some(response)) => Ok(Reservation::Replay {
                        response: response.clone(),
                    }),
                    Some(None) => Ok(Reservation::Pending),
                    None => {
                        map.insert((tool.to_string(), key.to_string()), None);
                        Ok(Reservation::Fresh)
                    }
                }
            }
            Inner::Sqlite(driver) => {
                // Lazy expiry sweep, as in `consent.rs`: correctness rests on the
                // TTL filter in the lookup below, not on this running.
                let cutoff = (chrono::Utc::now() - chrono::Duration::seconds(IDEMPOTENCY_TTL_SECS))
                    .to_rfc3339();
                if let Err(error) = driver.execute(
                    "DELETE FROM idempotency_keys WHERE created_at < ?1",
                    &[DbValue::Text(cutoff.clone())],
                ) {
                    tracing::warn!(
                        target: "hkask.mcp.kata_kanban",
                        %error,
                        "idempotency key sweep failed - stale keys retained"
                    );
                }

                // Claim attempt. A primary-key conflict means someone was here
                // first, which is the signal we want — not an error.
                let inserted = driver.execute(
                    "INSERT OR IGNORE INTO idempotency_keys (tool, key, response, created_at) \
                     VALUES (?1, ?2, NULL, ?3)",
                    &[
                        DbValue::Text(tool.to_string()),
                        DbValue::Text(key.to_string()),
                        DbValue::Text(chrono::Utc::now().to_rfc3339()),
                    ],
                )?;
                if inserted > 0 {
                    return Ok(Reservation::Fresh);
                }

                // Someone claimed it. Fetch what they recorded, ignoring
                // entries the sweep should have removed.
                let row = driver.query_optional(
                    "SELECT response FROM idempotency_keys \
                     WHERE tool = ?1 AND key = ?2 AND created_at >= ?3",
                    &[
                        DbValue::Text(tool.to_string()),
                        DbValue::Text(key.to_string()),
                        DbValue::Text(cutoff),
                    ],
                )?;
                let Some(row) = row else {
                    // Expired between the sweep and this read. Treat as fresh:
                    // no live gesture is waiting on an hour-old reservation.
                    return Ok(Reservation::Fresh);
                };
                match row.get_str(0) {
                    Ok(response) => Ok(Reservation::Replay {
                        response: response.to_string(),
                    }),
                    // NULL response — claimed but never completed.
                    Err(_) => Ok(Reservation::Pending),
                }
            }
        }
    }

    /// Attach the response for a completed call, making later replays return it.
    ///
    /// Best-effort by design: the work already succeeded, so failing the call
    /// because bookkeeping failed would be worse than losing replay protection
    /// for it. Logged loudly instead — a silent loss would leave the next retry
    /// duplicating work while the operator believed it was protected.
    pub fn record(&self, tool: &str, key: &str, response: &str) {
        match &self.inner {
            Inner::Memory(map) => {
                map.lock().unwrap_or_else(|e| e.into_inner()).insert(
                    (tool.to_string(), key.to_string()),
                    Some(response.to_string()),
                );
            }
            Inner::Sqlite(driver) => {
                if let Err(error) = driver.execute(
                    "UPDATE idempotency_keys SET response = ?3 WHERE tool = ?1 AND key = ?2",
                    &[
                        DbValue::Text(tool.to_string()),
                        DbValue::Text(key.to_string()),
                        DbValue::Text(response.to_string()),
                    ],
                ) {
                    tracing::warn!(
                        target: "hkask.mcp.kata_kanban",
                        tool = %tool,
                        %error,
                        "failed to record idempotency response - a retry of this call \
                         will re-run it instead of replaying"
                    );
                }
            }
        }
    }

    /// Release a claim so a later attempt can retry cleanly.
    ///
    /// Called when the work failed: the key must not stay `Pending`, or a retry
    /// would be told "outcome unknown" when in fact nothing happened.
    pub fn release(&self, tool: &str, key: &str) {
        match &self.inner {
            Inner::Memory(map) => {
                map.lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&(tool.to_string(), key.to_string()));
            }
            Inner::Sqlite(driver) => {
                if let Err(error) = driver.execute(
                    "DELETE FROM idempotency_keys WHERE tool = ?1 AND key = ?2",
                    &[
                        DbValue::Text(tool.to_string()),
                        DbValue::Text(key.to_string()),
                    ],
                ) {
                    tracing::warn!(
                        target: "hkask.mcp.kata_kanban",
                        tool = %tool,
                        %error,
                        "failed to release idempotency claim - a retry will report \
                         'outcome unknown' even though the call failed cleanly"
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sqlite_store() -> IdempotencyStore {
        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        IdempotencyStore::with_driver(driver).expect("schema init")
    }

    /// A shared driver models two server processes over one database file: the
    /// governed `McpRuntime` instance and the per-project `ContextServerStore`
    /// instance both open the same kanban DB.
    fn sqlite_pair() -> (IdempotencyStore, IdempotencyStore) {
        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        (
            IdempotencyStore::with_driver(driver.clone()).expect("process A"),
            IdempotencyStore::with_driver(driver).expect("process B"),
        )
    }

    #[test]
    fn first_reserve_is_fresh_and_replay_returns_the_recorded_response() {
        for store in [sqlite_store(), IdempotencyStore::default()] {
            assert_eq!(
                store.reserve("kanban_task_create", "k1").expect("reserve"),
                Reservation::Fresh
            );
            store.record("kanban_task_create", "k1", r#"{"task_id":"t-1"}"#);
            assert_eq!(
                store.reserve("kanban_task_create", "k1").expect("replay"),
                Reservation::Replay {
                    response: r#"{"task_id":"t-1"}"#.to_string()
                },
                "a replayed key must return the original response, not re-run the tool"
            );
        }
    }

    /// The headline guarantee: a replayed create yields one task, not two.
    #[test]
    fn replay_does_not_yield_a_second_fresh_reservation() {
        let store = sqlite_store();
        assert_eq!(
            store.reserve("kanban_task_create", "gesture").unwrap(),
            Reservation::Fresh
        );
        store.record("kanban_task_create", "gesture", r#"{"task_id":"t-1"}"#);
        for _ in 0..5 {
            assert!(
                !matches!(
                    store.reserve("kanban_task_create", "gesture").unwrap(),
                    Reservation::Fresh
                ),
                "no replay may be treated as fresh work - that is the duplicate-create bug"
            );
        }
    }

    /// A claim with no recorded response is `Pending`, never `Fresh`.
    ///
    /// This is the crash-between-phases case. Reporting `Fresh` would re-run work
    /// that may already have landed — exactly what the store exists to prevent.
    #[test]
    fn claimed_but_unrecorded_key_is_pending_not_fresh() {
        for store in [sqlite_store(), IdempotencyStore::default()] {
            assert_eq!(
                store.reserve("kanban_task_spawn", "k9").unwrap(),
                Reservation::Fresh
            );
            assert_eq!(
                store.reserve("kanban_task_spawn", "k9").unwrap(),
                Reservation::Pending,
                "a key claimed but never completed must report Pending - whether the \
                 work landed is precisely what is unknown"
            );
        }
    }

    /// Keys are scoped per tool, so the same gesture id used by two tools does
    /// not cross-suppress.
    #[test]
    fn keys_are_scoped_per_tool() {
        let store = sqlite_store();
        assert_eq!(
            store.reserve("kanban_task_create", "same").unwrap(),
            Reservation::Fresh
        );
        assert_eq!(
            store.reserve("kanban_board_create", "same").unwrap(),
            Reservation::Fresh,
            "a key claimed for one tool must not suppress a different tool"
        );
    }

    /// Releasing a failed call lets a retry start clean rather than inheriting a
    /// `Pending` verdict for work that never happened.
    #[test]
    fn release_allows_a_clean_retry() {
        for store in [sqlite_store(), IdempotencyStore::default()] {
            assert_eq!(
                store.reserve("kanban_task_create", "k2").unwrap(),
                Reservation::Fresh
            );
            store.release("kanban_task_create", "k2");
            assert_eq!(
                store.reserve("kanban_task_create", "k2").unwrap(),
                Reservation::Fresh,
                "a released key must be freshly claimable - the call failed cleanly, \
                 so a retry must not be told the outcome is unknown"
            );
        }
    }

    /// The real concurrency case: two processes over one database file. Exactly
    /// one may win the claim.
    ///
    /// Mirrors `consent.rs`'s
    /// `consent_store_sqlite_token_minted_in_one_process_consumed_in_another`.
    #[test]
    fn only_one_process_wins_the_same_key() {
        let (process_a, process_b) = sqlite_pair();
        assert_eq!(
            process_a.reserve("kanban_task_create", "shared").unwrap(),
            Reservation::Fresh
        );
        assert_eq!(
            process_b.reserve("kanban_task_create", "shared").unwrap(),
            Reservation::Pending,
            "the second process must not also win the claim - both creating would be \
             the duplicate-task bug across processes"
        );

        process_a.record("kanban_task_create", "shared", r#"{"task_id":"t-1"}"#);
        assert_eq!(
            process_b.reserve("kanban_task_create", "shared").unwrap(),
            Reservation::Replay {
                response: r#"{"task_id":"t-1"}"#.to_string()
            },
            "a response recorded in one process must replay in the other"
        );
    }

    /// The in-memory fallback reports itself as non-durable so callers can say so.
    #[test]
    fn durability_is_reported_honestly() {
        assert!(sqlite_store().is_durable());
        assert!(
            !IdempotencyStore::default().is_durable(),
            "the in-memory fallback cannot dedupe across a restart and must not \
             claim otherwise"
        );
    }

    #[test]
    fn empty_and_oversized_keys_are_rejected() {
        assert!(IdempotencyStore::validate_key("").is_err());
        assert!(IdempotencyStore::validate_key("   ").is_err());
        assert!(IdempotencyStore::validate_key(&"k".repeat(MAX_KEY_LEN + 1)).is_err());
        assert!(IdempotencyStore::validate_key("2f8a-c1").is_ok());
    }
}
