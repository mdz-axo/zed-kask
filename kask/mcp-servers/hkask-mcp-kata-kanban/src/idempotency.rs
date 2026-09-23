//! Replay protection for state-changing kanban tools.
//!
//! # Why this exists
//!
//! An MCP client cannot always tell whether a tool call took effect. `rmcp`
//! reports both "the send failed" and "the response channel dropped" as the same
//! `ServiceError::TransportClosed`, so once a request reaches a live peer, a
//! transport loss is not proof of non-delivery (see
//! `hkask_tool_port::ToolPortError::Interrupted`). The client is therefore
//! forced to choose between never retrying (and stranding the operator) or
//! retrying blindly (and risking a duplicate).
//!
//! That ambiguity can only be *resolved* here, at the server: if a replayed call
//! returns the original response instead of performing the work again, the client
//! can retry an interrupted call safely.
//!
//! # Scope: only tools a replay would duplicate
//!
//! Convergent updates and assignments need no replay record. Mutations that
//! **mint fresh server-side identity** (`Id::new()` → `Uuid::new_v4()`) do,
//! because the client has no name for the thing it asked to create and cannot
//! determine whether an interrupted call landed:
//!
//! - `kanban_board_create` — a replay creates a second board
//! - `kanban_task_create` — a replay creates a second task
//! - `kanban_task_spawn` — a replay starts a second subagent
//! - `kanban_board_import` — a replay creates a second board aggregate
//! - `kanban_goal_create` — a replay creates a second functional goal
//!
//! # Design
//!
//! Keyed on `(tool, key)`, with the full caller WebID and canonical request
//! fingerprint bound to the claim. `reserve` is a single `INSERT` whose
//! `UNIQUE` violation *is* the "already seen" signal, so
//! two concurrent replays cannot both win — no read-then-write race, and no
//! transaction (which `dyn DatabaseDriver` does not expose anyway).
//!
//! The reservation is two-phase because the work happens between the phases:
//! `reserve` claims the key, then `record` attaches the response. A key reserved
//! but never recorded (the server died mid-work) is `Pending` — reported as such
//! rather than silently re-run, because whether the work landed is exactly what
//! is unknown. Tools whose work has post-effect steps (see
//! `kanban_task_spawn`'s result note) must fold those failures into a partial
//! SUCCESS response rather than returning `Err`. The tool wrapper retains
//! pending claims on uncertain spawn transport errors; known pre-effect
//! validation/authorization failures may release them for a corrected retry.
//!
//! Mirrors `hkask_mcp_swarm::consent`'s store shape deliberately: that code
//! already proved the cross-process single-use pattern against SQLite.

use std::sync::Arc;

use hkask_storage::database::driver::DatabaseDriver;
use hkask_storage::database::types::DbError;
use hkask_storage::database::value::DbValue;

/// SHA-256 of the canonical JSON request (excluding the transport replay key).
/// Kept local because this crate's manifest is outside this change's write scope.
/// The digest, not the potentially sensitive request body, is persisted.
pub fn request_sha256(request: &serde_json::Value) -> Result<String, serde_json::Error> {
    fn canonical(value: &serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Object(fields) => {
                let mut keys: Vec<&String> = fields.keys().collect();
                keys.sort();
                let mut sorted = serde_json::Map::new();
                for key in keys {
                    if let Some(value) = fields.get(key) {
                        sorted.insert(key.clone(), canonical(value));
                    }
                }
                serde_json::Value::Object(sorted)
            }
            serde_json::Value::Array(items) => {
                serde_json::Value::Array(items.iter().map(canonical).collect())
            }
            _ => value.clone(),
        }
    }
    let bytes = serde_json::to_vec(&canonical(request))?;
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let bit_len = (bytes.len() as u64).wrapping_mul(8);
    let mut padded = bytes;
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());
    for block in padded.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, word) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = state;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (entry, value) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *entry = entry.wrapping_add(value);
        }
    }
    Ok(state.iter().map(|word| format!("{word:08x}")).collect())
}

/// Validation failure for a client-supplied idempotency key.
#[derive(Debug, Clone, thiserror::Error)]
pub(crate) enum IdempotencyKeyError {
    #[error("idempotency_key must not be empty or whitespace")]
    Empty,
    #[error("idempotency_key exceeds {max_len} bytes (got {actual_len})")]
    TooLong { max_len: usize, actual_len: usize },
}

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
    /// A different caller claimed this key; never reveal its response.
    OtherCaller,
    /// The caller reused this key for different work.
    DifferentRequest,
    /// A pre-binding row has no trustworthy caller/request identity.
    UnknownIdentity,
}

/// Replay-protection store for state-changing kanban tools.
///
/// SQLite-backed replay protection shares the Kanban database. An in-memory
/// SQLite driver remains available to tests, but there is no second backend.
pub struct IdempotencyStore {
    driver: Arc<dyn DatabaseDriver>,
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
                 owner_webid TEXT, \
                 request_sha256 TEXT, \
                 PRIMARY KEY (tool, key) \
             )",
        )?;
        // An older row cannot be bound retrospectively. Preserve it with NULL
        // identity and refuse every retry rather than replaying or rerunning.
        let columns = driver.query("PRAGMA table_info(idempotency_keys)", &[])?;
        let has_column = |name: &str| {
            columns
                .iter()
                .any(|row| row.get_str(1).is_ok_and(|value| value == name))
        };
        if !has_column("owner_webid") {
            driver.execute_batch("ALTER TABLE idempotency_keys ADD COLUMN owner_webid TEXT")?;
        }
        if !has_column("request_sha256") {
            driver.execute_batch("ALTER TABLE idempotency_keys ADD COLUMN request_sha256 TEXT")?;
        }
        Ok(Self { driver })
    }

    /// Whether a recorded key survives a server restart.
    ///
    /// The underlying driver reports whether keys survive a restart; tests
    /// using in-memory SQLite truthfully report `false`.
    pub fn is_durable(&self) -> bool {
        self.driver.is_durable()
    }

    /// Validate a client-supplied key.
    ///
    /// Rejects empty and over-long keys. A whitespace-only key is a client bug
    /// that would otherwise collapse distinct gestures onto one reservation.
    pub(crate) fn validate_key(key: &str) -> Result<(), IdempotencyKeyError> {
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

    /// Claim `key` for `tool`, bound to the full WebID and canonical request.
    ///
    /// Atomic: the claim is a single `INSERT` whose primary-key violation means
    /// another attempt already claimed it. Two concurrent replays therefore
    /// cannot both receive [`Reservation::Fresh`].
    ///
    /// A store failure returns `Err` — fail closed. Proceeding on an unrecorded
    /// reservation would silently drop the very protection the caller asked for.
    pub fn reserve(
        &self,
        tool: &str,
        key: &str,
        owner_webid: &str,
        request_sha256: &str,
    ) -> Result<Reservation, DbError> {
        let driver = &self.driver;
        // No expiry sweep: deleting the claim lets a stale same-key retry
        // duplicate work (or adopt a different payload). A key is never reused.
        let inserted = driver.execute(
            "INSERT OR IGNORE INTO idempotency_keys \
                 (tool, key, response, created_at, owner_webid, request_sha256) \
                 VALUES (?1, ?2, NULL, ?3, ?4, ?5)",
            &[
                DbValue::Text(tool.to_string()),
                DbValue::Text(key.to_string()),
                DbValue::Text(chrono::Utc::now().to_rfc3339()),
                DbValue::Text(owner_webid.to_string()),
                DbValue::Text(request_sha256.to_string()),
            ],
        )?;
        if inserted > 0 {
            return Ok(Reservation::Fresh);
        }
        let row = driver.query_optional(
            "SELECT response, owner_webid, request_sha256 FROM idempotency_keys \
                 WHERE tool = ?1 AND key = ?2",
            &[
                DbValue::Text(tool.to_string()),
                DbValue::Text(key.to_string()),
            ],
        )?;
        let Some(row) = row else {
            // A concurrent release removed the claim after our insert attempt.
            // The outcome cannot be inferred safely from this race.
            return Ok(Reservation::UnknownIdentity);
        };
        let (Ok(saved_owner), Ok(saved_request)) = (row.get_str(1), row.get_str(2)) else {
            return Ok(Reservation::UnknownIdentity);
        };
        if saved_owner != owner_webid {
            return Ok(Reservation::OtherCaller);
        }
        if saved_request != request_sha256 {
            return Ok(Reservation::DifferentRequest);
        }
        match row.get_str(0) {
            Ok(response) => Ok(Reservation::Replay {
                response: response.to_string(),
            }),
            Err(_) => Ok(Reservation::Pending),
        }
    }

    /// Attach the response for a completed call, making later replays return it.
    ///
    /// Best-effort by design: the work already succeeded; a failed update
    /// leaves the claim Pending, so a retry is refused rather than duplicated.
    pub fn record(&self, tool: &str, key: &str, response: &str) {
        let driver = &self.driver;
        {
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
                     will be refused as outcome unknown"
                );
            }
        }
    }

    /// Release a claim so a later attempt can retry cleanly.
    ///
    /// Called when the work failed: the key must not stay `Pending`, or a retry
    /// would be told "outcome unknown" when in fact nothing happened.
    pub fn release(&self, tool: &str, key: &str) {
        let driver = &self.driver;
        {
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
