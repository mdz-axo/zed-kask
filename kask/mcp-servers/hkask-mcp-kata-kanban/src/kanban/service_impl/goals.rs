//! Atomic canonical goal transitions. In-process host authority is deliberately
//! absent from the MCP router. Retrieval projections never own canonical data.
use super::{service::KanbanService, types::KanbanError};
use crate::kanban::VerificationCriterion;
use crate::kanban::types::goal::*;
use chrono::{DateTime, Utc};
use hkask_storage::HMem;
use hkask_types::{WebID, id::GoalID};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::HashSet;

const GOAL_ENTITY: &str = "kanban:goal";
impl From<hkask_storage::HMemError> for KanbanError {
    fn from(error: hkask_storage::HMemError) -> Self {
        KanbanError::Internal(format!("atomic goal transition failed: {error}"))
    }
}
fn invalid(message: impl Into<String>) -> KanbanError {
    KanbanError::InvalidInput(message.into())
}
fn nonempty(value: &str, field: &str) -> Result<(), KanbanError> {
    if value.trim().is_empty() {
        return Err(invalid(format!("{field} must be non-empty")));
    }
    Ok(())
}
fn digest(value: &impl Serialize) -> Result<String, KanbanError> {
    let bytes = serde_json::to_vec(value).map_err(|e| KanbanError::Internal(e.to_string()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
fn validate_digest_hex(value: &str) -> Result<(), KanbanError> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid("expected_sha256 must be exactly 64 hex digits"));
    }
    Ok(())
}
fn validate_source(source: &GoalSource) -> Result<(), KanbanError> {
    nonempty(&source.interaction_id, "interaction_id")?;
    nonempty(&source.session_id, "session_id")?;
    nonempty(&source.source, "source")
}
fn require_owner(goal: &Goal, owner: WebID) -> Result<(), KanbanError> {
    if goal.owner != owner {
        return Err(KanbanError::PermissionDenied(
            "goal is not owned by caller".into(),
        ));
    }
    Ok(())
}
fn evidence_ids(goal: &Goal) -> HashSet<&str> {
    goal.closure
        .iter()
        .flat_map(|c| &c.input.evidence_manifest)
        .map(|e| e.id.as_str())
        .collect()
}
fn validate_refs(ids: &[String], known: &HashSet<&str>) -> Result<(), KanbanError> {
    let mut unique = HashSet::new();
    for id in ids {
        if !unique.insert(id) || !known.contains(id.as_str()) {
            return Err(invalid(format!("duplicate or unknown evidence id {id}")));
        }
    }
    Ok(())
}

/// Validate host observations without performing any I/O or trusting worker
/// oracle metadata. Byte identities and consent are enforced before freezing input.
fn validate_artifacts(
    closure: &NativeGoalClosure,
    artifacts: &[GoalArtifactSnapshot],
    now: DateTime<Utc>,
) -> Result<(), KanbanError> {
    if artifacts.len() > GOAL_ARTIFACT_MAX_FILES {
        return Err(invalid("too many artifact observations"));
    }
    // Empty observations remain meaningful for historical/reference-only callers,
    // but can never authorize a supported/contradicted completion.
    if !artifacts.is_empty() && artifacts.len() != closure.evidence_manifest.len() {
        return Err(invalid(
            "artifact observations must cover every frozen manifest entry",
        ));
    }
    let mut ids = HashSet::new();
    let mut bytes = 0u64;
    for artifact in artifacts {
        let evidence = closure
            .evidence_manifest
            .iter()
            .find(|evidence| evidence.id == artifact.evidence_id)
            .ok_or_else(|| invalid("artifact observation references unknown evidence"))?;
        if !ids.insert(&artifact.evidence_id)
            || artifact.observed_at < closure.occurred_at
            || artifact.observed_at > now
        {
            return Err(invalid(
                "duplicate artifact observation or invalid read time",
            ));
        }
        let read = match &artifact.outcome {
            GoalArtifactReadOutcome::Fetched {
                source,
                sha256,
                byte_count,
                content,
            } => {
                if closure.artifact_access != Some(LocalArtifactAccessApproval::BoundedUtf8V1) {
                    return Err(invalid(
                        "fetched artifacts require authenticated native read approval",
                    ));
                }
                let actual = format!("{:x}", Sha256::digest(content.as_bytes()));
                let expected = evidence
                    .digest
                    .strip_prefix("sha256:")
                    .unwrap_or(&evidence.digest);
                if !std::path::Path::new(source).is_absolute()
                    || actual != *sha256
                    || !actual.eq_ignore_ascii_case(expected)
                    || *byte_count != content.len() as u64
                    || content.len() > GOAL_ARTIFACT_MAX_FILE_BYTES
                {
                    return Err(invalid(
                        "fetched artifact source, byte count or digest mismatch",
                    ));
                }
                *byte_count
            }
            GoalArtifactReadOutcome::Unavailable {
                reason, bytes_read, ..
            } => {
                nonempty(reason, "artifact read failure reason")?;
                if *bytes_read > (GOAL_ARTIFACT_MAX_FILE_BYTES + 1) as u64 {
                    return Err(invalid("failed artifact read exceeds per-file bound"));
                }
                *bytes_read
            }
        };
        bytes = bytes
            .checked_add(read)
            .ok_or_else(|| invalid("artifact byte count overflow"))?;
        if bytes > GOAL_ARTIFACT_MAX_TOTAL_BYTES as u64 {
            return Err(invalid("artifact observations exceed aggregate byte bound"));
        }
    }
    Ok(())
}

/// Read old records into a typed archive without reconstructing lost authority,
/// timing or probabilities. Raw historical fields (including scores) survive.
fn decode_goal(value: serde_json::Value) -> Result<Goal, KanbanError> {
    if let Some(version) = value.get("schema_version") {
        if version.as_u64() != Some(2) {
            return Err(invalid("unsupported canonical goal schema version"));
        }
        return serde_json::from_value(value)
            .map_err(|e| KanbanError::Internal(format!("goal decode: {e}")));
    }
    #[derive(serde::Deserialize)]
    struct HistoricalGoal {
        id: GoalID,
        goal_text: String,
        criteria: Vec<VerificationCriterion>,
        task_id: Option<hkask_types::id::TaskId>,
        verdicts: Vec<GoalVerdict>,
        owner: WebID,
        created_at: chrono::DateTime<Utc>,
        updated_at: chrono::DateTime<Utc>,
    }
    let old: HistoricalGoal = serde_json::from_value(value.clone())
        .map_err(|e| KanbanError::Internal(format!("legacy goal decode: {e}")))?;
    let spec_digest = digest(&(&old.goal_text, &old.criteria, old.task_id))?;
    Ok(Goal {
        schema_version: 2, id: old.id, goal_text: old.goal_text,
        criterion_ids: (1..=old.criteria.len()).map(|n| format!("criterion-{n}")).collect(),
        criteria: old.criteria, task_id: old.task_id, source: None, supersedes: None,
        spec_digest, verdicts: old.verdicts, closure: None, human_corrections: vec![], assessment_pending: false,
        assessments: vec![], forecast: None, observations: vec![], machine_artifact_observation: None,
        forecast_status: ForecastStatus::LegacyUnscored, scores: vec![], projections: vec![], revision: 0,
        legacy: Some(LegacyGoalArchive { raw: value, limitation: "Historical provenance-limited archive; original human receipt and prospective forecast eligibility not established. Unscored; no reconstructed outcomes.".into() }),
        owner: old.owner, created_at: old.created_at, updated_at: old.updated_at,
    })
}
fn publish(goal: &mut Goal, now: DateTime<Utc>) -> Result<(), KanbanError> {
    goal.revision = goal
        .revision
        .checked_add(1)
        .ok_or_else(|| invalid("goal revision overflow"))?;
    goal.updated_at = now;
    let mut snapshot = goal.clone();
    snapshot.projections.clear();
    let payload =
        serde_json::to_value(snapshot).map_err(|e| KanbanError::Internal(e.to_string()))?;
    goal.projections.push(GoalProjection {
        event_id: format!("{}:{}", goal.id, goal.revision),
        revision: goal.revision,
        payload_digest: digest(&payload)?,
        payload,
        delivered_receipt: None,
    });
    Ok(())
}
fn validate_forecast(forecast: &GoalForecast, now: DateTime<Utc>) -> Result<(), KanbanError> {
    for (value, field) in [
        (&forecast.proposition, "proposition"),
        (&forecast.execution_policy, "execution_policy"),
        (&forecast.resources, "resources"),
        (&forecast.cohort, "cohort"),
    ] {
        nonempty(value, field)?;
    }
    if !(0.0..=1.0).contains(&forecast.probability)
        || !(0.0..=1.0).contains(&forecast.baseline_probability)
    {
        return Err(invalid(
            "forecast and baseline probabilities must be finite and in [0,1]",
        ));
    }
    if forecast.horizon <= now || forecast.resolution_deadline < forecast.horizon {
        return Err(invalid(
            "forecast must be declared before horizon; resolution_deadline must not precede horizon",
        ));
    }
    if let GoalForecastResolver::MachineArtifactDigest {
        locator,
        expected_sha256,
    } = &forecast.resolver
    {
        local_artifact_path(locator).map_err(invalid)?;
        validate_digest_hex(expected_sha256)?;
    }
    if let GoalForecastResolver::MachineEvidence { source, oracle } = &forecast.resolver {
        return Err(invalid(format!(
            "machine_evidence is ineligible: no production machine observation adapter is implemented for source {source:?}, oracle {oracle:?}; supported resolvers: human_acceptance, machine_artifact_digest"
        )));
    }
    Ok(())
}

/// Resolve only the frozen target, after an assessor attempt. Unknown and
/// disputed records never synthesize y. Prior score versions remain auditable.
fn resolve_forecast(goal: &mut Goal, now: DateTime<Utc>) -> Result<(), KanbanError> {
    let Some(forecast) = &goal.forecast else {
        return Ok(());
    };
    if matches!(
        forecast.resolver,
        GoalForecastResolver::MachineEvidence { .. }
    ) {
        goal.forecast_status = ForecastStatus::UnsupportedResolver;
        return Ok(());
    }
    if !goal
        .assessments
        .iter()
        .any(|a| a.completion.is_some() || a.failure.is_some())
    {
        goal.forecast_status = ForecastStatus::PendingAssessment;
        return Ok(());
    }
    let outcome = match &forecast.resolver {
        GoalForecastResolver::HumanAcceptance => {
            goal.closure.as_ref().and_then(|closure| {
                // Corrections change the label of the original event, not its
                // horizon. A later receipt cannot move an early outcome into
                // the scoring window or invalidate a timely original outcome.
                let time = closure.input.occurred_at;
                let (event_id, outcome) = goal
                    .human_corrections
                    .last()
                    .map(|c| (c.event_id.clone(), c.outcome))
                    .unwrap_or_else(|| {
                        (
                            closure.input.event_id.clone(),
                            closure.input.choice.outcome(),
                        )
                    });
                (time >= forecast.horizon && time <= forecast.resolution_deadline)
                    .then_some((event_id, outcome))
            })
        }
        GoalForecastResolver::MachineArtifactDigest {
            expected_sha256, ..
        } => goal.machine_artifact_observation.as_ref().map(|receipt| {
            let within_window = receipt.observed_at >= forecast.horizon
                && receipt.observed_at <= forecast.resolution_deadline
                && goal.closure.as_ref().is_some_and(|closure| {
                    closure.input.occurred_at <= forecast.resolution_deadline
                });
            let outcome = if within_window {
                match &receipt.read {
                    MachineArtifactDigestRead::Observed { actual_sha256, .. } => {
                        Some(expected_sha256.eq_ignore_ascii_case(actual_sha256))
                    }
                    MachineArtifactDigestRead::Unavailable { .. } => None,
                }
            } else {
                None
            };
            (receipt.event_id.clone(), outcome)
        }),
        GoalForecastResolver::MachineEvidence { .. } => None,
    };
    let Some((event_id, outcome)) = outcome else {
        goal.forecast_status = if now > forecast.resolution_deadline {
            ForecastStatus::DeadlineElapsed
        } else {
            ForecastStatus::PendingOutcome
        };
        return Ok(());
    };
    let Some(outcome) = outcome else {
        goal.forecast_status = ForecastStatus::Unknown;
        return Ok(());
    };
    goal.forecast_status = ForecastStatus::Resolved;
    if goal
        .scores
        .last()
        .is_some_and(|s| s.outcome_event_id == event_id)
    {
        return Ok(());
    }
    let brier = hkask_forecast::brier_score(forecast.probability, outcome);
    let baseline_brier = hkask_forecast::brier_score(forecast.baseline_probability, outcome);
    let previous = goal.scores.last().map(|s| s.version);
    let version = previous
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| invalid("score version overflow"))?;
    goal.scores.push(GoalForecastScore {
        version,
        outcome_event_id: event_id,
        outcome,
        brier,
        baseline_brier,
        skill: (baseline_brier > 0.0).then(|| 1.0 - brier / baseline_brier),
        supersedes_version: previous,
        recorded_at: now,
    });
    Ok(())
}

impl KanbanService {
    pub fn goal_create(
        &self,
        declaration: GoalDeclaration,
        owner: WebID,
    ) -> Result<Goal, KanbanError> {
        nonempty(&declaration.goal_text, "goal_text")?;
        if !(1..=4).contains(&declaration.criteria.len()) {
            return Err(invalid("goal requires 1–4 explicit criteria"));
        }
        for criterion in &declaration.criteria {
            nonempty(&criterion.description, "criterion description")?;
        }
        validate_source(&declaration.source)?;
        if let Some(forecast) = &declaration.forecast {
            validate_forecast(forecast, self.goal_now())?;
        }
        if let Some(id) = declaration.supersedes {
            self.goal_get(id, owner)?
                .ok_or_else(|| Self::goal_not_found(id))?;
        }
        if let Some(id) = declaration.task_id {
            let task = self.task_get(id)?.ok_or_else(|| {
                KanbanError::NotFound(hkask_types::NotFound {
                    entity_type: "task".into(),
                    id: id.to_string(),
                })
            })?;
            if task.owner != owner {
                return Err(KanbanError::PermissionDenied(
                    "linked task is not owned by caller".into(),
                ));
            }
        }
        let spec_digest = digest(&(
            &declaration.goal_text,
            &declaration.criteria,
            &declaration.source,
            declaration.supersedes,
        ))?;
        let now = self.goal_now();
        let mut goal = Goal {
            schema_version: 2,
            id: GoalID::new(),
            goal_text: declaration.goal_text,
            criterion_ids: (1..=declaration.criteria.len())
                .map(|n| format!("criterion-{n}"))
                .collect(),
            criteria: declaration.criteria,
            task_id: declaration.task_id,
            source: Some(declaration.source),
            supersedes: declaration.supersedes,
            spec_digest,
            verdicts: vec![],
            closure: None,
            human_corrections: vec![],
            assessment_pending: false,
            assessments: vec![],
            forecast_status: if declaration.forecast.is_some() {
                ForecastStatus::PendingAssessment
            } else {
                ForecastStatus::NotDeclared
            },
            forecast: declaration.forecast,
            observations: vec![],
            machine_artifact_observation: None,
            scores: vec![],
            projections: vec![],
            revision: 0,
            legacy: None,
            owner,
            created_at: now,
            updated_at: now,
        };
        publish(&mut goal, self.goal_now())?;
        let value =
            serde_json::to_value(&goal).map_err(|e| KanbanError::Internal(e.to_string()))?;
        let ontology = hkask_types::HMemOntology {
            dimensions: vec![hkask_types::Dimension::Why.as_str().into()],
            dc_type: hkask_bridge_ontology::pko::STEP.into(),
            dc_source: "kanban".into(),
            pko_procedure: Some(goal.id.to_string()),
            ..Default::default()
        };
        self.store.insert(
            &HMem::new(GOAL_ENTITY, &goal.id.to_string(), value, owner).with_ontology(ontology),
        )?;
        Ok(goal)
    }

    fn goal_not_found(id: GoalID) -> KanbanError {
        KanbanError::NotFound(hkask_types::NotFound {
            entity_type: "goal".into(),
            id: id.to_string(),
        })
    }

    /// Resolve by semantic identity, not a potentially drifted storage key.
    /// Duplicate identities and row/payload ownership disagreement are errors.
    fn goal_rows(&self, owner: WebID) -> Result<Vec<(HMem, Goal)>, KanbanError> {
        let mut ids = HashSet::new();
        let mut rows = vec![];
        for row in self.store.query_by_entity(GOAL_ENTITY)? {
            if row.access.owner_webid != owner {
                continue;
            }
            let goal = decode_goal(row.value.clone())?;
            require_owner(&goal, owner)?;
            if !ids.insert(goal.id) {
                return Err(KanbanError::Internal(format!(
                    "duplicate durable goal {}",
                    goal.id
                )));
            }
            rows.push((row, goal));
        }
        Ok(rows)
    }
    pub fn goal_get(&self, id: GoalID, owner: WebID) -> Result<Option<Goal>, KanbanError> {
        Ok(self
            .goal_rows(owner)?
            .into_iter()
            .find(|(_, g)| g.id == id)
            .map(|(_, g)| g))
    }
    pub fn goal_list(&self, owner: &WebID) -> Result<Vec<Goal>, KanbanError> {
        let mut goals: Vec<_> = self
            .goal_rows(*owner)?
            .into_iter()
            .map(|(_, g)| g)
            .collect();
        goals.sort_by_key(|g| std::cmp::Reverse(g.created_at));
        Ok(goals)
    }
    /// Reconcile stored history atomically: archive legacy rows and mark older
    /// machine forecasts unsupported without deleting their observations/scores.
    /// No old claim becomes an eligible current producer or new scored case.
    pub fn goal_archive_legacy(&self, owner: WebID) -> Result<usize, KanbanError> {
        let mut count = 0;
        for (row, _) in self.goal_rows(owner)? {
            let changed =
                self.store
                    .update_value_atomic(GOAL_ENTITY, &row.attribute, |value| {
                        if value.get("schema_version").is_some() {
                            let mut goal = decode_goal(value)?;
                            require_owner(&goal, owner)?;
                            if matches!(
                                goal.forecast.as_ref().map(|forecast| &forecast.resolver),
                                Some(GoalForecastResolver::MachineEvidence { .. })
                            ) && goal.forecast_status != ForecastStatus::UnsupportedResolver
                            {
                                goal.forecast_status = ForecastStatus::UnsupportedResolver;
                                publish(&mut goal, self.goal_now())?;
                                return Ok((
                                    Some(serde_json::to_value(goal).map_err(|error| {
                                        KanbanError::Internal(error.to_string())
                                    })?),
                                    true,
                                ));
                            }
                            return Ok::<_, KanbanError>((None, false));
                        }
                        let goal = decode_goal(value)?;
                        require_owner(&goal, owner)?;
                        let value = serde_json::to_value(goal)
                            .map_err(|e| KanbanError::Internal(e.to_string()))?;
                        Ok((Some(value), true))
                    })?;
            if changed == Some(true) {
                count += 1;
            }
        }
        Ok(count)
    }

    fn transition_goal(
        &self,
        id: GoalID,
        owner: WebID,
        project: bool,
        change: impl FnOnce(&mut Goal) -> Result<bool, KanbanError>,
    ) -> Result<Goal, KanbanError> {
        let (row, _) = self
            .goal_rows(owner)?
            .into_iter()
            .find(|(_, g)| g.id == id)
            .ok_or_else(|| Self::goal_not_found(id))?;
        self.store
            .update_value_atomic(GOAL_ENTITY, &row.attribute, |value| {
                let mut goal = decode_goal(value)?;
                if goal.id != id {
                    return Err(invalid("goal identity changed during update"));
                }
                require_owner(&goal, owner)?;
                if goal.legacy.is_some() {
                    return Err(invalid(
                        "legacy goal is archived and unscored; declare a successor instead",
                    ));
                }
                let changed = change(&mut goal)?;
                if changed && project {
                    publish(&mut goal, self.goal_now())?;
                }
                let value = if changed {
                    Some(
                        serde_json::to_value(&goal)
                            .map_err(|e| KanbanError::Internal(e.to_string()))?,
                    )
                } else {
                    None
                };
                Ok::<_, KanbanError>((value, goal))
            })?
            .ok_or_else(|| Self::goal_not_found(id))
    }

    pub fn goal_judge(
        &self,
        id: GoalID,
        verdict: GoalVerdict,
        owner: WebID,
    ) -> Result<Goal, KanbanError> {
        self.transition_goal(id, owner, true, |goal| {
            if goal.closure.is_some() {
                return Err(invalid(
                    "work is closed; declare a successor for changed requirements",
                ));
            }
            if !(0.0..=1.0).contains(&verdict.confidence) {
                return Err(invalid("confidence must be finite and in [0,1]"));
            }
            let indices: HashSet<_> = verdict.criterion_results.iter().map(|c| c.index).collect();
            if indices.len() != goal.criteria.len()
                || indices.len() != verdict.criterion_results.len()
                || indices.iter().any(|i| *i >= goal.criteria.len())
            {
                return Err(invalid("verdict must cover every criterion exactly once"));
            }
            if verdict.verdict == GoalVerdictValue::Done
                && verdict.criterion_results.iter().any(|c| !c.passed)
            {
                return Err(invalid("done contradicts failed criteria"));
            }
            nonempty(&verdict.reasoning, "reasoning")?;
            for result in &verdict.criterion_results {
                nonempty(&result.note, "criterion evidence note")?;
            }
            goal.verdicts.push(verdict);
            Ok(true)
        })
    }

    /// Trusted host ingress. No actor string grants authority and no MCP route
    /// can reach this method. Authentication occurs in the native host adapter.
    pub fn goal_close_native(
        &self,
        id: GoalID,
        owner: WebID,
        input: NativeGoalClosure,
    ) -> Result<Goal, KanbanError> {
        self.transition_goal(id, owner, true, |goal| {
            let payload_digest = digest(&input)?;
            if let Some(existing) = &goal.closure {
                return if existing.payload_digest == payload_digest && existing.input == input {
                    Ok(false)
                } else {
                    Err(invalid("conflicting closure replay"))
                };
            }
            nonempty(&input.event_id, "closure event_id")?;
            validate_source(&input.source)?;
            if input.expected_spec_digest != goal.spec_digest {
                return Err(invalid("closure specification digest mismatch"));
            }
            if input.occurred_at < goal.created_at || input.occurred_at > self.goal_now() {
                return Err(invalid(
                    "closure occurred_at must be between declaration and receipt",
                ));
            }
            if input.evidence_manifest.len() > GOAL_ARTIFACT_MAX_FILES {
                return Err(invalid("evidence manifest exceeds 16-artifact bound"));
            }
            if let Some(approval) = &input.machine_artifact_access {
                let forecast = goal.forecast.as_ref().ok_or_else(|| invalid("machine artifact approval requires a forecast"))?;
                if !matches!(forecast.resolver, GoalForecastResolver::MachineArtifactDigest { .. }) || approval.spec_digest != goal.spec_digest || &approval.forecast != forecast {
                    return Err(invalid("native machine artifact consent must match the exact frozen specification and resolver"));
                }
            }
            let mut ids = HashSet::new();
            for evidence in &input.evidence_manifest {
                for (value, field) in [
                    (&evidence.id, "evidence id"),
                    (&evidence.source, "evidence source"),
                    (&evidence.locator, "evidence locator"),
                    (&evidence.digest, "evidence digest"),
                    (&evidence.reliability, "evidence reliability"),
                    (&evidence.oracle, "evidence oracle"),
                ] {
                    nonempty(value, field)?;
                }
                if !ids.insert(&evidence.id) {
                    return Err(invalid("duplicate evidence id"));
                }
            }
            goal.closure = Some(GoalClosure {
                input,
                payload_digest,
                recorded_at: self.goal_now(),
            });
            goal.assessment_pending = true;
            Ok(true)
        })
    }
    /// Append a source-backed human correction without rewriting work closure.
    pub fn goal_correct_human_outcome(
        &self,
        id: GoalID,
        owner: WebID,
        correction: NativeGoalOutcomeCorrection,
    ) -> Result<Goal, KanbanError> {
        self.transition_goal(id, owner, true, |goal| {
            if let Some(old) = goal.human_corrections.iter().find(|c| c.event_id == correction.event_id) {
                return if old == &correction { Ok(false) } else { Err(invalid("conflicting human correction replay")) };
            }
            let closure = goal.closure.as_ref().ok_or_else(|| invalid("correction requires original closure"))?;
            let previous = goal.human_corrections.last().map(|c| (&c.event_id, c.occurred_at))
                .unwrap_or((&closure.input.event_id, closure.input.occurred_at));
            nonempty(&correction.event_id, "correction event_id")?;
            nonempty(&correction.reason, "correction reason")?;
            validate_source(&correction.source)?;
            if correction.event_id == closure.input.event_id || correction.supersedes_event_id != *previous.0 || correction.occurred_at < previous.1 || correction.occurred_at > self.goal_now() || correction.expected_spec_digest != goal.spec_digest {
                return Err(invalid("correction must reference latest outcome and original specification with valid source time"));
            }
            goal.human_corrections.push(correction);
            resolve_forecast(goal, self.goal_now())?;
            Ok(true)
        })
    }
    pub fn goal_pending_work(&self, owner: &WebID) -> Result<Vec<Goal>, KanbanError> {
        Ok(self
            .goal_list(owner)?
            .into_iter()
            .filter(|g| {
                g.assessment_pending
                    || g.projections.iter().any(|p| p.delivered_receipt.is_none())
                    || matches!(
                        g.forecast_status,
                        ForecastStatus::PendingAssessment
                            | ForecastStatus::PendingOutcome
                            | ForecastStatus::Unknown
                            | ForecastStatus::Disputed
                    )
            })
            .collect())
    }
    /// Trusted ingress for the ONE production digest source adapter. Never a
    /// worker MCP route; no caller-supplied outcome boolean is accepted.
    #[doc(hidden)]
    pub fn goal_observe_machine_artifact_digest(
        &self,
        id: GoalID,
        owner: WebID,
        receipt: MachineArtifactDigestReceipt,
    ) -> Result<Goal, KanbanError> {
        self.transition_goal(id, owner, true, |goal| {
            if let Some(existing) = &goal.machine_artifact_observation {
                return if existing == &receipt { Ok(false) } else { Err(invalid("conflicting immutable machine artifact observation replay")) };
            }
            let forecast = goal.forecast.as_ref().ok_or_else(|| invalid("digest observation requires a declared forecast"))?;
            let GoalForecastResolver::MachineArtifactDigest { locator, .. } = &forecast.resolver else { return Err(invalid("digest observation requires the machine_artifact_digest source capability")); };
            let closure = goal.closure.as_ref().ok_or_else(|| invalid("digest observation requires native closure"))?;
            if receipt.event_id != format!("{}:machine-artifact-digest:v1", goal.id) || receipt.spec_digest != goal.spec_digest || &receipt.locator != locator {
                return Err(invalid("digest receipt does not match the exact frozen source, specification and event"));
            }
            if receipt.observed_at < forecast.horizon || receipt.observed_at < closure.input.occurred_at || receipt.observed_at > self.goal_now() {
                return Err(invalid("digest observation is premature or future-dated"));
            }
            if receipt.reliability != MACHINE_ARTIFACT_DIGEST_RELIABILITY || receipt.limitation != MACHINE_ARTIFACT_DIGEST_LIMITATION {
                return Err(invalid("digest receipt must retain file-byte-only authority limits"));
            }
            match &receipt.read {
                MachineArtifactDigestRead::Observed { canonical_path, actual_sha256, byte_count } => {
                    let approval = closure.input.machine_artifact_access.as_ref().ok_or_else(|| invalid("digest read requires separate native forecast-source consent"))?;
                    if approval.spec_digest != goal.spec_digest || &approval.forecast != forecast { return Err(invalid("digest read consent does not match the frozen resolver")); }
                    let path = local_artifact_path(locator).map_err(invalid)?;
                    if path.as_os_str() != std::path::Path::new(canonical_path).as_os_str() || *byte_count > GOAL_ARTIFACT_MAX_FILE_BYTES as u64 { return Err(invalid("digest read source or byte bound mismatch")); }
                    validate_digest_hex(actual_sha256)?;
                }
                MachineArtifactDigestRead::Unavailable { reason, observed_sha256, bytes_read } => {
                    nonempty(reason, "digest read limitation")?;
                    if *bytes_read > (GOAL_ARTIFACT_MAX_FILE_BYTES + 1) as u64 { return Err(invalid("failed digest read exceeded the bounded source capability")); }
                    if let Some(hash) = observed_sha256 { validate_digest_hex(hash)?; }
                }
            }
            goal.machine_artifact_observation = Some(receipt);
            resolve_forecast(goal, self.goal_now())?;
            Ok(true)
        })
    }

    /// Reconcile deadlines without a model turn; elapsed is not a false outcome.
    pub fn goal_refresh_forecast(&self, id: GoalID, owner: WebID) -> Result<Goal, KanbanError> {
        self.transition_goal(id, owner, true, |goal| {
            let old = goal.forecast_status.clone();
            resolve_forecast(goal, self.goal_now())?;
            Ok(old != goal.forecast_status)
        })
    }
    pub fn goal_claim_assessment(
        &self,
        id: GoalID,
        owner: WebID,
        lease: AssessmentLeaseRequest,
        artifacts: Vec<GoalArtifactSnapshot>,
    ) -> Result<GoalAssessmentInput, KanbanError> {
        let attempt_id = lease.attempt_id.clone();
        let goal = self.transition_goal(id, owner, true, |goal| {
            if let Some(attempt) = goal
                .assessments
                .iter()
                .find(|a| a.lease.attempt_id == lease.attempt_id)
            {
                return if attempt.lease == lease && attempt.input.artifacts == artifacts {
                    Ok(false)
                } else {
                    Err(invalid("conflicting assessment claim replay"))
                };
            }
            if !goal.assessment_pending {
                return Err(invalid("no pending assessment"));
            }
            nonempty(&lease.attempt_id, "attempt_id")?;
            nonempty(&lease.assessor_id, "assessor_id")?;
            nonempty(&lease.model_version, "model_version")?;
            nonempty(&lease.tool_version, "tool_version")?;
            if lease.lease_until <= self.goal_now() {
                return Err(invalid("assessment lease must expire in the future"));
            }
            if let Some(active) = goal
                .assessments
                .last_mut()
                .filter(|a| a.completion.is_none() && a.failure.is_none())
            {
                if active.lease.lease_until > self.goal_now() {
                    return Err(invalid("assessment lease is already held"));
                }
                active.failure = Some(AssessmentFailure {
                    attempt_id: active.lease.attempt_id.clone(),
                    input_digest: active.input_digest.clone(),
                    reason: "assessment lease expired".into(),
                });
            }
            let closure = goal
                .closure
                .as_ref()
                .ok_or_else(|| invalid("assessment requires closure"))?;
            validate_artifacts(&closure.input, &artifacts, self.goal_now())?;
            let input = GoalAssessmentInput {
                goal_id: id,
                spec_digest: goal.spec_digest.clone(),
                goal_text: goal.goal_text.clone(),
                criteria: goal
                    .criterion_ids
                    .iter()
                    .zip(&goal.criteria)
                    .map(|(id, c)| AssessmentCriterion {
                        id: id.clone(),
                        description: c.description.clone(),
                    })
                    .collect(),
                evidence_manifest: closure.input.evidence_manifest.clone(),
                artifacts,
            };
            let input_digest = digest(&input)?;
            goal.assessments.push(AssessmentAttempt {
                lease,
                input,
                input_digest,
                started_at: self.goal_now(),
                completion: None,
                failure: None,
            });
            resolve_forecast(goal, self.goal_now())?;
            Ok(true)
        })?;
        goal.assessments
            .iter()
            .find(|attempt| attempt.lease.attempt_id == attempt_id)
            .map(|a| a.input.clone())
            .ok_or_else(|| invalid("assessment input absent"))
    }

    pub fn goal_complete_assessment(
        &self,
        id: GoalID,
        owner: WebID,
        completion: AssessmentCompletion,
    ) -> Result<Goal, KanbanError> {
        self.transition_goal(id, owner, true, |goal| {
            let attempt = goal
                .assessments
                .iter()
                .find(|a| a.lease.attempt_id == completion.attempt_id)
                .ok_or_else(|| invalid("unknown assessment attempt"))?;
            if let Some(existing) = &attempt.completion {
                return if existing == &completion {
                    Ok(false)
                } else {
                    Err(invalid("conflicting completion replay"))
                };
            }
            if attempt.failure.is_some()
                || attempt.lease.lease_until <= self.goal_now()
                || attempt.input_digest != completion.input_digest
            {
                return Err(invalid(
                    "failed/expired assessment lease or input digest mismatch",
                ));
            }
            let ids: HashSet<_> = completion
                .findings
                .iter()
                .map(|f| f.criterion_id.as_str())
                .collect();
            if ids.len() != goal.criterion_ids.len()
                || ids.len() != completion.findings.len()
                || goal
                    .criterion_ids
                    .iter()
                    .any(|id| !ids.contains(id.as_str()))
            {
                return Err(invalid(
                    "assessment must cover every original criterion exactly once",
                ));
            }
            let known = evidence_ids(goal);
            let fetched: HashSet<_> = attempt.input.artifacts.iter().filter_map(|artifact| match artifact.outcome { GoalArtifactReadOutcome::Fetched { .. } => Some(artifact.evidence_id.as_str()), _ => None }).collect();
            for finding in &completion.findings {
                nonempty(&finding.note, "finding note")?;
                validate_refs(&finding.evidence_ids, &known)?;
                if finding.status != CriterionEvidenceStatus::Unknown {
                    if finding.evidence_ids.is_empty() || finding.evidence_ids.iter().any(|id| !fetched.contains(id.as_str())) {
                        return Err(invalid("supported/contradicted finding requires actually fetched digest-matched artifact IDs"));
                    }
                }
            }
            let attempt = goal
                .assessments
                .iter_mut()
                .find(|a| a.lease.attempt_id == completion.attempt_id)
                .ok_or_else(|| invalid("assessment disappeared"))?;
            attempt.completion = Some(completion);
            goal.assessment_pending = false;
            resolve_forecast(goal, self.goal_now())?;
            Ok(true)
        })
    }
    pub fn goal_fail_assessment(
        &self,
        id: GoalID,
        owner: WebID,
        failure: AssessmentFailure,
    ) -> Result<Goal, KanbanError> {
        self.transition_goal(id, owner, true, |goal| {
            let attempt = goal
                .assessments
                .iter_mut()
                .find(|a| a.lease.attempt_id == failure.attempt_id)
                .ok_or_else(|| invalid("unknown assessment attempt"))?;
            if let Some(existing) = &attempt.failure {
                return if existing == &failure {
                    Ok(false)
                } else {
                    Err(invalid("conflicting failure replay"))
                };
            }
            if attempt.completion.is_some() || attempt.input_digest != failure.input_digest {
                return Err(invalid("assessment completed or input digest mismatch"));
            }
            nonempty(&failure.reason, "assessment failure reason")?;
            attempt.failure = Some(failure);
            goal.assessment_pending = true;
            resolve_forecast(goal, self.goal_now())?;
            Ok(true)
        })
    }

    /// Mark an exact stored projection delivered. Never delete canonical data,
    /// and never publish a receipt-about-a-receipt (an infinite outbox loop).
    pub fn goal_mark_projection_delivered(
        &self,
        id: GoalID,
        owner: WebID,
        receipt: GoalProjectionReceipt,
    ) -> Result<Goal, KanbanError> {
        self.transition_goal(id, owner, false, |goal| {
            nonempty(&receipt.receipt_id, "receipt_id")?;
            let projection = goal
                .projections
                .iter_mut()
                .find(|p| p.event_id == receipt.event_id)
                .ok_or_else(|| invalid("unknown projection event"))?;
            if projection.revision != receipt.revision
                || projection.payload_digest != receipt.payload_digest
            {
                return Err(invalid("projection revision/digest mismatch"));
            }
            if let Some(existing) = &projection.delivered_receipt {
                return if existing == &receipt.receipt_id {
                    Ok(false)
                } else {
                    Err(invalid("conflicting projection receipt"))
                };
            }
            projection.delivered_receipt = Some(receipt.receipt_id);
            Ok(true)
        })
    }
}
