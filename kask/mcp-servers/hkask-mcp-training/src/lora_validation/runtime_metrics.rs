//! Runtime-metrics validation (G-R1) — validates runtime training metrics
//! (loss spikes, NaN gradients, vanishing loss, training stalls) against the
//! HuggingFace trackio alert pattern. Produces `ValidationFinding`s with
//! `evidence_kind: runtime_measurement`.
//!
//! Anchored to: trackio alert API, QLoRA paper §3 (training stability), Razin
//! et al. (arXiv:2410.21228).

use super::{ValidationFinding, ValidationSeverity};
pub fn validate_runtime_metrics(
    manifest: &crate::huggingface::CompletionManifest,
) -> Vec<ValidationFinding> {
    let mut findings = Vec::new();

    // Process explicit alerts first — these are operator/runtime-supplied signals.
    for alert in &manifest.alerts {
        let severity = match alert.level.as_str() {
            "error" | "critical" | "fatal" => ValidationSeverity::Refuse,
            "warn" => ValidationSeverity::Warn,
            // Unknown non-empty levels default to Warn (safer than Info —
            // surfaces the alert rather than silently downgrading it).
            _ => ValidationSeverity::Warn,
        };
        let step_str = alert
            .step
            .map(|s| format!(" at step {s}"))
            .unwrap_or_default();
        findings.push(ValidationFinding {
            gate_id: "G-R1",
            severity,
            message: format!("{}{}: {}", alert.title, step_str, alert.text),
            source: "Runtime alert (trackio.alert pattern) — huggingface-trackio skill §Alerts",
            remediation:
                "Investigate the alert condition and adjust hyperparameters or stop the run"
                    .to_string(),
        });
    }

    // Detect NaN or infinite loss — training has diverged.
    // NaN comparisons are always false in Rust, so `loss > 5.0` would silently
    // pass. We check is_nan()/is_infinite() explicitly before the divergence
    // and vanishing checks.
    if let Some(loss) = manifest.loss {
        if loss.is_nan() {
            findings.push(ValidationFinding {
                gate_id: "G-R1",
                severity: ValidationSeverity::Refuse,
                message: "NaN loss detected — training has diverged".to_string(),
                source: "QLoRA paper §3 (training stability); IEEE-754 NaN semantics",
                remediation: "Stop the run, reduce learning rate, enable gradient clipping, or check for numerical instability".to_string(),
            });
        } else if loss.is_infinite() {
            findings.push(ValidationFinding {
                gate_id: "G-R1",
                severity: ValidationSeverity::Refuse,
                message: format!("Infinite loss ({}) — training has diverged", loss),
                source: "QLoRA paper §3 (training stability)",
                remediation: "Stop the run, reduce learning rate, or enable gradient clipping"
                    .to_string(),
            });
        }
    }

    // Detect loss divergence: loss > 5.0 after step 100.
    // Only reached if loss is finite (NaN/inf handled above).
    if let (Some(step), Some(loss)) = (manifest.current_step, manifest.loss)
        && loss.is_finite()
        && step > 100
        && loss > 5.0
    {
        findings.push(ValidationFinding {
            gate_id: "G-R1",
            severity: ValidationSeverity::Refuse,
            message: format!(
                "Loss divergence: loss {:.4} still high after {} steps",
                loss, step
            ),
            source: "trackio.alert pattern — huggingface-trackio skill §Autonomous ML Experiment Workflow",
            remediation: "Stop the run, reduce learning rate, or check for dataset/label errors".to_string(),
        });
    }

    // Detect vanishing loss: |loss| < 1e-8 after step 0.
    // Only reached if loss is finite.
    if let (Some(step), Some(loss)) = (manifest.current_step, manifest.loss)
        && loss.is_finite()
        && step > 0
        && loss.abs() < 1e-8
    {
        findings.push(ValidationFinding {
            gate_id: "G-R1",
            severity: ValidationSeverity::Warn,
            message: format!(
                "Vanishing loss: loss {:.2e} near zero at step {} — possible gradient collapse",
                loss, step
            ),
            source: "trackio.alert pattern — huggingface-trackio skill §Alerts",
            remediation: "Check gradient flow, learning rate, and dataset labels".to_string(),
        });
    }

    // Detect NaN gradient norm.
    if let Some(grad_norm) = manifest.grad_norm {
        if grad_norm.is_nan() {
            findings.push(ValidationFinding {
                gate_id: "G-R1",
                severity: ValidationSeverity::Refuse,
                message: "NaN gradient norm detected — training has diverged".to_string(),
                source: "QLoRA paper §3 (training stability); trackio alert pattern",
                remediation: "Stop the run, reduce learning rate, enable gradient clipping, or check for numerical instability".to_string(),
            });
        } else if grad_norm.is_infinite() {
            findings.push(ValidationFinding {
                gate_id: "G-R1",
                severity: ValidationSeverity::Refuse,
                message: format!(
                    "Infinite gradient norm ({}) — training has diverged",
                    grad_norm
                ),
                source: "QLoRA paper §3 (training stability)",
                remediation: "Stop the run, reduce learning rate, or enable gradient clipping"
                    .to_string(),
            });
        }
    }

    findings
}
