//! Atomic canonical goal transitions. In-process host authority is deliberately
//! absent from the MCP router. Retrieval projections never own canonical data.
use super::{service::KanbanService, types::KanbanError};
use crate::kanban::types::goal::*;
use chrono::{DateTime, Utc};
use hkask_storage::HMem;
use hkask_types::{WebID, id::GoalID};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

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
    // Empty observations are permitted for reference-only callers,
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

fn decode_goal(value: serde_json::Value) -> Result<Goal, KanbanError> {
    if value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        != Some(3)
    {
        return Err(invalid("unsupported goal schema: expected version 3"));
    }
    serde_json::from_value(value).map_err(|e| KanbanError::Internal(format!("goal decode: {e}")))
}
fn owned_goals(
    owner: WebID,
    rows: impl IntoIterator<Item = HMem>,
) -> Result<Vec<Goal>, KanbanError> {
    let mut goals = Vec::new();
    let mut ids = HashSet::new();
    for row in rows {
        if row.access.owner_webid != owner {
            continue;
        }
        let goal = decode_goal(row.value)?;
        require_owner(&goal, owner)?;
        if row.attribute != goal.id.to_string() || !ids.insert(goal.id) {
            return Err(invalid("goal key mismatch or duplicate canonical identity"));
        }
        goals.push(goal);
    }
    goals.sort_by_key(|goal| std::cmp::Reverse(goal.created_at));
    Ok(goals)
}

fn publish(goal: &mut Goal, now: DateTime<Utc>) -> Result<(), KanbanError> {
    goal.revision = goal
        .revision
        .checked_add(1)
        .ok_or_else(|| invalid("goal revision overflow"))?;
    goal.updated_at = now;
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
    Ok(())
}

/// Resolve only the frozen target, after an assessor attempt. Unknown and
/// ineligible records never synthesize y. Prior score versions remain auditable.
fn resolve_forecast(goal: &mut Goal, now: DateTime<Utc>) -> Result<(), KanbanError> {
    let Some(forecast) = &goal.forecast else {
        return Ok(());
    };
    if !goal.assessments.iter().any(|a| a.result.is_some()) {
        goal.forecast_status = ForecastStatus::PendingAssessment;
        return Ok(());
    }
    let outcome = match &forecast.resolver {
        GoalForecastResolver::HumanAcceptance {} => {
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
        // Every goal carries the standing trust-economics clause
        // (see `GoalTrustClause`). If the declaration doesn't provide
        // one, apply the default — trust is a feasibility constraint on
        // the goal, not a preference weight the caller can omit.
        let now = self.goal_now();
        let trust_clause = declaration
            .trust_clause
            .clone()
            .unwrap_or_else(|| GoalTrustClause::default_clause(now));
        let spec_digest = digest(&(
            &declaration.goal_text,
            &declaration.criteria,
            &declaration.source,
            declaration.supersedes,
            &trust_clause,
        ))?;
        let mut goal = Goal {
            schema_version: 3,
            id: GoalID::new(),
            goal_text: declaration.goal_text,
            criterion_ids: (1..=declaration.criteria.len())
                .map(|n| format!("criterion-{n}"))
                .collect(),
            criteria: declaration.criteria,
            task_id: declaration.task_id,
            source: declaration.source,
            supersedes: declaration.supersedes,
            trust_clause: Some(trust_clause),
            trust_events: vec![],
            spec_digest,
            verdicts: vec![],
            closure: None,
            human_corrections: vec![],
            assessments: vec![],
            forecast_status: if declaration.forecast.is_some() {
                ForecastStatus::PendingAssessment
            } else {
                ForecastStatus::NotDeclared
            },
            forecast: declaration.forecast,
            scores: vec![],
            revision: 0,
            indexed_revision: 0,
            assessment_claim: None,
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

    pub fn goal_get(&self, id: GoalID, owner: WebID) -> Result<Option<Goal>, KanbanError> {
        let rows = self
            .store
            .query_by_entity_attribute(GOAL_ENTITY, &id.to_string())?;
        if rows.len() > 1 {
            return Err(invalid("duplicate canonical goal key"));
        }
        rows.into_iter()
            .next()
            .map(|row| {
                if row.access.owner_webid != owner {
                    return Err(KanbanError::PermissionDenied(
                        "goal is not owned by caller".into(),
                    ));
                }
                let goal = decode_goal(row.value)?;
                require_owner(&goal, owner)?;
                if goal.id != id {
                    return Err(invalid("goal key or owner mismatch"));
                }
                Ok(goal)
            })
            .transpose()
    }
    pub fn goal_list(&self, owner: &WebID) -> Result<Vec<Goal>, KanbanError> {
        owned_goals(*owner, self.store.query_by_entity(GOAL_ENTITY)?)
    }

    /// Trusted native-host scheduling scan, not an MCP route. Validate each
    /// owner's records through the same collector as caller-scoped listing.
    pub fn goal_scan_for_host(&self) -> Result<HashMap<WebID, Vec<Goal>>, KanbanError> {
        let mut rows_by_owner: HashMap<WebID, Vec<HMem>> = HashMap::new();
        for row in self.store.query_by_entity(GOAL_ENTITY)? {
            rows_by_owner
                .entry(row.access.owner_webid)
                .or_default()
                .push(row);
        }
        rows_by_owner
            .into_iter()
            .map(|(owner, rows)| owned_goals(owner, rows).map(|goals| (owner, goals)))
            .collect()
    }
    fn transition_goal(
        &self,
        id: GoalID,
        owner: WebID,
        project: bool,
        change: impl FnOnce(&mut Goal) -> Result<bool, KanbanError>,
    ) -> Result<Goal, KanbanError> {
        self.store
            .update_value_atomic(GOAL_ENTITY, &id.to_string(), |value| {
                let mut goal = decode_goal(value)?;
                if goal.id != id {
                    return Err(invalid("goal identity changed during update"));
                }
                require_owner(&goal, owner)?;
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

    /// Record a trust event on a goal's running trust ledger.
    ///
    /// The event captures one instruction-compliance outcome — the
    /// instruction text as extracted at intake, whether it was complied
    /// with or violated, the severity class when violated, and the
    /// evidence for the claim. The ledger is append-only: recording a
    /// correction appends a new event, it never rewrites the history.
    /// The regulation loop reads the violation rates from the ledger
    /// via [`Goal::trust_summary`].
    pub fn goal_trust_record(
        &self,
        id: GoalID,
        event: GoalTrustEvent,
        owner: WebID,
    ) -> Result<Goal, KanbanError> {
        self.transition_goal(id, owner, true, |goal| {
            if goal.closure.is_some() {
                return Err(invalid(
                    "work is closed; declare a successor for changed requirements",
                ));
            }
            nonempty(&event.instruction, "trust event instruction")?;
            nonempty(&event.evidence, "trust event evidence")?;
            if event.status == GoalTrustStatus::Violated && event.severity.is_none() {
                return Err(invalid("violated trust event requires a severity class"));
            }
            if event.status == GoalTrustStatus::Complied && event.severity.is_some() {
                return Err(invalid(
                    "complied trust event cannot carry a severity class",
                ));
            }
            if event.recorded_at > self.goal_now() {
                return Err(invalid("trust event recorded_at cannot be in the future"));
            }
            goal.trust_events.push(event);
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
            if let Some(existing) = &goal.closure {
                return if existing.input == input {
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
                recorded_at: self.goal_now(),
            });
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
    /// Own the complete assessment transaction sequence. Callers supply only
    /// evidence capture and independent inference, never lease/completion messages.
    ///
    /// The capture/assess closure results carry String errors by the D8 seam
    /// contract: both closures are implemented in kask_bridge, where String
    /// errors are accepted (check-string-errors gate scope note), and the
    /// attempt record persists the message as its audit trail
    /// (`AssessmentAttempt::result`).
    pub async fn goal_assess<C, A, F>(
        &self,
        id: GoalID,
        owner: WebID,
        capture: C,
        assess: A,
    ) -> Result<(), KanbanError>
    where
        C: FnOnce(&NativeGoalClosure) -> Result<Vec<GoalArtifactSnapshot>, String>, // string-error-ok
        A: FnOnce(GoalAssessmentInput) -> F,
        F: std::future::Future<Output = Result<AssessmentResult, String>>, // string-error-ok
    {
        let current = self
            .goal_get(id, owner)?
            .ok_or_else(|| Self::goal_not_found(id))?;
        if !current.needs_assessment(self.goal_now()) {
            return Ok(());
        }
        let closure = &current
            .closure
            .as_ref()
            .ok_or_else(|| invalid("assessment requires closure"))?
            .input;
        let captured = capture(closure);
        let artifacts = captured.as_ref().cloned().unwrap_or_default();
        let token = hkask_types::id::GoalID::new().to_string();
        let mut claimed = false;
        let goal = self.transition_goal(id, owner, true, |goal| {
            if !goal.needs_assessment(self.goal_now()) {
                return Ok(false);
            }
            if let Some(attempt) = goal.assessments.last_mut().filter(|a| a.result.is_none()) {
                attempt.result = Some(Err("assessment claim expired".into()));
            }
            validate_artifacts(
                &goal
                    .closure
                    .as_ref()
                    .ok_or_else(|| invalid("closure absent"))?
                    .input,
                &artifacts,
                self.goal_now(),
            )?;
            goal.assessments.push(AssessmentAttempt {
                input: GoalAssessmentInput {
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
                    evidence_manifest: closure.evidence_manifest.clone(),
                    artifacts,
                },
                started_at: self.goal_now(),
                result: None,
            });
            goal.assessment_claim = Some(AssessmentClaim {
                token: token.clone(),
                expires_at: self.goal_now() + chrono::Duration::seconds(90),
            });
            claimed = true;
            Ok(true)
        })?;
        if !claimed {
            return Ok(());
        }
        let input = goal
            .assessments
            .last()
            .ok_or_else(|| invalid("assessment input absent"))?
            .input
            .clone();
        let result = match captured {
            Err(error) => Err(error),
            Ok(_) if !input.artifacts.iter().any(|a| matches!(a.outcome, GoalArtifactReadOutcome::Fetched { .. })) => Ok(AssessmentResult {
                findings: input.criteria.iter().map(|c| CriterionFinding { criterion_id: c.id.clone(), status: CriterionEvidenceStatus::Unknown, evidence_ids: vec![], note: "No independently fetched artifact establishes or contradicts this criterion".into() }).collect(),
                limitations: vec!["No approved digest-matched UTF-8 contents; no model request was made".into()],
            }),
            Ok(_) => assess(input).await,
        };
        self.transition_goal(id, owner, true, |goal| {
            if !goal
                .assessment_claim
                .as_ref()
                .is_some_and(|c| c.token == token && c.expires_at > self.goal_now())
            {
                return Err(invalid("stale or expired assessment result"));
            }
            let result = result.and_then(|reply| {
                validate_assessment(goal, &reply).map_err(|e| e.to_string())?;
                Ok(reply)
            });
            goal.assessments
                .last_mut()
                .ok_or_else(|| invalid("assessment absent"))?
                .result = Some(result);
            goal.assessment_claim = None;
            resolve_forecast(goal, self.goal_now())?;
            Ok(true)
        })?;
        Ok(())
    }

    /// Prepare outside the canonical lock; publish and acknowledge only while
    /// the exact revision is still current. A destination commit followed by a
    /// canonical commit failure is retried by revising the same memory record.
    ///
    /// The prepare/publish closure results carry String errors by the D8 seam
    /// contract: both closures are implemented in kask_bridge, where String
    /// errors are accepted (check-string-errors gate scope note).
    pub async fn goal_index_current<P, F, W>(
        &self,
        id: GoalID,
        owner: WebID,
        prepare: P,
    ) -> Result<(), KanbanError>
    where
        P: FnOnce(Goal) -> F,
        F: std::future::Future<Output = Result<W, String>>, // string-error-ok
        W: FnOnce() -> Result<(), String>,                  // string-error-ok
    {
        self.transition_goal(id, owner, true, |goal| {
            let old = goal.forecast_status.clone();
            resolve_forecast(goal, self.goal_now())?;
            Ok(old != goal.forecast_status)
        })?;
        let snapshot = self
            .goal_get(id, owner)?
            .ok_or_else(|| Self::goal_not_found(id))?;
        if snapshot.indexed_revision == snapshot.revision {
            return Ok(());
        }
        let revision = snapshot.revision;
        let write = prepare(snapshot).await.map_err(KanbanError::Internal)?;
        self.transition_goal(id, owner, false, |goal| {
            if goal.revision != revision {
                return Err(invalid(
                    "goal changed while preparing recall; retry current revision",
                ));
            }
            if goal.indexed_revision == revision {
                return Ok(false);
            }
            write().map_err(KanbanError::Internal)?;
            goal.indexed_revision = revision;
            Ok(true)
        })?;
        Ok(())
    }
}
fn validate_assessment(goal: &Goal, reply: &AssessmentResult) -> Result<(), KanbanError> {
    let ids: HashSet<_> = reply
        .findings
        .iter()
        .map(|f| f.criterion_id.as_str())
        .collect();
    if ids.len() != goal.criterion_ids.len()
        || ids.len() != reply.findings.len()
        || goal
            .criterion_ids
            .iter()
            .any(|id| !ids.contains(id.as_str()))
    {
        return Err(invalid(
            "assessment must cover every original criterion exactly once",
        ));
    }
    if reply.limitations.is_empty() {
        return Err(invalid("assessment requires evidence-access limitations"));
    }
    let attempt = goal
        .assessments
        .last()
        .ok_or_else(|| invalid("assessment absent"))?;
    let known = evidence_ids(goal);
    let fetched: HashSet<_> = attempt
        .input
        .artifacts
        .iter()
        .filter_map(|a| match a.outcome {
            GoalArtifactReadOutcome::Fetched { .. } => Some(a.evidence_id.as_str()),
            _ => None,
        })
        .collect();
    for finding in &reply.findings {
        nonempty(&finding.note, "finding note")?;
        validate_refs(&finding.evidence_ids, &known)?;
        if finding.status != CriterionEvidenceStatus::Unknown
            && (finding.evidence_ids.is_empty()
                || finding
                    .evidence_ids
                    .iter()
                    .any(|id| !fetched.contains(id.as_str())))
        {
            return Err(invalid(
                "supported/contradicted finding requires actually fetched digest-matched artifact IDs",
            ));
        }
    }
    Ok(())
}
