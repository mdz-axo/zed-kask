//! Sensor trait — pluggable metric sensors (Fermi Extractor pattern).
//!
//! Fermi's `Extractor` trait separates domain data extraction from the fitting
//! loop. Sensor applies the same pattern to hKask's regulation loop:
//! each metric gets its own `Sensor` implementation, registered with
//! a `SensorRegistry`. The `CyberneticsLoop::sense()` method walks the registry
//! instead of containing inline sensing logic.
//!
//! ## Why this lives in hkask-regulation
//!
//! Sensor providers are Regulation regulation infrastructure. They live alongside
//! `CyberneticsLoop`, `StagnationDetector`, and `SetPoints` in `hkask-regulation`,
//! the crate responsible for homeostatic self-regulation.
//!
//! ## Unified Sensor Catalog (v0.32.0)
//!
//! The `SensorRegistry` provides a single registration point for sensors
//! across ALL loops, not just Cybernetics. Each loop owns a `SensorRegistry`
//! for its local sensors, but the `SensorRegistry` tracks all of them for
//! monitoring, health checks, and dynamic registration. This eliminates the
//! fragmentation where each loop had inline `sense()` methods that couldn't
//! be discovered or managed from a central point.

use super::types::loops::{LoopId, Signal, SignalMetric};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

/// A pluggable sensor that produces one kind of signal metric.
///
/// Each implementation senses a single `SignalMetric` from its data source.
/// Fermi pattern: the `Extractor` trait takes a domain payload and produces
/// a scalar; `Sensor` takes system state and produces an optional
/// `Signal`. If the sensor has nothing to report (metric is healthy),
/// it returns `None`.
#[async_trait::async_trait]
pub trait Sensor: Send + Sync {
    /// Sense the current state and produce a signal if the metric is
    /// in a reportable state. Returns `None` if nothing to report.
    async fn sense(&self) -> Option<Signal>;

    /// The metric this sensor produces. Used for catalog indexing and
    /// deduplication. Default implementation returns `None` for backward
    /// compatibility with sensors that produce dynamic metrics.
    fn metric(&self) -> Option<SignalMetric> {
        None
    }

    /// Human-readable name for this sensor. Used in catalog listings and
    /// health checks. Default implementation returns the type name.
    fn name(&self) -> &str {
        std::any::type_name::<Self>()
    }

    /// The loop this sensor is registered under. Used by the catalog to
    /// route signals to the correct loop's `sense()` call. Default
    /// implementation returns `None` for backward compatibility.
    fn loop_id(&self) -> Option<LoopId> {
        None
    }
}

/// Sensor bus for a single loop — actively walks sensors each tick.
///
/// Providers are registered at construction time and executed in order.
/// Order doesn't matter — each provider independently decides whether
/// to emit a signal. The bus aggregates their signals into a single
/// `Vec<Signal>` for the loop's `sense()` phase.
pub struct SensorBus {
    providers: Mutex<Vec<Arc<dyn Sensor>>>,
}

impl SensorBus {
    /// expect: "The system provides pluggable metric sensing for the cybernetic regulation loop"
    pub fn new() -> Self {
        Self {
            providers: Mutex::new(Vec::new()),
        }
    }

    /// expect: "The system provides pluggable metric sensing for the cybernetic regulation loop"
    pub fn register(&self, provider: Arc<dyn Sensor>) {
        self.providers.lock().push(provider);
    }

    /// expect: "The system provides pluggable metric sensing for the cybernetic regulation loop"
    pub async fn sense_all(&self, source: LoopId) -> Vec<Signal> {
        let providers: Vec<Arc<dyn Sensor>> = { self.providers.lock().clone() }; // Lock dropped here — no .await while holding it.
        let mut signals = Vec::new();
        for provider in &providers {
            if let Some(signal) = provider.sense().await {
                signals.push(signal);
            }
        }
        for s in &mut signals {
            s.source = source;
        }
        signals
    }

    /// Number of registered providers.
    pub fn len(&self) -> usize {
        self.providers.lock().len()
    }

    /// Whether the registry has no providers.
    pub fn is_empty(&self) -> bool {
        self.providers.lock().is_empty()
    }

    /// List provider names for diagnostics.
    pub fn provider_names(&self) -> Vec<String> {
        self.providers
            .lock()
            .iter()
            .map(|p| p.name().to_string())
            .collect()
    }
}

impl Default for SensorBus {
    fn default() -> Self {
        Self::new()
    }
}

/// Registry of all sensors across all loops in the system.
///
/// Provides a single registration point for sensors across ALL loops, not
/// just Cybernetics. Each loop owns a `SensorBus` for its local sensors,
/// but the `SensorRegistry` tracks all of them for monitoring, health checks,
/// and dynamic registration.
///
/// This eliminates the fragmentation where each loop had inline `sense()`
/// methods that couldn't be discovered or managed from a central point.
///
/// # Architecture
///
/// ```text
/// SensorRegistry (singleton, system-level)
/// ├── LoopId::Cybernetics → SensorBus
/// ├── LoopId::Inference   → SensorBus
/// ├── LoopId::Episodic    → SensorBus
/// ├── LoopId::Semantic    → SensorBus
/// ├── LoopId::Curation    → SensorBus
/// ├── LoopId::Snapshot    → SensorBus
/// ├── LoopId::StorageGuard → SensorBus
/// └── LoopId::McpServerGuard → SensorBus
/// ```
///
/// Each loop's `sense()` method calls `registry.sense_all(loop_id)` instead
/// of containing inline sensing logic. Sensors are registered at startup
/// via `registry.register_for(loop_id, provider)`.
pub struct SensorRegistry {
    /// Per-loop sensor buses. Each loop owns its own bus.
    registries: Mutex<HashMap<LoopId, SensorBus>>,
}

impl SensorRegistry {
    /// Create a new empty catalog.
    pub fn new() -> Self {
        Self {
            registries: Mutex::new(HashMap::new()),
        }
    }

    /// Get or create the sensor bus for a specific loop.
    pub fn bus_for(&self, loop_id: LoopId) -> SensorBus {
        let registries = self.registries.lock();
        registries
            .get(&loop_id)
            .cloned()
            .unwrap_or_else(SensorBus::new)
    }

    /// Register a sensor for a specific loop.
    pub fn register_for(&self, loop_id: LoopId, provider: Arc<dyn Sensor>) {
        let mut registries = self.registries.lock();
        registries.entry(loop_id).or_default().register(provider);
    }

    /// Sense all signals for a specific loop.
    pub async fn sense_all(&self, loop_id: LoopId) -> Vec<Signal> {
        let registry = {
            let registries = self.registries.lock();
            registries.get(&loop_id).cloned()
        };
        match registry {
            Some(reg) => reg.sense_all(loop_id).await,
            None => Vec::new(),
        }
    }

    /// Total number of sensors across all loops.
    pub fn total_sensors(&self) -> usize {
        self.registries.lock().values().map(|r| r.len()).sum()
    }

    /// List all sensor names grouped by loop.
    pub fn sensor_inventory(&self) -> Vec<(LoopId, Vec<String>)> {
        self.registries
            .lock()
            .iter()
            .map(|(loop_id, registry)| (*loop_id, registry.provider_names()))
            .collect()
    }

    /// Health check: which loops have no sensors registered?
    pub fn loops_without_sensors(&self) -> Vec<LoopId> {
        let registries = self.registries.lock();
        let all_loops = [
            LoopId::Inference,
            LoopId::Episodic,
            LoopId::Semantic,
            LoopId::Curation,
            LoopId::Cybernetics,
            LoopId::Snapshot,
            LoopId::StorageGuard,
            LoopId::McpServerGuard,
        ];
        all_loops
            .iter()
            .filter(|id| {
                !registries.contains_key(id) || registries.get(*id).is_none_or(|r| r.is_empty())
            })
            .copied()
            .collect()
    }
}

impl Default for SensorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for SensorBus {
    fn clone(&self) -> Self {
        Self {
            providers: Mutex::new(self.providers.lock().clone()),
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// CONCRETE SENSOR PROVIDERS
// ═════════════════════════════════════════════════════════════════════════════

/// Senses energy budget remaining ratios across all agents.
///
/// Data source: `GasBudgetManager`. Produces a signal per agent.
pub struct EnergyBudgetSensor {
    budget_manager: Arc<tokio::sync::RwLock<super::energy_budget_management::GasBudgetManager>>,
    set_point: f64,
}

impl EnergyBudgetSensor {
    /// expect: "The system provides pluggable metric sensing for the cybernetic regulation loop"
    pub fn new(
        budget_manager: Arc<tokio::sync::RwLock<super::energy_budget_management::GasBudgetManager>>,
        set_point: f64,
    ) -> Self {
        Self {
            budget_manager,
            set_point,
        }
    }
}

#[async_trait::async_trait]
impl Sensor for EnergyBudgetSensor {
    async fn sense(&self) -> Option<Signal> {
        let statuses = self.budget_manager.read().await.all_agent_statuses().await;
        // Use the worst remaining ratio as the aggregate signal.
        let worst = statuses
            .iter()
            .map(|(_, s)| s.remaining.0 as f64 / s.cap.0.max(1) as f64)
            .fold(1.0, f64::min);
        Some(Signal::new(
            LoopId::Cybernetics, // placeholder — registry backfills
            SignalMetric::EnergyRemaining,
            worst,
            self.set_point,
        ))
    }

    fn metric(&self) -> Option<SignalMetric> {
        Some(SignalMetric::EnergyRemaining)
    }

    fn loop_id(&self) -> Option<LoopId> {
        Some(LoopId::Cybernetics)
    }
}

/// Senses variety deficit from the Regulation runtime.
///
/// Data source: `RegulationLedger`. Produces a single aggregate signal.
pub struct VarietySensor {
    ledger: Arc<tokio::sync::RwLock<super::runtime::RegulationLedger>>,
    set_point: f64,
}

impl VarietySensor {
    /// expect: "The system provides pluggable metric sensing for the cybernetic regulation loop"
    pub fn new(
        ledger: Arc<tokio::sync::RwLock<super::runtime::RegulationLedger>>,
        set_point: f64,
    ) -> Self {
        Self { ledger, set_point }
    }
}

#[async_trait::async_trait]
impl Sensor for VarietySensor {
    async fn sense(&self) -> Option<Signal> {
        let ledger = self.ledger.read().await;
        let health = ledger.health().await;
        Some(Signal::new(
            LoopId::Cybernetics, // placeholder — registry backfills
            SignalMetric::VarietyDeficit,
            health.overall_deficit as f64,
            self.set_point,
        ))
    }

    fn metric(&self) -> Option<SignalMetric> {
        Some(SignalMetric::VarietyDeficit)
    }

    fn loop_id(&self) -> Option<LoopId> {
        Some(LoopId::Cybernetics)
    }
}

/// Senses wallet API key health from the gas budget manager.
pub struct WalletKeyHealthSensor {
    budget_manager: Arc<tokio::sync::RwLock<super::energy_budget_management::GasBudgetManager>>,
}

impl WalletKeyHealthSensor {
    /// expect: "The system provides pluggable metric sensing for the cybernetic regulation loop"
    pub fn new(
        budget_manager: Arc<tokio::sync::RwLock<super::energy_budget_management::GasBudgetManager>>,
    ) -> Self {
        Self { budget_manager }
    }
}

#[async_trait::async_trait]
impl Sensor for WalletKeyHealthSensor {
    async fn sense(&self) -> Option<Signal> {
        let key_alerts = self.budget_manager.read().await.wallet_key_alerts().await;
        if key_alerts.is_empty() {
            return None; // Healthy — nothing to report.
        }
        Some(Signal::new(
            LoopId::Cybernetics, // placeholder — registry backfills
            SignalMetric::WalletKeyHealth,
            1.0, // alert active
            0.0, // set-point: no alerts
        ))
    }

    fn metric(&self) -> Option<SignalMetric> {
        Some(SignalMetric::WalletKeyHealth)
    }

    fn loop_id(&self) -> Option<LoopId> {
        Some(LoopId::Cybernetics)
    }
}

/// Senses wallet balance ratio from the gas budget manager.
///
/// Replaces the inline wallet ratio sensing that was in `CyberneticsLoop::sense()`.
/// Data source: `GasBudgetManager::wallet_balance_ratios()`. Produces a signal
/// per agent wallet.
pub struct WalletBalanceRatioSensor {
    budget_manager: Arc<tokio::sync::RwLock<super::energy_budget_management::GasBudgetManager>>,
    set_point: f64,
}

impl WalletBalanceRatioSensor {
    /// Create a new wallet balance ratio sensor.
    ///
    /// `set_point` is the alert threshold (default: 0.1 = alert when below 10%).
    pub fn new(
        budget_manager: Arc<tokio::sync::RwLock<super::energy_budget_management::GasBudgetManager>>,
        set_point: f64,
    ) -> Self {
        Self {
            budget_manager,
            set_point,
        }
    }
}

#[async_trait::async_trait]
impl Sensor for WalletBalanceRatioSensor {
    async fn sense(&self) -> Option<Signal> {
        let wallet_ratios = self
            .budget_manager
            .read()
            .await
            .wallet_balance_ratios()
            .await;
        // Use the worst ratio as the aggregate signal.
        let worst = wallet_ratios
            .iter()
            .map(|(ratio, _cap)| *ratio)
            .fold(1.0, f64::min);
        if worst >= self.set_point {
            return None; // Healthy — nothing to report.
        }
        Some(Signal::new(
            LoopId::Cybernetics,
            SignalMetric::WalletBalanceRatio,
            worst,
            self.set_point,
        ))
    }

    fn metric(&self) -> Option<SignalMetric> {
        Some(SignalMetric::WalletBalanceRatio)
    }

    fn loop_id(&self) -> Option<LoopId> {
        Some(LoopId::Cybernetics)
    }
}

/// Senses tool reliability across all MCP tools.
pub struct ToolReliabilitySensor {
    tool_stats: Arc<crate::tool_stats::ToolStats>,
    threshold: f64,
}

impl ToolReliabilitySensor {
    /// expect: "The system provides pluggable metric sensing for the cybernetic regulation loop"
    pub fn new(tool_stats: Arc<crate::tool_stats::ToolStats>, threshold: f64) -> Self {
        Self {
            tool_stats,
            threshold,
        }
    }
}

#[async_trait::async_trait]
impl Sensor for ToolReliabilitySensor {
    async fn sense(&self) -> Option<Signal> {
        let alerts = self.tool_stats.reliability_alerts().await;
        if alerts.is_empty() {
            return None;
        }
        let worst = alerts
            .iter()
            .map(|a| a.success_probability)
            .fold(1.0, f64::min);
        Some(Signal::new(
            LoopId::Cybernetics, // placeholder — registry backfills
            SignalMetric::ToolReliability,
            worst,
            self.threshold,
        ))
    }

    fn metric(&self) -> Option<SignalMetric> {
        Some(SignalMetric::ToolReliability)
    }

    fn loop_id(&self) -> Option<LoopId> {
        Some(LoopId::Cybernetics)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A test sensor that always returns a fixed signal.
    struct TestSensor {
        metric: SignalMetric,
        loop_id: LoopId,
        value: f64,
        set_point: f64,
    }

    #[async_trait::async_trait]
    impl Sensor for TestSensor {
        async fn sense(&self) -> Option<Signal> {
            Some(Signal::new(
                self.loop_id,
                self.metric,
                self.value,
                self.set_point,
            ))
        }

        fn metric(&self) -> Option<SignalMetric> {
            Some(self.metric)
        }

        fn loop_id(&self) -> Option<LoopId> {
            Some(self.loop_id)
        }
    }

    #[tokio::test]
    async fn sensor_bus_sense_all_returns_signals() {
        let bus = SensorBus::new();
        bus.register(Arc::new(TestSensor {
            metric: SignalMetric::EnergyRemaining,
            loop_id: LoopId::Cybernetics,
            value: 0.5,
            set_point: 0.2,
        }));
        bus.register(Arc::new(TestSensor {
            metric: SignalMetric::VarietyDeficit,
            loop_id: LoopId::Cybernetics,
            value: 10.0,
            set_point: 5.0,
        }));

        let signals = bus.sense_all(LoopId::Cybernetics).await;
        assert_eq!(signals.len(), 2);
        assert_eq!(signals[0].source, LoopId::Cybernetics);
        assert_eq!(signals[1].source, LoopId::Cybernetics);
    }

    #[tokio::test]
    async fn sensor_bus_empty_returns_no_signals() {
        let bus = SensorBus::new();
        let signals = bus.sense_all(LoopId::Cybernetics).await;
        assert!(signals.is_empty());
    }

    #[tokio::test]
    async fn sensor_bus_provider_names() {
        let bus = SensorBus::new();
        bus.register(Arc::new(TestSensor {
            metric: SignalMetric::EnergyRemaining,
            loop_id: LoopId::Cybernetics,
            value: 0.5,
            set_point: 0.2,
        }));
        let names = bus.provider_names();
        assert_eq!(names.len(), 1);
        assert!(names[0].contains("TestSensor"));
    }

    #[tokio::test]
    async fn sensor_catalog_register_and_sense() {
        let catalog = SensorRegistry::new();
        catalog.register_for(
            LoopId::Cybernetics,
            Arc::new(TestSensor {
                metric: SignalMetric::EnergyRemaining,
                loop_id: LoopId::Cybernetics,
                value: 0.3,
                set_point: 0.2,
            }),
        );
        catalog.register_for(
            LoopId::Inference,
            Arc::new(TestSensor {
                metric: SignalMetric::InferenceGasRemaining,
                loop_id: LoopId::Inference,
                value: 0.1,
                set_point: 0.2,
            }),
        );

        let cybernetics_signals = catalog.sense_all(LoopId::Cybernetics).await;
        assert_eq!(cybernetics_signals.len(), 1);
        assert_eq!(cybernetics_signals[0].metric, SignalMetric::EnergyRemaining);

        let inference_signals = catalog.sense_all(LoopId::Inference).await;
        assert_eq!(inference_signals.len(), 1);
        assert_eq!(
            inference_signals[0].metric,
            SignalMetric::InferenceGasRemaining
        );
    }

    #[tokio::test]
    async fn sensor_catalog_sense_empty_loop_returns_nothing() {
        let catalog = SensorRegistry::new();
        let signals = catalog.sense_all(LoopId::Cybernetics).await;
        assert!(signals.is_empty());
    }

    #[tokio::test]
    async fn sensor_catalog_total_sensors() {
        let catalog = SensorRegistry::new();
        catalog.register_for(
            LoopId::Cybernetics,
            Arc::new(TestSensor {
                metric: SignalMetric::EnergyRemaining,
                loop_id: LoopId::Cybernetics,
                value: 0.3,
                set_point: 0.2,
            }),
        );
        catalog.register_for(
            LoopId::Cybernetics,
            Arc::new(TestSensor {
                metric: SignalMetric::VarietyDeficit,
                loop_id: LoopId::Cybernetics,
                value: 10.0,
                set_point: 5.0,
            }),
        );
        catalog.register_for(
            LoopId::Inference,
            Arc::new(TestSensor {
                metric: SignalMetric::InferenceGasRemaining,
                loop_id: LoopId::Inference,
                value: 0.1,
                set_point: 0.2,
            }),
        );
        assert_eq!(catalog.total_sensors(), 3);
    }

    #[tokio::test]
    async fn sensor_catalog_sensor_inventory() {
        let catalog = SensorRegistry::new();
        catalog.register_for(
            LoopId::Cybernetics,
            Arc::new(TestSensor {
                metric: SignalMetric::EnergyRemaining,
                loop_id: LoopId::Cybernetics,
                value: 0.3,
                set_point: 0.2,
            }),
        );
        catalog.register_for(
            LoopId::Inference,
            Arc::new(TestSensor {
                metric: SignalMetric::InferenceGasRemaining,
                loop_id: LoopId::Inference,
                value: 0.1,
                set_point: 0.2,
            }),
        );

        let inventory = catalog.sensor_inventory();
        assert_eq!(inventory.len(), 2);
        // Verify each loop has its sensor registered
        let cybernetics_entry = inventory
            .iter()
            .find(|(id, _)| *id == LoopId::Cybernetics)
            .expect("Cybernetics should have sensors");
        assert_eq!(cybernetics_entry.1.len(), 1);
        let inference_entry = inventory
            .iter()
            .find(|(id, _)| *id == LoopId::Inference)
            .expect("Inference should have sensors");
        assert_eq!(inference_entry.1.len(), 1);
    }

    #[tokio::test]
    async fn sensor_catalog_loops_without_sensors() {
        let catalog = SensorRegistry::new();
        // No sensors registered — all loops should be listed as without sensors
        let empty_loops = catalog.loops_without_sensors();
        assert!(empty_loops.contains(&LoopId::Cybernetics));
        assert!(empty_loops.contains(&LoopId::Inference));
        assert!(empty_loops.contains(&LoopId::StorageGuard));

        // Register a sensor for Cybernetics
        catalog.register_for(
            LoopId::Cybernetics,
            Arc::new(TestSensor {
                metric: SignalMetric::EnergyRemaining,
                loop_id: LoopId::Cybernetics,
                value: 0.3,
                set_point: 0.2,
            }),
        );
        let empty_loops = catalog.loops_without_sensors();
        assert!(!empty_loops.contains(&LoopId::Cybernetics));
        assert!(empty_loops.contains(&LoopId::Inference));
    }

    #[tokio::test]
    async fn sensor_registry_clone_preserves_providers() {
        let bus = SensorBus::new();
        bus.register(Arc::new(TestSensor {
            metric: SignalMetric::EnergyRemaining,
            loop_id: LoopId::Cybernetics,
            value: 0.5,
            set_point: 0.2,
        }));
        let cloned = bus.clone();
        let signals = cloned.sense_all(LoopId::Cybernetics).await;
        assert_eq!(signals.len(), 1);
    }
}
