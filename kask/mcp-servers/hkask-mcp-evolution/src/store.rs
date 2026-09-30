//! SQLCipher-backed experiment registry (§P8.4).
//!
//! One database per server (the per-agent default under the hKask data dir,
//! overridable via `HKASK_EVOLUTION_DB`), opened through the canonical
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

use crate::types::{
    EvolutionError, ExperimentRecord, FitnessRecord, Prediction, STATUS_PROPOSED, STATUS_RESOLVED,
    STATUS_RUNNING, SelectionRecord, VERDICT_REJECTED, VERDICT_SELECTED, VariantRecord,
};

/// Serialize a column value as canonical JSON text.
fn value_column<T: serde::Serialize>(value: &T) -> Result<String, EvolutionError> {
    serde_json::to_string(value).map_err(|error| EvolutionError::Serialization(error.to_string()))
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

fn experiment_from_row(row: &DbRow) -> Result<ExperimentRecord, EvolutionError> {
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
        status: row.get_str(9)?.to_string(),
        created_at: row.get_str(10)?.to_string(),
    })
}

fn variant_from_row(row: &DbRow) -> Result<VariantRecord, EvolutionError> {
    Ok(VariantRecord {
        id: row.get_str(0)?.to_string(),
        experiment_id: row.get_str(1)?.to_string(),
        genotype_config: row.get_json(2)?,
        parent_variant_id: optional_text(row, 3),
        created_at: row.get_str(4)?.to_string(),
    })
}

fn fitness_from_row(row: &DbRow) -> Result<FitnessRecord, EvolutionError> {
    Ok(FitnessRecord {
        id: row.get_str(0)?.to_string(),
        experiment_id: row.get_str(1)?.to_string(),
        variant_id: row.get_str(2)?.to_string(),
        runs: row.get_json(3)?,
        scores: row.get_json(4)?,
        created_at: row.get_str(5)?.to_string(),
    })
}

fn selection_from_row(row: &DbRow) -> Result<SelectionRecord, EvolutionError> {
    Ok(SelectionRecord {
        id: row.get_str(0)?.to_string(),
        experiment_id: row.get_str(1)?.to_string(),
        verdict: row.get_str(2)?.to_string(),
        selected_variant_id: optional_text(row, 3),
        reject_reasons: row.get_json(4)?,
        algedonic_reference: optional_text(row, 5),
        created_at: row.get_str(6)?.to_string(),
    })
}

pub struct EvolutionStore {
    driver: Arc<dyn DatabaseDriver>,
}

impl EvolutionStore {
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
                 status TEXT NOT NULL, \
                 created_at TEXT NOT NULL \
             ); \
             CREATE TABLE IF NOT EXISTS variants (\
                 id TEXT PRIMARY KEY, \
                 experiment_id TEXT NOT NULL, \
                 variant_key TEXT, \
                 genotype_config TEXT NOT NULL, \
                 parent_variant_id TEXT, \
                 created_at TEXT NOT NULL, \
                 UNIQUE (experiment_id, variant_key) \
             ); \
             CREATE TABLE IF NOT EXISTS fitness_records (\
                 id TEXT PRIMARY KEY, \
                 experiment_id TEXT NOT NULL, \
                 variant_id TEXT NOT NULL, \
                 runs TEXT NOT NULL, \
                 scores TEXT NOT NULL, \
                 created_at TEXT NOT NULL \
             ); \
             CREATE TABLE IF NOT EXISTS selection_records (\
                 id TEXT PRIMARY KEY, \
                 experiment_id TEXT NOT NULL, \
                 verdict TEXT NOT NULL, \
                 selected_variant_id TEXT, \
                 reject_reasons TEXT NOT NULL, \
                 algedonic_reference TEXT, \
                 created_at TEXT NOT NULL \
             ); \
             CREATE INDEX IF NOT EXISTS variants_by_experiment ON variants(experiment_id); \
             CREATE INDEX IF NOT EXISTS fitness_by_variant ON fitness_records(variant_id); \
             CREATE INDEX IF NOT EXISTS selection_by_experiment ON selection_records(experiment_id);",
        )?;
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
        prediction: &Prediction,
        budget: &serde_json::Value,
        experiment_key: Option<&str>,
    ) -> Result<ExperimentRecord, EvolutionError> {
        let id = format!("exp_{}", uuid::Uuid::new_v4().simple());
        let created_at = chrono::Utc::now().to_rfc3339();
        let inserted = self.driver.execute(
            "INSERT OR IGNORE INTO experiments \
             (id, experiment_key, hypothesis, layer, genotype_refs, eval_set, \
              fitness_fn, noise_band, prediction, budget, status, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
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
                DbValue::Text(STATUS_PROPOSED.to_string()),
                DbValue::Text(created_at),
            ],
        )?;
        if inserted == 0 {
            // Converged on an existing experiment_key — return that record.
            let key = experiment_key.ok_or_else(|| {
                EvolutionError::Serialization(
                    "INSERT was ignored without an experiment_key — an unexpected \
                     constraint was violated"
                        .to_string(),
                )
            })?;
            return self.experiment_by_key(key);
        }
        self.experiment_by_id(&id)
    }

    pub fn experiment_by_id(&self, id: &str) -> Result<ExperimentRecord, EvolutionError> {
        let row = self.driver.query_optional(
            "SELECT id, hypothesis, layer, genotype_refs, eval_set, fitness_fn, \
             noise_band, prediction, budget, status, created_at \
             FROM experiments WHERE id = ?1",
            &[DbValue::Text(id.to_string())],
        )?;
        row.map(|row| experiment_from_row(&row))
            .transpose()?
            .ok_or_else(|| EvolutionError::ExperimentNotFound(id.to_string()))
    }

    fn experiment_by_key(&self, key: &str) -> Result<ExperimentRecord, EvolutionError> {
        let row = self.driver.query_optional(
            "SELECT id, hypothesis, layer, genotype_refs, eval_set, fitness_fn, \
             noise_band, prediction, budget, status, created_at \
             FROM experiments WHERE experiment_key = ?1",
            &[DbValue::Text(key.to_string())],
        )?;
        row.map(|row| experiment_from_row(&row))
            .transpose()?
            .ok_or_else(|| EvolutionError::ExperimentNotFound(format!("experiment_key {key}")))
    }

    // ── Variants ───────────────────────────────────────────────────────

    /// Register a variant inside an experiment. The first variant moves the
    /// experiment to running. A resolved experiment refuses new variants.
    pub fn register_variant(
        &self,
        experiment_id: &str,
        genotype_config: &serde_json::Value,
        parent_variant_id: Option<&str>,
        variant_key: Option<&str>,
    ) -> Result<VariantRecord, EvolutionError> {
        let experiment = self.experiment_by_id(experiment_id)?;
        if experiment.status == STATUS_RESOLVED {
            return Err(EvolutionError::ExperimentResolved(
                experiment_id.to_string(),
            ));
        }
        if let Some(parent) = parent_variant_id {
            let parent_record = self.variant_by_id(parent)?;
            if parent_record.experiment_id != experiment_id {
                return Err(EvolutionError::VariantNotInExperiment(
                    parent.to_string(),
                    experiment_id.to_string(),
                ));
            }
        }
        let id = format!("var_{}", uuid::Uuid::new_v4().simple());
        let created_at = chrono::Utc::now().to_rfc3339();
        let inserted = self.driver.execute(
            "INSERT OR IGNORE INTO variants \
             (id, experiment_id, variant_key, genotype_config, parent_variant_id, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            &[
                DbValue::Text(id.clone()),
                DbValue::Text(experiment_id.to_string()),
                text_or_null(variant_key),
                DbValue::Text(value_column(genotype_config)?),
                text_or_null(parent_variant_id),
                DbValue::Text(created_at.clone()),
            ],
        )?;
        if inserted == 0 {
            let key = variant_key.ok_or_else(|| {
                EvolutionError::Serialization(
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
            created_at,
        })
    }

    pub fn variant_by_id(&self, id: &str) -> Result<VariantRecord, EvolutionError> {
        let row = self.driver.query_optional(
            "SELECT id, experiment_id, genotype_config, parent_variant_id, created_at \
             FROM variants WHERE id = ?1",
            &[DbValue::Text(id.to_string())],
        )?;
        row.map(|row| variant_from_row(&row))
            .transpose()?
            .ok_or_else(|| EvolutionError::VariantNotFound(id.to_string()))
    }

    fn variant_by_key(
        &self,
        experiment_id: &str,
        key: &str,
    ) -> Result<VariantRecord, EvolutionError> {
        let row = self.driver.query_optional(
            "SELECT id, experiment_id, genotype_config, parent_variant_id, created_at \
             FROM variants WHERE experiment_id = ?1 AND variant_key = ?2",
            &[
                DbValue::Text(experiment_id.to_string()),
                DbValue::Text(key.to_string()),
            ],
        )?;
        row.map(|row| variant_from_row(&row))
            .transpose()?
            .ok_or_else(|| EvolutionError::VariantNotFound(format!("variant_key {key}")))
    }

    pub fn variants_for_experiment(
        &self,
        experiment_id: &str,
    ) -> Result<Vec<VariantRecord>, EvolutionError> {
        let rows = self.driver.query(
            "SELECT id, experiment_id, genotype_config, parent_variant_id, created_at \
             FROM variants WHERE experiment_id = ?1 ORDER BY created_at",
            &[DbValue::Text(experiment_id.to_string())],
        )?;
        rows.iter().map(variant_from_row).collect()
    }

    /// The variant's ancestry chain, oldest parent first, ending with the
    /// variant itself (§P8.1 lineage). Cycle-guarded: a malformed parent loop
    /// surfaces as an error, never an infinite walk.
    pub fn variant_ancestry(&self, variant_id: &str) -> Result<Vec<VariantRecord>, EvolutionError> {
        let mut chain = Vec::new();
        let mut current = Some(variant_id.to_string());
        while let Some(id) = current {
            let variant = self.variant_by_id(&id)?;
            current = variant.parent_variant_id.clone();
            chain.push(variant);
            if chain.len() > 128 {
                return Err(EvolutionError::Serialization(
                    "variant ancestry exceeds 128 members — a parent cycle is suspected"
                        .to_string(),
                ));
            }
        }
        chain.reverse();
        Ok(chain)
    }

    // ── Fitness ────────────────────────────────────────────────────────

    /// Append an immutable, grounded fitness record. Recorded report refs
    /// only — the caller asserts these name real harness/evaluator reports
    /// (§P8.3: never simulated). A resolved experiment refuses new fitness.
    pub fn record_fitness(
        &self,
        experiment_id: &str,
        variant_id: &str,
        runs: &[String],
        scores: &serde_json::Value,
    ) -> Result<FitnessRecord, EvolutionError> {
        let experiment = self.experiment_by_id(experiment_id)?;
        if experiment.status == STATUS_RESOLVED {
            return Err(EvolutionError::ExperimentResolved(
                experiment_id.to_string(),
            ));
        }
        let variant = self.variant_by_id(variant_id)?;
        if variant.experiment_id != experiment_id {
            return Err(EvolutionError::VariantNotInExperiment(
                variant_id.to_string(),
                experiment_id.to_string(),
            ));
        }
        let id = format!("fit_{}", uuid::Uuid::new_v4().simple());
        let created_at = chrono::Utc::now().to_rfc3339();
        self.driver.execute(
            "INSERT INTO fitness_records \
             (id, experiment_id, variant_id, runs, scores, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            &[
                DbValue::Text(id.clone()),
                DbValue::Text(experiment_id.to_string()),
                DbValue::Text(variant_id.to_string()),
                DbValue::Text(value_column(&runs)?),
                DbValue::Text(value_column(scores)?),
                DbValue::Text(created_at.clone()),
            ],
        )?;
        Ok(FitnessRecord {
            id,
            experiment_id: experiment_id.to_string(),
            variant_id: variant_id.to_string(),
            runs: runs.to_vec(),
            scores: scores.clone(),
            created_at,
        })
    }

    pub fn fitness_for_variant(
        &self,
        variant_id: &str,
    ) -> Result<Vec<FitnessRecord>, EvolutionError> {
        let rows = self.driver.query(
            "SELECT id, experiment_id, variant_id, runs, scores, created_at \
             FROM fitness_records WHERE variant_id = ?1 ORDER BY created_at",
            &[DbValue::Text(variant_id.to_string())],
        )?;
        rows.iter().map(fitness_from_row).collect()
    }

    pub fn fitness_for_experiment(
        &self,
        experiment_id: &str,
    ) -> Result<Vec<FitnessRecord>, EvolutionError> {
        let rows = self.driver.query(
            "SELECT id, experiment_id, variant_id, runs, scores, created_at \
             FROM fitness_records WHERE experiment_id = ?1 ORDER BY created_at",
            &[DbValue::Text(experiment_id.to_string())],
        )?;
        rows.iter().map(fitness_from_row).collect()
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
    ) -> Result<(SelectionRecord, ExperimentRecord), EvolutionError> {
        let experiment = self.experiment_by_id(experiment_id)?;
        if experiment.status == STATUS_RESOLVED {
            return Err(EvolutionError::ExperimentResolved(
                experiment_id.to_string(),
            ));
        }
        match verdict {
            VERDICT_SELECTED => {
                let Some(selected) = selected_variant_id else {
                    return Err(EvolutionError::SelectedWithoutVariant);
                };
                let variant = self.variant_by_id(selected)?;
                if variant.experiment_id != experiment_id {
                    return Err(EvolutionError::VariantNotInExperiment(
                        selected.to_string(),
                        experiment_id.to_string(),
                    ));
                }
            }
            VERDICT_REJECTED => {
                if reject_reasons.is_empty() {
                    return Err(EvolutionError::RejectedWithoutReasons);
                }
            }
            other => return Err(EvolutionError::UnknownVerdict(other.to_string())),
        }
        let id = format!("sel_{}", uuid::Uuid::new_v4().simple());
        let created_at = chrono::Utc::now().to_rfc3339();
        self.driver.execute(
            "INSERT INTO selection_records \
             (id, experiment_id, verdict, selected_variant_id, reject_reasons, \
              algedonic_reference, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            &[
                DbValue::Text(id.clone()),
                DbValue::Text(experiment_id.to_string()),
                DbValue::Text(verdict.to_string()),
                text_or_null(selected_variant_id),
                DbValue::Text(value_column(&reject_reasons)?),
                text_or_null(algedonic_reference),
                DbValue::Text(created_at.clone()),
            ],
        )?;
        self.driver.execute(
            "UPDATE experiments SET status = ?2 WHERE id = ?1",
            &[
                DbValue::Text(experiment_id.to_string()),
                DbValue::Text(STATUS_RESOLVED.to_string()),
            ],
        )?;
        let updated = self.experiment_by_id(experiment_id)?;
        let record = SelectionRecord {
            id,
            experiment_id: experiment_id.to_string(),
            verdict: verdict.to_string(),
            selected_variant_id: selected_variant_id.map(str::to_string),
            reject_reasons: reject_reasons.to_vec(),
            algedonic_reference: algedonic_reference.map(str::to_string),
            created_at,
        };
        Ok((record, updated))
    }

    pub fn selections_for_experiment(
        &self,
        experiment_id: &str,
    ) -> Result<Vec<SelectionRecord>, EvolutionError> {
        let rows = self.driver.query(
            "SELECT id, experiment_id, verdict, selected_variant_id, reject_reasons, \
             algedonic_reference, created_at \
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
    ) -> Result<Vec<ExperimentRecord>, EvolutionError> {
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
                 noise_band, prediction, budget, status, created_at \
                 FROM experiments WHERE {where_clause} ORDER BY created_at DESC LIMIT {limit}"
            ),
            &params,
        )?;
        rows.iter().map(experiment_from_row).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Prediction;
    use hkask_storage::database::sqlite::SqliteDriver;

    fn store() -> EvolutionStore {
        EvolutionStore::with_driver(SqliteDriver::in_memory_driver())
            .expect("in-memory driver store")
    }

    fn propose(store: &EvolutionStore, layer: &str, key: Option<&str>) -> ExperimentRecord {
        store
            .propose_experiment(
                "Variant B beats the baseline on the fixed task set",
                layer,
                &[".agents/skills/self-improvement/SKILL.md".to_string()],
                &serde_json::json!({"task_set": "fixed-5", "pin_suites": ["lisp_eval_tool"]}),
                "evaluator pass rate on the fixed set",
                &serde_json::json!({"cost_band_pct": 10}),
                &Prediction {
                    claim: "variant B wins".to_string(),
                    confidence: 0.7,
                },
                &serde_json::json!({"energy_budget": "one eval run"}),
                key,
            )
            .expect("propose")
    }

    fn variant(
        store: &EvolutionStore,
        experiment_id: &str,
        name: &str,
        parent: Option<&str>,
        key: Option<&str>,
    ) -> VariantRecord {
        store
            .register_variant(
                experiment_id,
                &serde_json::json!({"name": name}),
                parent,
                key,
            )
            .expect("variant")
    }

    #[test]
    fn lifecycle_propose_register_fitness_select() {
        let store = store();
        let experiment = propose(&store, "skill", None);
        assert_eq!(experiment.status, STATUS_PROPOSED);

        let baseline = variant(&store, &experiment.id, "baseline", None, Some("baseline"));
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
        let fitness = store
            .record_fitness(
                &experiment.id,
                &challenger.id,
                &["swarm_eval_agent_local run 42".to_string()],
                &serde_json::json!({"pass_rate": 0.8, "total_tokens": 1200}),
            )
            .expect("fitness");
        assert_eq!(fitness.variant_id, challenger.id);
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
            .register_variant(&experiment.id, &serde_json::json!({}), None, None)
            .expect_err("resolved refuses variants");
        assert!(matches!(error, EvolutionError::ExperimentResolved(_)));
        let error = store
            .record_selection(
                &experiment.id,
                VERDICT_REJECTED,
                None,
                &["late".into()],
                None,
            )
            .expect_err("resolved refuses re-selection");
        assert!(matches!(error, EvolutionError::ExperimentResolved(_)));
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
                None,
            )
            .expect_err("cross-experiment parent");
        assert!(matches!(
            error,
            EvolutionError::VariantNotInExperiment(_, _)
        ));
    }

    #[test]
    fn selection_verdict_validation() {
        let store = store();
        let experiment = propose(&store, "skill", None);
        variant(&store, &experiment.id, "challenger", None, None);
        let error = store
            .record_selection(&experiment.id, VERDICT_SELECTED, None, &[], None)
            .expect_err("selected without a variant");
        assert!(matches!(error, EvolutionError::SelectedWithoutVariant));
        let error = store
            .record_selection(&experiment.id, VERDICT_REJECTED, None, &[], None)
            .expect_err("rejected without reasons");
        assert!(matches!(error, EvolutionError::RejectedWithoutReasons));
        let error = store
            .record_selection(&experiment.id, "maybe", None, &["reason".into()], None)
            .expect_err("unknown verdict");
        assert!(matches!(error, EvolutionError::UnknownVerdict(_)));
        // A valid rejected selection still resolves the experiment.
        let (record, updated) = store
            .record_selection(
                &experiment.id,
                VERDICT_REJECTED,
                None,
                &["no variant beat the noise band".to_string()],
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
}
