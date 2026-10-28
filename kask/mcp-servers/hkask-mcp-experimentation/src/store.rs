//! SQLCipher-backed experiment registry (§P8.4).
//!
//! One database per server (the per-agent default under the hKask data dir,
//! overridable via `HKASK_EXPERIMENTATION_DB`), opened through the canonical
//! passphrase chain. The registry is the record of record from day one —
//! there is no legacy-import path (§P8.7-Q5).
//!
//! Replay convergence: `experiment_propose` and `variant_register` mint
//! server-side identity, so an interrupted-then-retried call could duplicate
//! a registry row. Optional caller-supplied keys converge those replays onto
//! the existing record (a `UNIQUE` index plus `INSERT OR IGNORE` — the same
//! single-statement-atomicity shape as the kata-kanban replay store).

use std::sync::Arc;

use hkask_storage::database::driver::DatabaseDriver;
use hkask_storage::database::types::DbError;
use hkask_storage::database::value::{DbRow, DbValue};
use sha2::{Digest, Sha256};

use crate::types::{
    ExperimentHealth, ExperimentRecord, ExperimentationError, ExperimentationHealthSnapshot,
    FinalEvaluationRecord, FitnessRecord, Prediction, Rubric, SCIENTIFIC_OUTCOME_REJECTED,
    SCIENTIFIC_OUTCOMES, STATUS_PROPOSED, STATUS_RESOLVED, STATUS_RUNNING, ScientificHeadroom,
    SealRecord, SelectionRecord, VARIANT_ROLE_ABLATION, VARIANT_ROLE_BASELINE, VARIANT_ROLES,
    VERDICT_REJECTED, VERDICT_SELECTED, VariantRecord,
};

/// Serialize a column value as canonical JSON text. `?Sized` admits slice
/// references (`&[Rubric]`) alongside owned values.
fn value_column<T: serde::Serialize + ?Sized>(value: &T) -> Result<String, ExperimentationError> {
    serde_json::to_string(value)
        .map_err(|error| ExperimentationError::Serialization(error.to_string()))
}

/// Optional key columns: an absent key is NULL (SQLite treats NULLs as
/// distinct in UNIQUE indexes, so convergence applies only when a key is
/// actually supplied).
fn text_or_null(value: Option<&str>) -> DbValue {
    value
        .map(|text| DbValue::Text(text.to_string()))
        .unwrap_or(DbValue::Null)
}

/// Read an optional TEXT column (NULL or missing value → None).
fn optional_text(row: &DbRow, idx: usize) -> Option<String> {
    row.get_str(idx).ok().map(str::to_string)
}

/// Read an optional JSON column. A NULL or absent cell reads as `None`; a
/// present cell is parsed and a malformed value surfaces as a serialization
/// error rather than a silent `None` (the silent-fallback trap). The typed
/// scientific fields (`design_spec`, `scientific_headroom`) are optional.
fn optional_json<T: serde::de::DeserializeOwned>(
    row: &DbRow,
    idx: usize,
) -> Result<Option<T>, ExperimentationError> {
    match row.get_str(idx) {
        Ok(text) => serde_json::from_str(text).map(Some).map_err(|error| {
            ExperimentationError::Serialization(format!("JSON column {idx}: {error}"))
        }),
        Err(_) => Ok(None),
    }
}

/// Read a required INTEGER column as u64 — a value outside the u64 range
/// (negative on read, or wrapped by a legacy write) surfaces as a
/// serialization error, never as a silently-normalized zero.
fn required_int(row: &DbRow, idx: usize) -> Result<u64, ExperimentationError> {
    let value = row.get_int(idx).map_err(ExperimentationError::from)?;
    u64::try_from(value).map_err(|_| {
        ExperimentationError::Serialization(format!("negative integer at column {idx}"))
    })
}

/// Convert a u64 budget/run count to the DbValue::Integer storage range —
/// values above i64::MAX are rejected, never wrapped by an unchecked cast
/// (a wrapped negative ceiling reads as an unspent budget forever: the
/// budget refusal and the stuck-running sensor fail together). Callers
/// validate the range at the tool boundary; this is the defensive floor
/// under any caller that skips it.
fn db_int(value: u64) -> Result<DbValue, ExperimentationError> {
    i64::try_from(value).map(DbValue::Integer).map_err(|_| {
        ExperimentationError::Serialization(format!(
            "integer {value} exceeds the i64 storage range"
        ))
    })
}

/// Column names of a table, via `PRAGMA table_info` (the same introspection
/// the kanban idempotency store uses to gate its additive columns).
fn column_names(driver: &dyn DatabaseDriver, table: &str) -> Result<Vec<String>, DbError> {
    let rows = driver.query(&format!("PRAGMA table_info({table})"), &[])?;
    Ok(rows
        .iter()
        .filter_map(|row| row.get_str(1).ok().map(str::to_string))
        .collect())
}

/// Guarded additive migration for the 2026-10-19 scientific-design evolution
/// (task `f1a90cd2`). `experiments.design_spec` and
/// `selection_records.scientific_outcome` are plain nullable `ADD COLUMN`s.
/// `seals.baseline_pass_rate` must relax from `NOT NULL` (a design-family
/// seal carries `scientific_headroom` instead), which SQLite cannot do in
/// place — that table is rebuilt with its rows copied verbatim, so a sealed
/// experiment is preserved rather than dropped. Each step is gated on the
/// column set, so a fresh database (already created with the target schema)
/// and an already-migrated one are both no-ops.
fn migrate_scientific_design_columns(driver: &dyn DatabaseDriver) -> Result<(), DbError> {
    if !column_names(driver, "experiments")?
        .iter()
        .any(|column| column == "design_spec")
    {
        driver.execute_batch("ALTER TABLE experiments ADD COLUMN design_spec TEXT")?;
    }
    if !column_names(driver, "selection_records")?
        .iter()
        .any(|column| column == "scientific_outcome")
    {
        driver.execute_batch("ALTER TABLE selection_records ADD COLUMN scientific_outcome TEXT")?;
    }
    if !column_names(driver, "seals")?
        .iter()
        .any(|column| column == "scientific_headroom")
    {
        // The rebuild runs inside ONE transaction: SQLite DDL is
        // transactional, so a failure between the RENAME and the DROP rolls
        // back to the intact original `seals` table. `execute_batch` in
        // autocommit would leave the table renamed-and-absent on a mid-batch
        // failure — an unacceptable window over the live registry's seals.
        driver.transaction(&mut |tx| {
            tx.execute_batch(
                "ALTER TABLE seals RENAME TO seals_pre_scientific; \
                 CREATE TABLE seals (\
                     id TEXT PRIMARY KEY, \
                     experiment_id TEXT NOT NULL UNIQUE, \
                     fingerprint TEXT NOT NULL, \
                     baseline_variant_id TEXT NOT NULL, \
                     baseline_fitness_ids TEXT NOT NULL, \
                     baseline_pass_rate TEXT, \
                     created_at TEXT NOT NULL, \
                     scientific_headroom TEXT \
                 ); \
                 INSERT INTO seals (id, experiment_id, fingerprint, baseline_variant_id, \
                                    baseline_fitness_ids, baseline_pass_rate, created_at, \
                                    scientific_headroom) \
                     SELECT id, experiment_id, fingerprint, baseline_variant_id, \
                            baseline_fitness_ids, baseline_pass_rate, created_at, NULL \
                     FROM seals_pre_scientific; \
                 DROP TABLE seals_pre_scientific;",
            )
        })?;
    }
    Ok(())
}

fn experiment_from_row(row: &DbRow) -> Result<ExperimentRecord, ExperimentationError> {
    Ok(ExperimentRecord {
        id: row.get_str(0)?.to_string(),
        hypothesis: row.get_str(1)?.to_string(),
        layer: row.get_str(2)?.to_string(),
        genotype_refs: row.get_json(3)?,
        eval_set: row.get_json(4)?,
        fitness_fn: row.get_str(5)?.to_string(),
        noise_band: row.get_json(6)?,
        prediction: row.get_json(7)?,
        budget: row.get_json(8)?,
        max_runs: required_int(row, 11)?,
        linked_goal_id: optional_text(row, 12),
        rubrics: row.get_json(13)?,
        design_spec: optional_json(row, 14)?,
        status: row.get_str(9)?.to_string(),
        created_at: row.get_str(10)?.to_string(),
    })
}

fn variant_from_row(row: &DbRow) -> Result<VariantRecord, ExperimentationError> {
    Ok(VariantRecord {
        id: row.get_str(0)?.to_string(),
        experiment_id: row.get_str(1)?.to_string(),
        genotype_config: row.get_json(2)?,
        parent_variant_id: optional_text(row, 3),
        created_at: row.get_str(4)?.to_string(),
        role: row.get_str(5)?.to_string(),
    })
}

fn fitness_from_row(row: &DbRow) -> Result<FitnessRecord, ExperimentationError> {
    Ok(FitnessRecord {
        id: row.get_str(0)?.to_string(),
        experiment_id: row.get_str(1)?.to_string(),
        variant_id: row.get_str(2)?.to_string(),
        runs: row.get_json(3)?,
        run_count: required_int(row, 5)?,
        scores: row.get_json(4)?,
        seal_id: optional_text(row, 7),
        created_at: row.get_str(6)?.to_string(),
    })
}

fn selection_from_row(row: &DbRow) -> Result<SelectionRecord, ExperimentationError> {
    Ok(SelectionRecord {
        id: row.get_str(0)?.to_string(),
        experiment_id: row.get_str(1)?.to_string(),
        verdict: row.get_str(2)?.to_string(),
        selected_variant_id: optional_text(row, 3),
        reject_reasons: row.get_json(4)?,
        algedonic_reference: optional_text(row, 5),
        scientific_outcome: optional_text(row, 7),
        created_at: row.get_str(6)?.to_string(),
    })
}

fn seal_from_row(row: &DbRow) -> Result<SealRecord, ExperimentationError> {
    let pass_rate = optional_text(row, 5)
        .map(|text| {
            text.parse::<f64>().map_err(|error| {
                ExperimentationError::Serialization(format!("baseline_pass_rate column: {error}"))
            })
        })
        .transpose()?;
    Ok(SealRecord {
        id: row.get_str(0)?.to_string(),
        experiment_id: row.get_str(1)?.to_string(),
        fingerprint: row.get_str(2)?.to_string(),
        baseline_variant_id: row.get_str(3)?.to_string(),
        baseline_fitness_ids: row.get_json(4)?,
        baseline_pass_rate: pass_rate,
        created_at: row.get_str(6)?.to_string(),
        scientific_headroom: optional_json(row, 7)?,
    })
}

fn final_eval_from_row(row: &DbRow) -> Result<FinalEvaluationRecord, ExperimentationError> {
    Ok(FinalEvaluationRecord {
        id: row.get_str(0)?.to_string(),
        experiment_id: row.get_str(1)?.to_string(),
        variant_id: row.get_str(2)?.to_string(),
        seal_id: row.get_str(3)?.to_string(),
        runs: row.get_json(4)?,
        run_count: required_int(row, 5)?,
        scores: row.get_json(6)?,
        created_at: row.get_str(7)?.to_string(),
    })
}

pub struct ExperimentationStore {
    driver: Arc<dyn DatabaseDriver>,
}

impl ExperimentationStore {
    /// Build the store over an existing driver, creating the schema if
    /// needed. Takes a driver rather than a path so the store shares the
    /// server's durability domain (and its encryption).
    pub fn with_driver(driver: Arc<dyn DatabaseDriver>) -> Result<Self, DbError> {
        driver.execute_batch(
            "CREATE TABLE IF NOT EXISTS experiments (\
                 id TEXT PRIMARY KEY, \
                 experiment_key TEXT UNIQUE, \
                 hypothesis TEXT NOT NULL, \
                 layer TEXT NOT NULL, \
                 genotype_refs TEXT NOT NULL, \
                 eval_set TEXT NOT NULL, \
                 fitness_fn TEXT NOT NULL, \
                 noise_band TEXT NOT NULL, \
                 prediction TEXT NOT NULL, \
                 budget TEXT NOT NULL, \
                 max_runs INTEGER NOT NULL, \
                 linked_goal_id TEXT, \
                 rubrics TEXT NOT NULL, \
                 design_spec TEXT, \
                 status TEXT NOT NULL, \
                 created_at TEXT NOT NULL \
             ); \
             CREATE TABLE IF NOT EXISTS variants (\
                 id TEXT PRIMARY KEY, \
                 experiment_id TEXT NOT NULL, \
                 variant_key TEXT, \
                 genotype_config TEXT NOT NULL, \
                 parent_variant_id TEXT, \
                 role TEXT NOT NULL, \
                 created_at TEXT NOT NULL, \
                 UNIQUE (experiment_id, variant_key) \
             ); \
             CREATE TABLE IF NOT EXISTS fitness_records (\
                 id TEXT PRIMARY KEY, \
                 experiment_id TEXT NOT NULL, \
                 variant_id TEXT NOT NULL, \
                 runs TEXT NOT NULL, \
                 scores TEXT NOT NULL, \
                 run_count INTEGER NOT NULL, \
                 seal_id TEXT, \
                 created_at TEXT NOT NULL \
             ); \
             CREATE TABLE IF NOT EXISTS selection_records (\
                 id TEXT PRIMARY KEY, \
                 experiment_id TEXT NOT NULL, \
                 verdict TEXT NOT NULL, \
                 selected_variant_id TEXT, \
                 reject_reasons TEXT NOT NULL, \
                 algedonic_reference TEXT, \
                 created_at TEXT NOT NULL, \
                 scientific_outcome TEXT \
             ); \
            CREATE TABLE IF NOT EXISTS seals (\
                 id TEXT PRIMARY KEY, \
                 experiment_id TEXT NOT NULL UNIQUE, \
                 fingerprint TEXT NOT NULL, \
                 baseline_variant_id TEXT NOT NULL, \
                 baseline_fitness_ids TEXT NOT NULL, \
                 baseline_pass_rate TEXT, \
                 created_at TEXT NOT NULL, \
                 scientific_headroom TEXT \
             ); \
            CREATE TABLE IF NOT EXISTS final_evaluations (\
                 id TEXT PRIMARY KEY, \
                 experiment_id TEXT NOT NULL, \
                 variant_id TEXT NOT NULL, \
                 seal_id TEXT NOT NULL, \
                 runs TEXT NOT NULL, \
                 run_count INTEGER NOT NULL, \
                 scores TEXT NOT NULL, \
                 created_at TEXT NOT NULL \
             ); \
             CREATE INDEX IF NOT EXISTS variants_by_experiment ON variants(experiment_id); \
             CREATE INDEX IF NOT EXISTS fitness_by_variant ON fitness_records(variant_id); \
             CREATE INDEX IF NOT EXISTS selection_by_experiment ON selection_records(experiment_id)",
        )?;
        // Full schema at creation — there is no legacy-import path: the
        // pre-contract accommodations (nullable max_runs, roleless
        // variants, rubric-less declarations) were removed 2026-10-15 under
        // the operator's no-backward-compatibility ruling (§P8.7-Q5).
        //
        // The 2026-10-19 scientific-design evolution (task `f1a90cd2`) is
        // NOT such an accommodation: it adds capability to a table that
        // already holds real resolved experiments (drift probes, the
        // prompt-enhance and FEDFUNDS cycles), so its columns are added by a
        // guarded additive migration that preserves every existing row — the
        // `IdempotencyStore::with_driver` shape. `CREATE TABLE IF NOT EXISTS`
        // is a no-op on an existing table, so a fresh database gets the
        // columns above and an existing one is upgraded below.
        migrate_scientific_design_columns(&*driver)?;
        Ok(Self { driver })
    }

    // ── Experiments ────────────────────────────────────────────────────

    /// Register an experiment. When `experiment_key` is supplied and already
    /// present, the existing experiment is returned (replay convergence).
    #[allow(clippy::too_many_arguments)]
    pub fn propose_experiment(
        &self,
        hypothesis: &str,
        layer: &str,
        genotype_refs: &[String],
        eval_set: &serde_json::Value,
        fitness_fn: &str,
        noise_band: &serde_json::Value,
        rubrics: &[Rubric],
        prediction: &Prediction,
        budget: &serde_json::Value,
        max_runs: u64,
        linked_goal_id: Option<&str>,
        experiment_key: Option<&str>,
        design_spec: Option<&serde_json::Value>,
    ) -> Result<ExperimentRecord, ExperimentationError> {
        let id = format!("exp_{}", uuid::Uuid::new_v4().simple());
        let created_at = chrono::Utc::now().to_rfc3339();
        let inserted = self.driver.execute(
            "INSERT OR IGNORE INTO experiments \
             (id, experiment_key, hypothesis, layer, genotype_refs, eval_set, \
              fitness_fn, noise_band, prediction, budget, max_runs, linked_goal_id, \
              rubrics, design_spec, status, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            &[
                DbValue::Text(id.clone()),
                text_or_null(experiment_key),
                DbValue::Text(hypothesis.to_string()),
                DbValue::Text(layer.to_string()),
                DbValue::Text(value_column(&genotype_refs)?),
                DbValue::Text(value_column(eval_set)?),
                DbValue::Text(fitness_fn.to_string()),
                DbValue::Text(value_column(noise_band)?),
                DbValue::Text(value_column(prediction)?),
                DbValue::Text(value_column(budget)?),
                db_int(max_runs)?,
                text_or_null(linked_goal_id),
                DbValue::Text(value_column(rubrics)?),
                design_spec
                    .map(value_column)
                    .transpose()?
                    .map(DbValue::Text)
                    .unwrap_or(DbValue::Null),
                DbValue::Text(STATUS_PROPOSED.to_string()),
                DbValue::Text(created_at),
            ],
        )?;
        if inserted == 0 {
            // Converged on an existing experiment_key — return that record.
            let key = experiment_key.ok_or_else(|| {
                ExperimentationError::Serialization(
                    "INSERT was ignored without an experiment_key — an unexpected \
                     constraint was violated"
                        .to_string(),
                )
            })?;
            return self.experiment_by_key(key);
        }
        self.experiment_by_id(&id)
    }

    pub fn experiment_by_id(&self, id: &str) -> Result<ExperimentRecord, ExperimentationError> {
        let row = self.driver.query_optional(
            "SELECT id, hypothesis, layer, genotype_refs, eval_set, fitness_fn, \
             noise_band, prediction, budget, status, created_at, max_runs, linked_goal_id, rubrics, \
             design_spec \
             FROM experiments WHERE id = ?1",
            &[DbValue::Text(id.to_string())],
        )?;
        row.map(|row| experiment_from_row(&row))
            .transpose()?
            .ok_or_else(|| ExperimentationError::ExperimentNotFound(id.to_string()))
    }

    fn experiment_by_key(&self, key: &str) -> Result<ExperimentRecord, ExperimentationError> {
        let row = self.driver.query_optional(
            "SELECT id, hypothesis, layer, genotype_refs, eval_set, fitness_fn, \
             noise_band, prediction, budget, status, created_at, max_runs, linked_goal_id, rubrics, \
             design_spec \
             FROM experiments WHERE experiment_key = ?1",
            &[DbValue::Text(key.to_string())],
        )?;
        row.map(|row| experiment_from_row(&row))
            .transpose()?
            .ok_or_else(|| {
                ExperimentationError::ExperimentNotFound(format!("experiment_key {key}"))
            })
    }

    // ── Variants ───────────────────────────────────────────────────────

    /// Register a variant inside an experiment. The first variant moves the
    /// experiment to running. A resolved experiment refuses new variants.
    pub fn register_variant(
        &self,
        experiment_id: &str,
        genotype_config: &serde_json::Value,
        parent_variant_id: Option<&str>,
        role: &str,
        variant_key: Option<&str>,
    ) -> Result<VariantRecord, ExperimentationError> {
        let experiment = self.experiment_by_id(experiment_id)?;
        if experiment.status == STATUS_RESOLVED {
            return Err(ExperimentationError::ExperimentResolved(
                experiment_id.to_string(),
            ));
        }
        if !VARIANT_ROLES.contains(&role) {
            return Err(ExperimentationError::UnknownVariantRole(role.to_string()));
        }
        if role == VARIANT_ROLE_BASELINE {
            // Exactly one baseline per experiment (P4) — the pre-registered
            // comparison point the seal attests. The check-then-insert shape
            // matches the parent validation below (the same TOCTOU class);
            // the seal names its baseline explicitly, so a raced duplicate
            // cannot corrupt the attestation.
            let rows = self.driver.query(
                "SELECT id FROM variants WHERE experiment_id = ?1 AND role = ?2",
                &[
                    DbValue::Text(experiment_id.to_string()),
                    DbValue::Text(VARIANT_ROLE_BASELINE.to_string()),
                ],
            )?;
            if let Some(existing) = rows.first() {
                let existing_id = existing.get_str(0)?.to_string();
                return Err(ExperimentationError::DuplicateBaseline(
                    experiment_id.to_string(),
                    existing_id,
                ));
            }
        }
        if role == VARIANT_ROLE_ABLATION && parent_variant_id.is_none() {
            return Err(ExperimentationError::AblationWithoutParent);
        }
        if let Some(parent) = parent_variant_id {
            let parent_record = self.variant_by_id(parent)?;
            if parent_record.experiment_id != experiment_id {
                return Err(ExperimentationError::VariantNotInExperiment(
                    parent.to_string(),
                    experiment_id.to_string(),
                ));
            }
        }
        let id = format!("var_{}", uuid::Uuid::new_v4().simple());
        let created_at = chrono::Utc::now().to_rfc3339();
        let inserted = self.driver.execute(
            "INSERT OR IGNORE INTO variants \
             (id, experiment_id, variant_key, genotype_config, parent_variant_id, role, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            &[
                DbValue::Text(id.clone()),
                DbValue::Text(experiment_id.to_string()),
                text_or_null(variant_key),
                DbValue::Text(value_column(genotype_config)?),
                text_or_null(parent_variant_id),
                DbValue::Text(role.to_string()),
                DbValue::Text(created_at.clone()),
            ],
        )?;
        if inserted == 0 {
            let key = variant_key.ok_or_else(|| {
                ExperimentationError::Serialization(
                    "INSERT was ignored without a variant_key — an unexpected \
                     constraint was violated"
                        .to_string(),
                )
            })?;
            return self.variant_by_key(experiment_id, key);
        }
        // First variant moves the experiment from proposed to running.
        self.driver.execute(
            "UPDATE experiments SET status = ?2 WHERE id = ?1 AND status = ?3",
            &[
                DbValue::Text(experiment_id.to_string()),
                DbValue::Text(STATUS_RUNNING.to_string()),
                DbValue::Text(STATUS_PROPOSED.to_string()),
            ],
        )?;
        Ok(VariantRecord {
            id,
            experiment_id: experiment_id.to_string(),
            genotype_config: genotype_config.clone(),
            parent_variant_id: parent_variant_id.map(str::to_string),
            role: role.to_string(),
            created_at,
        })
    }

    pub fn variant_by_id(&self, id: &str) -> Result<VariantRecord, ExperimentationError> {
        let row = self.driver.query_optional(
            "SELECT id, experiment_id, genotype_config, parent_variant_id, created_at, role \
             FROM variants WHERE id = ?1",
            &[DbValue::Text(id.to_string())],
        )?;
        row.map(|row| variant_from_row(&row))
            .transpose()?
            .ok_or_else(|| ExperimentationError::VariantNotFound(id.to_string()))
    }

    fn variant_by_key(
        &self,
        experiment_id: &str,
        key: &str,
    ) -> Result<VariantRecord, ExperimentationError> {
        let row = self.driver.query_optional(
            "SELECT id, experiment_id, genotype_config, parent_variant_id, created_at, role \
             FROM variants WHERE experiment_id = ?1 AND variant_key = ?2",
            &[
                DbValue::Text(experiment_id.to_string()),
                DbValue::Text(key.to_string()),
            ],
        )?;
        row.map(|row| variant_from_row(&row))
            .transpose()?
            .ok_or_else(|| ExperimentationError::VariantNotFound(format!("variant_key {key}")))
    }

    pub fn variants_for_experiment(
        &self,
        experiment_id: &str,
    ) -> Result<Vec<VariantRecord>, ExperimentationError> {
        let rows = self.driver.query(
            "SELECT id, experiment_id, genotype_config, parent_variant_id, created_at, role \
             FROM variants WHERE experiment_id = ?1 ORDER BY created_at",
            &[DbValue::Text(experiment_id.to_string())],
        )?;
        rows.iter().map(variant_from_row).collect()
    }

    /// The variant's ancestry chain, oldest parent first, ending with the
    /// variant itself (§P8.1 lineage). Cycle-guarded: a malformed parent loop
    /// surfaces as an error, never an infinite walk.
    pub fn variant_ancestry(
        &self,
        variant_id: &str,
    ) -> Result<Vec<VariantRecord>, ExperimentationError> {
        let mut chain = Vec::new();
        let mut current = Some(variant_id.to_string());
        while let Some(id) = current {
            let variant = self.variant_by_id(&id)?;
            current = variant.parent_variant_id.clone();
            chain.push(variant);
            if chain.len() > 128 {
                return Err(ExperimentationError::Serialization(
                    "variant ancestry exceeds 128 members — a parent cycle is suspected"
                        .to_string(),
                ));
            }
        }
        chain.reverse();
        Ok(chain)
    }

    // ── Fitness ────────────────────────────────────────────────────────

    /// Append an immutable fitness record. Recorded report refs only —
    /// the caller asserts these name real harness/evaluator reports
    /// (§P8.3); the store validates shape and bookkeeping, not report
    /// authenticity. A resolved experiment refuses new fitness.
    ///
    /// §P8.9 step 2 (D-1: enforce): `run_count` is the number of rollouts the
    /// referenced reports cover; the store sums recorded run counts against
    /// the experiment's declared `max_runs` ceiling and refuses the record
    /// with a typed `BudgetExhausted` error — never a silent overshoot.
    pub fn record_fitness(
        &self,
        experiment_id: &str,
        variant_id: &str,
        runs: &[String],
        run_count: u64,
        scores: &serde_json::Value,
        seal_id: Option<&str>,
    ) -> Result<FitnessRecord, ExperimentationError> {
        let experiment = self.experiment_by_id(experiment_id)?;
        if experiment.status == STATUS_RESOLVED {
            return Err(ExperimentationError::ExperimentResolved(
                experiment_id.to_string(),
            ));
        }
        let variant = self.variant_by_id(variant_id)?;
        if variant.experiment_id != experiment_id {
            return Err(ExperimentationError::VariantNotInExperiment(
                variant_id.to_string(),
                experiment_id.to_string(),
            ));
        }
        // P2: the seal gates challenger measurement. Pre-seal, only the
        // baseline variant records fitness; post-seal, every record cites
        // the active seal.
        let active_seal = self.seal_for_experiment(experiment_id)?;
        let seal_column = match (active_seal.as_ref(), seal_id) {
            (Some(active), Some(cited)) => {
                if cited != active.id {
                    return Err(ExperimentationError::SealMismatch(
                        cited.to_string(),
                        active.id.clone(),
                    ));
                }
                Some(active.id.clone())
            }
            (Some(active), None) => {
                return Err(ExperimentationError::SealCitationMissing(
                    experiment_id.to_string(),
                    active.id.clone(),
                ));
            }
            (None, Some(_)) => {
                return Err(ExperimentationError::ExperimentNotSealed(
                    experiment_id.to_string(),
                ));
            }
            (None, None) => {
                if variant.role != VARIANT_ROLE_BASELINE {
                    return Err(ExperimentationError::ExperimentNotSealed(
                        experiment_id.to_string(),
                    ));
                }
                None
            }
        };
        let id = format!("fit_{}", uuid::Uuid::new_v4().simple());
        let created_at = chrono::Utc::now().to_rfc3339();
        let ceiling = experiment.max_runs;
        // Single-statement atomicity (the crate's INSERT OR IGNORE shape):
        // the budget condition and the write are ONE statement, so two
        // concurrent fitness_record calls cannot both pass the check —
        // SQLite serializes writers and the second statement's WHERE
        // sees the first's committed SUM. The predicate counts fitness
        // records AND final evaluations — both consume the same budget
        // (§P8.9 step 2), so a final evaluation followed by a fitness
        // record cannot undercount and slip past the ceiling.
        let params = [
            DbValue::Text(id.clone()),
            DbValue::Text(experiment_id.to_string()),
            DbValue::Text(variant_id.to_string()),
            DbValue::Text(value_column(&runs)?),
            db_int(run_count)?,
            DbValue::Text(value_column(scores)?),
            text_or_null(seal_column.as_deref()),
            DbValue::Text(created_at.clone()),
            db_int(ceiling)?,
        ];
        let inserted = self.driver.execute(
            "INSERT INTO fitness_records \
             (id, experiment_id, variant_id, runs, run_count, scores, seal_id, created_at) \
             SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8 \
             WHERE ?5 + COALESCE((SELECT SUM(run_count) FROM fitness_records \
               WHERE experiment_id = ?2), 0) \
                 + COALESCE((SELECT SUM(run_count) FROM final_evaluations \
               WHERE experiment_id = ?2), 0) <= ?9",
            &params,
        )?;
        if inserted == 0 {
            let row = self.driver.query_optional(
                "SELECT COALESCE((SELECT SUM(run_count) FROM fitness_records \
                   WHERE experiment_id = ?1), 0) + COALESCE((SELECT SUM(run_count) \
                   FROM final_evaluations WHERE experiment_id = ?1), 0)",
                &[DbValue::Text(experiment_id.to_string())],
            )?;
            let recorded = match row {
                Some(row) => required_int(&row, 0)?,
                None => 0,
            };
            return Err(ExperimentationError::BudgetExhausted(
                experiment_id.to_string(),
                recorded + run_count,
                ceiling,
            ));
        }
        Ok(FitnessRecord {
            id,
            experiment_id: experiment_id.to_string(),
            variant_id: variant_id.to_string(),
            runs: runs.to_vec(),
            run_count,
            scores: scores.clone(),
            seal_id: seal_column,
            created_at,
        })
    }

    pub fn fitness_for_variant(
        &self,
        variant_id: &str,
    ) -> Result<Vec<FitnessRecord>, ExperimentationError> {
        let rows = self.driver.query(
            "SELECT id, experiment_id, variant_id, runs, scores, run_count, created_at, seal_id \
             FROM fitness_records WHERE variant_id = ?1 ORDER BY created_at",
            &[DbValue::Text(variant_id.to_string())],
        )?;
        rows.iter().map(fitness_from_row).collect()
    }

    pub fn fitness_for_experiment(
        &self,
        experiment_id: &str,
    ) -> Result<Vec<FitnessRecord>, ExperimentationError> {
        let rows = self.driver.query(
            "SELECT id, experiment_id, variant_id, runs, scores, run_count, created_at, seal_id \
             FROM fitness_records WHERE experiment_id = ?1 ORDER BY created_at",
            &[DbValue::Text(experiment_id.to_string())],
        )?;
        rows.iter().map(fitness_from_row).collect()
    }

    // ── Seals (P2) ─────────────────────────────────────────────────

    /// Seal the experiment's eval contract (P2): freeze the declaration's
    /// (eval_set, rubrics, fitness_fn, noise_band) under a SHA-256
    /// fingerprint and record the headroom attestation — the baseline
    /// variant, its recorded fitness evidence, and its measured pass rate
    /// (§5 Q6: a saturated baseline is the no-headroom void condition).
    /// UNIQUE(experiment_id) makes the insert race-safe: an identical
    /// retried seal converges on the existing record; a different attempt
    /// is a typed `AlreadySealed` refusal.
    pub fn seal_experiment(
        &self,
        experiment_id: &str,
        baseline_variant_id: &str,
        baseline_pass_rate: Option<f64>,
        scientific_headroom: Option<&ScientificHeadroom>,
    ) -> Result<SealRecord, ExperimentationError> {
        let experiment = self.experiment_by_id(experiment_id)?;
        if experiment.status == STATUS_RESOLVED {
            return Err(ExperimentationError::ExperimentResolved(
                experiment_id.to_string(),
            ));
        }
        // Exactly one attestation: a meaningful binary pass rate or a tagged
        // scientific headroom criterion. Both-supplied or neither is a typed
        // refusal — the headroom pre-check must have a subject.
        match (&baseline_pass_rate, &scientific_headroom) {
            (Some(pass_rate), None) => {
                if !(0.0..=1.0).contains(pass_rate) {
                    return Err(ExperimentationError::SealPassRateInvalid(*pass_rate));
                }
                if *pass_rate >= 1.0 {
                    return Err(ExperimentationError::NoHeadroom(*pass_rate));
                }
            }
            (None, Some(headroom)) => {
                crate::types::validate_scientific_headroom(headroom)?;
            }
            _ => return Err(ExperimentationError::SealAttestationConflict),
        }
        let variant = self.variant_by_id(baseline_variant_id)?;
        if variant.experiment_id != experiment_id {
            return Err(ExperimentationError::VariantNotInExperiment(
                baseline_variant_id.to_string(),
                experiment_id.to_string(),
            ));
        }
        if variant.role != VARIANT_ROLE_BASELINE {
            return Err(ExperimentationError::SealBaselineMismatch(
                baseline_variant_id.to_string(),
                variant.role,
            ));
        }
        let baseline_fitness = self.fitness_for_variant(baseline_variant_id)?;
        if baseline_fitness.is_empty() {
            return Err(ExperimentationError::SealFitnessMissing);
        }
        // The fingerprint freezes the eval contract: SHA-256 over the
        // canonical JSON of the sealed declaration fields (serde_json's
        // object map is key-sorted, so the serialization is canonical). The
        // typed `design_spec` joins the fingerprint (task `f1a90cd2`), so
        // post-seal design drift yields a different fingerprint rather than
        // silently reusing a stale design. A `None` design serializes as
        // JSON `null`, so legacy declarations seal to a stable fingerprint.
        let sealed_contract = serde_json::json!({
            "eval_set": experiment.eval_set,
            "fitness_fn": experiment.fitness_fn,
            "noise_band": experiment.noise_band,
            "rubrics": experiment.rubrics,
            "design_spec": experiment.design_spec,
        });
        let mut hasher = Sha256::new();
        hasher.update(value_column(&sealed_contract)?.as_bytes());
        let fingerprint = format!("{:x}", hasher.finalize());
        let id = format!("seal_{}", uuid::Uuid::new_v4().simple());
        let created_at = chrono::Utc::now().to_rfc3339();
        let baseline_fitness_ids: Vec<String> = baseline_fitness
            .iter()
            .map(|record| record.id.clone())
            .collect();
        let inserted = self.driver.execute(
            "INSERT OR IGNORE INTO seals \
             (id, experiment_id, fingerprint, baseline_variant_id, baseline_fitness_ids, \
              baseline_pass_rate, created_at, scientific_headroom) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            &[
                DbValue::Text(id.clone()),
                DbValue::Text(experiment_id.to_string()),
                DbValue::Text(fingerprint.clone()),
                DbValue::Text(baseline_variant_id.to_string()),
                DbValue::Text(value_column(&baseline_fitness_ids)?),
                baseline_pass_rate
                    .map(|rate| DbValue::Text(rate.to_string()))
                    .unwrap_or(DbValue::Null),
                DbValue::Text(created_at.clone()),
                scientific_headroom
                    .map(value_column)
                    .transpose()?
                    .map(DbValue::Text)
                    .unwrap_or(DbValue::Null),
            ],
        )?;
        if inserted == 0 {
            let existing = self.seal_for_experiment(experiment_id)?.ok_or_else(|| {
                ExperimentationError::Serialization(
                    "seal INSERT was ignored but no seal exists — an unexpected \
                     constraint was violated"
                        .to_string(),
                )
            })?;
            let pass_rate_matches = match (existing.baseline_pass_rate, baseline_pass_rate) {
                (Some(a), Some(b)) => (a - b).abs() < f64::EPSILON,
                (None, None) => true,
                _ => false,
            };
            if existing.baseline_variant_id == baseline_variant_id
                && existing.fingerprint == fingerprint
                && pass_rate_matches
            {
                return Ok(existing);
            }
            return Err(ExperimentationError::AlreadySealed(
                experiment_id.to_string(),
                existing.id,
            ));
        }
        Ok(SealRecord {
            id,
            experiment_id: experiment_id.to_string(),
            fingerprint,
            baseline_variant_id: baseline_variant_id.to_string(),
            baseline_fitness_ids,
            baseline_pass_rate,
            scientific_headroom: scientific_headroom.cloned(),
            created_at,
        })
    }

    /// The experiment's active seal, if any (P2).
    pub fn seal_for_experiment(
        &self,
        experiment_id: &str,
    ) -> Result<Option<SealRecord>, ExperimentationError> {
        let row = self.driver.query_optional(
            "SELECT id, experiment_id, fingerprint, baseline_variant_id, \
             baseline_fitness_ids, baseline_pass_rate, created_at, scientific_headroom \
             FROM seals WHERE experiment_id = ?1",
            &[DbValue::Text(experiment_id.to_string())],
        )?;
        row.map(|row| seal_from_row(&row)).transpose()
    }

    // ── Final evaluations (P3) ─────────────────────────────────────

    /// Record the sealed final evaluation (P3): the leading variant
    /// re-scored on the selection set. The server enforces run-ref
    /// disjointness — no ref may appear in any prior fitness record or
    /// final evaluation for the experiment (the GEPA feedback/selection
    /// split made mechanical) — and the run count joins the declared
    /// budget ceiling atomically (fitness + final evaluations).
    pub fn record_final_evaluation(
        &self,
        experiment_id: &str,
        variant_id: &str,
        runs: &[String],
        run_count: u64,
        scores: &serde_json::Value,
    ) -> Result<FinalEvaluationRecord, ExperimentationError> {
        let experiment = self.experiment_by_id(experiment_id)?;
        if experiment.status == STATUS_RESOLVED {
            return Err(ExperimentationError::ExperimentResolved(
                experiment_id.to_string(),
            ));
        }
        let seal = self
            .seal_for_experiment(experiment_id)?
            .ok_or_else(|| ExperimentationError::ExperimentNotSealed(experiment_id.to_string()))?;
        let variant = self.variant_by_id(variant_id)?;
        if variant.experiment_id != experiment_id {
            return Err(ExperimentationError::VariantNotInExperiment(
                variant_id.to_string(),
                experiment_id.to_string(),
            ));
        }
        // Disjointness (P3): the selection set must not reuse any run ref
        // the experiment has already recorded.
        let mut seen_refs = std::collections::HashSet::new();
        for record in self.fitness_for_experiment(experiment_id)? {
            seen_refs.extend(record.runs.iter().cloned());
        }
        for record in self.final_evaluations_for_experiment(experiment_id)? {
            seen_refs.extend(record.runs.iter().cloned());
        }
        let overlap: Vec<String> = runs
            .iter()
            .filter(|run| seen_refs.contains(*run))
            .cloned()
            .collect();
        if !overlap.is_empty() {
            return Err(ExperimentationError::RunRefsNotDisjoint(overlap));
        }
        let id = format!("fev_{}", uuid::Uuid::new_v4().simple());
        let created_at = chrono::Utc::now().to_rfc3339();
        let ceiling = experiment.max_runs;
        let params = [
            DbValue::Text(id.clone()),
            DbValue::Text(experiment_id.to_string()),
            DbValue::Text(variant_id.to_string()),
            DbValue::Text(seal.id.clone()),
            DbValue::Text(value_column(&runs)?),
            db_int(run_count)?,
            DbValue::Text(value_column(scores)?),
            DbValue::Text(created_at.clone()),
            db_int(ceiling)?,
        ];
        let inserted = self.driver.execute(
            "INSERT INTO final_evaluations \
             (id, experiment_id, variant_id, seal_id, runs, run_count, scores, created_at) \
             SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8 \
             WHERE ?6 + COALESCE((SELECT SUM(run_count) FROM fitness_records \
               WHERE experiment_id = ?2), 0) \
                 + COALESCE((SELECT SUM(run_count) FROM final_evaluations \
               WHERE experiment_id = ?2), 0) <= ?9",
            &params,
        )?;
        if inserted == 0 {
            let row = self.driver.query_optional(
                "SELECT COALESCE((SELECT SUM(run_count) FROM fitness_records \
                   WHERE experiment_id = ?1), 0) + COALESCE((SELECT SUM(run_count) \
                   FROM final_evaluations WHERE experiment_id = ?1), 0)",
                &[DbValue::Text(experiment_id.to_string())],
            )?;
            let recorded = match row {
                Some(row) => required_int(&row, 0)?,
                None => 0,
            };
            return Err(ExperimentationError::BudgetExhausted(
                experiment_id.to_string(),
                recorded + run_count,
                ceiling,
            ));
        }
        Ok(FinalEvaluationRecord {
            id,
            experiment_id: experiment_id.to_string(),
            variant_id: variant_id.to_string(),
            seal_id: seal.id,
            runs: runs.to_vec(),
            run_count,
            scores: scores.clone(),
            created_at,
        })
    }

    /// The experiment's final evaluations, oldest first (P3).
    pub fn final_evaluations_for_experiment(
        &self,
        experiment_id: &str,
    ) -> Result<Vec<FinalEvaluationRecord>, ExperimentationError> {
        let rows = self.driver.query(
            "SELECT id, experiment_id, variant_id, seal_id, runs, run_count, scores, created_at \
             FROM final_evaluations WHERE experiment_id = ?1 ORDER BY created_at",
            &[DbValue::Text(experiment_id.to_string())],
        )?;
        rows.iter().map(|row| final_eval_from_row(row)).collect()
    }

    // ── Selection ──────────────────────────────────────────────────────

    /// Record the selection verdict and resolve the experiment. Selected
    /// verdicts name their winning variant; rejected verdicts record at
    /// least one reason — both persist as fossils (§P8.7-Q4). The first
    /// selection resolves the experiment; a further selection is an
    /// `ExperimentResolved` conflict.
    pub fn record_selection(
        &self,
        experiment_id: &str,
        verdict: &str,
        selected_variant_id: Option<&str>,
        reject_reasons: &[String],
        algedonic_reference: Option<&str>,
        scientific_outcome: Option<&str>,
    ) -> Result<(SelectionRecord, ExperimentRecord), ExperimentationError> {
        let experiment = self.experiment_by_id(experiment_id)?;
        if experiment.status == STATUS_RESOLVED {
            return Err(ExperimentationError::ExperimentResolved(
                experiment_id.to_string(),
            ));
        }
        // The resolution token is either a legacy verdict (selected/rejected)
        // or a typed scientific outcome (task `f1a90cd2`). A scientific
        // outcome is stored as itself — never renamed to selected/rejected,
        // which would mislabel the fossil.
        if let Some(outcome) = scientific_outcome {
            if !SCIENTIFIC_OUTCOMES.contains(&outcome) {
                return Err(ExperimentationError::UnknownScientificOutcome(
                    outcome.to_string(),
                ));
            }
            if verdict != outcome {
                return Err(ExperimentationError::SelectionResolutionConflict(
                    Some(verdict.to_string()),
                    Some(outcome.to_string()),
                ));
            }
        }
        match verdict {
            VERDICT_SELECTED => {
                let Some(selected) = selected_variant_id else {
                    return Err(ExperimentationError::SelectedWithoutVariant);
                };
                let variant = self.variant_by_id(selected)?;
                if variant.experiment_id != experiment_id {
                    return Err(ExperimentationError::VariantNotInExperiment(
                        selected.to_string(),
                        experiment_id.to_string(),
                    ));
                }
            }
            VERDICT_REJECTED => {
                if reject_reasons.is_empty() {
                    return Err(ExperimentationError::RejectedWithoutReasons);
                }
            }
            other if SCIENTIFIC_OUTCOMES.contains(&other) => {
                // A scientific outcome needs no binary variant; a rejection
                // still names why, every other outcome stands on the typed
                // field alone.
                if other == SCIENTIFIC_OUTCOME_REJECTED && reject_reasons.is_empty() {
                    return Err(ExperimentationError::RejectedWithoutReasons);
                }
            }
            other => return Err(ExperimentationError::UnknownVerdict(other.to_string())),
        }
        let id = format!("sel_{}", uuid::Uuid::new_v4().simple());
        let created_at = chrono::Utc::now().to_rfc3339();
        // Serialized before the closure: the closure's error type is DbError,
        // so the ExperimentationError-producing column serialization happens here.
        let reject_reasons_column = value_column(&reject_reasons)?;
        // The insert and the resolve flip are ONE transaction (§P8.9 follow-up,
        // 2026-10-01): the conditional UPDATE is the authoritative
        // first-selection gate — a concurrent selection that committed first
        // leaves this UPDATE at zero rows and the whole transaction (insert
        // included) rolls back, so exactly one fossil ever records. The prior
        // check-then-insert shape could double-record under concurrent
        // dispatch (the same TOCTOU class as the budget gate).
        let mut concurrently_resolved = false;
        let transaction = self.driver.transaction(&mut |tx| {
            tx.execute(
                "INSERT INTO selection_records \
                 (id, experiment_id, verdict, selected_variant_id, reject_reasons, \
                  algedonic_reference, created_at, scientific_outcome) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                &[
                    DbValue::Text(id.clone()),
                    DbValue::Text(experiment_id.to_string()),
                    DbValue::Text(verdict.to_string()),
                    text_or_null(selected_variant_id),
                    DbValue::Text(reject_reasons_column.clone()),
                    text_or_null(algedonic_reference),
                    DbValue::Text(created_at.clone()),
                    text_or_null(scientific_outcome),
                ],
            )?;
            let resolved = tx.execute(
                "UPDATE experiments SET status = ?2 WHERE id = ?1 AND status != ?2",
                &[
                    DbValue::Text(experiment_id.to_string()),
                    DbValue::Text(STATUS_RESOLVED.to_string()),
                ],
            )?;
            if resolved == 0 {
                concurrently_resolved = true;
                return Err(DbError::Database(
                    "selection lost the resolve race — the experiment was \
                     resolved concurrently; this insert rolls back"
                        .to_string(),
                ));
            }
            Ok(())
        });
        if let Err(error) = transaction {
            if concurrently_resolved {
                return Err(ExperimentationError::ExperimentResolved(
                    experiment_id.to_string(),
                ));
            }
            return Err(ExperimentationError::Database(error));
        }
        let updated = self.experiment_by_id(experiment_id)?;
        let record = SelectionRecord {
            id,
            experiment_id: experiment_id.to_string(),
            verdict: verdict.to_string(),
            selected_variant_id: selected_variant_id.map(str::to_string),
            reject_reasons: reject_reasons.to_vec(),
            algedonic_reference: algedonic_reference.map(str::to_string),
            scientific_outcome: scientific_outcome.map(str::to_string),
            created_at,
        };
        Ok((record, updated))
    }

    pub fn selections_for_experiment(
        &self,
        experiment_id: &str,
    ) -> Result<Vec<SelectionRecord>, ExperimentationError> {
        let rows = self.driver.query(
            "SELECT id, experiment_id, verdict, selected_variant_id, reject_reasons, \
             algedonic_reference, created_at, scientific_outcome \
             FROM selection_records WHERE experiment_id = ?1 ORDER BY created_at",
            &[DbValue::Text(experiment_id.to_string())],
        )?;
        rows.iter().map(selection_from_row).collect()
    }

    // ── Population ──────────────────────────────────────────────────────

    /// Query the experiment population, newest first. Filters compose; the
    /// limit is interpolated as a u32 (no injection surface).
    pub fn population(
        &self,
        layer: Option<&str>,
        status: Option<&str>,
        created_since: Option<&str>,
        limit: u32,
    ) -> Result<Vec<ExperimentRecord>, ExperimentationError> {
        let mut clauses: Vec<String> = Vec::new();
        let mut params: Vec<DbValue> = Vec::new();
        if let Some(layer) = layer {
            params.push(DbValue::Text(layer.to_string()));
            clauses.push(format!("layer = ?{}", params.len()));
        }
        if let Some(status) = status {
            params.push(DbValue::Text(status.to_string()));
            clauses.push(format!("status = ?{}", params.len()));
        }
        if let Some(created_since) = created_since {
            params.push(DbValue::Text(created_since.to_string()));
            clauses.push(format!("created_at >= ?{}", params.len()));
        }
        let where_clause = if clauses.is_empty() {
            "1 = 1".to_string()
        } else {
            clauses.join(" AND ")
        };
        let rows = self.driver.query(
            &format!(
                "SELECT id, hypothesis, layer, genotype_refs, eval_set, fitness_fn, \
                 noise_band, prediction, budget, status, created_at, max_runs, linked_goal_id, rubrics, \
                 design_spec \
                 FROM experiments WHERE {where_clause} ORDER BY created_at DESC LIMIT {limit}",
            ),
            &params,
        )?;
        rows.iter().map(experiment_from_row).collect()
    }

    // ── Regulation health (§P8.9 step 1) ────────────────────────────────

    /// The Layer-A afferent view: every experiment with its recorded-run
    /// total, prediction confidence, and verdict. The bridge source feeds
    /// this to the cybernetics loop's `ExperimentationHealthSensor`.
    pub fn health_snapshot(&self) -> Result<ExperimentationHealthSnapshot, ExperimentationError> {
        let rows = self.driver.query(
            "SELECT e.id, e.layer, e.status, e.created_at, e.max_runs, e.prediction, \
             (SELECT COALESCE(SUM(f.run_count), 0) FROM fitness_records f \
               WHERE f.experiment_id = e.id) AS recorded_runs, \
             (SELECT s.verdict FROM selection_records s WHERE s.experiment_id = e.id \
               ORDER BY s.created_at LIMIT 1) AS verdict, \
             (SELECT s.reject_reasons FROM selection_records s WHERE s.experiment_id = e.id \
               ORDER BY s.created_at LIMIT 1) AS reject_reasons, \
             (SELECT s.scientific_outcome FROM selection_records s WHERE s.experiment_id = e.id \
               ORDER BY s.created_at LIMIT 1) AS scientific_outcome \
             FROM experiments e ORDER BY e.created_at DESC",
            &[],
        )?;
        let experiments = rows
            .iter()
            .map(|row| {
                let prediction: Prediction = row.get_json(5)?;
                Ok(ExperimentHealth {
                    id: row.get_str(0)?.to_string(),
                    layer: row.get_str(1)?.to_string(),
                    status: row.get_str(2)?.to_string(),
                    created_at: row.get_str(3)?.to_string(),
                    max_runs: required_int(row, 4)?,
                    prediction_confidence: Some(prediction.confidence),
                    recorded_runs: required_int(row, 6)?,
                    verdict: optional_text(row, 7),
                    scientific_outcome: optional_text(row, 9),
                    reject_reasons: row.get_json::<Vec<String>>(8).ok().unwrap_or_default(),
                })
            })
            .collect::<Result<Vec<_>, ExperimentationError>>()?;
        Ok(ExperimentationHealthSnapshot { experiments })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        Prediction, RUBRIC_KIND_GROUNDED_REPORT, RUBRIC_KIND_IMPLEMENTATION, RUBRIC_KIND_METRIC,
        Rubric, RubricVerification, SCIENTIFIC_OUTCOME_CORROBORATED,
        SCIENTIFIC_OUTCOME_DESIGN_VOID, SCIENTIFIC_OUTCOME_EQUIVALENCE,
        SCIENTIFIC_OUTCOME_INCONCLUSIVE, SCIENTIFIC_OUTCOME_MEASUREMENT_VOID, ScientificHeadroom,
        VARIANT_ROLE_ABLATION, VARIANT_ROLE_BASELINE, VARIANT_ROLE_CHALLENGER, validate_rubrics,
    };
    use hkask_storage::database::sqlite::SqliteDriver;

    fn store() -> ExperimentationStore {
        ExperimentationStore::with_driver(SqliteDriver::in_memory_driver())
            .expect("in-memory driver store")
    }

    /// A valid rubric bundle (P1): one metric rubric verified by a recorded
    /// harness report, one implementation rubric verified by a contains
    /// check — the swarm evaluator vocabulary.
    fn rubrics() -> Vec<Rubric> {
        vec![
            Rubric {
                id: "pass_rate_threshold".to_string(),
                description: "evaluator pass rate on the fixed set reaches 0.8".to_string(),
                kind: RUBRIC_KIND_METRIC.to_string(),
                verification: RubricVerification::ReportRef {
                    harness: "swarm_eval_agent_local".to_string(),
                },
            },
            Rubric {
                id: "uses_lisp_gate".to_string(),
                description: "the skill routes its gate through lisp_eval".to_string(),
                kind: RUBRIC_KIND_IMPLEMENTATION.to_string(),
                verification: RubricVerification::Contains {
                    spec: "lisp_eval".to_string(),
                },
            },
        ]
    }

    fn propose(store: &ExperimentationStore, layer: &str, key: Option<&str>) -> ExperimentRecord {
        store
            .propose_experiment(
                "Variant B beats the baseline on the fixed task set",
                layer,
                &[".agents/skills/self-improvement/SKILL.md".to_string()],
                &serde_json::json!({"task_set": "fixed-5", "pin_suites": ["lisp_eval_tool"]}),
                "evaluator pass rate on the fixed set",
                &serde_json::json!({"cost_band_pct": 10}),
                &rubrics(),
                &Prediction {
                    claim: "variant B wins".to_string(),
                    confidence: 0.7,
                },
                &serde_json::json!({"energy_budget": "one eval run", "max_runs": 18}),
                18,
                None,
                key,
                None,
            )
            .expect("propose")
    }

    fn variant(
        store: &ExperimentationStore,
        experiment_id: &str,
        name: &str,
        parent: Option<&str>,
        key: Option<&str>,
    ) -> VariantRecord {
        variant_role(
            store,
            experiment_id,
            name,
            parent,
            key,
            VARIANT_ROLE_CHALLENGER,
        )
    }

    /// Register a variant with an explicit role (P4) — the default helper
    /// above registers challengers, matching the server's default.
    fn variant_role(
        store: &ExperimentationStore,
        experiment_id: &str,
        name: &str,
        parent: Option<&str>,
        key: Option<&str>,
        role: &str,
    ) -> VariantRecord {
        store
            .register_variant(
                experiment_id,
                &serde_json::json!({"name": name}),
                parent,
                role,
                key,
            )
            .expect("variant")
    }

    #[test]
    fn lifecycle_propose_register_fitness_select() {
        let store = store();
        let experiment = propose(&store, "skill", None);
        assert_eq!(experiment.status, STATUS_PROPOSED);

        let baseline = variant_role(
            &store,
            &experiment.id,
            "baseline",
            None,
            Some("baseline"),
            VARIANT_ROLE_BASELINE,
        );
        assert_eq!(baseline.experiment_id, experiment.id);
        // First variant moves the experiment to running.
        assert_eq!(
            store
                .experiment_by_id(&experiment.id)
                .expect("reload")
                .status,
            STATUS_RUNNING
        );

        let challenger = variant(
            &store,
            &experiment.id,
            "challenger",
            Some(&baseline.id),
            None,
        );
        // The baseline measures, then the seal freezes the contract (P2) —
        // the challenger's fitness cites the seal.
        store
            .record_fitness(
                &experiment.id,
                &baseline.id,
                &["baseline harness report".to_string()],
                3,
                &serde_json::json!({"pass_rate": 0.6}),
                None,
            )
            .expect("baseline fitness");
        let seal = store
            .seal_experiment(&experiment.id, &baseline.id, Some(0.6), None)
            .expect("seal");
        let fitness = store
            .record_fitness(
                &experiment.id,
                &challenger.id,
                &["swarm_eval_agent_local run 42".to_string()],
                3,
                &serde_json::json!({"pass_rate": 0.8, "total_tokens": 1200}),
                Some(&seal.id),
            )
            .expect("fitness");
        assert_eq!(fitness.variant_id, challenger.id);
        assert_eq!(fitness.run_count, 3);
        assert_eq!(fitness.seal_id.as_deref(), Some(seal.id.as_str()));
        assert_eq!(
            fitness.runs,
            vec!["swarm_eval_agent_local run 42".to_string()]
        );

        let (selection, updated) = store
            .record_selection(
                &experiment.id,
                VERDICT_SELECTED,
                Some(&challenger.id),
                &[],
                Some("algedonic 2026-09-30#1"),
                None,
            )
            .expect("selection");
        assert_eq!(selection.verdict, VERDICT_SELECTED);
        assert_eq!(
            selection.selected_variant_id.as_deref(),
            Some(challenger.id.as_str())
        );
        assert_eq!(updated.status, STATUS_RESOLVED);

        // Resolved experiments refuse further work.
        let error = store
            .register_variant(
                &experiment.id,
                &serde_json::json!({}),
                None,
                VARIANT_ROLE_CHALLENGER,
                None,
            )
            .expect_err("resolved refuses variants");
        assert!(matches!(error, ExperimentationError::ExperimentResolved(_)));
        let error = store
            .record_selection(
                &experiment.id,
                VERDICT_REJECTED,
                None,
                &["late".into()],
                None,
                None,
            )
            .expect_err("resolved refuses re-selection");
        assert!(matches!(error, ExperimentationError::ExperimentResolved(_)));
    }

    #[test]
    fn experiment_key_converges_replays() {
        let store = store();
        let first = propose(&store, "skill", Some("key-1"));
        let second = propose(&store, "skill", Some("key-1"));
        assert_eq!(
            first.id, second.id,
            "a retried propose with the same key must converge, not duplicate"
        );
    }

    #[test]
    fn variant_key_converges_within_an_experiment() {
        let store = store();
        let experiment = propose(&store, "agent_card", None);
        let first = variant(&store, &experiment.id, "v1", None, Some("v1-key"));
        let second = variant(&store, &experiment.id, "v1", None, Some("v1-key"));
        assert_eq!(
            first.id, second.id,
            "a retried register with the same key must converge, not duplicate"
        );
    }

    #[test]
    fn parent_must_belong_to_the_same_experiment() {
        let store = store();
        let first = propose(&store, "skill", None);
        let second = propose(&store, "skill", None);
        let parent = variant(&store, &first.id, "parent", None, None);
        let error = store
            .register_variant(
                &second.id,
                &serde_json::json!({"name": "orphan"}),
                Some(&parent.id),
                VARIANT_ROLE_CHALLENGER,
                None,
            )
            .expect_err("cross-experiment parent");
        assert!(matches!(
            error,
            ExperimentationError::VariantNotInExperiment(_, _)
        ));
    }

    #[test]
    fn selection_verdict_validation() {
        let store = store();
        let experiment = propose(&store, "skill", None);
        variant(&store, &experiment.id, "challenger", None, None);
        let error = store
            .record_selection(&experiment.id, VERDICT_SELECTED, None, &[], None, None)
            .expect_err("selected without a variant");
        assert!(matches!(
            error,
            ExperimentationError::SelectedWithoutVariant
        ));
        let error = store
            .record_selection(&experiment.id, VERDICT_REJECTED, None, &[], None, None)
            .expect_err("rejected without reasons");
        assert!(matches!(
            error,
            ExperimentationError::RejectedWithoutReasons
        ));
        let error = store
            .record_selection(
                &experiment.id,
                "maybe",
                None,
                &["reason".into()],
                None,
                None,
            )
            .expect_err("unknown verdict");
        assert!(matches!(error, ExperimentationError::UnknownVerdict(_)));
        // A valid rejected selection still resolves the experiment.
        let (record, updated) = store
            .record_selection(
                &experiment.id,
                VERDICT_REJECTED,
                None,
                &["no variant beat the noise band".to_string()],
                None,
                None,
            )
            .expect("rejected selection");
        assert_eq!(record.verdict, VERDICT_REJECTED);
        assert_eq!(updated.status, STATUS_RESOLVED);
    }

    #[test]
    fn ancestry_walks_parents_oldest_first() {
        let store = store();
        let experiment = propose(&store, "skill", None);
        let v1 = variant(&store, &experiment.id, "v1", None, None);
        let v2 = variant(&store, &experiment.id, "v2", Some(&v1.id), None);
        let v3 = variant(&store, &experiment.id, "v3", Some(&v2.id), None);
        let ancestry = store.variant_ancestry(&v3.id).expect("ancestry");
        let ids: Vec<&str> = ancestry.iter().map(|v| v.id.as_str()).collect();
        assert_eq!(ids, vec![v1.id.as_str(), v2.id.as_str(), v3.id.as_str()]);
    }

    #[test]
    fn population_filters_by_layer_and_status() {
        let store = store();
        let skill = propose(&store, "skill", None);
        let card = propose(&store, "agent_card", None);
        let all = store.population(None, None, None, 50).expect("all");
        assert_eq!(all.len(), 2);
        let skills = store
            .population(Some("skill"), None, None, 50)
            .expect("skills");
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].id, skill.id);
        // Registering a variant moves the card experiment to running.
        variant(&store, &card.id, "v1", None, None);
        let running = store
            .population(Some("agent_card"), Some(STATUS_RUNNING), None, 50)
            .expect("running");
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].id, card.id);
        let proposed = store
            .population(Some("skill"), Some(STATUS_PROPOSED), None, 50)
            .expect("proposed");
        assert_eq!(proposed.len(), 1);
        assert_eq!(proposed[0].id, skill.id);
    }

    #[test]
    fn budget_refusal_at_the_boundary() {
        let store = store();
        let experiment = propose(&store, "skill", None); // max_runs = 18
        let baseline = variant_role(
            &store,
            &experiment.id,
            "baseline",
            None,
            Some("baseline"),
            VARIANT_ROLE_BASELINE,
        );
        // 9 recorded: 9/18 — fine.
        store
            .record_fitness(
                &experiment.id,
                &baseline.id,
                &["r1".to_string()],
                9,
                &serde_json::json!({}),
                None,
            )
            .expect("first half");
        // 9 more: exactly at the ceiling (18) — allowed (§P8.9 step 2: the
        // refusal is for records that would EXCEED the ceiling).
        store
            .record_fitness(
                &experiment.id,
                &baseline.id,
                &["r2".to_string()],
                9,
                &serde_json::json!({}),
                None,
            )
            .expect("at ceiling");
        // One more run would make 19 > 18 — typed refusal naming the
        // experiment, the would-be total, and the declared ceiling.
        let error = store
            .record_fitness(
                &experiment.id,
                &baseline.id,
                &["r3".to_string()],
                1,
                &serde_json::json!({}),
                None,
            )
            .expect_err("over ceiling");
        assert!(
            matches!(&error, ExperimentationError::BudgetExhausted(id, would_record, ceiling)
                if id == &experiment.id && *would_record == 19 && *ceiling == 18),
            "unexpected error: {error}"
        );
    }

    /// A-1: a max_runs above the i64 storage range must be rejected at the
    /// store boundary — an unchecked `as i64` cast wraps it negative, and a
    /// negative ceiling reads as an unspent budget forever (the budget
    /// refusal and the stuck-running sensor fail together). The defensive
    /// db_int floor rejects it even when a caller skips tool-boundary
    /// validation.
    #[test]
    fn oversized_max_runs_is_rejected_never_wrapped() {
        let store = store();
        let oversized = i64::MAX as u64 + 1; // 9223372036854775808 — wraps to i64::MIN
        let error = store
            .propose_experiment(
                "Hypothesis",
                "skill",
                &[".agents/skills/self-improvement/SKILL.md".to_string()],
                &serde_json::json!({"task_set": "fixed-5"}),
                "evaluator pass rate on the fixed set",
                &serde_json::json!({"cost_band_pct": 10}),
                &rubrics(),
                &Prediction {
                    claim: "variant B wins".to_string(),
                    confidence: 0.7,
                },
                &serde_json::json!({"max_runs": 18}),
                oversized,
                None,
                None,
                None,
            )
            .expect_err("oversized max_runs rejected, never wrapped");
        assert!(
            matches!(&error, ExperimentationError::Serialization(_)),
            "unexpected error: {error}"
        );
        // No partially-inserted experiment row survives the refusal — the
        // ceiling column never carries a wrapped value.
        let population = store.population(None, None, None, 50).expect("population");
        assert!(population.is_empty(), "no residue behind the refusal");
    }

    /// A-1: the fitness-record defensive floor — an oversized run_count is
    /// rejected before the write, and the budget refusal it produces names
    /// the true recorded total, not an undercounted one.
    #[test]
    fn oversized_run_count_is_rejected_never_wrapped() {
        let store = store();
        let experiment = propose(&store, "skill", None); // max_runs = 18
        let baseline = variant_role(
            &store,
            &experiment.id,
            "baseline",
            None,
            Some("baseline"),
            VARIANT_ROLE_BASELINE,
        );
        let oversized = i64::MAX as u64 + 1;
        let error = store
            .record_fitness(
                &experiment.id,
                &baseline.id,
                &["r1".to_string()],
                oversized,
                &serde_json::json!({}),
                None,
            )
            .expect_err("oversized run_count rejected, never wrapped");
        assert!(
            matches!(&error, ExperimentationError::Serialization(_)),
            "unexpected error: {error}"
        );
        // No partially-inserted fitness row survives behind the error.
        let fitness = store
            .fitness_for_experiment(&experiment.id)
            .expect("fitness list");
        assert!(fitness.is_empty(), "no residue behind the refusal");
    }

    /// A-1: the fitness budget predicate counts fitness records AND final
    /// evaluations — both consume the same declared budget (§P8.9 step 2),
    /// so a final evaluation followed by a fitness record cannot
    /// undercount and slip past the ceiling.
    #[test]
    fn fitness_budget_counts_final_evaluations_too() {
        let store = store();
        let experiment = propose(&store, "skill", None); // max_runs = 18
        let baseline = variant_role(
            &store,
            &experiment.id,
            "baseline",
            None,
            Some("baseline"),
            VARIANT_ROLE_BASELINE,
        );
        // Baseline fitness is required before a seal (SealFitnessMissing).
        store
            .record_fitness(
                &experiment.id,
                &baseline.id,
                &["r0".to_string()],
                3,
                &serde_json::json!({}),
                None,
            )
            .expect("baseline fitness");
        // Seal so a final evaluation is possible.
        let seal = store
            .seal_experiment(&experiment.id, &baseline.id, Some(0.5), None)
            .expect("seal");
        // Final evaluation consumes 12 more of the 18-run budget.
        store
            .record_final_evaluation(
                &experiment.id,
                &baseline.id,
                &["fev1".to_string()],
                12,
                &serde_json::json!({}),
            )
            .expect("final evaluation");
        // A subsequent post-seal fitness record of 9 would total 3 + 12 + 9
        // = 24 > 18 — the OLD fitness predicate counted only fitness_records
        // (3) and let 3 + 9 = 12 through; the repaired predicate counts both
        // tables and refuses it.
        let error = store
            .record_fitness(
                &experiment.id,
                &baseline.id,
                &["r1".to_string()],
                9,
                &serde_json::json!({}),
                Some(&seal.id),
            )
            .expect_err("fitness after final evaluation must count both");
        assert!(
            matches!(&error, ExperimentationError::BudgetExhausted(id, would_record, ceiling)
                if id == &experiment.id && *would_record == 24 && *ceiling == 18),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn health_snapshot_reports_recorded_runs_verdict_and_brier() {
        let store = store();
        let experiment = propose(&store, "skill", None); // confidence 0.7
        let baseline = variant_role(
            &store,
            &experiment.id,
            "baseline",
            None,
            Some("baseline"),
            VARIANT_ROLE_BASELINE,
        );
        store
            .record_fitness(
                &experiment.id,
                &baseline.id,
                &["r1".to_string()],
                9,
                &serde_json::json!({}),
                None,
            )
            .expect("fitness");
        store
            .record_selection(
                &experiment.id,
                VERDICT_SELECTED,
                Some(&baseline.id),
                &[],
                Some("card 1"),
                None,
            )
            .expect("selection");
        let snapshot = store.health_snapshot().expect("snapshot");
        assert_eq!(snapshot.experiments.len(), 1);
        let row = &snapshot.experiments[0];
        assert_eq!(row.id, experiment.id);
        assert_eq!(row.status, STATUS_RESOLVED);
        assert_eq!(row.max_runs, 18);
        assert_eq!(row.recorded_runs, 9);
        assert_eq!(row.verdict.as_deref(), Some(VERDICT_SELECTED));
        // selected at confidence 0.7 → Brier (0.7-1)^2 = 0.09
        let (brier, claims) = snapshot.resolved_claim_brier().expect("brier");
        assert!((brier - 0.09).abs() < 1e-9, "brier: {brier}");
        assert_eq!(claims, 1);
        // Resolved + selected → not stuck regardless of budget.
        assert!(snapshot.stuck_running(7).is_empty());
    }

    #[test]
    fn snapshot_stuck_and_void_exclusion_rules() {
        use crate::types::{ExperimentHealth, ExperimentationHealthSnapshot};
        let old = (chrono::Utc::now() - chrono::Duration::days(30)).to_rfc3339();
        let fresh = chrono::Utc::now().to_rfc3339();
        let snapshot = ExperimentationHealthSnapshot {
            experiments: vec![
                // Running, created 30 days ago → stuck on age (D-3: 7 days).
                ExperimentHealth {
                    id: "exp_stale".to_string(),
                    layer: "skill".to_string(),
                    status: STATUS_RUNNING.to_string(),
                    created_at: old,
                    max_runs: 18,
                    recorded_runs: 3,
                    prediction_confidence: Some(0.5),
                    verdict: None,
                    scientific_outcome: None,
                    reject_reasons: Vec::new(),
                },
                // Running, fresh, but the whole budget is recorded with no
                // verdict → stuck on spend.
                ExperimentHealth {
                    id: "exp_spent".to_string(),
                    layer: "skill".to_string(),
                    status: STATUS_RUNNING.to_string(),
                    created_at: fresh.clone(),
                    max_runs: 9,
                    recorded_runs: 9,
                    prediction_confidence: Some(0.5),
                    verdict: None,
                    scientific_outcome: None,
                    reject_reasons: Vec::new(),
                },
                // Running, fresh, budget unspent → healthy.
                ExperimentHealth {
                    id: "exp_healthy".to_string(),
                    layer: "skill".to_string(),
                    status: STATUS_RUNNING.to_string(),
                    created_at: fresh.clone(),
                    max_runs: 18,
                    recorded_runs: 3,
                    prediction_confidence: Some(0.5),
                    verdict: None,
                    scientific_outcome: None,
                    reject_reasons: Vec::new(),
                },
                // Rejected at 0.5 confidence → Brier 0.25.
                ExperimentHealth {
                    id: "exp_refuted".to_string(),
                    layer: "skill".to_string(),
                    status: STATUS_RESOLVED.to_string(),
                    created_at: fresh.clone(),
                    max_runs: 18,
                    recorded_runs: 18,
                    prediction_confidence: Some(0.5),
                    verdict: Some(VERDICT_REJECTED.to_string()),
                    scientific_outcome: None,
                    reject_reasons: vec!["claim refuted".to_string()],
                },
                // No-headroom void → excluded from the calibration record.
                ExperimentHealth {
                    id: "exp_void".to_string(),
                    layer: "skill".to_string(),
                    status: STATUS_RESOLVED.to_string(),
                    created_at: fresh.clone(),
                    max_runs: 9,
                    recorded_runs: 9,
                    prediction_confidence: Some(0.9),
                    verdict: Some(VERDICT_REJECTED.to_string()),
                    scientific_outcome: None,
                    reject_reasons: vec!["no-headroom: baseline saturated at 1.0".to_string()],
                },
                // Measurement-void (infrastructure failure) → also excluded:
                // the claim was never tested, so scoring it as refuted would
                // pollute the calibration record with provider noise.
                ExperimentHealth {
                    id: "exp_meas_void".to_string(),
                    layer: "skill".to_string(),
                    status: STATUS_RESOLVED.to_string(),
                    created_at: fresh,
                    max_runs: 18,
                    recorded_runs: 18,
                    prediction_confidence: Some(0.5),
                    verdict: Some(VERDICT_REJECTED.to_string()),
                    scientific_outcome: None,
                    reject_reasons: vec![
                        "measurement-void: 7/9 rollouts errored at the provider (0 tokens)"
                            .to_string(),
                    ],
                },
            ],
        };
        let stuck = snapshot.stuck_running(7);
        assert_eq!(
            stuck,
            vec!["exp_stale".to_string(), "exp_spent".to_string()]
        );
        // Only the refuted claim counts: both voids (no-headroom and
        // measurement-void) are excluded — mean Brier 0.25 over 1 claim.
        let (brier, claims) = snapshot.resolved_claim_brier().expect("brier");
        assert!((brier - 0.25).abs() < 1e-9, "brier: {brier}");
        assert_eq!(claims, 1);
    }

    /// The §P8.9 follow-up (2026-10-01): concurrent selections on one
    /// experiment record exactly one fossil. The insert and the conditional
    /// resolve are ONE transaction; a loser's UPDATE sees the winner's
    /// committed status, rolls back its insert, and surfaces
    /// `ExperimentResolved` — never a double-recorded verdict. Runs on a
    /// multi-connection file-backed pool (the in-memory helper is
    /// max_size(1) and serializes at the pool, which cannot reproduce the
    /// race).
    #[test]
    fn concurrent_selections_record_exactly_one_fossil() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("experimentation-race.db");
        let db_path = db_path.to_string_lossy().to_string();
        let pool = SqliteDriver::file_pool(&db_path).expect("file pool");
        let driver: Arc<dyn hkask_storage::database::driver::DatabaseDriver> =
            Arc::new(SqliteDriver::new_labeled(pool, db_path.as_str()));
        let store = Arc::new(ExperimentationStore::with_driver(driver).expect("race store"));

        let experiment = propose(&store, "skill", None);
        let variant = variant(&store, &experiment.id, "challenger", None, None);

        let (winners, losers) = std::thread::scope(|scope| {
            let mut handles = Vec::new();
            for _ in 0..4 {
                let store = Arc::clone(&store);
                let experiment_id = experiment.id.clone();
                let variant_id = variant.id.clone();
                handles.push(scope.spawn(move || {
                    store.record_selection(
                        &experiment_id,
                        VERDICT_SELECTED,
                        Some(&variant_id),
                        &[],
                        None,
                        None,
                    )
                }));
            }
            let mut winners = 0;
            let mut losers = 0;
            for handle in handles {
                match handle.join().expect("selection thread") {
                    Ok(_) => winners += 1,
                    Err(ExperimentationError::ExperimentResolved(_)) => losers += 1,
                    Err(other) => panic!("unexpected error: {other}"),
                }
            }
            (winners, losers)
        });
        assert_eq!(winners, 1, "exactly one selection records");
        assert_eq!(losers, 3, "the losers surface ExperimentResolved");
        let selections = store
            .selections_for_experiment(&experiment.id)
            .expect("selections");
        assert_eq!(selections.len(), 1, "exactly one fossil row");
        assert_eq!(
            store
                .experiment_by_id(&experiment.id)
                .expect("reload")
                .status,
            STATUS_RESOLVED
        );
    }

    #[test]
    fn rubric_bundle_round_trips_on_the_declaration() {
        let store = store();
        let experiment = propose(&store, "skill", None);
        assert_eq!(experiment.rubrics.len(), 2);
        assert_eq!(experiment.rubrics[0].id, "pass_rate_threshold");
        assert_eq!(experiment.rubrics[0].kind, RUBRIC_KIND_METRIC);
        assert_eq!(experiment.rubrics[1].kind, RUBRIC_KIND_IMPLEMENTATION);
    }

    #[test]
    fn rubric_validation_rejects_missing_kinds_duplicates_and_bad_ids() {
        let metric = || Rubric {
            id: "pass_rate".to_string(),
            description: "pass rate reaches threshold".to_string(),
            kind: RUBRIC_KIND_METRIC.to_string(),
            verification: RubricVerification::ReportRef {
                harness: "swarm_eval_agent_local".to_string(),
            },
        };
        let implementation = || Rubric {
            id: "uses_gate".to_string(),
            description: "routes the gate through lisp_eval".to_string(),
            kind: RUBRIC_KIND_IMPLEMENTATION.to_string(),
            verification: RubricVerification::Contains {
                spec: "lisp_eval".to_string(),
            },
        };
        // Empty bundle.
        assert!(matches!(
            validate_rubrics(&[]),
            Err(ExperimentationError::RubricsRequired)
        ));
        // Missing the metric rubric.
        assert!(matches!(
            validate_rubrics(&[implementation()]),
            Err(ExperimentationError::RubricsMissingKind(kind)) if kind == "metric"
        ));
        // Missing the implementation/grounded_report rubric.
        assert!(matches!(
            validate_rubrics(&[metric()]),
            Err(ExperimentationError::RubricsMissingKind(_))
        ));
        // Duplicate ids.
        let duplicate = vec![
            metric(),
            Rubric {
                id: "pass_rate".to_string(),
                description: "duplicate id".to_string(),
                kind: RUBRIC_KIND_GROUNDED_REPORT.to_string(),
                verification: RubricVerification::ReportRef {
                    harness: "swarm_eval_agent_local".to_string(),
                },
            },
        ];
        assert!(matches!(
            validate_rubrics(&duplicate),
            Err(ExperimentationError::DuplicateRubricId(_))
        ));
        // Unknown kind.
        let unknown = vec![
            Rubric {
                id: "a".to_string(),
                description: "d".to_string(),
                kind: "vibe".to_string(),
                verification: RubricVerification::Contains {
                    spec: "x".to_string(),
                },
            },
            implementation(),
        ];
        assert!(matches!(
            validate_rubrics(&unknown),
            Err(ExperimentationError::UnknownRubricKind(_))
        ));
        // Non-snake-case id.
        let bad_id = vec![
            Rubric {
                id: "PassRate".to_string(),
                ..metric()
            },
            implementation(),
        ];
        assert!(matches!(
            validate_rubrics(&bad_id),
            Err(ExperimentationError::RubricIdInvalid(_))
        ));
        // A valid bundle passes.
        assert!(validate_rubrics(&[metric(), implementation()]).is_ok());
    }

    #[test]
    fn variant_roles_are_enforced_one_baseline_and_parented_ablations() {
        let store = store();
        let experiment = propose(&store, "skill", None);
        let baseline = variant_role(
            &store,
            &experiment.id,
            "baseline",
            None,
            None,
            VARIANT_ROLE_BASELINE,
        );
        assert_eq!(baseline.role, VARIANT_ROLE_BASELINE);

        // Exactly one baseline — the refusal names the existing one.
        let error = store
            .register_variant(
                &experiment.id,
                &serde_json::json!({"name": "second baseline"}),
                None,
                VARIANT_ROLE_BASELINE,
                None,
            )
            .expect_err("second baseline refused");
        assert!(matches!(
            &error,
            ExperimentationError::DuplicateBaseline(id, existing)
                if id == &experiment.id && existing == &baseline.id
        ));

        // An ablation names its parent — the component it ablates.
        let error = store
            .register_variant(
                &experiment.id,
                &serde_json::json!({"name": "orphan ablation"}),
                None,
                VARIANT_ROLE_ABLATION,
                None,
            )
            .expect_err("unparented ablation refused");
        assert!(matches!(error, ExperimentationError::AblationWithoutParent));
        let ablation = variant_role(
            &store,
            &experiment.id,
            "no gate",
            Some(&baseline.id),
            None,
            VARIANT_ROLE_ABLATION,
        );
        assert_eq!(ablation.role, VARIANT_ROLE_ABLATION);
        assert_eq!(
            ablation.parent_variant_id.as_deref(),
            Some(baseline.id.as_str())
        );

        // Unknown roles are refused.
        let error = store
            .register_variant(
                &experiment.id,
                &serde_json::json!({"name": "vibe"}),
                None,
                "vibe",
                None,
            )
            .expect_err("unknown role refused");
        assert!(matches!(error, ExperimentationError::UnknownVariantRole(_)));

        // The default path registers a challenger.
        let challenger = variant(&store, &experiment.id, "challenger", None, None);
        assert_eq!(challenger.role, VARIANT_ROLE_CHALLENGER);
    }

    #[test]
    fn scientific_hypothesis_layer_round_trips() {
        // D2 (operator ruling 2026-10-15): the registry is the general lab
        // notebook — a scientific-hypothesis experiment declares and
        // queries like any other layer.
        let store = store();
        let experiment = propose(&store, "scientific_hypothesis", None);
        assert_eq!(experiment.layer, "scientific_hypothesis");
        let hypotheses = store
            .population(Some("scientific_hypothesis"), None, None, 50)
            .expect("population");
        assert_eq!(hypotheses.len(), 1);
        assert_eq!(hypotheses[0].id, experiment.id);
    }

    #[test]
    fn seal_freezes_the_contract_and_gates_challengers() {
        let store = store();
        let experiment = propose(&store, "skill", None);
        let baseline = variant_role(
            &store,
            &experiment.id,
            "baseline",
            None,
            None,
            VARIANT_ROLE_BASELINE,
        );

        // Pre-seal, a challenger cannot record fitness.
        let challenger = variant(&store, &experiment.id, "challenger", None, None);
        let error = store
            .record_fitness(
                &experiment.id,
                &challenger.id,
                &["r".to_string()],
                1,
                &serde_json::json!({}),
                None,
            )
            .expect_err("pre-seal challenger fitness refused");
        assert!(matches!(
            error,
            ExperimentationError::ExperimentNotSealed(_)
        ));

        // The baseline measures, then the seal freezes the contract.
        store
            .record_fitness(
                &experiment.id,
                &baseline.id,
                &["baseline run".to_string()],
                3,
                &serde_json::json!({"pass_rate": 0.6}),
                None,
            )
            .expect("baseline fitness");
        let seal = store
            .seal_experiment(&experiment.id, &baseline.id, Some(0.6), None)
            .expect("seal");
        assert_eq!(seal.baseline_variant_id, baseline.id);
        assert_eq!(seal.baseline_fitness_ids.len(), 1);
        assert_eq!(seal.fingerprint.len(), 64, "SHA-256 hex");

        // An identical retried seal converges on the existing record.
        let retried = store
            .seal_experiment(&experiment.id, &baseline.id, Some(0.6), None)
            .expect("replay converges");
        assert_eq!(retried.id, seal.id);

        // A different seal attempt is a typed refusal.
        let error = store
            .seal_experiment(&experiment.id, &baseline.id, Some(0.7), None)
            .expect_err("different seal refused");
        assert!(matches!(error, ExperimentationError::AlreadySealed(_, _)));

        // Post-seal, challenger fitness cites the seal.
        let fitness = store
            .record_fitness(
                &experiment.id,
                &challenger.id,
                &["challenger run".to_string()],
                3,
                &serde_json::json!({"pass_rate": 0.8}),
                Some(&seal.id),
            )
            .expect("post-seal challenger fitness");
        assert_eq!(fitness.seal_id.as_deref(), Some(seal.id.as_str()));

        // Without the citation, post-seal fitness is refused.
        let error = store
            .record_fitness(
                &experiment.id,
                &challenger.id,
                &["r".to_string()],
                1,
                &serde_json::json!({}),
                None,
            )
            .expect_err("missing citation refused");
        assert!(matches!(
            error,
            ExperimentationError::SealCitationMissing(_, _)
        ));

        // A wrong citation is refused.
        let error = store
            .record_fitness(
                &experiment.id,
                &challenger.id,
                &["r".to_string()],
                1,
                &serde_json::json!({}),
                Some("seal_other"),
            )
            .expect_err("wrong seal refused");
        assert!(matches!(error, ExperimentationError::SealMismatch(_, _)));
    }

    #[test]
    fn seal_headroom_and_baseline_role_are_enforced() {
        let store = store();
        let experiment = propose(&store, "skill", None);
        let baseline = variant_role(
            &store,
            &experiment.id,
            "baseline",
            None,
            None,
            VARIANT_ROLE_BASELINE,
        );
        store
            .record_fitness(
                &experiment.id,
                &baseline.id,
                &["r".to_string()],
                1,
                &serde_json::json!({"pass_rate": 1.0}),
                None,
            )
            .expect("saturated baseline fitness");

        // A saturated baseline is the no-headroom void condition (§5 Q6).
        let error = store
            .seal_experiment(&experiment.id, &baseline.id, Some(1.0), None)
            .expect_err("saturation refused");
        assert!(matches!(
            error,
            ExperimentationError::NoHeadroom(rate) if rate == 1.0
        ));

        // An out-of-range rate is invalid input.
        let error = store
            .seal_experiment(&experiment.id, &baseline.id, Some(1.5), None)
            .expect_err("out-of-range rate refused");
        assert!(matches!(
            error,
            ExperimentationError::SealPassRateInvalid(_)
        ));

        // The seal's variant must carry the baseline role.
        let challenger = variant(&store, &experiment.id, "challenger", None, None);
        let error = store
            .seal_experiment(&experiment.id, &challenger.id, Some(0.6), None)
            .expect_err("non-baseline seal refused");
        assert!(matches!(
            error,
            ExperimentationError::SealBaselineMismatch(_, _)
        ));

        // An unmeasured baseline cannot seal.
        let fresh = propose(&store, "skill", None);
        let fresh_baseline = variant_role(
            &store,
            &fresh.id,
            "baseline",
            None,
            None,
            VARIANT_ROLE_BASELINE,
        );
        let error = store
            .seal_experiment(&fresh.id, &fresh_baseline.id, Some(0.6), None)
            .expect_err("unmeasured baseline refused");
        assert!(matches!(error, ExperimentationError::SealFitnessMissing));
    }

    #[test]
    fn seal_fingerprint_is_deterministic_over_the_contract() {
        let store = store();
        let seal_baseline = |experiment: &ExperimentRecord| -> SealRecord {
            let baseline = variant_role(
                &store,
                &experiment.id,
                "baseline",
                None,
                None,
                VARIANT_ROLE_BASELINE,
            );
            store
                .record_fitness(
                    &experiment.id,
                    &baseline.id,
                    &["r".to_string()],
                    1,
                    &serde_json::json!({"pass_rate": 0.5}),
                    None,
                )
                .expect("baseline fitness");
            store
                .seal_experiment(&experiment.id, &baseline.id, Some(0.5), None)
                .expect("seal")
        };
        // Identical declarations seal to identical fingerprints.
        let first = propose(&store, "skill", None);
        let second = propose(&store, "skill", None);
        let first_seal = seal_baseline(&first);
        let second_seal = seal_baseline(&second);
        assert_eq!(first_seal.fingerprint, second_seal.fingerprint);

        // A different contract seals to a different fingerprint.
        let third = store
            .propose_experiment(
                "Different contract",
                "skill",
                &[".agents/skills/self-improvement/SKILL.md".to_string()],
                &serde_json::json!({"task_set": "fixed-5"}),
                "evaluator pass rate on the fixed set",
                &serde_json::json!({"cost_band_pct": 20}),
                &rubrics(),
                &Prediction {
                    claim: "c".to_string(),
                    confidence: 0.7,
                },
                &serde_json::json!({"max_runs": 18}),
                18,
                None,
                None,
                None,
            )
            .expect("propose");
        let third_seal = seal_baseline(&third);
        assert_ne!(first_seal.fingerprint, third_seal.fingerprint);
    }

    #[test]
    fn final_evaluation_requires_seal_disjointness_and_budget() {
        let store = store();
        let experiment = propose(&store, "skill", None); // max_runs = 18

        // No seal — the final evaluation requires one (P3).
        let baseline = variant_role(
            &store,
            &experiment.id,
            "baseline",
            None,
            None,
            VARIANT_ROLE_BASELINE,
        );
        let error = store
            .record_final_evaluation(
                &experiment.id,
                &baseline.id,
                &["r".to_string()],
                1,
                &serde_json::json!({}),
            )
            .expect_err("unsealed final evaluation refused");
        assert!(matches!(
            error,
            ExperimentationError::ExperimentNotSealed(_)
        ));

        // The sealed flow: baseline (3 runs) + seal + challenger (3 runs).
        store
            .record_fitness(
                &experiment.id,
                &baseline.id,
                &["b1".to_string()],
                3,
                &serde_json::json!({"pass_rate": 0.6}),
                None,
            )
            .expect("baseline fitness");
        let seal = store
            .seal_experiment(&experiment.id, &baseline.id, Some(0.6), None)
            .expect("seal");
        let challenger = variant(&store, &experiment.id, "challenger", None, None);
        store
            .record_fitness(
                &experiment.id,
                &challenger.id,
                &["c1".to_string()],
                3,
                &serde_json::json!({"pass_rate": 0.8}),
                Some(&seal.id),
            )
            .expect("challenger fitness");

        // A reused run ref is a typed refusal naming the overlap.
        let error = store
            .record_final_evaluation(
                &experiment.id,
                &challenger.id,
                &["c1".to_string()],
                3,
                &serde_json::json!({}),
            )
            .expect_err("overlapping refs refused");
        assert!(matches!(
            error,
            ExperimentationError::RunRefsNotDisjoint(overlap) if overlap == vec!["c1".to_string()]
        ));

        // A disjoint selection set records under the seal and the budget.
        let final_eval = store
            .record_final_evaluation(
                &experiment.id,
                &challenger.id,
                &["f1".to_string()],
                3,
                &serde_json::json!({"pass_rate": 0.85}),
            )
            .expect("final evaluation");
        assert_eq!(final_eval.seal_id, seal.id);
        assert_eq!(final_eval.variant_id, challenger.id);
        let readback = store
            .final_evaluations_for_experiment(&experiment.id)
            .expect("readback");
        assert_eq!(readback.len(), 1);
        assert_eq!(readback[0].id, final_eval.id);

        // The budget counts fitness + final evaluations: 9 recorded, a
        // 10-run final evaluation would make 19 > 18 — typed refusal.
        let error = store
            .record_final_evaluation(
                &experiment.id,
                &challenger.id,
                &["f2".to_string()],
                10,
                &serde_json::json!({}),
            )
            .expect_err("over ceiling refused");
        assert!(matches!(
            error,
            ExperimentationError::BudgetExhausted(id, would, ceiling)
                if id == experiment.id && would == 19 && ceiling == 18
        ));
    }

    // ── Scientific-design evolution (task `f1a90cd2`) ────────────────────

    /// Propose an experiment carrying a typed `DesignSpec` — the scientific
    /// declaration the artifact-only `propose` helper cannot express.
    fn propose_design(
        store: &ExperimentationStore,
        design_spec: &serde_json::Value,
    ) -> ExperimentRecord {
        store
            .propose_experiment(
                "Carrier exposure versus targeting dominates ADC efficacy",
                "scientific_hypothesis",
                &["experimental-design/design-spec-v1".to_string()],
                &serde_json::json!({"schema": "experimental-design-registry-adapter-v1"}),
                "primary interaction confidence width relative to the ratified margin",
                &serde_json::json!({"tie_rule": "within the practical margin"}),
                &rubrics(),
                &Prediction {
                    claim: "targeting contributes a practically meaningful advantage".to_string(),
                    confidence: 0.6,
                },
                &serde_json::json!({"max_runs": 24}),
                24,
                None,
                None,
                Some(design_spec),
            )
            .expect("propose with design")
    }

    fn design_spec(stopping: &str) -> serde_json::Value {
        serde_json::json!({
            "spec_version": "design-spec-v1",
            "spec_id": "adc-factorial-v1",
            "design_family": "blocked_factorial",
            "stopping": stopping,
        })
    }

    /// Register the baseline arm and record its fitness — the pre-seal
    /// preamble every seal test needs.
    fn baseline_with_fitness(
        store: &ExperimentationStore,
        experiment_id: &str,
        pass_rate: f64,
    ) -> VariantRecord {
        let baseline = variant_role(
            store,
            experiment_id,
            "baseline",
            None,
            None,
            VARIANT_ROLE_BASELINE,
        );
        store
            .record_fitness(
                experiment_id,
                &baseline.id,
                &["baseline report".to_string()],
                2,
                &serde_json::json!({"pass_rate": pass_rate}),
                None,
            )
            .expect("baseline fitness");
        baseline
    }

    /// Criterion 1: a typed, versioned `design_spec` is stored and read back
    /// through the record; a legacy eval_set-only declaration carries `None`.
    #[test]
    fn design_spec_is_stored_and_read_back() {
        let store = store();
        let with_design = propose_design(&store, &design_spec("fixed: all branches once"));
        assert_eq!(
            with_design
                .design_spec
                .as_ref()
                .and_then(|v| v.get("spec_id")),
            Some(&serde_json::json!("adc-factorial-v1")),
        );
        let readback = store.experiment_by_id(&with_design.id).expect("readback");
        assert!(readback.design_spec.is_some());

        // Legacy artifact declaration: no design_spec.
        let legacy = propose(&store, "skill", None);
        assert!(legacy.design_spec.is_none());
    }

    /// The store persists whatever design object it is handed — the
    /// version check is a tool-boundary contract (`validate_design_spec`,
    /// pinned in `tool_behavior.rs`), matching the `validate_rubrics` split.
    /// This pins the store half: an arbitrary object round-trips.
    #[test]
    fn store_round_trips_the_design_object_verbatim() {
        let store = store();
        let design = serde_json::json!({"spec_version": "design-spec-v1", "nested": {"a": 1}});
        let experiment = propose_design(&store, &design);
        assert_eq!(experiment.design_spec.as_ref(), Some(&design));
    }

    /// Criterion 1 (fingerprint): changing only the DesignSpec yields a
    /// different seal fingerprint — post-seal design drift is refused, not
    /// silently reused.
    #[test]
    fn design_spec_joins_the_seal_fingerprint() {
        let store = store();
        let seal_for = |stopping: &str| -> SealRecord {
            let experiment = propose_design(&store, &design_spec(stopping));
            let baseline = baseline_with_fitness(&store, &experiment.id, 0.6);
            store
                .seal_experiment(&experiment.id, &baseline.id, Some(0.6), None)
                .expect("seal")
        };
        let fixed = seal_for("fixed: all branches once");
        let sequential = seal_for("sequential: stop at 3 matches");
        assert_ne!(fixed.fingerprint, sequential.fingerprint);

        // A design-less legacy seal fingerprints stably (design_spec → null).
        let legacy = propose(&store, "skill", None);
        let legacy_baseline = baseline_with_fitness(&store, &legacy.id, 0.6);
        let a = store
            .seal_experiment(&legacy.id, &legacy_baseline.id, Some(0.6), None)
            .expect("legacy seal");
        assert!(a.baseline_pass_rate == Some(0.6));
        assert!(a.scientific_headroom.is_none());
    }

    /// Criterion 2: a design-family seal attests `scientific_headroom` with no
    /// baseline pass rate; supplying both or neither is a typed refusal.
    /// Covers the single-arm and observational-estimation design families.
    #[test]
    fn scientific_headroom_replaces_the_pass_rate() {
        let store = store();
        for family in ["single_arm", "observational_estimation"] {
            let experiment = propose_design(&store, &design_spec("fixed"));
            let baseline = baseline_with_fitness(&store, &experiment.id, 0.6);
            let headroom = ScientificHeadroom {
                design_family: family.to_string(),
                criterion: "the estimate interval width is below the ratified margin".to_string(),
                evidence: vec!["power simulation report".to_string()],
            };
            let seal = store
                .seal_experiment(&experiment.id, &baseline.id, None, Some(&headroom))
                .expect("headroom seal");
            assert!(seal.baseline_pass_rate.is_none());
            assert_eq!(
                seal.scientific_headroom
                    .as_ref()
                    .map(|h| h.design_family.as_str()),
                Some(family)
            );
        }

        let headroom = ScientificHeadroom {
            design_family: "single_arm".to_string(),
            criterion: "the single-arm interval width is below the ratified margin".to_string(),
            evidence: vec!["power simulation report".to_string()],
        };

        // Both supplied → typed refusal.
        let both = propose_design(&store, &design_spec("fixed"));
        let both_baseline = baseline_with_fitness(&store, &both.id, 0.6);
        let error = store
            .seal_experiment(&both.id, &both_baseline.id, Some(0.6), Some(&headroom))
            .expect_err("both attestations refused");
        assert!(matches!(
            error,
            ExperimentationError::SealAttestationConflict
        ));

        // Neither supplied → typed refusal.
        let neither = propose_design(&store, &design_spec("fixed"));
        let neither_baseline = baseline_with_fitness(&store, &neither.id, 0.6);
        let error = store
            .seal_experiment(&neither.id, &neither_baseline.id, None, None)
            .expect_err("no attestation refused");
        assert!(matches!(
            error,
            ExperimentationError::SealAttestationConflict
        ));
    }

    /// Criterion 3 + calibration: only `corroborated` and `rejected` typed
    /// outcomes calibrate; equivalence, inconclusive and the two void
    /// outcomes are excluded on the typed field, never by reject-reason text.
    /// Each outcome gets its own store so the aggregate Brier reading isolates
    /// exactly one claim.
    #[test]
    fn scientific_outcomes_drive_calibration_exclusion() {
        let outcome_brier = |outcome: &str| -> Option<(f64, u32)> {
            let store = store();
            let experiment = propose_design(&store, &design_spec("fixed"));
            baseline_with_fitness(&store, &experiment.id, 0.6);
            let reasons: Vec<String> = if outcome == SCIENTIFIC_OUTCOME_REJECTED {
                vec!["no variant beat the margin".to_string()]
            } else {
                Vec::new()
            };
            store
                .record_selection(&experiment.id, outcome, None, &reasons, None, Some(outcome))
                .expect("selection");
            store
                .health_snapshot()
                .expect("snapshot")
                .resolved_claim_brier()
        };
        // `corroborated` calibrates (claim held).
        assert!(outcome_brier(SCIENTIFIC_OUTCOME_CORROBORATED).is_some());
        // `rejected` calibrates (claim refuted).
        assert!(outcome_brier(SCIENTIFIC_OUTCOME_REJECTED).is_some());
        // The bounded and void outcomes never enter calibration.
        assert!(outcome_brier(SCIENTIFIC_OUTCOME_EQUIVALENCE).is_none());
        assert!(outcome_brier(SCIENTIFIC_OUTCOME_INCONCLUSIVE).is_none());
        assert!(outcome_brier(SCIENTIFIC_OUTCOME_MEASUREMENT_VOID).is_none());
        assert!(outcome_brier(SCIENTIFIC_OUTCOME_DESIGN_VOID).is_none());
    }

    /// Criterion 3 (vocabulary): an unknown scientific outcome is a typed
    /// refusal, and a resolution conflict (outcome without matching token) is
    /// refused — a scientific outcome is never renamed selected/rejected.
    #[test]
    fn scientific_outcome_vocabulary_is_closed() {
        let store = store();
        let experiment = propose_design(&store, &design_spec("fixed"));
        baseline_with_fitness(&store, &experiment.id, 0.6);
        let error = store
            .record_selection(&experiment.id, "proved", None, &[], None, Some("proved"))
            .expect_err("unknown outcome refused");
        assert!(matches!(
            error,
            ExperimentationError::UnknownScientificOutcome(_)
        ));
        // A legacy verdict token paired with a scientific outcome is refused.
        let error = store
            .record_selection(
                &experiment.id,
                VERDICT_REJECTED,
                None,
                &[],
                None,
                Some(SCIENTIFIC_OUTCOME_INCONCLUSIVE),
            )
            .expect_err("resolution conflict refused");
        assert!(matches!(
            error,
            ExperimentationError::SelectionResolutionConflict(..)
        ));
    }

    /// Criterion 4 (legacy): a legacy artifact experiment still seals with a
    /// pass rate and resolves selected/rejected — the scientific vocabulary
    /// is additive, never a replacement.
    #[test]
    fn legacy_artifact_experiments_are_unchanged() {
        let store = store();
        let experiment = propose(&store, "skill", None);
        let baseline = baseline_with_fitness(&store, &experiment.id, 0.5);
        let seal = store
            .seal_experiment(&experiment.id, &baseline.id, Some(0.5), None)
            .expect("legacy seal");
        assert_eq!(seal.baseline_pass_rate, Some(0.5));
        let (selection, updated) = store
            .record_selection(
                &experiment.id,
                VERDICT_SELECTED,
                Some(&baseline.id),
                &[],
                None,
                None,
            )
            .expect("legacy selection");
        assert_eq!(selection.verdict, VERDICT_SELECTED);
        assert!(selection.scientific_outcome.is_none());
        assert_eq!(updated.status, STATUS_RESOLVED);
    }

    /// The migration guard (task `f1a90cd2`): a database created with the
    /// PRE-evolution schema (no `design_spec`, no `scientific_outcome`, and a
    /// `NOT NULL` `baseline_pass_rate`) is upgraded in place — existing rows
    /// survive with their values intact, and the new columns arrive. This is
    /// the property that protects the live registry's resolved experiments.
    #[test]
    fn pre_evolution_database_is_migrated_without_data_loss() {
        let driver = SqliteDriver::in_memory_driver();
        // The pre-2026-10-19 schema, verbatim from the previous revision.
        driver
            .execute_batch(
                "CREATE TABLE experiments (\
                     id TEXT PRIMARY KEY, experiment_key TEXT UNIQUE, hypothesis TEXT NOT NULL, \
                     layer TEXT NOT NULL, genotype_refs TEXT NOT NULL, eval_set TEXT NOT NULL, \
                     fitness_fn TEXT NOT NULL, noise_band TEXT NOT NULL, prediction TEXT NOT NULL, \
                     budget TEXT NOT NULL, max_runs INTEGER NOT NULL, linked_goal_id TEXT, \
                     rubrics TEXT NOT NULL, status TEXT NOT NULL, created_at TEXT NOT NULL \
                 ); \
                 CREATE TABLE selection_records (\
                     id TEXT PRIMARY KEY, experiment_id TEXT NOT NULL, verdict TEXT NOT NULL, \
                     selected_variant_id TEXT, reject_reasons TEXT NOT NULL, \
                     algedonic_reference TEXT, created_at TEXT NOT NULL \
                 ); \
                 CREATE TABLE seals (\
                     id TEXT PRIMARY KEY, experiment_id TEXT NOT NULL UNIQUE, \
                     fingerprint TEXT NOT NULL, baseline_variant_id TEXT NOT NULL, \
                     baseline_fitness_ids TEXT NOT NULL, baseline_pass_rate TEXT NOT NULL, \
                     created_at TEXT NOT NULL \
                 );",
            )
            .expect("pre-evolution schema");
        // One pre-existing resolved experiment with a sealed pass rate.
        driver
            .execute_batch(
                "INSERT INTO experiments VALUES (\
                     'exp_legacy', NULL, 'legacy hypothesis', 'skill', '[\"r\"]', '{}', 'f', '{}', \
                     '{\"claim\":\"c\",\"confidence\":0.7}', '{\"max_runs\":6}', 6, NULL, \
                     '[]', 'resolved', '2026-10-16T09:00:00Z'); \
                 INSERT INTO seals VALUES (\
                     'seal_legacy', 'exp_legacy', 'fp', 'var_legacy', '[]', '0.6', \
                     '2026-10-16T09:00:00Z');",
            )
            .expect("legacy rows");

        // Opening the store runs the guarded migration.
        let store = ExperimentationStore::with_driver(driver).expect("migrated store");

        // The pre-existing seal survived with its pass rate, and the new
        // nullable column is present and NULL.
        let seal = store
            .seal_for_experiment("exp_legacy")
            .expect("seal read")
            .expect("seal present");
        assert_eq!(seal.baseline_pass_rate, Some(0.6));
        assert!(seal.scientific_headroom.is_none());
        // The experiment read back with no design_spec.
        let experiment = store
            .experiment_by_id("exp_legacy")
            .expect("experiment read");
        assert!(experiment.design_spec.is_none());
    }

    /// The migration guard is idempotent: opening an already-migrated store a
    /// second time is a no-op (no duplicate columns, no error).
    #[test]
    fn migration_is_idempotent() {
        let driver = SqliteDriver::in_memory_driver();
        let _first = ExperimentationStore::with_driver(driver.clone()).expect("first open");
        // A fresh store over a database whose schema was created by the first
        // open must not error on the second migration pass.
        ExperimentationStore::with_driver(driver).expect("second open");
    }
}
