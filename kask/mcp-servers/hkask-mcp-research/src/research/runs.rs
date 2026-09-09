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
        Some(score_evidence_set(&artifacts, &DEFAULT_PROFILE))
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

    Ok(Some(serde_json::json!({
        "run_id": run_id,
        "question": question,
        "status": status,
        "began_at": began_at,
        "updated_at": updated_at,
        "sources": sources,
    })))
}
