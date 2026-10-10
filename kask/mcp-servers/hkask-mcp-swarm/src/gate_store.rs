//! Per-agent grounding-gate counters — fermi's gate-account readings
//! (`src/gate_api.rs`), ported onto the local swarm server's SQLCipher
//! store.
//!
//! # Why this module exists
//!
//! The grounding gate counted nothing. A gate that wrongly strips a field
//! reads healthy in every aggregate the server keeps — the per-delegation
//! verdicts died with the delegation result. fermi's answer
//! (`src/gate_api.rs`): count what each gate approved, refused and could
//! not decide, then derive a reading — because "a gate that approves 90% of
//! what it sees and refuses the other 10% **wrongly** reads
//! `discriminating`", and no per-delegation record fixes that. What the
//! counters *can* catch is the pattern: a contract that strips every
//! delegation's output looks identical, from any single delegation, to a
//! gate doing its job. `strips_everything` names the contract as the
//! suspect, not the agent.
//!
//! # Scope (operator ruling 2026-10-10: aggregate pattern visibility only)
//!
//! This is the counters + readings half of fermi's gate ledger. The
//! per-decision rows and the after-the-fact review table (fermi
//! `src/gate_review.rs`) are deliberately NOT ported: the operator ruled
//! aggregate pattern visibility adequate — the algedonic gemba reads the
//! readings; it does not sample individual decisions. A wrong individual
//! strip surfaces the way every other finding does: as a review-board
//! card with the evidence pasted in.
//!
//! # The vocabulary, and its one owner
//!
//! The reliance-token vocabulary is owned by [`crate::grounding::RELIANCE_TOKENS`].
//! The counters table stores token strings; the reading fold validates
//! against the closed set and surfaces unknown-token rows as
//! `unknown_tokens` — never silently dropped (a stored token outside the
//! type that writes it is exactly the drift fermi's `seam_vocabulary`
//! exists to catch; here the check and the writer share one
//! implementation).

use crate::error::LocalSwarmError;
use hkask_storage::DatabaseDriver;
use hkask_storage::database::value::DbValue;
use serde::Serialize;

/// Applied decisions required before a reading is classified. Below this
/// the counts are reported and the reading is `thin_evidence` — a single
/// `amended` decision is not yet a contract bug.
pub const MIN_APPLIED_FOR_READING: i64 = 3;

/// Per-token decision counts for one agent. The six tokens are
/// [`crate::grounding::RELIANCE_TOKENS`]; the fold places rows by that
/// closed set.
#[derive(Debug, Default, Clone, Serialize)]
pub struct GateTokenCounts {
    pub unusable: i64,
    pub malformed: i64,
    pub amended: i64,
    pub incomplete: i64,
    pub unchecked: i64,
    pub clean: i64,
}

impl GateTokenCounts {
    /// Decisions where a grounding contract was applied (every token
    /// except `unchecked` — `unusable` and `malformed` are applied
    /// verdicts: the gate ran and graded).
    fn applied(&self) -> i64 {
        self.unusable + self.malformed + self.amended + self.incomplete + self.clean
    }
}

/// One agent's aggregate gate reading — the counters plus the
/// classification, with its `why` said once here so the vocabulary and the
/// explanation cannot drift (the `reliance_why` pattern).
#[derive(Debug, Serialize)]
pub struct GateReading {
    pub agent_id: String,
    /// Total decisions counted (every `grade()` call — the opportunity
    /// count behind `never_asked`).
    pub total: i64,
    /// Decisions where a contract was applied.
    pub applied: i64,
    pub counts: GateTokenCounts,
    pub stripped_total: i64,
    pub owed_total: i64,
    /// Distinct stored tokens outside [`crate::grounding::RELIANCE_TOKENS`]
    /// — surfaced, never dropped. Their decisions sit in neither
    /// `applied` nor `unchecked`; `total − applied − unchecked` counts them.
    pub unknown_tokens: Vec<String>,
    /// never_asked | thin_evidence | never_structured | strips_everything |
    /// all_clean | discriminating.
    pub reading: &'static str,
    /// unknown | fault | idle — fermi's reading classes.
    pub class: &'static str,
    pub why: &'static str,
}

/// Classify one agent's counts into a reading. Pure — one producer, next
/// to the vocabulary it names. fermi's gate-account vocabulary maps onto
/// the grounding gate by structure: `refuses_everything` (fault) →
/// `strips_everything`; `never_asked`/`admits_everything` (unknown) →
/// `never_asked`/`all_clean`; `discriminating` (idle) → `discriminating`.
fn classify(counts: &GateTokenCounts) -> (&'static str, &'static str, &'static str) {
    let applied = counts.applied();
    if applied == 0 {
        return (
            "never_asked",
            "unknown",
            "every decision unchecked — no grounding contract; the gate never \
             engaged. Inert, not pass.",
        );
    }
    if applied < MIN_APPLIED_FOR_READING {
        return (
            "thin_evidence",
            "unknown",
            "fewer applied decisions than the reading requires — the counts are \
             reported, the classification deferred.",
        );
    }
    if counts.unusable == applied {
        return (
            "never_structured",
            "unknown",
            "every applied decision unusable — the agent never produces the \
             document its contract commissions.",
        );
    }
    if counts.amended == applied {
        return (
            "strips_everything",
            "fault",
            "every applied decision stripped fields — the mis-declared-contract \
             signature. The contract is the suspect, not the agent.",
        );
    }
    if counts.clean == applied {
        return (
            "all_clean",
            "unknown",
            "applied every time, never fired — a well-behaved agent or a \
             toothless contract; the counters cannot tell.",
        );
    }
    (
        "discriminating",
        "idle",
        "mixed outcomes — the healthy state.",
    )
}

/// The grounding-gate counter store. One store, two handles (the runtime
/// counts at the grade sites; the gate tool surfaces the readings),
/// mirroring the agent-stats pattern. Lazily opened on first use.
pub struct GateCounterStore {
    path: String,
    passphrase: String,
    database: tokio::sync::OnceCell<hkask_storage::Database>,
}

impl GateCounterStore {
    pub fn new(path: String, passphrase: String) -> Self {
        Self {
            path,
            passphrase,
            database: tokio::sync::OnceCell::new(),
        }
    }

    async fn database(&self) -> Result<&hkask_storage::Database, LocalSwarmError> {
        self.database
            .get_or_try_init(|| async {
                if self.passphrase.len() < 8 {
                    return Err(LocalSwarmError::InvalidInput(
                        "HKASK_DB_PASSPHRASE must be at least eight characters for the gate counters"
                            .into(),
                    ));
                }
                if let Some(parent) = std::path::Path::new(&self.path).parent() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        LocalSwarmError::Io(format!("cannot create gate counters directory: {e}"))
                    })?;
                }
                let db =
                    hkask_storage::Database::open(&self.path, &self.passphrase).map_err(|e| {
                        LocalSwarmError::Database(format!("cannot open gate counters DB: {e}"))
                    })?;
                let pool = db.sqlite_pool().map_err(|e| {
                    LocalSwarmError::Database(format!("cannot connect to gate counters DB: {e}"))
                })?;
                hkask_storage::SqliteDriver::new(pool)
                    .execute_batch(
                        "CREATE TABLE IF NOT EXISTS gate_counters (\
                         agent_id TEXT NOT NULL, reliance_token TEXT NOT NULL, \
                         decisions INTEGER NOT NULL, \
                         stripped_total INTEGER NOT NULL, owed_total INTEGER NOT NULL, \
                         PRIMARY KEY (agent_id, reliance_token));",
                    )
                    .map_err(|e| {
                        LocalSwarmError::Database(format!("gate counters schema: {e}"))
                    })?;
                Ok(db)
            })
            .await
    }

    /// Count one gate decision: increment the `(agent_id, reliance_token)`
    /// row, accumulating the stripped/owed totals. Called for EVERY
    /// `grade()` outcome — including `unchecked` and `unusable`, which
    /// are the opportunity counts behind the `never_asked` reading.
    pub async fn bump(
        &self,
        agent_id: &str,
        reliance_token: &str,
        stripped: usize,
        owed: usize,
    ) -> Result<(), LocalSwarmError> {
        let pool = self
            .database()
            .await?
            .sqlite_pool()
            .map_err(|e| LocalSwarmError::Database(format!("gate counters pool: {e}")))?;
        let driver = hkask_storage::SqliteDriver::new(pool);
        driver
            .execute(
                "INSERT INTO gate_counters \
                 (agent_id, reliance_token, decisions, stripped_total, owed_total) \
                 VALUES (?1, ?2, 1, ?3, ?4) \
                 ON CONFLICT (agent_id, reliance_token) DO UPDATE SET \
                 decisions = decisions + 1, \
                 stripped_total = stripped_total + ?3, \
                 owed_total = owed_total + ?4",
                &[
                    DbValue::Text(agent_id.into()),
                    DbValue::Text(reliance_token.into()),
                    DbValue::Integer(stripped.min(i64::MAX as usize) as i64),
                    DbValue::Integer(owed.min(i64::MAX as usize) as i64),
                ],
            )
            .map_err(|e| LocalSwarmError::Database(format!("gate counters bump: {e}")))?;
        Ok(())
    }

    /// Per-agent gate readings over the whole counter store. `agent_id`
    /// filters to one agent. One producer: this is the only place the
    /// counters become a reading.
    pub async fn readings(
        &self,
        agent_id: Option<&str>,
    ) -> Result<Vec<GateReading>, LocalSwarmError> {
        let pool = self
            .database()
            .await?
            .sqlite_pool()
            .map_err(|e| LocalSwarmError::Database(format!("gate counters pool: {e}")))?;
        let driver = hkask_storage::SqliteDriver::new(pool);
        let (sql, params): (&str, Vec<DbValue>) = match agent_id {
            Some(agent) => (
                "SELECT agent_id, reliance_token, decisions, stripped_total, owed_total \
                 FROM gate_counters WHERE agent_id = ?1 \
                 ORDER BY agent_id, reliance_token",
                vec![DbValue::Text(agent.into())],
            ),
            None => (
                "SELECT agent_id, reliance_token, decisions, stripped_total, owed_total \
                 FROM gate_counters ORDER BY agent_id, reliance_token",
                vec![],
            ),
        };
        let rows = driver
            .query(sql, &params)
            .map_err(|e| LocalSwarmError::Database(format!("gate counters query: {e}")))?;

        // Fold rows into per-agent accumulators. The token placement
        // validates against the closed vocabulary; unknown tokens are
        // surfaced, never dropped.
        let mut agents: std::collections::BTreeMap<String, GateReading> =
            std::collections::BTreeMap::new();
        for row in rows {
            let agent = row.get_str(0)?.to_owned();
            let token = row.get_str(1)?;
            let decisions = row.get_int(2)?;
            let entry = agents.entry(agent.clone()).or_insert_with(|| GateReading {
                agent_id: agent.clone(),
                total: 0,
                applied: 0,
                counts: GateTokenCounts::default(),
                stripped_total: 0,
                owed_total: 0,
                unknown_tokens: Vec::new(),
                reading: "",
                class: "",
                why: "",
            });
            entry.total += decisions;
            entry.stripped_total += row.get_int(3)?;
            entry.owed_total += row.get_int(4)?;
            match token {
                "unusable" => entry.counts.unusable += decisions,
                "malformed" => entry.counts.malformed += decisions,
                "amended" => entry.counts.amended += decisions,
                "incomplete" => entry.counts.incomplete += decisions,
                "unchecked" => entry.counts.unchecked += decisions,
                "clean" => entry.counts.clean += decisions,
                other => {
                    if !entry.unknown_tokens.iter().any(|t| t == other) {
                        entry.unknown_tokens.push(other.to_owned());
                    }
                }
            }
        }

        let mut readings: Vec<GateReading> = agents.into_values().collect();
        for reading in &mut readings {
            let (r, class, why) = classify(&reading.counts);
            reading.reading = r;
            reading.class = class;
            reading.why = why;
            reading.applied = reading.counts.applied();
        }
        Ok(readings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_in_temp() -> GateCounterStore {
        let dir =
            std::env::temp_dir().join(format!("hkask-gate-counters-test-{}", uuid::Uuid::new_v4()));
        GateCounterStore::new(
            dir.join("gate.db").to_string_lossy().into_owned(),
            "test-passphrase".into(),
        )
    }

    async fn bump(
        store: &GateCounterStore,
        agent: &str,
        token: &str,
        stripped: usize,
        owed: usize,
    ) {
        store
            .bump(agent, token, stripped, owed)
            .await
            .expect("bump writes");
    }

    #[tokio::test]
    async fn bump_accumulates_per_token_row() {
        let store = store_in_temp();
        bump(&store, "agent_a", "clean", 0, 0).await;
        bump(&store, "agent_a", "clean", 0, 0).await;
        bump(&store, "agent_a", "amended", 2, 0).await;
        bump(&store, "agent_a", "incomplete", 0, 1).await;
        bump(&store, "agent_b", "unchecked", 0, 0).await;
        let readings = store.readings(None).await.expect("readings");
        assert_eq!(readings.len(), 2);
        let a = readings
            .iter()
            .find(|r| r.agent_id == "agent_a")
            .expect("a");
        assert_eq!(
            (
                a.total,
                a.counts.clean,
                a.counts.amended,
                a.counts.incomplete
            ),
            (4, 2, 1, 1)
        );
        assert_eq!((a.stripped_total, a.owed_total), (2, 1));
        let b = readings
            .iter()
            .find(|r| r.agent_id == "agent_b")
            .expect("b");
        assert_eq!((b.total, b.counts.unchecked), (1, 1));
    }

    /// Every reading classification, over synthetic histories: the
    /// mis-declared-contract signature reads fault; the inert and
    /// never-fired readings read unknown (never pass); mixed reads idle.
    #[tokio::test]
    async fn readings_classify_each_signature() {
        let store = store_in_temp();
        for _ in 0..3 {
            bump(&store, "strips_all", "amended", 1, 0).await;
            bump(&store, "no_contract", "unchecked", 0, 0).await;
            bump(&store, "all_pass", "clean", 0, 0).await;
            bump(&store, "prose_only", "unusable", 0, 0).await;
        }
        bump(&store, "mixed", "clean", 0, 0).await;
        bump(&store, "mixed", "amended", 1, 0).await;
        bump(&store, "mixed", "incomplete", 0, 1).await;
        bump(&store, "thin", "amended", 1, 0).await;
        bump(&store, "thin", "clean", 0, 0).await;

        let readings = store.readings(None).await.expect("readings");
        let by_agent = |name: &str| {
            readings
                .iter()
                .find(|r| r.agent_id == name)
                .unwrap_or_else(|| panic!("reading for {name}"))
        };
        assert_eq!(
            (by_agent("strips_all").reading, by_agent("strips_all").class),
            ("strips_everything", "fault"),
            "the mis-declared-contract signature"
        );
        assert_eq!(
            (
                by_agent("no_contract").reading,
                by_agent("no_contract").class
            ),
            ("never_asked", "unknown"),
            "inert is unknown, never pass"
        );
        assert_eq!(
            (by_agent("all_pass").reading, by_agent("all_pass").class),
            ("all_clean", "unknown"),
            "never fired is unknown — perfect or toothless, the counters cannot tell"
        );
        assert_eq!(
            (by_agent("prose_only").reading, by_agent("prose_only").class),
            ("never_structured", "unknown")
        );
        assert_eq!(
            (by_agent("mixed").reading, by_agent("mixed").class),
            ("discriminating", "idle"),
            "mixed outcomes — the healthy state"
        );
        assert_eq!(
            (by_agent("thin").reading, by_agent("thin").class),
            ("thin_evidence", "unknown"),
            "two applied decisions defer classification"
        );
        let strips = by_agent("strips_all");
        assert_eq!(
            (strips.total, strips.applied, strips.stripped_total),
            (3, 3, 3)
        );
    }

    /// A stored token outside the closed vocabulary is surfaced by name,
    /// never dropped — and its decisions sit in neither `applied` nor
    /// `unchecked` (reconcilable as `total − applied − unchecked`).
    #[tokio::test]
    async fn unknown_token_rows_are_surfaced_never_dropped() {
        let store = store_in_temp();
        bump(&store, "agent_a", "clean", 0, 0).await;
        // A row no writer of this crate could produce (the fold's defense
        // against another version's vocabulary).
        let pool = store
            .database()
            .await
            .expect("open")
            .sqlite_pool()
            .expect("pool");
        hkask_storage::SqliteDriver::new(pool)
            .execute(
                "INSERT INTO gate_counters \
                 (agent_id, reliance_token, decisions, stripped_total, owed_total) \
                 VALUES (?1, ?2, 1, 0, 0)",
                &[
                    DbValue::Text("agent_a".into()),
                    DbValue::Text("mystery_token".into()),
                ],
            )
            .expect("seed unknown token row");
        let readings = store.readings(Some("agent_a")).await.expect("readings");
        let a = &readings[0];
        assert_eq!(a.total, 2, "both rows counted");
        assert_eq!(a.applied, 1, "the unknown row is not applied");
        assert_eq!(a.counts.unchecked, 0);
        assert_eq!(
            a.unknown_tokens,
            vec!["mystery_token".to_string()],
            "the unknown token is named, never dropped"
        );
    }
}
