//! Domain filtering, run-ledger append, extraction validation, and evidence helpers.

use hkask_mcp_server::server::McpToolError;

use crate::ResearchServer;
use crate::research::{ArtifactScore, EvidenceReport, RunSourceRecord, SensitivityStatus};
use crate::spawn_db;

/// Enforce the caller's domain boundary even when a provider ignores its
/// domain parameters. A host suffix matches only at a dot boundary, never
/// inside a longer hostname or a URL's userinfo/path.
pub(crate) fn search_url_in_domains(url: &str, include: &[String], exclude: &[String]) -> bool {
    if include.is_empty() && exclude.is_empty() {
        return true;
    }
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    let Some(host) = parsed.host_str() else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    let matches_domain = |raw: &String| {
        let domain = raw.trim().trim_start_matches('.').to_ascii_lowercase();
        !domain.is_empty()
            && (host == domain
                || host
                    .strip_suffix(&domain)
                    .is_some_and(|prefix| prefix.ends_with('.')))
    };
    (include.is_empty() || include.iter().any(matches_domain))
        && !exclude.iter().any(matches_domain)
}

/// Map one search or find-similar result to its run-ledger record — the
/// shared field shape of the two ledger appends.
pub(crate) fn run_source_record(
    url: &str,
    title: &str,
    published: Option<&str>,
    source: Option<&str>,
    content_preview: Option<&str>,
    description: Option<&str>,
    provider: Option<String>,
) -> RunSourceRecord {
    RunSourceRecord {
        url: url.to_string(),
        provider,
        title: Some(title.to_string()),
        published: published.map(str::to_string),
        source: source.map(str::to_string),
        excerpt: content_preview.or(description).map(str::to_string),
    }
}

/// Never present binary or a downgraded provider response as a successful extraction.
pub(crate) fn validate_extraction(
    requested_format: &str,
    actual_format: Option<&str>,
    content: Option<&str>,
) -> Result<(), McpToolError> {
    let content = content.ok_or_else(|| McpToolError::internal("Extraction has no content"))?;
    if content.starts_with("%PDF-") {
        return Err(McpToolError::internal(
            "Provider returned raw PDF bytes; use a PDF extraction provider",
        ));
    }
    if actual_format != Some(requested_format) {
        return Err(McpToolError::internal(format!(
            "Provider returned {:?} instead of requested {requested_format} extraction",
            actual_format
        )));
    }
    if requested_format == "json" && serde_json::from_str::<serde_json::Value>(content).is_err() {
        return Err(McpToolError::internal(
            "Provider returned invalid JSON for structured extraction",
        ));
    }
    Ok(())
}

/// The SEPIO:0000440 (has-supporting-evidence) note in the duplicate-aware
/// model's terms: independent units corroborate, or the artifacts are one
/// duplicated unit — never a domain count that syndication inflates.
pub(crate) fn supporting_evidence_note(
    score: &ArtifactScore,
    scored: &EvidenceReport,
) -> serde_json::Value {
    if score.corroboration_count > 1 {
        serde_json::Value::String(format!(
            "{} independent evidence units corroborate the evidence base",
            score.corroboration_count
        ))
    } else if scored.sourced_count > 1 {
        serde_json::Value::String(format!(
            "{} artifacts form one evidence unit — duplication, not independent corroboration",
            scored.sourced_count
        ))
    } else {
        serde_json::Value::Null
    }
}

/// The wire form of the sensitivity report.
pub(crate) fn sensitivity_json(sensitivity: &SensitivityStatus) -> serde_json::Value {
    match sensitivity {
        SensitivityStatus::Stable { profiles_evaluated } => serde_json::json!({
            "status": "stable",
            "profiles_evaluated": profiles_evaluated,
        }),
        SensitivityStatus::Unstable { driver } => serde_json::json!({
            "status": "unstable",
            "driver": driver.name(),
        }),
        SensitivityStatus::NotEvaluable { reason } => serde_json::json!({
            "status": "not_evaluable",
            "reason": reason,
        }),
    }
}

impl ResearchServer {
    /// Append server-observed records to a run's ledger (the non-repudiation path).
    pub(crate) async fn append_run_ledger(
        &self,
        run_id: &str,
        records: Vec<RunSourceRecord>,
    ) -> serde_json::Value {
        let Some(database) = self.research_db.clone() else {
            return serde_json::json!({
                "recorded": 0,
                "error": "research database not available — set HKASK_DB_PASSPHRASE \
                          to record runs (HKASK_RESEARCH_DB only for a custom location)",
            });
        };
        let run_id_for_task = run_id.to_string();
        let submitted = records.len();
        let append = spawn_db(database, move |connection| {
            crate::research::runs::append_run_sources(connection, &run_id_for_task, &records)
        })
        .await;
        match append {
            Ok(Ok(inserted)) => serde_json::json!({
                "recorded": inserted,
                "already_recorded": submitted.saturating_sub(inserted),
            }),
            Ok(Err(error)) => {
                tracing::warn!(
                    target: "hkask.research.runs",
                    run_id,
                    error = %error,
                    "run ledger append failed"
                );
                serde_json::json!({ "recorded": 0, "error": error.to_string() })
            }
            Err(error) => {
                tracing::warn!(
                    target: "hkask.research.runs",
                    run_id,
                    error = %error,
                    "run ledger append task failed"
                );
                serde_json::json!({ "recorded": 0, "error": error.to_string() })
            }
        }
    }
}

#[cfg(test)]
mod domain_filter_tests {
    use super::search_url_in_domains;

    #[test]
    fn parsed_host_boundary_rejects_lookalikes_and_userinfo() {
        let official = vec!["amf-france.org".to_string()];
        assert!(search_url_in_domains(
            "https://bdif.amf-france.org/back/api/v1/documents/notice.pdf",
            &official,
            &[],
        ));
        for url in [
            "https://notamf-france.org/notice",
            "https://amf-france.org.evil.test/notice",
            "https://amf-france.org@evil.test/notice",
            "not a url",
        ] {
            assert!(!search_url_in_domains(url, &official, &[]), "{url}");
        }
    }
}
