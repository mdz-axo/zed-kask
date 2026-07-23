//
// Integration test: real RegulationArchive with reg.gas.settled events is read by
// GasReport, which calibrates a DynamicGasTable. The calibrated table is then
// used to build a CompositeEnergyEstimator whose per-server costs reflect the
// observed actual/estimated ratios.

use chrono::{Duration, Utc};
use hkask_regulation::EnergyEstimator;
use hkask_regulation::composite_energy_estimator::CompositeEnergyEstimator;
use hkask_regulation::dynamic_gas_table::DynamicGasTable;
use hkask_regulation::gas_report::GasReport;
use hkask_storage::RegulationArchive;
use hkask_types::LedgerStoragePort;
use hkask_types::RegulationSink;
use hkask_types::WebID;
use hkask_types::event::{CyclePhase, RegulationRecord, Span, SpanKind};
use std::sync::Arc;

fn settled_event(agent: WebID, server: &str, reserved: u64, actual: u64) -> RegulationRecord {
    RegulationRecord::new(
        agent,
        Span::from_kind(SpanKind::GasSettled),
        CyclePhase::Act,
        serde_json::json!({
            "server": server,
            "tool": "test_tool",
            "reserved": reserved,
            "actual": actual,
            "refunded": reserved.saturating_sub(actual),
        }),
        0,
    )
}

#[test]
fn gas_report_calibrates_dynamic_table_from_settled_events() {
    let agent = WebID::new();
    let server = "hkask-mcp-media";

    let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
    let event_store: Arc<RegulationArchive> = Arc::new(RegulationArchive::from_driver(driver));

    // Actual cost is double the reserved cost → ratio 2.0 → cost should double.
    let event = settled_event(agent, server, 100, 200);
    event_store.persist(&event).expect("persist settled event");

    let store: Arc<dyn LedgerStoragePort> = Arc::clone(&event_store) as Arc<dyn LedgerStoragePort>;
    let report = GasReport::new(store);
    let mut table = DynamicGasTable::new();

    let since = Utc::now() - Duration::minutes(1);
    let until = Utc::now() + Duration::minutes(1);
    let adjusted = report
        .calibrate_table(&mut table, since, until)
        .expect("calibrate table");

    assert_eq!(adjusted, 1, "server with ratio 2.0 should be adjusted");
    assert_eq!(
        table.report_table()[server],
        200,
        "media cost should double from 100 to 200"
    );
}

#[test]
fn calibrated_table_flows_into_composite_estimator() {
    let agent = WebID::new();
    let server = "hkask-mcp-memory";

    let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
    let event_store: Arc<RegulationArchive> = Arc::new(RegulationArchive::from_driver(driver));

    // Actual is half of reserved → ratio 0.5 → cost should halve (5 → 2, floored at 1).
    let event = settled_event(agent, server, 10, 5);
    event_store.persist(&event).expect("persist settled event");

    let store: Arc<dyn LedgerStoragePort> = Arc::clone(&event_store) as Arc<dyn LedgerStoragePort>;
    let report = GasReport::new(store);
    let mut table = DynamicGasTable::new();
    report
        .calibrate_table(
            &mut table,
            Utc::now() - Duration::minutes(1),
            Utc::now() + Duration::minutes(1),
        )
        .expect("calibrate table");

    let estimator = CompositeEnergyEstimator::from_dynamic_table(&table);
    let cost = estimator.estimate_cost(server, "spec_query", &serde_json::json!({}));
    assert_eq!(
        cost, 2,
        "estimator should use calibrated cost from settled event"
    );
}
