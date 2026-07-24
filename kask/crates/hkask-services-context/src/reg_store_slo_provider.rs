//! Regulation Store SLO Provider — Bridges regulation record store to SLO evaluation.
//!
//! Implements `SloDataProvider` by querying the real regulation record store.
//! Lives in `hkask-services-context` because it bridges Regulation (domain) and
//! Storage (infrastructure) — the service layer's role in hexagonal architecture.

use hkask_regulation::slo_manager::{SloDataPoint, SloDataProvider, SloManagerError};
use hkask_storage::{DecayConfig, RegulationArchive};
use std::sync::Arc;

/// SLO data provider backed by the real RegulationArchive.
pub struct RegStoreSloProvider {
    store: Arc<RegulationArchive>,
}

impl RegStoreSloProvider {
    pub fn new(store: Arc<RegulationArchive>) -> Self {
        Self { store }
    }
}

impl SloDataProvider for RegStoreSloProvider {
    fn query(
        &self,
        span_namespace: &str,
        window_seconds: u64,
    ) -> Result<SloDataPoint, SloManagerError> {
        let since = chrono::Utc::now() - chrono::Duration::seconds(window_seconds as i64);
        let config = DecayConfig::default();

        let weighted = self
            .store
            .replay_weighted(since, 10_000, &config)
            .map_err(|e| SloManagerError::DataProvider(e.to_string()))?;

        let matching: Vec<_> = weighted
            .into_iter()
            .filter(|we| we.event.span.namespace.as_str().starts_with(span_namespace))
            .collect();
        let total_ops = matching.len() as u64;
        let successful = matching
            .iter()
            .filter(|we| !is_error_event(&we.event.observation))
            .count() as u64;

        Ok(SloDataPoint {
            total_operations: total_ops,
            successful_operations: successful,
        })
    }
}

fn is_error_event(observation: &serde_json::Value) -> bool {
    if let Some(obj) = observation.as_object() {
        if obj.contains_key("error") {
            return true;
        }
        if let Some(status) = obj.get("status").and_then(|v| v.as_str())
            && (status == "error" || status == "failed")
        {
            return true;
        }
        if let Some(outcome) = obj.get("outcome").and_then(|v| v.as_str())
            && outcome == "failure"
        {
            return true;
        }
    }
    false
}
