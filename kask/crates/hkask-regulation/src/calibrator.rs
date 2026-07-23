//! Shared calibration loop infrastructure.
//!
//! Both `CalibratedEnergyEstimator` and `WalletGasCalibrator` share the same
//! background spawn pattern: set alive flag, `tokio::spawn`, loop + sleep +
//! calibrate + log errors. This module extracts that pattern into a trait +
//! free function to eliminate the duplication.

use hkask_types::InfrastructureError;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tracing::{info, warn};

/// A self-calibrating subsystem that can run a background calibration loop.
///
/// Implemented by `CalibratedEnergyEstimator` and `WalletGasCalibrator`.
/// Each provides its specific calibration logic via `run_calibration` and
/// its Regulation span target via `calibration_target`.
#[async_trait::async_trait]
pub trait Calibrator: Send + Sync + 'static {
    /// Run one calibration pass. Returns the number of adjustments made (0 = none).
    async fn run_calibration(&self) -> Result<usize, InfrastructureError>;

    /// Whether the background calibration loop is running.
    fn calibration_alive(&self) -> &AtomicBool;

    /// Regulation tracing target for calibration spans and logs.
    fn calibration_target(&self) -> &'static str;
}

/// Spawn a background calibration loop that periodically calls `run_calibration`.
///
/// The loop runs until the tokio runtime is shut down. Errors are logged but
/// do not terminate the loop — the calibrator keeps retrying.
pub fn spawn_calibration_loop<C: Calibrator>(calibrator: Arc<C>, interval: Duration) {
    let calibrator_target = calibrator.calibration_target();
    calibrator
        .calibration_alive()
        .store(true, Ordering::Release);
    tokio::spawn(async move {
        info!(
            target: "hkask.calibration",
            calibrator_target,
            interval_secs = interval.as_secs(),
            "Background calibration started",
        );
        loop {
            tokio::time::sleep(interval).await;
            if let Err(e) = calibrator.run_calibration().await {
                warn!(
                    target: "hkask.calibration",
                    calibrator_target,
                    error = %e,
                    "Background calibration failed",
                );
            }
        }
    });
}
