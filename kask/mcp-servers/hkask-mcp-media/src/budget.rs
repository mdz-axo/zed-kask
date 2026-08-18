//! Cost-gating and rJoule-budget accounting for the media server.
//!
//! Extracted from the root module as a cohesive concern: per-operation unit
//! costs, rJoule estimation, the budget gate (check-before-charge under a
//! mutex), threshold-crossing warning, and startup resolution from env vars.

use std::sync::Arc;

use hkask_mcp_server::server::McpToolError;

use crate::error::MediaError;

/// Parse a `f64` from an env var, falling back to `default` on absence,
/// parse failure, or a non-finite/negative value. Used for budget config
/// resolved once at startup (not per call) so the gate is deterministic and
/// tests are env-isolated.
fn env_f64(key: &str, default: f64) -> f64 {
    std::env::var(key)
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|v: &f64| v.is_finite() && *v >= 0.0)
        .unwrap_or(default)
}

/// Per-operation rJoule (USD) unit costs. Resolved once at startup from
/// `HKASK_MEDIA_RJOULE_PER_*` env vars and stored in [`MediaBudget`] so
/// [`estimate_rjoule`] is a pure function of these + the request params (no
/// per-call env reads, deterministic in tests). Defaults are conservative
/// placeholders, not real pricing — set them to your provider's actual rates.
#[derive(Clone, Copy)]
pub struct UnitCosts {
    pub per_image: f64,
    pub per_transform: f64,
    pub per_upscale: f64,
    pub per_video_second: f64,
}

impl UnitCosts {
    /// Default conservative placeholder unit costs (1 rJoule = $1 USD).
    pub const DEFAULT: Self = Self {
        per_image: 0.05,
        per_transform: 0.04,
        per_upscale: 0.02,
        per_video_second: 1.0,
    };

    /// Resolve unit costs from env vars, falling back to [`DEFAULT`].
    fn from_env() -> Self {
        Self {
            per_image: env_f64("HKASK_MEDIA_RJOULE_PER_IMAGE", Self::DEFAULT.per_image),
            per_transform: env_f64(
                "HKASK_MEDIA_RJOULE_PER_TRANSFORM",
                Self::DEFAULT.per_transform,
            ),
            per_upscale: env_f64("HKASK_MEDIA_RJOULE_PER_UPSCALE", Self::DEFAULT.per_upscale),
            per_video_second: env_f64(
                "HKASK_MEDIA_RJOULE_PER_VIDEO_SECOND",
                Self::DEFAULT.per_video_second,
            ),
        }
    }
}

/// Resolved rJoule budget configuration for the media server. Resolved once at
/// startup ([`build_media_budget`]) so the gate is deterministic and tests are
/// env-isolated. `tracker = None` means enforcement is disabled
/// (`HKASK_MEDIA_RJOULE_CAP` unset or `0`); `unit_costs` and `alert_threshold`
/// are still carried so a disabled budget is self-describing.
pub struct MediaBudget {
    /// `None` = enforcement disabled (cap unset/0). Gas (compute) is enforced
    /// upstream at `McpRuntime::invoke` + `CyberneticsLoop`, so the tracker's
    /// gas cap is inert and never charged here.
    tracker: Option<Arc<tokio::sync::Mutex<hkask_templates::budget::BudgetTracker>>>,
    unit_costs: UnitCosts,
    /// Fraction of the cap (0.0–1.0) at which the threshold-crossing warning
    /// fires once. The enforcement point for `HKASK_MEDIA_RJOULE_ALERT_THRESHOLD`.
    alert_threshold: f64,
    /// One-shot guard for the threshold warning (mirrors `BudgetTracker`'s
    /// private `rjoule_alerted`, which we can't reach without
    /// `check_exhausted` — and we avoid `check_exhausted` because its
    /// exhaustion check is redundant with our own pre-charge gate).
    alerted: std::sync::atomic::AtomicBool,
}

impl MediaBudget {
    /// A disabled budget (enforcement off) carrying the given unit costs +
    /// alert threshold so it stays self-describing.
    fn disabled(unit_costs: UnitCosts, alert_threshold: f64) -> Self {
        Self {
            tracker: None,
            unit_costs,
            alert_threshold,
            alerted: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

/// Build an rJoule-only `BudgetTracker` (the media server never charges gas —
/// enforcement is upstream at `McpRuntime::invoke`). Shared by
/// [`build_media_budget`] (production) and the gate tests.
fn make_rjoule_tracker(
    cap: u32,
    alert_threshold: f64,
) -> Arc<tokio::sync::Mutex<hkask_templates::budget::BudgetTracker>> {
    use hkask_templates::bundle::config::RjouleConfig;
    let rjoule = RjouleConfig {
        cap,
        alert_threshold,
        hard_limit: true,
    };
    Arc::new(tokio::sync::Mutex::new(
        hkask_templates::budget::BudgetTracker::new(&rjoule)))
}

/// Estimate the rJoule (USD) cost of a media generation call. Pure function of
/// `unit_costs` + `params` (no env reads) so it is deterministic and testable.
/// The estimate over-counts conservatively so the hard gate trips before the
/// billable API call rather than after.
fn estimate_rjoule(
    unit_costs: &UnitCosts,
    tool: &str,
    params: &hkask_types::MediaGenerateParams,
) -> f64 {
    match tool {
        "generate_image" => unit_costs.per_image * params.count.unwrap_or(1).max(1) as f64,
        "image_to_image" => {
            // transform cost scales with strength (0.0..=1.0).
            unit_costs.per_transform * (1.0 + params.strength.unwrap_or(1.0) as f64)
        }
        "upscale" => {
            // cost ~ pixel-area growth, so scale^2.
            let scale = params.scale.unwrap_or(2).max(1) as f64;
            unit_costs.per_upscale * scale * scale
        }
        "generate_video" => {
            unit_costs.per_video_second * params.duration.unwrap_or(5.0).max(1.0) as f64
        }
        // TTS/STT/processing are billable API calls without a dedicated
        // unit-cost field. Use `per_image` as a conservative floor — it
        // over-counts (TTS/STT are typically cheaper than image gen), which
        // is the safe direction for a hard cost gate. Add dedicated fields
        // to `UnitCosts` when per-call pricing needs precision.
        "generate_speech" | "transcribe" | "remove_background" | "image_to_video" => {
            unit_costs.per_image
        }
        _ => unit_costs.per_image,
    }
}

/// Pre-charge the rJoule budget for an estimated call and enforce the hard
/// limit. Returns `Ok(())` when enforcement is disabled (`tracker` is `None`)
/// or the remaining budget covers the estimate; returns an `McpToolError`
/// when the budget is exhausted.
///
/// Check-before-charge under the mutex: a rejected request consumes no budget,
/// and concurrent bursts serialize so they cannot all pass then overspend.
/// After a successful charge, fires the threshold-crossing warning once (when
/// `used/cap >= alert_threshold`) — the enforcement point for
/// `HKASK_MEDIA_RJOULE_ALERT_THRESHOLD`. This is the enforcement point for the
/// rJoule gate; `MediaServer::charge_budget` is a thin delegate so the gate can
/// be tested without constructing a full server.
pub(super) async fn charge_budget_gate(
    budget: &MediaBudget,
    tool: &str,
    params: &hkask_types::MediaGenerateParams,
) -> Result<(), McpToolError> {
    let Some(tracker) = budget.tracker.as_ref() else {
        return Ok(()); // enforcement disabled
    };
    let estimate = estimate_rjoule(&budget.unit_costs, tool, params);
    let mut tracker = tracker.lock().await;
    let remaining = tracker.remaining_rjoule();
    if remaining < estimate {
        let snap = tracker.snapshot();
        tracing::warn!(
            target: "hkask.mcp.media.budget",
            tool = tool,
            estimate = estimate,
            rjoule_used = snap.rjoule_used,
            rjoule_cap = snap.rjoule_cap,
            rjoule_remaining = remaining,
            "rJoule budget exhausted — rejecting media call"
        );
        return Err(McpToolError::unavailable(format!(
            "rJoule budget exhausted: this call needs ~{estimate:.4} rJoule but only \
             {remaining:.4} of {cap:.4} remains. Raise HKASK_MEDIA_RJOULE_CAP to allow \
             this call.",
            cap = snap.rjoule_cap
        )));
    }
    tracker.charge_rjoule(estimate);
    let snap = tracker.snapshot();
    // Threshold-crossing warning (once per budget lifetime). Fires when the
    // charge brings used/cap to >= alert_threshold.
    if budget.alert_threshold > 0.0
        && snap.rjoule_cap > 0.0
        && (snap.rjoule_used / snap.rjoule_cap) >= budget.alert_threshold
        && !budget
            .alerted
            .swap(true, std::sync::atomic::Ordering::Relaxed)
    {
        tracing::warn!(
            target: "hkask.mcp.media.budget",
            tool = tool,
            rjoule_used = snap.rjoule_used,
            rjoule_cap = snap.rjoule_cap,
            pct = (snap.rjoule_used / snap.rjoule_cap) * 100.0,
            threshold = budget.alert_threshold,
            "rJoule budget crossed alert threshold"
        );
    }
    tracing::debug!(
        target: "hkask.mcp.media.budget",
        tool = tool,
        estimate = estimate,
        remaining = snap.rjoule_remaining,
        "charged rJoule for media call"
    );
    Ok(())
}

/// Construct the rJoule budget configuration from env vars, resolved once at
/// startup so the gate is deterministic.
///
/// `HKASK_MEDIA_RJOULE_CAP` sets the total rJoule (USD) ceiling. Unset or `0` =
/// enforcement disabled. A set-but-malformed value (e.g. `100.5`, `1e3`) is a
/// config error — the server returns an error from `run()` rather than
/// silently disabling enforcement (fail-closed on a cost-control setting: a
/// typo in the cap must not remove the spend ceiling silently). The gas
/// (compute) cap is constructed inert — gas is enforced upstream at
/// `McpRuntime::invoke` + `CyberneticsLoop`, and the media server never
/// charges gas itself, so a gas cap here would be dead config.
pub(super) fn build_media_budget() -> Result<MediaBudget, MediaError> {
    let unit_costs = UnitCosts::from_env();
    let alert_threshold = env_f64("HKASK_MEDIA_RJOULE_ALERT_THRESHOLD", 0.8).clamp(0.0, 1.0);
    match std::env::var("HKASK_MEDIA_RJOULE_CAP") {
        // Unset — legitimately disabled (the default).
        Err(_) => Ok(MediaBudget::disabled(unit_costs, alert_threshold)),
        Ok(raw) => match raw.trim().parse::<u32>() {
            // Explicit opt-out (documented: 0 = disabled).
            Ok(0) => {
                tracing::info!(
                    target: "hkask.mcp.media.budget",
                    "HKASK_MEDIA_RJOULE_CAP=0 — rJoule enforcement disabled"
                );
                Ok(MediaBudget::disabled(unit_costs, alert_threshold))
            }
            Ok(cap) => {
                tracing::info!(
                    target: "hkask.mcp.media.budget",
                    rjoule_cap = cap,
                    alert_threshold = alert_threshold,
                    "rJoule budget enforcement enabled"
                );
                Ok(MediaBudget {
                    tracker: Some(make_rjoule_tracker(cap, alert_threshold)),
                    unit_costs,
                    alert_threshold,
                    alerted: std::sync::atomic::AtomicBool::new(false),
                })
            }
            // Malformed cap — fail closed. A typo in the cap must not silently
            // remove the spend ceiling. Return an error so `run()` surfaces it
            // to the operator rather than starting with enforcement off.
            Err(_) => Err(MediaError::BudgetConfig(format!(
                "HKASK_MEDIA_RJOULE_CAP is set to '{raw}' but is not a valid u32 \
                 (expected a positive integer rJoule ceiling, e.g. 100). \
                 Fix the value or unset it to disable enforcement."
            ))),
        },
    }
}

#[cfg(test)]
mod estimate_rjoule_tests {
    use super::{UnitCosts, estimate_rjoule};
    use hkask_types::MediaGenerateParams;

    // `estimate_rjoule` is a pure function of `UnitCosts` + params — no env reads
    // — so these tests are fully env-isolated (the old version read
    // `HKASK_MEDIA_RJOULE_PER_*` per call and only passed by env accident).
    const UC: UnitCosts = UnitCosts::DEFAULT;

    #[test]
    fn generate_image_scales_with_count() {
        let one = estimate_rjoule(
            &UC,
            "generate_image",
            &MediaGenerateParams {
                count: Some(1),
                ..Default::default()
            },
        );
        let four = estimate_rjoule(
            &UC,
            "generate_image",
            &MediaGenerateParams {
                count: Some(4),
                ..Default::default()
            },
        );
        assert!((one - 0.05).abs() < 1e-9);
        assert!((four - 0.20).abs() < 1e-9);
    }

    #[test]
    fn generate_image_count_defaults_to_one_when_unset() {
        let est = estimate_rjoule(&UC, "generate_image", &MediaGenerateParams::default());
        assert!(
            (est - 0.05).abs() < 1e-9,
            "unset count should charge one image"
        );
    }

    #[test]
    fn transform_scales_with_strength() {
        let none = estimate_rjoule(&UC, "image_to_image", &MediaGenerateParams::default());
        let full = estimate_rjoule(
            &UC,
            "image_to_image",
            &MediaGenerateParams {
                strength: Some(1.0),
                ..Default::default()
            },
        );
        // no strength => 1.0 default => 0.04 * 2.0 = 0.08
        assert!((none - 0.08).abs() < 1e-9);
        assert!((full - 0.08).abs() < 1e-9);
        let half = estimate_rjoule(
            &UC,
            "image_to_image",
            &MediaGenerateParams {
                strength: Some(0.5),
                ..Default::default()
            },
        );
        assert!((half - 0.06).abs() < 1e-9); // 0.04 * 1.5
    }

    #[test]
    fn upscale_grows_quadratically_with_scale() {
        let x2 = estimate_rjoule(
            &UC,
            "upscale",
            &MediaGenerateParams {
                scale: Some(2),
                ..Default::default()
            },
        );
        let x4 = estimate_rjoule(
            &UC,
            "upscale",
            &MediaGenerateParams {
                scale: Some(4),
                ..Default::default()
            },
        );
        assert!((x2 - 0.08).abs() < 1e-9); // 0.02 * 2^2
        assert!((x4 - 0.32).abs() < 1e-9); // 0.02 * 4^2
    }

    #[test]
    fn generate_video_scales_with_duration() {
        let five = estimate_rjoule(
            &UC,
            "generate_video",
            &MediaGenerateParams {
                duration: Some(5.0),
                ..Default::default()
            },
        );
        let ten = estimate_rjoule(
            &UC,
            "generate_video",
            &MediaGenerateParams {
                duration: Some(10.0),
                ..Default::default()
            },
        );
        assert!((five - 5.0).abs() < 1e-9);
        assert!((ten - 10.0).abs() < 1e-9);
    }

    #[test]
    fn unknown_tool_charges_image_floor() {
        let est = estimate_rjoule(&UC, "mystery_op", &MediaGenerateParams::default());
        assert!((est - 0.05).abs() < 1e-9);
    }

    #[test]
    fn video_duration_clamped_to_minimum_one_second() {
        let est = estimate_rjoule(
            &UC,
            "generate_video",
            &MediaGenerateParams {
                duration: Some(0.0),
                ..Default::default()
            },
        );
        assert!(
            (est - 1.0).abs() < 1e-9,
            "zero-duration should charge 1 second"
        );
    }

    #[test]
    fn custom_unit_costs_drive_estimate() {
        // Proves the estimate is param-driven, not env-driven: a 2x image cost
        // doubles the estimate for the same params.
        let uc = UnitCosts {
            per_image: 0.10,
            ..UnitCosts::DEFAULT
        };
        let est = estimate_rjoule(
            &uc,
            "generate_image",
            &MediaGenerateParams {
                count: Some(3),
                ..Default::default()
            },
        );
        assert!((est - 0.30).abs() < 1e-9);
    }
}

#[cfg(test)]
mod charge_budget_gate_tests {
    use super::{MediaBudget, UnitCosts, charge_budget_gate, make_rjoule_tracker};
    use hkask_types::MediaGenerateParams;
    use std::sync::atomic::Ordering;

    // `charge_budget_gate` takes `&MediaBudget`; tests construct one directly (child
    // modules can access private fields) using the shared `make_rjoule_tracker` —
    // no env reads, fully isolated.
    fn gated(cap: u32, alert_threshold: f64) -> MediaBudget {
        MediaBudget {
            tracker: Some(make_rjoule_tracker(cap, alert_threshold)),
            unit_costs: UnitCosts::DEFAULT,
            alert_threshold,
            alerted: std::sync::atomic::AtomicBool::new(false),
        }
    }

    fn disabled() -> MediaBudget {
        MediaBudget::disabled(UnitCosts::DEFAULT, 0.8)
    }

    async fn remaining(budget: &MediaBudget) -> f64 {
        budget
            .tracker
            .as_ref()
            .unwrap()
            .lock()
            .await
            .remaining_rjoule()
    }

    async fn used(budget: &MediaBudget) -> f64 {
        budget.tracker.as_ref().unwrap().lock().await.rjoule_used()
    }

    #[tokio::test]
    async fn no_budget_is_open_gate() {
        // Disabled budget (tracker None) — every call passes.
        let res = charge_budget_gate(
            &disabled(),
            "generate_image",
            &MediaGenerateParams::default(),
        )
        .await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn under_budget_charges_and_passes() {
        // cap 1 rJoule; one image costs 0.05 → passes, leaves 0.95.
        let budget = gated(1, 0.8);
        let res =
            charge_budget_gate(&budget, "generate_image", &MediaGenerateParams::default()).await;
        assert!(res.is_ok());
        assert!((remaining(&budget).await - 0.95).abs() < 1e-9);
    }

    #[tokio::test]
    async fn exhausted_budget_rejects_without_charging() {
        // cap 1 rJoule; a 10-second video (cost 10.0) → rejected, and a rejected
        // call consumes no budget (check-before-charge).
        let budget = gated(1, 0.8);
        let res = charge_budget_gate(
            &budget,
            "generate_video",
            &MediaGenerateParams {
                duration: Some(10.0),
                ..Default::default()
            },
        )
        .await;
        assert!(res.is_err(), "over-budget call must be rejected");
        assert!(
            (used(&budget).await - 0.0).abs() < 1e-9,
            "rejected call must not consume budget"
        );
        assert!(
            (remaining(&budget).await - 1.0).abs() < 1e-9,
            "full budget remains after rejection"
        );
    }

    #[tokio::test]
    async fn successive_calls_drain_then_reject() {
        // cap 5 rJoule; four 1-second videos (4.0) pass, leaving 1.0.
        let budget = gated(5, 0.8);
        let params = MediaGenerateParams {
            duration: Some(1.0),
            ..Default::default()
        };
        for _ in 0..4 {
            charge_budget_gate(&budget, "generate_video", &params)
                .await
                .unwrap();
        }
        assert!((remaining(&budget).await - 1.0).abs() < 1e-9);
        // A 2-second call needs 2.0 > remaining 1.0 → rejected.
        let over = MediaGenerateParams {
            duration: Some(2.0),
            ..Default::default()
        };
        assert!(
            charge_budget_gate(&budget, "generate_video", &over)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn threshold_warning_fires_once_when_crossed() {
        // cap 10, alert 0.8 → a 9-rJoule charge (90% >= 80%) sets the alert
        // flag once. Pins the HKASK_MEDIA_RJOULE_ALERT_THRESHOLD enforcement (F3).
        let budget = gated(10, 0.8);
        charge_budget_gate(
            &budget,
            "generate_video",
            &MediaGenerateParams {
                duration: Some(9.0),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(
            budget.alerted.load(Ordering::Relaxed),
            "alert flag set after crossing 80%"
        );
    }

    #[tokio::test]
    async fn below_threshold_does_not_fire() {
        // cap 10, alert 0.8 → a 5-rJoule charge (50%) does not set the flag.
        let budget = gated(10, 0.8);
        charge_budget_gate(
            &budget,
            "generate_video",
            &MediaGenerateParams {
                duration: Some(5.0),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(
            !budget.alerted.load(Ordering::Relaxed),
            "alert flag not set below 80%"
        );
    }
}
