//! Local test-evidence validation; never approval or activation authority.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Identity {
    run_id: String,
    artifact_sha256: String,
    evaluator_sha256: String,
    contract_sha256: String,
    author: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Expected {
    identity: Identity,
    requested_at: DateTime<Utc>,
    max_age_seconds: u32,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Report {
    identity: Identity,
    started_at: DateTime<Utc>,
    finished_at: DateTime<Utc>,
    exit_code: i32,
    inputs_unchanged: bool,
    log_sha256: String,
}

#[derive(Debug, thiserror::Error)]
enum EvidenceError {
    #[error("missing or unreadable evidence: {0}")]
    Io(#[from] std::io::Error),
    #[error("malformed evidence: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid expected identity or age budget")]
    InvalidExpectation,
    #[error("artifact, evaluator, contract, run, or author mismatch")]
    IdentityMismatch,
    #[error("inputs changed during evaluation")]
    ChangedInputs,
    #[error("stale, future, or inconsistent measurement timestamps")]
    InvalidTime,
    #[error("raw test log does not match its digest")]
    LogMismatch,
    #[error("incomplete or unsupported test execution (including timeout/build failure)")]
    Incomplete,
    #[error("missing, malformed, or multiple library-test summaries")]
    InvalidSummary,
    #[error("no executed tests, filtered tests, or inconsistent exit status")]
    InvalidOutcome,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Disposition {
    EvaluationPassed,
    CorrectiveWorkRequired,
}

#[derive(Debug, Serialize)]
struct Receipt<'a> {
    identity: &'a Identity,
    measured_at: DateTime<Utc>,
    checked_at: DateTime<Utc>,
    disposition: Disposition,
    passed: u64,
    failed: u64,
    ignored: u64,
    authority: &'static str,
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn is_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

/// expect: "Only fresh results for my chosen artifact and contract inform corrective work." [P1]
fn assess<'a>(
    expected: &'a Expected,
    report: &Report,
    log: &[u8],
    now: DateTime<Utc>,
) -> Result<Receipt<'a>, EvidenceError> {
    let identity = &expected.identity;
    if identity.run_id.trim().is_empty()
        || identity.author.trim().is_empty()
        || ![
            &identity.artifact_sha256,
            &identity.evaluator_sha256,
            &identity.contract_sha256,
        ]
        .into_iter()
        .all(|value| is_digest(value))
        || expected.max_age_seconds == 0
    {
        return Err(EvidenceError::InvalidExpectation);
    }
    if identity != &report.identity {
        return Err(EvidenceError::IdentityMismatch);
    }
    if !report.inputs_unchanged {
        return Err(EvidenceError::ChangedInputs);
    }
    if report.started_at < expected.requested_at
        || report.finished_at < report.started_at
        || report.finished_at > now
        || now.signed_duration_since(report.finished_at)
            > chrono::Duration::seconds(expected.max_age_seconds.into())
    {
        return Err(EvidenceError::InvalidTime);
    }
    if digest(log) != report.log_sha256 {
        return Err(EvidenceError::LogMismatch);
    }
    if !matches!(report.exit_code, 0 | 101) {
        return Err(EvidenceError::Incomplete);
    }
    let text = std::str::from_utf8(log).map_err(|_| EvidenceError::InvalidSummary)?;
    let mut summaries = text
        .lines()
        .filter_map(|line| line.strip_prefix("test result: "));
    let summary = summaries.next().ok_or(EvidenceError::InvalidSummary)?;
    if summaries.next().is_some() {
        return Err(EvidenceError::InvalidSummary);
    }
    let (status, fields) = summary
        .split_once(". ")
        .ok_or(EvidenceError::InvalidSummary)?;
    let fields: Vec<_> = fields.split("; ").collect();
    let [passed, failed, ignored, measured, filtered, duration] = fields.as_slice() else {
        return Err(EvidenceError::InvalidSummary);
    };
    let count = |field: &str, suffix: &str| -> Result<u64, EvidenceError> {
        field
            .strip_suffix(suffix)
            .and_then(|number| number.parse().ok())
            .ok_or(EvidenceError::InvalidSummary)
    };
    let passed = count(passed, " passed")?;
    let failed = count(failed, " failed")?;
    let ignored = count(ignored, " ignored")?;
    let duration = duration
        .strip_prefix("finished in ")
        .and_then(|value| value.strip_suffix('s'))
        .and_then(|value| value.parse::<f64>().ok());
    if duration.is_none_or(|seconds| !seconds.is_finite() || seconds < 0.0)
        || count(measured, " measured")? != 0
        || count(filtered, " filtered out")? != 0
        || passed.checked_add(failed).is_none_or(|total| total == 0)
        || passed
            .checked_add(failed)
            .and_then(|total| total.checked_add(ignored))
            .is_none()
        || !matches!(
            (status, report.exit_code, failed),
            ("ok", 0, 0) | ("FAILED", 101, 1..)
        )
    {
        return Err(EvidenceError::InvalidOutcome);
    }
    Ok(Receipt {
        identity,
        measured_at: report.finished_at,
        checked_at: now,
        disposition: if failed == 0 {
            Disposition::EvaluationPassed
        } else {
            Disposition::CorrectiveWorkRequired
        },
        passed,
        failed,
        ignored,
        authority: "local_operator_report_not_approval",
    })
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, EvidenceError> {
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}

fn run(paths: &[String]) -> Result<i32, EvidenceError> {
    let expected: Expected = read_json(Path::new(&paths[0]))?;
    let report: Report = read_json(Path::new(&paths[1]))?;
    let log = std::fs::read(&paths[2])?;
    let receipt = assess(&expected, &report, &log, Utc::now())?;
    println!("{}", serde_json::to_string(&receipt)?);
    Ok(i32::from(
        receipt.disposition == Disposition::CorrectiveWorkRequired,
    ))
}

fn main() {
    let paths: Vec<_> = std::env::args().skip(1).collect();
    if paths.len() != 3 {
        eprintln!("usage: check_test_evidence EXPECTED.json REPORT.json RAW.log");
        std::process::exit(2);
    }
    match run(&paths) {
        Ok(status) => std::process::exit(status),
        Err(error) => {
            println!(
                "{}",
                serde_json::json!({"disposition": "evidence_rejected", "reason": error.to_string()})
            );
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (Expected, Report, Vec<u8>, DateTime<Utc>) {
        let now = DateTime::from_timestamp(1_800_000_000, 0).expect("timestamp");
        let identity = Identity {
            run_id: "run-one".into(),
            artifact_sha256: digest(b"candidate"),
            evaluator_sha256: digest(b"evaluator"),
            contract_sha256: digest(b"contract"),
            author: "operator".into(),
        };
        let log = b"test result: ok. 2 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.01s\n".to_vec();
        let expected = Expected {
            identity: identity.clone(),
            requested_at: now - chrono::Duration::seconds(10),
            max_age_seconds: 60,
        };
        let report = Report {
            identity,
            started_at: expected.requested_at,
            finished_at: now - chrono::Duration::seconds(2),
            exit_code: 0,
            inputs_unchanged: true,
            log_sha256: digest(&log),
        };
        (expected, report, log, now)
    }

    #[test]
    fn rereading_preserves_measurement_time_and_expires() {
        let (expected, report, log, now) = fixture();
        let receipt = assess(&expected, &report, &log, now).expect("valid evidence");
        assert_eq!(receipt.measured_at, report.finished_at);
        assert_eq!(receipt.ignored, 1);
        assert!(matches!(
            assess(
                &expected,
                &report,
                &log,
                now + chrono::Duration::seconds(61)
            ),
            Err(EvidenceError::InvalidTime)
        ));
    }

    #[test]
    fn failed_tests_require_correction_not_evidence_rejection() {
        let (expected, mut report, _, now) = fixture();
        let log = b"test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n";
        report.exit_code = 101;
        report.log_sha256 = digest(log);
        assert_eq!(
            assess(&expected, &report, log, now)
                .expect("valid failure")
                .disposition,
            Disposition::CorrectiveWorkRequired
        );
    }

    #[test]
    fn rejects_each_identity_mismatch() {
        for field in [
            "run_id",
            "artifact_sha256",
            "evaluator_sha256",
            "contract_sha256",
            "author",
        ] {
            let (expected, report, log, now) = fixture();
            let mut value = serde_json::to_value(report).expect("report");
            value["identity"][field] = "different".into();
            let report = serde_json::from_value(value).expect("report");
            assert!(
                matches!(
                    assess(&expected, &report, &log, now),
                    Err(EvidenceError::IdentityMismatch)
                ),
                "{field}"
            );
        }
    }

    #[test]
    fn rejects_changed_inputs_future_measurements_and_altered_logs() {
        let (expected, mut report, log, now) = fixture();
        report.inputs_unchanged = false;
        assert!(matches!(
            assess(&expected, &report, &log, now),
            Err(EvidenceError::ChangedInputs)
        ));
        report.inputs_unchanged = true;
        assert!(matches!(
            assess(&expected, &report, b"altered", now),
            Err(EvidenceError::LogMismatch)
        ));
        report.finished_at = now + chrono::Duration::seconds(1);
        assert!(matches!(
            assess(&expected, &report, &log, now),
            Err(EvidenceError::InvalidTime)
        ));
    }

    #[test]
    fn rejects_empty_filtered_overflow_partial_and_timeout_results() {
        let (expected, mut report, _, now) = fixture();
        for log in [
            "",
            "test result: ok. 0 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0s",
            "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out; finished in 0s",
            "test result: ok. 18446744073709551616 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0s",
        ] {
            report.log_sha256 = digest(log.as_bytes());
            assert!(assess(&expected, &report, log.as_bytes(), now).is_err());
        }
        let (_, mut report, log, _) = fixture();
        report.exit_code = 124;
        assert!(matches!(
            assess(&expected, &report, &log, now),
            Err(EvidenceError::Incomplete)
        ));
    }

    #[test]
    fn rejects_missing_fields_and_malformed_reports() {
        for text in ["{}", "{", "null"] {
            assert!(serde_json::from_str::<Report>(text).is_err());
        }
    }

    #[test]
    fn rejects_impossible_totals_and_invalid_durations() {
        let (expected, mut report, _, now) = fixture();
        for (ignored, duration) in [
            (u64::MAX, "0.01s"),
            (0, "nonsense"),
            (0, "NaNs"),
            (0, "-1s"),
        ] {
            let log = format!(
                "test result: ok. 1 passed; 0 failed; {ignored} ignored; 0 measured; 0 filtered out; finished in {duration}\n"
            );
            report.log_sha256 = digest(log.as_bytes());
            assert!(matches!(
                assess(&expected, &report, log.as_bytes(), now),
                Err(EvidenceError::InvalidOutcome)
            ));
        }
    }
}
