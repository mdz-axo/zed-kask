//! The wire form of [`LedgerEvent`] — the one encode/decode home every
//! reader of the trust ledger shares. The h_mem substrate glue
//! (`hkask_mcp_server::trust_ledger_hmem`) and the regulation loop's
//! `TrustSensor` both decode through this module, so the wire format
//! cannot fork between readers (the sensor's former inline decode — a
//! third copy that had already drifted on unparseable-`ts` rows — was
//! removed for this single home).
//!
//! The row's shape: the event id is the ROW KEY (the h_mem's attribute),
//! never embedded in the JSON value; every other field rides the value.
//!
//! Decode fallbacks (one rule, one place):
//! - `text`/`source` absent → empty string.
//! - `weight` unreadable or out of range → [`Weight::Explicit`] — the
//!   trust system fails toward distrust: an unreadable weight prices at
//!   the heaviest class rather than silently light.
//! - `severity` unreadable → `None` (unclassified), never a guessed class.
//! - `outcome` unreadable → [`Outcome::Unverified`] — visible, never a
//!   guessed resolution.
//! - `ts` unparseable → the row is in NO window ([`window_from_rows`]
//!   skips it): one corrupt row must not read as the whole account being
//!   unavailable — row-level garbage is not a store outage.

use chrono::{DateTime, Utc};

use crate::{LedgerEvent, LedgerEventId, Outcome, Severity, Weight};

/// Encode one event's wire value (the id is the row key — not embedded).
pub fn event_to_wire(event: &LedgerEvent) -> serde_json::Value {
    serde_json::json!({
        "text": event.text,
        "source": event.source,
        "weight": event.weight.points(),
        "severity": event.severity.map(severity_to_name),
        "outcome": match event.outcome {
            Outcome::Kept => "kept",
            Outcome::Violated => "violated",
            Outcome::Renegotiated => "renegotiated",
            Outcome::Unverified => "unverified",
        },
        "ts": event.ts.to_rfc3339(),
        "oracle_note": event.oracle_note,
    })
}

/// Rebuild one event from its row key and wire value (the inverse of
/// [`event_to_wire`]); absent fields fall back per the module's one set
/// of decode rules.
pub fn event_from_wire(id: &str, value: &serde_json::Value) -> LedgerEvent {
    LedgerEvent {
        id: LedgerEventId::from_string(id),
        text: value
            .get("text")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string(),
        source: value
            .get("source")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string(),
        weight: match value.get("weight").and_then(|w| w.as_i64()) {
            Some(1) => Weight::Implicit,
            Some(2) => Weight::Committed,
            _ => Weight::Explicit,
        },
        severity: value
            .get("severity")
            .and_then(|s| s.as_str())
            .and_then(severity_from_name),
        outcome: match value.get("outcome").and_then(|o| o.as_str()) {
            Some("kept") => Outcome::Kept,
            Some("violated") => Outcome::Violated,
            Some("renegotiated") => Outcome::Renegotiated,
            _ => Outcome::Unverified,
        },
        ts: value
            .get("ts")
            .and_then(|t| t.as_str())
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(Utc::now),
        oracle_note: value
            .get("oracle_note")
            .and_then(|n| n.as_str())
            .map(|s| s.to_string()),
    }
}

/// The events among `rows` — `(row key, wire value)` pairs — recorded at
/// or after `since`. A row without a parseable `ts` is in no window
/// (skipped per the module's decode rules — never an outage).
pub fn window_from_rows<'a>(
    rows: impl IntoIterator<Item = (&'a str, &'a serde_json::Value)>,
    since: DateTime<Utc>,
) -> Vec<LedgerEvent> {
    rows.into_iter()
        .filter(|(_, value)| {
            value
                .get("ts")
                .and_then(|t| t.as_str())
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| dt.with_timezone(&Utc) >= since)
                .unwrap_or(false)
        })
        .map(|(id, value)| event_from_wire(id, value))
        .collect()
}

/// The severity class's wire name (the violation class travels the row —
/// the goal→global bridge's channel).
fn severity_to_name(severity: Severity) -> &'static str {
    match severity {
        Severity::Competence => "competence",
        Severity::Integrity => "integrity",
    }
}

fn severity_from_name(name: &str) -> Option<Severity> {
    match name {
        "competence" => Some(Severity::Competence),
        "integrity" => Some(Severity::Integrity),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(severity: Option<Severity>) -> LedgerEvent {
        LedgerEvent {
            id: LedgerEventId::new(),
            text: "verify every quote".to_string(),
            source: "turn-1".to_string(),
            weight: Weight::Explicit,
            outcome: Outcome::Violated,
            severity,
            ts: Utc::now(),
            oracle_note: Some("cargo test: green".to_string()),
        }
    }

    /// The wire round-trip preserves every field including the severity
    /// class — the violation class travels the row.
    #[test]
    fn round_trip_preserves_every_field() {
        for severity in [None, Some(Severity::Competence), Some(Severity::Integrity)] {
            let original = event(severity);
            let wire = event_to_wire(&original);
            assert!(
                wire.get("id").is_none(),
                "the id is the row key, never embedded"
            );
            let round = event_from_wire(original.id.as_str(), &wire);
            assert_eq!(round.id, original.id);
            assert_eq!(round.text, original.text);
            assert_eq!(round.source, original.source);
            assert_eq!(round.weight, original.weight);
            assert_eq!(round.severity, original.severity);
            assert_eq!(round.outcome, original.outcome);
            assert_eq!(round.oracle_note, original.oracle_note);
        }
    }

    /// The decode fallbacks, pinned once: unreadable weight → Explicit
    /// (fail toward distrust); unreadable outcome → Unverified (visible,
    /// never guessed); absent severity → None (unclassified).
    #[test]
    fn decode_fallbacks_fail_toward_distrust() {
        let value = serde_json::json!({
            "text": "t",
            "source": "s",
            "weight": 99,
            "outcome": "garbage",
            "ts": Utc::now().to_rfc3339(),
        });
        let decoded = event_from_wire("row-key", &value);
        assert_eq!(decoded.weight, Weight::Explicit);
        assert_eq!(decoded.outcome, Outcome::Unverified);
        assert_eq!(decoded.severity, None);
    }

    /// A row without a parseable ts is in no window — skipped, never a
    /// store outage: one corrupt row must not read as the whole account
    /// unavailable (the exact drift this single decode home replaced).
    #[test]
    fn unparseable_ts_row_is_skipped_not_an_outage() {
        let since = Utc::now() - chrono::Duration::hours(24);
        let healthy = event(None);
        let corrupt = serde_json::json!({
            "text": "corrupt row",
            "source": "s",
            "weight": 3,
            "outcome": "violated",
            "ts": "not-a-timestamp",
        });
        let healthy_wire = event_to_wire(&healthy);
        let rows = vec![
            (healthy.id.as_str(), &healthy_wire),
            ("corrupt-row", &corrupt),
        ];
        let window = window_from_rows(rows, since);
        assert_eq!(
            window.len(),
            1,
            "the corrupt row is skipped, the healthy row decodes"
        );
        assert_eq!(window[0].text, "verify every quote");
    }

    /// Rows outside the window age out; the prior holds.
    #[test]
    fn rows_outside_the_window_age_out() {
        let since = Utc::now() - chrono::Duration::hours(24);
        let mut old = event(None);
        old.ts = Utc::now() - chrono::Duration::hours(48);
        let old_wire = event_to_wire(&old);
        let rows = vec![(old.id.as_str(), &old_wire)];
        assert!(window_from_rows(rows, since).is_empty());
    }
}
