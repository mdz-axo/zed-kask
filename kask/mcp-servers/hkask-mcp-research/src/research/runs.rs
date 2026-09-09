//! Research-run ledger — the server's non-repudiable record of what its
//! tools actually returned under a run, and (Commit 3) the agent's declared
//! verification states validated against that record.
//!
//! Feynman's `ResearchRun` manifest is agent-authored and can claim
//! anything; this design cannot be lied to about the server's own output:
//! rows with `recorded_by='server'` are written only by the server's tools,
//! from the results those tools actually returned (C2, the zed-kask
//! strengthening). The ledger shares the feed substrate's DB — one DB, one
//! passphrase, one pool (essentialist G3).

use rusqlite::Connection;
use rusqlite::OptionalExtension;

use crate::research::evidence::{DEFAULT_PROFILE, score_evidence_set};
use crate::research::types::EvaluateArtifact;
use hkask_types::time::now_rfc3339;

/// Cap for the `excerpt` audit copy: enough to identify the content (and to
/// shingle-cluster it in the manifest's confidence recompute), not a
/// content store — the `corpus_ref` seam carries the durable copy.
pub(crate) const RUN_SOURCE_EXCERPT_MAX_CHARS: usize = 512;

/// A begun run, as returned to the tool layer.
pub(crate) struct NewResearchRun {
    pub run_id: String,
}

/// Begin a run: `run_id` is `blake3(question || began_at)[..16]`, so the
/// identifier is deterministic in its inputs and collision-safe at operator
/// scale (16 hex chars).
pub(crate) fn begin_research_run(
    connection: &Connection,
    question: &str,
) -> Result<NewResearchRun, anyhow::Error> {
    let began_at = now_rfc3339();
    let mut hasher = blake3::Hasher::new();
    hasher.update(question.as_bytes());
    hasher.update(began_at.as_bytes());
    let run_id = hasher.finalize().to_hex().to_string()[..16].to_string();
    connection.execute(
        "INSERT INTO research_runs (run_id, question, status, began_at, updated_at) \
         VALUES (?1, ?2, 'planned', ?3, ?3)",
        rusqlite::params![run_id, question, began_at],
    )?;
    Ok(NewResearchRun { run_id })
}

/// One server-observed source to append to a run's ledger. Owned strings:
/// the append crosses into `spawn_db` and must be `Send + 'static`.
pub(crate) struct RunSourceRecord {
    pub url: String,
    pub provider: Option<String>,
    pub title: Option<String>,
    pub published: Option<String>,
    pub source: Option<String>,
    pub excerpt: Option<String>,
}

/// Append server-observed sources (`recorded_by='server'`). First
/// observation wins — `INSERT OR IGNORE` — so a re-serve in the same run
/// never clobbers the audit copy or an agent annotation (Commit 3's upsert
/// owns those fields). Returns the number of rows actually inserted.
/// A nonexistent `run_id` fails the foreign key and surfaces to the caller.
pub(crate) fn append_run_sources(
    connection: &Connection,
    run_id: &str,
    sources: &[RunSourceRecord],
) -> Result<usize, anyhow::Error> {
    let recorded_at = now_rfc3339();
    let mut inserted = 0usize;
    for record in sources {
        let excerpt = record.excerpt.as_deref().map(|text| {
            if text.chars().count() > RUN_SOURCE_EXCERPT_MAX_CHARS {
                text.chars()
                    .take(RUN_SOURCE_EXCERPT_MAX_CHARS)
                    .collect::<String>()
            } else {
                text.to_string()
            }
        });
        inserted += connection.execute(
            "INSERT OR IGNORE INTO run_sources \
             (run_id, url, provider, title, published, source, excerpt, \
              corpus_ref, recorded_by, verification_state, verification_basis, recorded_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, 'server', NULL, NULL, ?8)",
            rusqlite::params![
                run_id,
                record.url,
                record.provider,
                record.title,
                record.published,
                record.source,
                excerpt,
                recorded_at
            ],
        )?;
    }
    Ok(inserted)
}

/// The run manifest: question, status, timestamps, every source row (the
/// audit trail), and per-source confidence recomputed server-side from the
/// ledger's own excerpt copies (Commit 1's scorer — deterministic, G3
/// contract). `None` when the run does not exist.
pub(crate) fn get_research_run(
    connection: &Connection,
    run_id: &str,
) -> Result<Option<serde_json::Value>, anyhow::Error> {
    let Some((question, status, began_at, updated_at)) = connection
        .query_row(
            "SELECT question, status, began_at, updated_at FROM research_runs \
             WHERE run_id = ?1",
            [run_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?
    else {
        return Ok(None);
    };

    let mut statement = connection.prepare(
        "SELECT url, provider, title, published, source, excerpt, corpus_ref, \
         recorded_by, verification_state, verification_basis, recorded_at \
         FROM run_sources WHERE run_id = ?1 ORDER BY recorded_at, url",
    )?;
    let rows: Vec<(
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        String,
        Option<String>,
        Option<String>,
        String,
    )> = statement
        .query_map([run_id], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
                row.get(9)?,
                row.get(10)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    // Server-side confidence recompute from the ledger's own copies — the
    // manifest's scores are the server's, not the agent's claim.
    let artifacts: Vec<EvaluateArtifact> = rows
        .iter()
        .map(|row| EvaluateArtifact {
            url: row.0.clone(),
            title: row.2.clone(),
            published: row.3.clone(),
            source: row.4.clone(),
            content: row.5.clone(),
        })
        .collect();
    let scored = if artifacts.is_empty() {
        None
    } else {
        Some(score_evidence_set(&artifacts, &DEFAULT_PROFILE, None))
    };

    let sources: Vec<serde_json::Value> = rows
        .iter()
        .zip(
            scored
                .as_ref()
                .map_or(&Vec::new(), |report| &report.artifacts)
                .iter(),
        )
        .map(|(row, score)| {
            serde_json::json!({
                "url": row.0,
                "provider": row.1,
                "title": row.2,
                "published": row.3,
                "source": row.4,
                "excerpt": row.5,
                "corpus_ref": row.6,
                "recorded_by": row.7,
                "verification_state": row.8,
                "verification_basis": row.9,
                "recorded_at": row.10,
                "confidence": score.confidence,
            })
        })
        .collect();

    // The validation block: the pure validator's verdict over the
    // manifest's own data, surfaced — never silent.
    let record = ResearchRunRecord {
        question: question.clone(),
        status: status.clone(),
        sources: rows
            .iter()
            .map(|row| RunSourceState {
                url: row.0.clone(),
                recorded_by: row.7.clone(),
                verification_state: row.8.clone(),
                verification_basis: row.9.clone(),
            })
            .collect(),
    };
    let validation = match validate_research_run(&record) {
        Ok(()) => serde_json::json!({ "valid": true }),
        Err(violations) => serde_json::json!({
            "valid": false,
            "violations": violations,
        }),
    };

    Ok(Some(serde_json::json!({
        "run_id": run_id,
        "question": question,
        "status": status,
        "began_at": began_at,
        "updated_at": updated_at,
        "sources": sources,
        "validation": validation,
    })))
}

/// The verification states an agent may declare (Feynman's
/// `verificationStateValues`). `verified` is special: it requires a
/// server-recorded row and a basis (the fail-closed gate).
pub(crate) const VERIFICATION_STATES: &[&str] = &[
    "not_checked",
    "inferred",
    "partial",
    "verified",
    "blocked",
    "failed",
];

/// Run lifecycle statuses.
pub(crate) const RUN_STATUSES: &[&str] = &[
    "planned",
    "running",
    "completed",
    "partial",
    "blocked",
    "failed",
];

/// The manifest data the pure validator reasons over.
pub(crate) struct ResearchRunRecord {
    pub question: String,
    pub status: String,
    pub sources: Vec<RunSourceState>,
}

pub(crate) struct RunSourceState {
    pub url: String,
    pub recorded_by: String,
    pub verification_state: Option<String>,
    pub verification_basis: Option<String>,
}

/// Feynman's `validateResearchRun`, adapted: every rule names its violated
/// invariant in the returned message. The `verified` gate is the zed-kask
/// strengthening — the server refuses verification claims about sources it
/// never served under the run (Decision 4, strict / fail-closed).
pub(crate) fn validate_research_run(record: &ResearchRunRecord) -> Result<(), Vec<String>> {
    let mut violations: Vec<String> = Vec::new();
    if record.question.trim().is_empty() {
        violations.push("question must not be empty".to_string());
    }
    if !RUN_STATUSES.contains(&record.status.as_str()) {
        violations.push(format!(
            "status '{}' is not one of {}",
            record.status,
            RUN_STATUSES.join("|")
        ));
    }
    let has_server_recorded = record
        .sources
        .iter()
        .any(|source| source.recorded_by == "server");
    if (record.status == "completed" || record.status == "partial") && !has_server_recorded {
        violations.push(format!(
            "status '{}' requires at least one server-recorded source",
            record.status
        ));
    }
    for source in &record.sources {
        let Some(state) = source.verification_state.as_deref() else {
            continue;
        };
        if !VERIFICATION_STATES.contains(&state) {
            violations.push(format!(
                "verification_state '{}' for {} is not one of {}",
                state,
                source.url,
                VERIFICATION_STATES.join("|")
            ));
            continue;
        }
        if state == "verified" {
            if source.recorded_by != "server" {
                violations.push(format!(
                    "verified annotation for {} requires a server-recorded \
                     source — the server never served it under this run",
                    source.url
                ));
            }
            if source
                .verification_basis
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty()
            {
                violations.push(format!(
                    "verified annotation for {} requires a basis",
                    source.url
                ));
            }
        }
    }
    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations)
    }
}

/// Typed failures for annotation — the tool maps each to its MCP error
/// kind (not_found / invalid_argument / db classification).
pub(crate) enum AnnotateError {
    RunNotFound,
    Invalid(String),
    Db(anyhow::Error),
}

/// Annotate a run's source with an agent-declared verification state.
/// Upsert-idempotent on the PRIMARY KEY (run_id, url): an existing row
/// (server-recorded or agent-declared) has its verification fields
/// updated; a URL the ledger has never seen is inserted as an
/// agent-declared row. `verified` is accepted only for an existing
/// server-recorded row and only with a basis — otherwise `Invalid`, the
/// fail-closed gate.
pub(crate) fn annotate_run_source(
    connection: &Connection,
    run_id: &str,
    url: &str,
    verification_state: &str,
    verification_basis: Option<&str>,
) -> Result<(), AnnotateError> {
    if !VERIFICATION_STATES.contains(&verification_state) {
        return Err(AnnotateError::Invalid(format!(
            "verification_state '{verification_state}' is not one of {}",
            VERIFICATION_STATES.join("|")
        )));
    }
    let run_exists: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM research_runs WHERE run_id = ?1)",
            [run_id],
            |row| row.get(0),
        )
        .map_err(|error| AnnotateError::Db(error.into()))?;
    if !run_exists {
        return Err(AnnotateError::RunNotFound);
    }
    if verification_state == "verified" {
        let server_recorded: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM run_sources \
                 WHERE run_id = ?1 AND url = ?2 AND recorded_by = 'server')",
                rusqlite::params![run_id, url],
                |row| row.get(0),
            )
            .map_err(|error| AnnotateError::Db(error.into()))?;
        if !server_recorded {
            return Err(AnnotateError::Invalid(format!(
                "verified requires a server-recorded source — \
                 the server never served {url} under this run"
            )));
        }
        if verification_basis.map(str::trim).unwrap_or("").is_empty() {
            return Err(AnnotateError::Invalid(format!(
                "verified annotation for {url} requires a basis"
            )));
        }
    }
    let recorded_at = now_rfc3339();
    let updated = connection
        .execute(
            "UPDATE run_sources SET verification_state = ?1, verification_basis = ?2, \
             recorded_at = ?3 WHERE run_id = ?4 AND url = ?5",
            rusqlite::params![
                verification_state,
                verification_basis,
                recorded_at,
                run_id,
                url
            ],
        )
        .map_err(|error| AnnotateError::Db(error.into()))?;
    if updated == 0 {
        connection
            .execute(
                "INSERT INTO run_sources \
                 (run_id, url, provider, title, published, source, excerpt, corpus_ref, \
                  recorded_by, verification_state, verification_basis, recorded_at) \
                 VALUES (?1, ?2, NULL, NULL, NULL, NULL, NULL, NULL, 'agent', ?3, ?4, ?5)",
                rusqlite::params![
                    run_id,
                    url,
                    verification_state,
                    verification_basis,
                    recorded_at
                ],
            )
            .map_err(|error| AnnotateError::Db(error.into()))?;
    }
    Ok(())
}

#[cfg(test)]
mod run_validation_tests {
    use super::*;

    fn source(
        url: &str,
        recorded_by: &str,
        verification_state: Option<&str>,
        verification_basis: Option<&str>,
    ) -> RunSourceState {
        RunSourceState {
            url: url.to_string(),
            recorded_by: recorded_by.to_string(),
            verification_state: verification_state.map(str::to_string),
            verification_basis: verification_basis.map(str::to_string),
        }
    }

    fn record(status: &str, sources: Vec<RunSourceState>) -> ResearchRunRecord {
        ResearchRunRecord {
            question: "is the claim corroborated?".to_string(),
            status: status.to_string(),
            sources,
        }
    }

    #[test]
    fn valid_manifest_passes() {
        let record = record(
            "running",
            vec![source("https://a.example/1", "server", None, None)],
        );
        assert_eq!(validate_research_run(&record), Ok(()));
    }

    #[test]
    fn empty_question_is_a_violation() {
        let mut record = record("planned", Vec::new());
        record.question = "   ".to_string();
        assert!(
            validate_research_run(&record)
                .err()
                .is_some_and(|violations| violations
                    .iter()
                    .any(|violation| violation.contains("question")))
        );
    }

    #[test]
    fn unknown_status_is_a_violation() {
        let record = record("finished", Vec::new());
        assert!(
            validate_research_run(&record)
                .err()
                .is_some_and(|violations| violations
                    .iter()
                    .any(|violation| violation.contains("status")))
        );
    }

    #[test]
    fn completed_without_server_sources_is_a_violation() {
        let record = record(
            "completed",
            vec![source(
                "https://agent.example/1",
                "agent",
                Some("inferred"),
                None,
            )],
        );
        assert!(
            validate_research_run(&record)
                .err()
                .is_some_and(|violations| {
                    violations
                        .iter()
                        .any(|violation| violation.contains("server-recorded"))
                })
        );
    }

    #[test]
    fn completed_with_server_source_passes() {
        let record = record(
            "completed",
            vec![source(
                "https://a.example/1",
                "server",
                Some("verified"),
                Some("checked against the primary source"),
            )],
        );
        assert_eq!(validate_research_run(&record), Ok(()));
    }

    #[test]
    fn verified_without_server_row_is_a_violation() {
        // The fail-closed gate: an agent may not verify a source the
        // server never served under this run.
        let record = record(
            "running",
            vec![source(
                "https://invented.example/1",
                "agent",
                Some("verified"),
                Some("trust me"),
            )],
        );
        assert!(
            validate_research_run(&record)
                .err()
                .is_some_and(|violations| {
                    violations
                        .iter()
                        .any(|violation| violation.contains("server-recorded"))
                })
        );
    }

    #[test]
    fn verified_without_basis_is_a_violation() {
        let record = record(
            "running",
            vec![source(
                "https://a.example/1",
                "server",
                Some("verified"),
                Some("   "),
            )],
        );
        assert!(
            validate_research_run(&record)
                .err()
                .is_some_and(|violations| violations
                    .iter()
                    .any(|violation| violation.contains("basis")))
        );
    }

    #[test]
    fn unknown_verification_state_is_a_violation() {
        let record = record(
            "running",
            vec![source(
                "https://a.example/1",
                "server",
                Some("double-checked"),
                None,
            )],
        );
        assert!(
            validate_research_run(&record)
                .err()
                .is_some_and(|violations| {
                    violations
                        .iter()
                        .any(|violation| violation.contains("verification_state"))
                })
        );
    }
}
