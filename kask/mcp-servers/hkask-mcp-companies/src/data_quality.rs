//! Data quality annotation layer for financial data.
//!
//! Addresses FinGPT's core insight about the low signal-to-noise ratio
//! (SNR) in financial data (§3.2 Real-Time Data Curation Pipeline) by
//! providing staleness markers, confidence scoring, outlier detection,
//! and cyclicality flags on normalized and aggregated financial metrics.
//!
//! This is a data engineering layer — it annotates, not transforms.
//! The core contract "return FIBO-anchored financial data" is unchanged;
//! this module tells consumers how much to trust that data.

use serde::Serialize;

// ── Signal quality ───────────────────────────────────────────────────────────

/// Quality assessment for a single financial metric.
///
/// Computed from multi-period historical data. Each quality dimension
/// corresponds to a known failure mode in financial data:
///
/// | Dimension      | Failure Mode                      | FinGPT Source  |
/// |----------------|-----------------------------------|----------------|
/// | CV             | Erratic metrics (cyclical, one-off)| §3.2 Low SNR   |
/// | Data points    | Insufficient history for inference | §5.1 Dataset   |
/// | Outliers       | One-time charges, M&A distortion   | §3.2 Cleaning  |
/// | Cyclicality    | Mean-reverting margins             | §4.1 Dynamism  |
/// | Staleness      | Temporal sensitivity               | §3.2 Real-Time |
#[derive(Debug, Clone, Serialize)]
pub struct SignalQuality {
    /// Coefficient of variation (σ / |μ|) across available periods.
    /// 0.0 = perfectly stable; > 0.5 = highly volatile.
    pub coefficient_of_variation: f64,
    /// Number of data points used.
    pub data_points: usize,
    /// Whether any extreme outliers (>2σ from mean) were detected.
    pub has_outliers: bool,
    /// Whether the metric shows cyclical mean-reversion.
    pub is_cyclical: bool,
    /// Data staleness: days since the most recent observation.
    /// None if the observation date can't be determined.
    pub staleness_days: Option<u32>,
    /// Overall confidence in the metric's reliability (0.0–1.0).
    /// Penalized by high CV, few points, outliers, cyclicality, staleness.
    pub confidence: f64,
    /// Human-readable explanation for confidence penalty, if any.
    pub confidence_note: Option<String>,
}

impl SignalQuality {
    /// No data: confidence is zero.
    pub fn empty() -> Self {
        SignalQuality {
            coefficient_of_variation: 0.0,
            data_points: 0,
            has_outliers: false,
            is_cyclical: false,
            staleness_days: None,
            confidence: 0.0,
            confidence_note: Some("no data points available".into()),
        }
    }

    /// Compute quality from a series of historical values.
    pub fn from_series(values: &[f64]) -> Self {
        Self::from_series_with_staleness(values, None)
    }

    /// Compute quality from a series with a known staleness in days.
    pub fn from_series_with_staleness(values: &[f64], staleness_days: Option<u32>) -> Self {
        let n = values.len();
        if n == 0 {
            return SignalQuality::empty();
        }
        // All-zero series = missing/uninitialized data, not a stable signal
        if values.iter().all(|&v| v == 0.0) {
            return SignalQuality {
                coefficient_of_variation: 0.0,
                data_points: n,
                has_outliers: false,
                is_cyclical: false,
                staleness_days,
                confidence: 0.0,
                confidence_note: Some("all values are zero — likely missing data".into()),
            };
        }

        let mean = values.iter().sum::<f64>() / n as f64;
        let variance = if n > 1 {
            values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1) as f64
        } else {
            0.0
        };
        let std_dev = variance.sqrt();
        let cv = if mean.abs() > 1e-10 {
            std_dev / mean.abs()
        } else if std_dev > 1e-10 {
            f64::INFINITY
        } else {
            0.0
        };

        // Outlier detection: values beyond 2σ from the mean.
        let has_outliers = n >= 3
            && values
                .iter()
                .any(|&v| (v - mean).abs() > 2.0 * std_dev.max(1e-10));

        // Cyclicality: alternating year-over-year direction changes.
        let is_cyclical = if n >= 4 {
            let diffs: Vec<f64> = values.windows(2).map(|w| w[1] - w[0]).collect();
            let sign_changes = diffs
                .windows(2)
                .filter(|w| w[0].signum() != w[1].signum() && w[0] != 0.0 && w[1] != 0.0)
                .count();
            sign_changes as f64 >= (n - 2) as f64 * 0.4
        } else {
            false
        };

        // Confidence: penalised by each quality dimension.
        let mut confidence: f64 = 1.0;
        let mut notes: Vec<String> = Vec::new();

        if n < 3 {
            let factor = if n == 1 { 0.4 } else { 0.7 };
            confidence *= factor;
            notes.push(format!("only {n} data points"));
        }
        if cv > 0.5 {
            confidence *= 0.5;
            notes.push(format!("high volatility (CV={cv:.2})"));
        } else if cv > 0.3 {
            confidence *= 0.7;
            notes.push(format!("moderate volatility (CV={cv:.2})"));
        }
        if has_outliers {
            confidence *= 0.8;
            notes.push("contains outliers".into());
        }
        if is_cyclical {
            confidence *= 0.9;
            notes.push("cyclical pattern detected".into());
        }
        if let Some(days) = staleness_days {
            if days > 365 {
                confidence *= 0.5;
                notes.push(format!("stale: {days} days old"));
            } else if days > 90 {
                confidence *= 0.7;
                notes.push(format!("aging: {days} days old"));
            } else if days > 30 {
                confidence *= 0.9;
                notes.push(format!("slightly stale: {days} days old"));
            }
        }

        // Clamp to [0, 1] after all penalties.
        confidence = confidence.clamp(0.0_f64, 1.0_f64);

        let confidence_note = if notes.is_empty() {
            None
        } else {
            Some(notes.join("; "))
        };

        SignalQuality {
            coefficient_of_variation: cv,
            data_points: n,
            has_outliers,
            is_cyclical,
            staleness_days,
            confidence,
            confidence_note,
        }
    }

    /// Compute quality for a metric as a ratio of two series (e.g., margin = profit / revenue).
    /// Undefined ratios (den ≈ 0) produce NaN values which reduce confidence to zero.
    pub fn from_ratio_series(
        numerators: &[f64],
        denominators: &[f64],
        staleness_days: Option<u32>,
    ) -> Self {
        let n = numerators.len().min(denominators.len());
        if n == 0 {
            return SignalQuality::empty();
        }
        let ratios: Vec<f64> = numerators
            .iter()
            .zip(denominators.iter())
            .take(n)
            .map(|(num, den)| {
                if den.abs() > 1e-10 {
                    num / den
                } else {
                    f64::NAN
                }
            })
            .collect();
        // If any ratio is NaN, flag immediately rather than computing bogus statistics
        if ratios.iter().any(|r| r.is_nan()) {
            return SignalQuality {
                coefficient_of_variation: 0.0,
                data_points: n,
                has_outliers: false,
                is_cyclical: false,
                staleness_days,
                confidence: 0.0,
                confidence_note: Some("undefined ratios — denominator near zero".into()),
            };
        }
        Self::from_series_with_staleness(&ratios, staleness_days)
    }
}

// ── Data quality for the entire historical snapshot ──────────────────────────

/// Aggregate quality assessment across the 11-line-item financial model.
///
/// Each field is a SignalQuality for one key driver. This is what gets
/// attached to DCF, scenario, and sensitivity outputs so consumers can
/// gauge how reliable the projections are.
#[derive(Debug, Clone, Serialize)]
pub struct ModelInputQuality {
    /// Revenue growth rate quality.
    pub revenue_growth: SignalQuality,
    /// Gross margin quality.
    pub gross_margin: SignalQuality,
    /// D&A-to-revenue quality.
    pub da_to_revenue: SignalQuality,
    /// Capex-to-revenue quality.
    pub capex_to_revenue: SignalQuality,
    /// NWC-to-revenue quality.
    pub nwc_to_revenue: SignalQuality,
    /// Tax rate quality.
    pub tax_rate: SignalQuality,
    /// Overall model confidence: geometric mean of all driver confidences.
    pub overall_confidence: f64,
    /// Summary of the worst quality issue, if any.
    pub quality_warning: Option<String>,
}

impl ModelInputQuality {
    /// Build from multi-year financial statement data.
    #[allow(clippy::too_many_arguments)]
    pub fn from_historical_series(
        revenue: &[f64],
        cogs: &[f64],
        da: &[f64],
        capex: &[f64],
        current_assets: &[f64],
        current_liabilities: &[f64],
        cash: &[f64],
        tax_expenses: &[f64],
        pre_tax_incomes: &[f64],
        staleness_days: Option<u32>,
    ) -> Self {
        // Validate all series have the same length
        if revenue.is_empty() {
            return ModelInputQuality {
                revenue_growth: SignalQuality::empty(),
                gross_margin: SignalQuality::empty(),
                da_to_revenue: SignalQuality::empty(),
                capex_to_revenue: SignalQuality::empty(),
                nwc_to_revenue: SignalQuality::empty(),
                tax_rate: SignalQuality::empty(),
                overall_confidence: 0.0,
                quality_warning: Some("no historical revenue data available".into()),
            };
        }
        // Growth rates: YoY revenue growth
        let growth_rates: Vec<f64> = revenue
            .windows(2)
            .filter_map(|w| {
                if w[0] > 0.0 {
                    Some((w[1] - w[0]) / w[0])
                } else {
                    None
                }
            })
            .collect();
        let revenue_growth =
            SignalQuality::from_series_with_staleness(&growth_rates, staleness_days);

        // Gross margin per year
        let gross_margin = SignalQuality::from_ratio_series(
            &revenue
                .iter()
                .zip(cogs.iter())
                .map(|(r, c)| r - c)
                .collect::<Vec<f64>>(),
            revenue,
            staleness_days,
        );

        let da_to_rev = SignalQuality::from_ratio_series(da, revenue, staleness_days);
        let capex_to_rev = SignalQuality::from_ratio_series(capex, revenue, staleness_days);

        // NWC = CA - CL - Cash
        let nwc_series: Vec<f64> = current_assets
            .iter()
            .zip(current_liabilities.iter())
            .zip(cash.iter())
            .map(|((ca, cl), ch)| ca - cl - ch)
            .collect();
        let nwc_to_rev = SignalQuality::from_ratio_series(&nwc_series, revenue, staleness_days);

        // Tax rate per year
        let tax_rate =
            SignalQuality::from_ratio_series(tax_expenses, pre_tax_incomes, staleness_days);

        let overall_confidence = (revenue_growth.confidence
            * gross_margin.confidence
            * da_to_rev.confidence
            * capex_to_rev.confidence
            * nwc_to_rev.confidence
            * tax_rate.confidence)
            .powf(1.0 / 6.0);

        // Build warning for worst quality dimension
        let quality_warning = {
            let dims: [(&str, &SignalQuality); 6] = [
                ("revenue_growth", &revenue_growth),
                ("gross_margin", &gross_margin),
                ("da_to_revenue", &da_to_rev),
                ("capex_to_revenue", &capex_to_rev),
                ("nwc_to_revenue", &nwc_to_rev),
                ("tax_rate", &tax_rate),
            ];
            let worst = dims
                .iter()
                .min_by(|a, b| {
                    a.1.confidence
                        .partial_cmp(&b.1.confidence)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .unwrap();
            if worst.1.confidence < 0.6 {
                Some(format!(
                    "Low confidence in {} (confidence={:.2}): {}",
                    worst.0,
                    worst.1.confidence,
                    worst.1.confidence_note.as_deref().unwrap_or("")
                ))
            } else {
                None
            }
        };

        ModelInputQuality {
            revenue_growth,
            gross_margin,
            da_to_revenue: da_to_rev,
            capex_to_revenue: capex_to_rev,
            nwc_to_revenue: nwc_to_rev,
            tax_rate,
            overall_confidence,
            quality_warning,
        }
    }
}

// ── Provider-specific quality annotations ────────────────────────────────────

/// Annotation on a normalized field indicating how it was derived.
#[derive(Debug, Clone, Serialize)]
pub struct NormalizationAnnotation {
    /// The original provider field name.
    pub source_field: String,
    /// Whether this field was approximated during normalization.
    pub was_approximated: bool,
    /// What was done to normalise it (empty if exact match).
    pub method: String,
}

/// Provider quality metadata attached to API responses.
#[derive(Debug, Clone, Serialize)]
pub struct ProviderQuality {
    /// Which provider served this data.
    pub provider: String,
    /// Fields that required normalization/approximation.
    pub approximated_fields: Vec<NormalizationAnnotation>,
    /// Overall provider confidence for this response type.
    pub provider_confidence: f64,
}

impl ProviderQuality {
    pub fn exact_match(provider: &str) -> Self {
        ProviderQuality {
            provider: provider.to_string(),
            approximated_fields: Vec::new(),
            provider_confidence: 1.0,
        }
    }

    pub fn with_approximations(provider: &str, approximated: Vec<NormalizationAnnotation>) -> Self {
        let confidence = if approximated.is_empty() {
            1.0
        } else {
            // Each approximation costs ~5% confidence, floor at 0.5
            (1.0 - 0.05 * approximated.len() as f64).max(0.5)
        };
        ProviderQuality {
            provider: provider.to_string(),
            approximated_fields: approximated,
            provider_confidence: confidence,
        }
    }
}

// ── Temporal coherence tracking ──────────────────────────────────────────────

/// Record of data freshness for temporal coherence learning (FinGPT RLSP-inspired).
///
/// When we fetch data for a symbol at time T, we can later check (at T+Δ)
/// whether subsequent price movements suggest the data was stale. This is
/// the objective market signal that FinGPT's RLSP uses — applied here to
/// data quality, not sentiment classification.
#[derive(Debug, Clone)]
pub struct TemporalSnapshot {
    /// When the data was fetched (RFC 3339).
    pub fetched_at: String,
    /// Stock price at fetch time.
    pub price_at_fetch: f64,
    /// Earnings announcement date for the most recent period in the data.
    pub latest_filing_date: Option<String>,
}

impl TemporalSnapshot {
    /// Compute staleness: days between fetch and the latest filing.
    /// If the filing date is unknown, returns None.
    pub fn staleness_days(&self, now: &chrono::DateTime<chrono::Utc>) -> Option<u32> {
        self.latest_filing_date.as_ref().and_then(|d| {
            chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
                .ok()
                .and_then(|filing_date| {
                    let filing_dt = filing_date
                        .and_hms_opt(0, 0, 0)
                        .and_then(|dt| dt.and_utc().into());
                    filing_dt.map(|dt: chrono::DateTime<chrono::Utc>| {
                        let duration = now.signed_duration_since(dt);
                        duration.num_days().max(0) as u32
                    })
                })
        })
    }
}

// ── Regulation span emission helpers ────────────────────────────────────────────────

/// Emit a Regulation data_quality span so hKask's homeostatic loop can monitor
/// financial data reliability (closes the variety deficit identified in
/// cybernetic analysis: 6 disturbance modes, previously only 2 monitored).
pub fn emit_data_quality_span(symbol: &str, tool: &str, quality: &ModelInputQuality) {
    tracing::debug!(
        target: "hkask.mcp.companies.data_quality",
        symbol = %symbol,
        tool = %tool,
        overall_confidence = %quality.overall_confidence,
        revenue_growth_confidence = %quality.revenue_growth.confidence,
        gross_margin_confidence = %quality.gross_margin.confidence,
        has_outliers = %quality.revenue_growth.has_outliers,
        is_cyclical = %quality.revenue_growth.is_cyclical,
        data_points = %quality.revenue_growth.data_points,
        quality_warning = %quality.quality_warning.as_deref().unwrap_or("none"),
        "Financial data quality assessment for Regulation variety monitoring"
    );
}

/// Emit a Regulation span for provider quality (normalization fidelity).
pub fn emit_provider_quality_span(symbol: &str, tool: &str, pq: &ProviderQuality) {
    tracing::debug!(
        target: "hkask.mcp.companies.data_quality",
        symbol = %symbol,
        tool = %tool,
        provider = %pq.provider,
        provider_confidence = %pq.provider_confidence,
        approximated_count = %pq.approximated_fields.len(),
        "Provider data quality: normalization fidelity assessment"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_quality_empty() {
        let q = SignalQuality::from_series(&[]);
        assert_eq!(q.data_points, 0);
        assert_eq!(q.confidence, 0.0);
        assert!(q.confidence_note.is_some());
    }

    #[test]
    fn signal_quality_stable() {
        // Perfectly stable: all values identical
        let q = SignalQuality::from_series(&[10.0, 10.0, 10.0, 10.0, 10.0]);
        assert_eq!(q.data_points, 5);
        assert!((q.coefficient_of_variation - 0.0).abs() < 1e-10);
        assert!(!q.has_outliers);
        assert!(!q.is_cyclical);
        assert!(q.confidence > 0.9);
        assert!(q.confidence_note.is_none());
    }

    #[test]
    fn signal_quality_volatile() {
        // High variance
        let q = SignalQuality::from_series(&[5.0, 50.0, 2.0, 40.0, 8.0]);
        assert_eq!(q.data_points, 5);
        assert!(q.coefficient_of_variation > 0.5);
        assert!(q.confidence < 0.7);
        assert!(
            q.confidence_note
                .as_ref()
                .unwrap()
                .contains("high volatility")
        );
    }

    #[test]
    fn signal_quality_few_points() {
        let q = SignalQuality::from_series(&[10.0]);
        assert_eq!(q.data_points, 1);
        assert!(q.confidence < 0.5);
        assert!(
            q.confidence_note
                .as_ref()
                .unwrap()
                .contains("only 1 data points")
        );
    }

    #[test]
    fn signal_quality_outlier_detection() {
        // One extreme outlier among many consistent values
        let q = SignalQuality::from_series(&[
            10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 50.0,
        ]);
        assert!(q.has_outliers);
        assert!(
            q.confidence_note
                .as_ref()
                .unwrap()
                .contains("contains outliers")
        );
    }

    #[test]
    fn signal_quality_cyclical() {
        // Alternating pattern
        let q = SignalQuality::from_series(&[10.0, 15.0, 10.0, 15.0, 10.0, 15.0]);
        assert!(q.is_cyclical);
    }

    #[test]
    fn signal_quality_staleness_penalty() {
        let q = SignalQuality::from_series_with_staleness(&[10.0, 12.0, 11.0], Some(400));
        assert!(q.staleness_days == Some(400));
        assert!(q.confidence < 0.6);
        assert!(q.confidence_note.as_ref().unwrap().contains("stale"));
    }

    #[test]
    fn signal_quality_aging_penalty() {
        let q = SignalQuality::from_series_with_staleness(&[10.0, 12.0, 11.0], Some(120));
        assert!(q.confidence < 0.8);
        assert!(q.confidence_note.as_ref().unwrap().contains("aging"));
    }

    #[test]
    fn model_input_quality_computes_all_dimensions() {
        let revenue = [80_000.0, 90_000.0, 100_000.0];
        let cogs = [48_000.0, 54_000.0, 60_000.0];
        let da = [3_000.0, 3_200.0, 3_500.0];
        let capex = [2_500.0, 2_800.0, 3_000.0];
        let ca = [50_000.0, 50_000.0, 50_000.0];
        let cl = [30_000.0, 30_000.0, 30_000.0];
        let cash = [10_000.0, 10_000.0, 10_000.0];
        let tax_exp = [5_000.0, 4_500.0, 5_000.0];
        let pre_tax = [20_000.0, 18_000.0, 20_000.0];

        let q = ModelInputQuality::from_historical_series(
            &revenue, &cogs, &da, &capex, &ca, &cl, &cash, &tax_exp, &pre_tax, None,
        );
        assert!(q.overall_confidence > 0.0);
        assert!(q.overall_confidence <= 1.0);
        assert!(q.revenue_growth.data_points > 0);
        assert!(q.gross_margin.data_points > 0);
    }

    #[test]
    fn model_input_quality_warns_on_low_confidence() {
        // Only 1 data point — should trigger a warning
        let revenue = [100_000.0];
        let cogs = [60_000.0];
        let da = [3_000.0];
        let capex = [2_500.0];
        let ca = [50_000.0];
        let cl = [30_000.0];
        let cash = [10_000.0];
        let tax_exp = [5_000.0];
        let pre_tax = [20_000.0];

        let q = ModelInputQuality::from_historical_series(
            &revenue, &cogs, &da, &capex, &ca, &cl, &cash, &tax_exp, &pre_tax, None,
        );
        assert!(q.overall_confidence < 0.6);
        assert!(q.quality_warning.is_some());
    }

    #[test]
    fn provider_quality_exact() {
        let pq = ProviderQuality::exact_match("FMP");
        assert_eq!(pq.provider_confidence, 1.0);
        assert!(pq.approximated_fields.is_empty());
    }

    #[test]
    fn provider_quality_with_approximations() {
        let pq = ProviderQuality::with_approximations(
            "EODHD",
            vec![
                NormalizationAnnotation {
                    source_field: "general.roe".into(),
                    was_approximated: true,
                    method: "derived from NetIncome / TotalEquity".into(),
                },
                NormalizationAnnotation {
                    source_field: "highlights.pe".into(),
                    was_approximated: true,
                    method: "mapped from MarketCapitalization / NetIncome".into(),
                },
            ],
        );
        assert!(pq.provider_confidence < 1.0);
        assert!(pq.provider_confidence >= 0.5);
        assert_eq!(pq.approximated_fields.len(), 2);
    }

    #[test]
    fn signal_quality_all_zero_is_flagged() {
        let q = SignalQuality::from_series(&[0.0, 0.0, 0.0, 0.0]);
        assert_eq!(q.data_points, 4);
        assert_eq!(q.confidence, 0.0);
        assert!(
            q.confidence_note
                .as_ref()
                .unwrap()
                .contains("all values are zero")
        );
    }

    #[test]
    fn from_ratio_series_nan_denominator() {
        let numerators = [10.0, 20.0, 30.0];
        let denominators = [100.0, 0.0, 200.0];
        let q = SignalQuality::from_ratio_series(&numerators, &denominators, None);
        assert_eq!(q.confidence, 0.0);
        assert!(
            q.confidence_note
                .as_ref()
                .unwrap()
                .contains("undefined ratios")
        );
    }

    #[test]
    fn from_historical_series_empty_revenue() {
        let q = ModelInputQuality::from_historical_series(
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            None,
        );
        assert_eq!(q.overall_confidence, 0.0);
        assert!(
            q.quality_warning
                .as_ref()
                .unwrap()
                .contains("no historical revenue")
        );
    }
}
