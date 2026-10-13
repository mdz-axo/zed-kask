use super::*;

async fn success_rate(ledger: &RegulationLedger, domain: &str) -> Option<f64> {
    ledger
        .outcome_breakdown()
        .await
        .into_iter()
        .find(|snapshot| snapshot.domain == domain)
        .and_then(|snapshot| snapshot.success_rate)
}

/// expect: "Quiet domains stop contributing stale deficits or outcome samples" [P9]
#[tokio::test]
async fn observations_expire_without_another_write() {
    use crate::sensor_provider::{Sensor, ToolReliabilitySensor};
    let shared = Arc::new(tokio::sync::RwLock::new(RegulationLedger::default()));
    let sensor = ToolReliabilitySensor::new(shared.clone(), 0.8);
    let ledger = shared.read().await;
    ledger.increment_variety("quiet", "one-tool").await;
    ledger
        .record_outcome("quiet", false, Some("internal"))
        .await;
    assert_eq!(success_rate(&ledger, "quiet").await, Some(0.0));
    // A second domain meeting the sensor's minimum-sample floor, so the
    // observe-level assertions exercise the sensor's real aggregation
    // path — the 1-sample "quiet" domain is below the floor, where
    // observe is None by design and would make the expiry assertions
    // vacuous.
    for _ in 0..crate::sensor_provider::TOOL_RELIABILITY_MIN_DOMAIN_SAMPLES {
        ledger
            .record_outcome("sampled", false, Some("internal"))
            .await;
    }
    assert_eq!(
        sensor.observe().await.expect("active observation").value,
        0.0
    );
    assert!(ledger.health().await.overall_deficit > 0);
    {
        let mut state = ledger.state.write().await;
        let old = Instant::now() - Duration::from_secs(OBSERVATION_WINDOW_SECS + 1);
        state.tracker.counter("quiet").window_start = old;
        for domain in ["quiet", "sampled"] {
            state
                .outcome
                .get_mut(domain)
                .expect("tracked domain")
                .window_start = old;
        }
    }
    assert_eq!(success_rate(&ledger, "quiet").await, None);
    assert!(
        sensor.observe().await.is_none(),
        "idle is no sample, not healthy"
    );
    assert!(ledger.check_outcome("quiet").await.is_none());
    assert_eq!(ledger.health().await.overall_deficit, 0);
    for _ in 0..crate::sensor_provider::TOOL_RELIABILITY_MIN_DOMAIN_SAMPLES {
        ledger.record_outcome("active", true, None).await;
    }
    let mut rates = Vec::new();
    for snapshot in ledger.outcome_breakdown().await {
        if let Some(rate) = snapshot.success_rate {
            rates.push(rate);
        }
    }
    assert_eq!(rates, vec![1.0]);
    assert_eq!(
        sensor.observe().await.expect("new active sample").value,
        1.0
    );
}

/// expect: "My configured outcome thresholds control alerts from startup" [P9]
#[tokio::test]
async fn configured_outcome_thresholds_apply_at_construction() -> anyhow::Result<()> {
    let configuration: crate::set_points::SetPointsConfig = serde_yaml_neo::from_str(
        "outcome_warning_threshold: 0.95\noutcome_critical_threshold: 0.90\n",
    )?;
    let points = crate::set_points::SetPoints::from_config(&configuration);
    points.validate()?;
    let ledger = RegulationLedger::with_set_points(100, &points);
    let default_ledger =
        RegulationLedger::with_set_points(100, &crate::set_points::SetPoints::default());
    for ledger in [&ledger, &default_ledger] {
        for _ in 0..9 {
            ledger.record_outcome("review", true, None).await;
        }
        ledger
            .record_outcome("review", false, Some("internal"))
            .await;
    }
    let alert = ledger.check_outcome("review").await.expect("90% must warn");
    assert_eq!(alert.severity, crate::algedonic::AlertSeverity::Warning);
    assert!(default_ledger.check_outcome("review").await.is_none());
    ledger
        .record_outcome("review", false, Some("internal"))
        .await;
    let alert = ledger
        .check_outcome("review")
        .await
        .expect("below 90% must escalate");
    assert_eq!(alert.severity, crate::algedonic::AlertSeverity::Critical);
    Ok(())
}

/// `variety()` must include server-name domains — the tool-dispatch feed
/// taxonomy. The previous `SpanNamespace::parse` filter hid every
/// server-named domain, making the query useless against the live feed.
#[tokio::test]
async fn variety_query_includes_server_name_domains() {
    let ledger = RegulationLedger::default();
    ledger
        .increment_variety("hkask-mcp-media", "gallery_search")
        .await;
    ledger
        .increment_variety("hkask-mcp-media", "gallery_add_media")
        .await;
    let variety = ledger.variety().await;
    assert_eq!(
        variety.get("hkask-mcp-media"),
        Some(&2),
        "server-name domains must appear with their distinct-tool count"
    );
}

/// Config-gap failures (missing binary / credential) must not degrade a
/// domain's success rate — they are operator-actionable environment
/// signals, not tool unreliability. Before this exclusion, a media
/// server without yt-dlp read as a failing media domain and fired
/// false `ToolReliabilityDegraded` alerts.
#[tokio::test]
async fn config_gap_failures_do_not_degrade_success_rate() {
    let ledger = RegulationLedger::default();
    ledger.record_outcome("media", true, None).await;
    ledger
        .record_outcome("media", false, Some("unavailable"))
        .await;
    ledger
        .record_outcome("media", false, Some("permission_denied"))
        .await;
    let rate = success_rate(&ledger, "media")
        .await
        .expect("media domain tracked");
    assert_eq!(
        rate, 1.0,
        "config-gap errors must not read as unreliability — the operator, \
         not the tool, must act"
    );

    // A real failure still degrades the rate.
    ledger
        .record_outcome("media", false, Some("internal"))
        .await;
    let rate = success_rate(&ledger, "media")
        .await
        .expect("media domain tracked");
    assert_eq!(rate, 0.5, "one real failure of two counted ops");
}

/// Caller-caused argument rejections (`invalid_argument`) must not
/// degrade a domain's success rate — the tool behaved as designed by
/// rejecting malformed arguments; the failure is the caller's (the
/// model-caused classes: truncated tool-call JSON, dropped parameters),
/// not the tool's. The kind stays in the per-kind breakdown (see
/// `outcome_breakdown_names_domains_and_kinds`).
#[tokio::test]
async fn invalid_argument_failures_do_not_degrade_success_rate() {
    let ledger = RegulationLedger::default();
    ledger.record_outcome("media", true, None).await;
    ledger
        .record_outcome("media", false, Some("invalid_argument"))
        .await;
    let rate = success_rate(&ledger, "media")
        .await
        .expect("media domain tracked");
    assert_eq!(
        rate, 1.0,
        "a caller-caused rejection is not tool unreliability — the tool \
         correctly rejected malformed input"
    );
}

/// The per-domain breakdown is the diagnosis surface that names the
/// failing domain — the aggregate the sensor reports cannot. It must
/// carry per-domain rates, operation counts, and per-kind tallies, and
/// keep not-tool-fault kinds visible even when they are the domain's
/// only in-window calls (the rate excludes them; the breakdown must not).
#[tokio::test]
async fn outcome_breakdown_names_domains_and_kinds() {
    let ledger = RegulationLedger::default();
    // "media": 2 successes, 1 internal failure → rate 2/3.
    ledger.record_outcome("media", true, None).await;
    ledger.record_outcome("media", true, None).await;
    ledger
        .record_outcome("media", false, Some("internal"))
        .await;
    // "companies": only caller-caused rejections → total 0 (excluded
    // from the rate math) but the breakdown must show the kinds.
    ledger
        .record_outcome("companies", false, Some("invalid_argument"))
        .await;
    ledger
        .record_outcome("companies", false, Some("invalid_argument"))
        .await;
    ledger
        .record_outcome("companies", false, Some("invalid_argument"))
        .await;

    let breakdown = ledger.outcome_breakdown().await;
    assert_eq!(breakdown.len(), 2, "both tracked domains appear");
    let companies = breakdown
        .iter()
        .find(|snapshot| snapshot.domain == "companies")
        .expect("companies snapshot");
    let media = breakdown
        .iter()
        .find(|snapshot| snapshot.domain == "media")
        .expect("media snapshot");
    assert_eq!(media.successes, 2);
    assert_eq!(media.failures, 1);
    assert_eq!(media.total_operations, 3);
    assert_eq!(media.success_rate, Some(2.0 / 3.0));
    assert_eq!(media.error_kinds, vec![("internal".to_string(), 1)]);
    assert_eq!(
        companies.total_operations, 0,
        "invalid_argument rejections are excluded from the rate math"
    );
    assert_eq!(companies.success_rate, None);
    assert_eq!(
        companies.error_kinds,
        vec![("invalid_argument".to_string(), 3)],
        "the breakdown keeps excluded kinds visible for diagnosis"
    );
}
