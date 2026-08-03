//! Deterministic compute primitives — the `compute` step dispatch surface.
//!
//! Extracted from the executor (continues the budget.rs / convergence.rs
//! extraction pattern). `dispatch_compute` maps a `compute_ref` string to a
//! canonical `hkask_forecast` / `hkask_lisp` primitive with no LLM round-trip.
//! The Kata convergence primitives (object/process gap, hypotenuse, Brier,
//! convergence_check) live here too — they replace the old LLM self-grade
//! convergence templates that caused 30s timeouts across 12+ skills.

use crate::ports::{Result, TemplateError};
use serde_json::Value;

/// Dispatch a `compute_ref` string to the matching `hkask_forecast` primitive.
///
/// The `input` JSON object carries the function's arguments, bound from prior
/// step results by `execute_compute`. Returns the function's result as a JSON
/// value consumable by downstream steps.
///
/// Supported `compute_ref` values (must match the conformance contract in
/// `registry/templates/superforecasting/README.md`):
/// - `calibrate_from_fermi` — in: `{questions: [{question, estimate, confidence}, ...]}`
/// - `outside_view_adjustment` — in: `{base_rate, inside_estimate, reference_count}`
/// - `bayesian_update` — in: `{prior, evidence_likelihood, evidence_base_rate}`
/// - `apply_calibration_adjustment` — in: `{prior, overconfidence_bias}`
/// - `brier_score` — in: `{probability, outcome_occurred}`
/// - `brier_score_multi` — in: `{probabilities: [f64], outcomes: [bool]}`
/// - `brier_interpretation` — in: `{score}`
pub(crate) fn dispatch_compute(compute_ref: &str, input: &Value) -> Result<Value> {
    use hkask_forecast as forecast;
    let get_f64 = |key: &str| -> Result<f64> {
        input.get(key).and_then(|v| v.as_f64()).ok_or_else(|| {
            TemplateError::Manifest(format!(
                "compute '{}': missing or non-numeric input '{}'",
                compute_ref, key
            ))
        })
    };
    let get_bool = |key: &str| -> Result<bool> {
        input.get(key).and_then(|v| v.as_bool()).ok_or_else(|| {
            TemplateError::Manifest(format!(
                "compute '{}': missing or non-boolean input '{}'",
                compute_ref, key
            ))
        })
    };
    let get_u64 = |key: &str| -> Result<u64> {
        input.get(key).and_then(|v| v.as_u64()).ok_or_else(|| {
            TemplateError::Manifest(format!(
                "compute '{}': missing or non-integer input '{}'",
                compute_ref, key
            ))
        })
    };

    match compute_ref {
        "calibrate_from_fermi" => {
            let questions = input
                .get("questions")
                .and_then(|v| v.as_array())
                .ok_or_else(|| {
                    TemplateError::Manifest(
                        "compute 'calibrate_from_fermi': missing 'questions' array".into(),
                    )
                })?;
            let fqs: Vec<forecast::FermiQuestion> = questions
                .iter()
                .map(|q| forecast::FermiQuestion {
                    question: q
                        .get("question")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    estimate: q.get("estimate").and_then(|v| v.as_f64()).unwrap_or(0.5),
                    confidence: q.get("confidence").and_then(|v| v.as_f64()).unwrap_or(0.5),
                })
                .collect();
            let calibrated = forecast::calibrate_from_fermi(&fqs)
                .map_err(|e| TemplateError::Manifest(format!("calibrate_from_fermi: {e}")))?;
            Ok(serde_json::json!({ "calibrated": calibrated }))
        }
        "outside_view_adjustment" => {
            let base_rate = get_f64("base_rate")?;
            let inside_estimate = get_f64("inside_estimate")?;
            let reference_count = get_u64("reference_count")?;
            let (calibrated, confidence) =
                forecast::outside_view_adjustment(base_rate, inside_estimate, reference_count);
            Ok(serde_json::json!({ "calibrated": calibrated, "confidence": confidence }))
        }
        "bayesian_update" => {
            let prior = get_f64("prior")?;
            let likelihood = get_f64("evidence_likelihood")?;
            let base_rate = get_f64("evidence_base_rate")?;
            let posterior = forecast::bayesian_update(prior, likelihood, base_rate);
            Ok(serde_json::json!({ "posterior": posterior }))
        }
        "apply_calibration_adjustment" => {
            let prior = get_f64("prior")?;
            let bias = get_f64("overconfidence_bias")?;
            let adjusted = forecast::apply_calibration_adjustment(prior, bias);
            Ok(serde_json::json!({ "adjusted": adjusted }))
        }
        "brier_score" => {
            let probability = get_f64("probability")?;
            let occurred = get_bool("outcome_occurred")?;
            let score = forecast::brier_score(probability, occurred);
            Ok(serde_json::json!({ "score": score }))
        }
        "brier_score_multi" => {
            let probabilities = input
                .get("probabilities")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.iter().map(|v| v.as_f64()).collect::<Option<Vec<f64>>>())
                .ok_or_else(|| {
                    TemplateError::Manifest(
                        "compute 'brier_score_multi': missing 'probabilities' f64 array".into(),
                    )
                })?;
            let outcomes = input
                .get("outcomes")
                .and_then(|v| v.as_array())
                .and_then(|arr| {
                    arr.iter()
                        .map(|v| v.as_bool())
                        .collect::<Option<Vec<bool>>>()
                })
                .ok_or_else(|| {
                    TemplateError::Manifest(
                        "compute 'brier_score_multi': missing 'outcomes' bool array".into(),
                    )
                })?;
            let score = forecast::brier_score_multi(&probabilities, &outcomes)
                .map_err(|e| TemplateError::Manifest(format!("brier_score_multi: {e}")))?;
            Ok(serde_json::json!({ "score": score }))
        }
        "brier_interpretation" => {
            let score = get_f64("score")?;
            Ok(serde_json::json!({ "interpretation": forecast::brier_interpretation(score) }))
        }
        // ── Kata convergence primitives ──
        //
        // These implement the Improvement Kata convergence model: the agent has
        // a target condition and a current condition, measured in two orthogonal
        // spaces (Dublin Core object space + PKO process space). The total
        // distance is the hypotenuse of the right triangle formed by the two
        // gaps. Each PDCA cycle produces a prediction (with confidence) and a
        // result; the Brier score tracks prediction calibration.
        //
        // These are deterministic `compute` steps — no inference, no timeout.
        // They replace the old LLM self-grade convergence-check templates that
        // caused the 30s timeouts across 12+ skills.
        //
        // Distance functions start with edge-counting (simplest well-defined
        // measure) and iterate based on Brier feedback. If the Brier score
        // converges, the distance function is good enough; if not, escalate to
        // information-content-weighted measures (Resnik/Lin).

        // Object-space gap (Dublin Core): artifact completeness.
        // Counts missing fields and ungrounded fields in the current artifacts
        // vs the target spec. Normalized to [0, 1].
        "kata.object_gap" => {
            let current = input.get("current_artifacts").ok_or_else(|| {
                TemplateError::Manifest(
                    "compute 'kata.object_gap': missing 'current_artifacts'".into(),
                )
            })?;
            let target = input.get("target_artifacts").ok_or_else(|| {
                TemplateError::Manifest(
                    "compute 'kata.object_gap': missing 'target_artifacts'".into(),
                )
            })?;
            let (gap, missing, ungrounded) = compute_object_gap(current, target);
            Ok(serde_json::json!({
                "object_gap": gap,
                "missing_fields": missing,
                "ungrounded_fields": ungrounded,
            }))
        }
        // Process-space gap (PKO): procedure progress.
        // Counts incomplete steps in the current procedure vs the target spec.
        // Steps in-progress are half-weighted. Normalized to [0, 1].
        "kata.process_gap" => {
            let current = input.get("current_procedure").ok_or_else(|| {
                TemplateError::Manifest(
                    "compute 'kata.process_gap': missing 'current_procedure'".into(),
                )
            })?;
            let target = input.get("target_procedure").ok_or_else(|| {
                TemplateError::Manifest(
                    "compute 'kata.process_gap': missing 'target_procedure'".into(),
                )
            })?;
            let (gap, incomplete) = compute_process_gap(current, target);
            Ok(serde_json::json!({
                "process_gap": gap,
                "incomplete_steps": incomplete,
            }))
        }
        // Hypotenuse: sqrt(object_gap² + process_gap²).
        // The total distance to the target in the combined object-process space.
        "kata.hypotenuse" => {
            let object_gap = get_f64("object_gap")?;
            let process_gap = get_f64("process_gap")?;
            let hypotenuse = (object_gap * object_gap + process_gap * process_gap).sqrt();
            Ok(serde_json::json!({
                "hypotenuse": hypotenuse,
                "object_gap": object_gap,
                "process_gap": process_gap,
            }))
        }
        // Prediction vs result: Brier score for one PDCA cycle.
        // The prediction carries a confidence in [0,1]; the result is whether
        // the predicted outcome occurred (bool) or the actual delta (f64).
        "kata.prediction_vs_result" => {
            let confidence = input
                .get("prediction")
                .and_then(|p| p.get("confidence"))
                .and_then(|v| v.as_f64())
                .ok_or_else(|| {
                    TemplateError::Manifest(
                        "compute 'kata.prediction_vs_result': missing prediction.confidence".into(),
                    )
                })?;
            // The outcome: either a bool (occurred) or a f64 (actual delta
            // normalized to [0,1]).
            let outcome = input
                .get("result")
                .and_then(|r| {
                    r.get("occurred")
                        .and_then(|v| v.as_bool())
                        .map(|b| if b { 1.0 } else { 0.0 })
                        .or_else(|| r.get("actual_delta").and_then(|v| v.as_f64()))
                })
                .ok_or_else(|| {
                    TemplateError::Manifest(
                        "compute 'kata.prediction_vs_result': missing result.occurred or result.actual_delta".into(),
                    )
                })?;
            let brier = (confidence - outcome).powi(2);
            let prediction_error = (confidence - outcome).abs();
            Ok(serde_json::json!({
                "brier": brier,
                "prediction_error": prediction_error,
                "confidence": confidence,
                "outcome": outcome,
            }))
        }
        // Full convergence check: combines hypotenuse and Brier trajectory.
        // Reads the histories from _convergence context (injected by the
        // tracker) and returns the convergence decision.
        "kata.convergence_check" => {
            // Full convergence check: combines gap, Cauchy, and calibration.
            // Reads the histories from _convergence context (injected by the
            // tracker) and returns the convergence decision.
            //
            // Three canonical stop conditions (any active one triggers):
            // 1. Gap: hypotenuse < hypotenuse_epsilon (limit of a sequence)
            // 2. Cauchy: max pairwise delta in cauchy_window < cauchy_epsilon
            //    (iterates stopped moving — learning exhausted)
            // 3. Calibration: rolling Brier < brier_threshold for brier_window
            //    (predictions are calibrated)
            let hypotenuse = get_f64("hypotenuse")?;
            let hypotenuse_epsilon = input
                .get("hypotenuse_epsilon")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.05);
            let cauchy_epsilon = input
                .get("cauchy_epsilon")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.03);
            let cauchy_window = input
                .get("cauchy_window")
                .and_then(|v| v.as_u64())
                .unwrap_or(3) as usize;
            let brier_history = input
                .get("brier_history")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.iter().map(|v| v.as_f64()).collect::<Option<Vec<f64>>>())
                .unwrap_or_default();
            let hypotenuse_history = input
                .get("hypotenuse_history")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.iter().map(|v| v.as_f64()).collect::<Option<Vec<f64>>>())
                .unwrap_or_default();
            let brier_threshold = input
                .get("brier_threshold")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.15);
            let brier_window = input
                .get("brier_window")
                .and_then(|v| v.as_u64())
                .unwrap_or(3) as usize;
            let mode = input
                .get("mode")
                .and_then(|v| v.as_str())
                .unwrap_or("gap_or_cauchy_or_calibration");

            // 1. Gap convergence
            let gap_converged = hypotenuse.is_finite() && hypotenuse < hypotenuse_epsilon;

            // 2. Cauchy convergence: max pairwise delta in window < epsilon
            let cauchy_converged = if hypotenuse_history.len() >= cauchy_window {
                let start = hypotenuse_history.len().saturating_sub(cauchy_window);
                let finite: Vec<f64> = hypotenuse_history[start..]
                    .iter()
                    .copied()
                    .filter(|f| f.is_finite())
                    .collect();
                if finite.len() >= cauchy_window {
                    let mut max_delta = 0.0_f64;
                    for i in 0..finite.len() {
                        for j in (i + 1)..finite.len() {
                            let delta = (finite[i] - finite[j]).abs();
                            if delta > max_delta {
                                max_delta = delta;
                            }
                        }
                    }
                    max_delta < cauchy_epsilon
                } else {
                    false
                }
            } else {
                false
            };

            // 3. Calibration convergence: rolling Brier < threshold
            let calibration_converged = if brier_history.len() >= brier_window {
                let start = brier_history.len().saturating_sub(brier_window);
                let recent: Vec<f64> = brier_history[start..]
                    .iter()
                    .copied()
                    .filter(|f| f.is_finite())
                    .collect();
                if recent.len() >= brier_window {
                    let rolling: f64 = recent.iter().sum::<f64>() / recent.len() as f64;
                    rolling < brier_threshold
                } else {
                    false
                }
            } else {
                false
            };

            let (converged, conv_mode, reason) = match mode {
                "gap" => (
                    gap_converged,
                    if gap_converged { "gap" } else { "none" },
                    if gap_converged {
                        format!("gap {hypotenuse:.4} < epsilon {hypotenuse_epsilon:.4}")
                    } else {
                        format!("gap {hypotenuse:.4} >= epsilon {hypotenuse_epsilon:.4}")
                    },
                ),
                "cauchy" => (
                    cauchy_converged,
                    if cauchy_converged { "cauchy" } else { "none" },
                    if cauchy_converged {
                        "iterates stabilized (Cauchy criterion met)".to_string()
                    } else {
                        "iterates not yet stabilized".to_string()
                    },
                ),
                "calibration" => (
                    calibration_converged,
                    if calibration_converged {
                        "calibration"
                    } else {
                        "none"
                    },
                    if calibration_converged {
                        "Brier score calibrated".to_string()
                    } else {
                        "Brier score not yet calibrated".to_string()
                    },
                ),
                "gap_or_cauchy" => {
                    if gap_converged {
                        (
                            true,
                            "gap",
                            format!("gap {hypotenuse:.4} < epsilon {hypotenuse_epsilon:.4}"),
                        )
                    } else if cauchy_converged {
                        (
                            true,
                            "cauchy",
                            "iterates stabilized (Cauchy criterion met)".to_string(),
                        )
                    } else {
                        (
                            false,
                            "none",
                            format!("gap {hypotenuse:.4} >= epsilon, not Cauchy"),
                        )
                    }
                }
                "gap_or_calibration" => {
                    if gap_converged {
                        (
                            true,
                            "gap",
                            format!("gap {hypotenuse:.4} < epsilon {hypotenuse_epsilon:.4}"),
                        )
                    } else if calibration_converged {
                        (true, "calibration", "Brier score calibrated".to_string())
                    } else {
                        (
                            false,
                            "none",
                            format!("gap {hypotenuse:.4} >= epsilon, Brier not calibrated"),
                        )
                    }
                }
                "cauchy_or_calibration" => {
                    if cauchy_converged {
                        (
                            true,
                            "cauchy",
                            "iterates stabilized (Cauchy criterion met)".to_string(),
                        )
                    } else if calibration_converged {
                        (true, "calibration", "Brier score calibrated".to_string())
                    } else {
                        (
                            false,
                            "none",
                            "not Cauchy, Brier not calibrated".to_string(),
                        )
                    }
                }
                _ => {
                    // gap_or_cauchy_or_calibration (default)
                    if gap_converged {
                        (
                            true,
                            "gap",
                            format!("gap {hypotenuse:.4} < epsilon {hypotenuse_epsilon:.4}"),
                        )
                    } else if cauchy_converged {
                        (
                            true,
                            "cauchy",
                            "iterates stabilized (Cauchy criterion met)".to_string(),
                        )
                    } else if calibration_converged {
                        (true, "calibration", "Brier score calibrated".to_string())
                    } else {
                        (
                            false,
                            "none",
                            format!(
                                "gap {hypotenuse:.4} >= epsilon, not Cauchy, Brier not calibrated"
                            ),
                        )
                    }
                }
            };

            Ok(serde_json::json!({
                "converged": converged,
                "mode": conv_mode,
                "reason": reason,
                "hypotenuse": hypotenuse,
                "gap_converged": gap_converged,
                "cauchy_converged": cauchy_converged,
                "calibration_converged": calibration_converged,
            }))
        }
        // ── Swarm cybernetic primitives (Cybernetic Swarm Plan C1/C3/C7) ──
        //
        // Deterministic accumulators + a second-order monitor over the
        // per-iteration log. S1 §5.4: "statistical functions over the action
        // log requiring no modifications to the underlying model." These are
        // the deterministic enforcement points for the swarm-intelligence
        // CONVERGE side: an LLM template cannot reliably maintain a running
        // set/sum across LOOP iterations, so the accumulators live here as
        // pure functions. The manifest threads the accumulator outputs back
        // through the loop step's input_mapping so the next iteration (and
        // DECIDE) can read them.
        "swarm.converge_accumulate" => {
            let d = get_f64("d")?;
            let task_success = input.get("task_success").cloned();
            let deficit_class = input
                .get("deficit_class")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let iteration_log = input
                .get("iteration_log")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let failed_edits = input
                .get("failed_edits")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let influence_scores = input
                .get("influence_scores")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            // Extract the primary DECIDE move (first proposed move) — the
            // `decision_action` and the `agent_type` it names. Both are read
            // from the whole `decisions` object so fragile Jinja indexing stays
            // out of the manifest.
            let (decision_action, agent_type) = extract_swarm_decision(input.get("decisions"));
            // Deterministic swarm-state signature: the deficit class + the
            // sorted multiset of the roster's agent_types. Two iterations with
            // the same deficit and the same roster shape share a signature.
            let roster_types = extract_roster_agent_types(input.get("swarm_state"));
            let mut sorted_roster = roster_types.clone();
            sorted_roster.sort();
            let swarm_state_signature = format!("{}|{}", deficit_class, sorted_roster.join(","));
            // Extract the deterministic task-success scalar `s`: score if
            // present, else 1.0/0.0 from `pass`, else null (not measured).
            let s = extract_task_success_scalar(&task_success);
            // d_delta vs the prior iteration's d (0 on the first iteration).
            let prior_d = iteration_log
                .last()
                .and_then(|e| e.get("d"))
                .and_then(|v| v.as_f64());
            let d_delta = match prior_d {
                Some(prior) => d - prior,
                None => 0.0,
            };
            let prior_s = iteration_log
                .last()
                .and_then(|e| e.get("s"))
                .and_then(|v| v.as_f64());
            // Append this iteration to the log.
            let mut new_log = iteration_log.clone();
            new_log.push(serde_json::json!({
                "d": d,
                "s": s,
                "deficit_class": deficit_class,
                "decision_action": decision_action,
            }));
            // Failed-edit memory (C3): record when the edit did not improve d
            // (d_delta <= 0) AND s did not improve. "s did not improve" = s is
            // null (not measured — d alone is the sensor-truth risk per C3's
            // Needs-C0 note) OR current s <= prior s. This is the anti-loop set
            // DECIDE rejects re-proposals against.
            let mut new_failed = failed_edits.clone();
            let s_not_improved = match (s, prior_s) {
                (Some(cur), Some(prev)) => cur <= prev,
                _ => true, // null or first iteration: cannot confirm improvement
            };
            if d_delta <= 0.0 && s_not_improved {
                new_failed.push(serde_json::json!({
                    "decision_action": decision_action,
                    "swarm_state_signature": swarm_state_signature,
                    "d_delta": d_delta,
                    "iteration": new_log.len(),
                }));
            }
            // Influence-weighted rejection (C7): per-agent_type running sum of
            // d_delta after the move. DECIDE rejects re-hire of a type whose
            // sum is <= 0 over the recent window.
            let mut new_influence = influence_scores.as_object().cloned().unwrap_or_default();
            if !agent_type.is_empty() {
                let cur = new_influence
                    .get(&agent_type)
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                new_influence.insert(agent_type.clone(), serde_json::json!(cur + d_delta));
            }
            Ok(serde_json::json!({
                "iteration_log": new_log,
                "failed_edits": new_failed,
                "influence_scores": serde_json::Value::Object(new_influence),
            }))
        }
        "swarm.second_order_monitor" => {
            // S1 P5 second-order monitor: two deterministic signals over the
            // iteration log. No LLM.
            let iteration_log = input
                .get("iteration_log")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let loop_window = input
                .get("loop_window")
                .and_then(|v| v.as_u64())
                .unwrap_or(3) as usize;
            if iteration_log.len() < 2 {
                return Ok(serde_json::json!({
                    "reasoning_loop": false,
                    "sensor_truth_divergence": false,
                    "detail": "fewer than 2 iterations logged",
                    "recommendation": "none",
                }));
            }
            // Signal 1 — reasoning loop: the last `loop_window` entries share
            // the same (deficit_class, decision_action) AND d did not improve
            // across the window (last d >= window's first d).
            let win_start = iteration_log.len().saturating_sub(loop_window);
            let window = &iteration_log[win_start..];
            let reasoning_loop = if window.len() >= 2 {
                let first_key = (
                    window[0]
                        .get("deficit_class")
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                    window[0]
                        .get("decision_action")
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                );
                let same_action = window.iter().all(|e| {
                    (
                        e.get("deficit_class")
                            .and_then(|v| v.as_str())
                            .unwrap_or(""),
                        e.get("decision_action")
                            .and_then(|v| v.as_str())
                            .unwrap_or(""),
                    ) == first_key
                });
                let first_d = window[0]
                    .get("d")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(f64::INFINITY);
                let last_d = window[window.len() - 1]
                    .get("d")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(f64::NEG_INFINITY);
                same_action && last_d >= first_d
            } else {
                false
            };
            // Signal 2 — sensor-truth divergence: over the entries with a
            // non-null s, d is non-increasing (improving) while s is
            // non-increasing (declining). Needs >= 3 such points to avoid a
            // two-point coincidence. This is the Go See diagnosis (§5)
            // automated: the swarm looks healthier but is failing more tasks.
            let measured: Vec<(f64, f64)> = iteration_log
                .iter()
                .filter_map(|e| {
                    let d = e.get("d").and_then(|v| v.as_f64())?;
                    let s = e.get("s").and_then(|v| v.as_f64())?;
                    Some((d, s))
                })
                .collect();
            let sensor_truth_divergence = if measured.len() >= 3 {
                let d_improving = measured.windows(2).all(|w| w[1].0 <= w[0].0 + f64::EPSILON);
                let s_declining = measured.windows(2).all(|w| w[1].1 <= w[0].1 + f64::EPSILON);
                d_improving && s_declining
            } else {
                false
            };
            let (recommendation, detail) = if sensor_truth_divergence {
                (
                    "go_see",
                    "d improving while s declining — sensor filters truth; escalate Go See"
                        .to_string(),
                )
            } else if reasoning_loop {
                (
                    "diversify_action",
                    format!(
                        "(deficit_class, decision_action) repeated for {} iterations with no d improvement",
                        window.len()
                    ),
                )
            } else {
                ("none", "no second-order anomaly detected".to_string())
            };
            Ok(serde_json::json!({
                "reasoning_loop": reasoning_loop,
                "sensor_truth_divergence": sensor_truth_divergence,
                "detail": detail,
                "recommendation": recommendation,
            }))
        }
        // ── Lisp evaluation primitive ──
        //
        // Deterministic evaluation of a Lisp form against a JSON environment.
        // No LLM round-trip, no I/O, no filesystem, no network. Bounded
        // recursion depth (64) and bounded evaluation steps (100000).
        // Used for recursive predicates over the context map — e.g.
        // capability-tree walks, structural invariant checks, falsifiability
        // counterfactuals that the LLM cannot reliably evaluate itself.
        //
        // Security: the interpreter has no `eval` builtin (Lisp code cannot
        // evaluate arbitrary strings), no `load`/`require`, and the
        // environment is immutable from Lisp's perspective. The caller must
        // gate `lisp.eval` to `category: skill` manifests only — infrastructure
        // manifests run without human review and a Turing-complete step
        // language is an attack surface (see .rules trap on manifests).
        "lisp.eval" => {
            let form = input.get("form").and_then(|v| v.as_str()).ok_or_else(|| {
                TemplateError::Manifest("compute 'lisp.eval': missing 'form' string".into())
            })?;
            let env_input = input.get("env").cloned().unwrap_or(Value::Null);
            let max_steps = input
                .get("max_steps")
                .and_then(|v| v.as_u64())
                .unwrap_or(100000);
            let max_depth = input
                .get("max_depth")
                .and_then(|v| v.as_u64())
                .unwrap_or(64);
            let result =
                hkask_lisp::eval_sandboxed_with_budget(form, &env_input, max_steps, max_depth)
                    .map_err(|e| TemplateError::Manifest(format!("lisp.eval: {e}")))?;
            Ok(result)
        }
        other => Err(TemplateError::Manifest(format!(
            "Unknown compute_ref: '{}'. Supported: calibrate_from_fermi, outside_view_adjustment, bayesian_update, apply_calibration_adjustment, brier_score, brier_score_multi, brier_interpretation, kata.object_gap, kata.process_gap, kata.hypotenuse, kata.prediction_vs_result, kata.convergence_check, lisp.eval, swarm.converge_accumulate, swarm.second_order_monitor",
            other
        ))),
    }
}

/// Extract the primary DECIDE move's `(decision_action, agent_type)` from
/// the `decisions` step result (the `swarm-decide.j2` output object).
/// `decision_action` = the first `proposed_moves[].move_type` ("hire"|
/// "delegate"|"remove"|"reconfigure_agent"), or "" when no moves were proposed.
/// `agent_type` = the first move's `agent_id_or_type` (the influence key C7
/// scores by — for a hire it is the agent type/name; for remove it is the
/// blamed agent). Returns `("", "")` when the shape is unexpected.
fn extract_swarm_decision(decisions: Option<&Value>) -> (String, String) {
    let Some(decisions) = decisions else {
        return (String::new(), String::new());
    };
    let Some(moves) = decisions.get("proposed_moves").and_then(|v| v.as_array()) else {
        return (String::new(), String::new());
    };
    let Some(first) = moves.first() else {
        return (String::new(), String::new());
    };
    let decision_action = first
        .get("move_type")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let agent_type = first
        .get("agent_id_or_type")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    (decision_action, agent_type)
}

/// Extract the roster's `agent_type` multiset from the SENSE step result
/// (`swarm_state`). Handles both ABW and local roster shapes: both nest the
/// agent list under `workspace_roster.agents[]` (SENSE emits
/// `workspace_roster`), and each agent carries `agent_type`. Returns an empty
/// vec when the shape is unexpected.
fn extract_roster_agent_types(swarm_state: Option<&Value>) -> Vec<String> {
    let Some(state) = swarm_state else {
        return Vec::new();
    };
    let roster = state.get("workspace_roster").unwrap_or(state);
    let Some(agents) = roster.get("agents").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    agents
        .iter()
        .filter_map(|a| {
            a.get("agent_type")
                .and_then(|v| v.as_str())
                .map(String::from)
        })
        .collect()
}

/// Extract the deterministic task-success scalar `s` from the CHECK step's
/// `task_success` verdict (Cybernetic Swarm Plan C0). `score` wins when
/// present (a deterministic evaluator's 0.0–1.0 score); else `1.0`/`0.0` from
/// the boolean `pass`; else `None` (not measured — an open task with no
/// oracle). Never fabricates a verdict (the `.rules` `unwrap_or(0)` trap on
/// regulation signals — null means "not measured", not "passed").
fn extract_task_success_scalar(task_success: &Option<Value>) -> Option<f64> {
    let Some(ts) = task_success else {
        return None;
    };
    if ts.is_null() {
        return None;
    }
    if let Some(score) = ts.get("score").and_then(|v| v.as_f64()) {
        return Some(score);
    }
    match ts.get("pass").and_then(|v| v.as_bool()) {
        Some(true) => Some(1.0),
        Some(false) => Some(0.0),
        None => None,
    }
}

/// Compute the object-space gap (Dublin Core artifact completeness).
///
/// Edge-counting distance: counts fields present in the target spec but
/// missing from the current artifacts (weight 1.0 each), plus fields that are
/// present but ungrounded (weight 0.5 each — an ungrounded field is halfway
/// between missing and complete). Normalized to [0, 1] by dividing by the
/// total field count in the target spec.
///
/// This is the simplest well-defined distance measure for object space.
/// If Brier scores don't converge with this measure, escalate to
/// information-content-weighted measures (Resnik/Lin).
fn compute_object_gap(
    current: &serde_json::Value,
    target: &serde_json::Value,
) -> (f64, Vec<String>, Vec<String>) {
    let target_fields = collect_field_keys(target);
    let mut missing: Vec<String> = Vec::new();
    let mut ungrounded: Vec<String> = Vec::new();
    let total = target_fields.len().max(1) as f64;

    for field in &target_fields {
        match current.get(field) {
            None | Some(serde_json::Value::Null) => {
                missing.push(field.clone());
            }
            Some(val) if is_ungrounded(val) => {
                ungrounded.push(field.clone());
            }
            Some(_) => { /* complete */ }
        }
    }

    let gap = (missing.len() as f64 + 0.5 * ungrounded.len() as f64) / total;
    (gap.min(1.0), missing, ungrounded)
}

/// A field value is "ungrounded" if it's an empty string, empty array, empty
/// object, or a string that looks like a placeholder ("TODO", "TBD", "?").
fn is_ungrounded(val: &serde_json::Value) -> bool {
    match val {
        serde_json::Value::String(s) => {
            let trimmed = s.trim();
            trimmed.is_empty()
                || matches!(
                    trimmed.to_lowercase().as_str(),
                    "todo" | "tbd" | "?" | "n/a" | "placeholder"
                )
        }
        serde_json::Value::Array(arr) => arr.is_empty(),
        serde_json::Value::Object(obj) => obj.is_empty(),
        _ => false,
    }
}

/// Compute the process-space gap (PKO procedure progress).
///
/// Edge-counting distance: counts steps in the target procedure that are not
/// yet complete in the current procedure. Steps that are "in_progress" are
/// half-weighted (halfway between not-started and complete). Normalized to
/// [0, 1] by dividing by the total step count.
///
/// The procedure is represented as an array of step objects, each with a
/// `status` field: "complete", "in_progress", "not_started" (or missing).
fn compute_process_gap(
    current: &serde_json::Value,
    target: &serde_json::Value,
) -> (f64, Vec<String>) {
    let target_steps = target
        .get("steps")
        .and_then(|v| v.as_array())
        .or_else(|| target.as_array())
        .cloned()
        .unwrap_or_default();
    let current_steps = current
        .get("steps")
        .and_then(|v| v.as_array())
        .or_else(|| current.as_array())
        .cloned()
        .unwrap_or_default();

    let total = target_steps.len().max(1) as f64;
    let mut incomplete: Vec<String> = Vec::new();
    let mut weighted_incomplete = 0.0_f64;

    for (i, target_step) in target_steps.iter().enumerate() {
        let step_name = target_step
            .get("name")
            .or_else(|| target_step.get("id"))
            .and_then(|v| v.as_str())
            .unwrap_or("unnamed")
            .to_string();
        let current_status = current_steps
            .get(i)
            .and_then(|s| s.get("status"))
            .and_then(|v| v.as_str())
            .unwrap_or("not_started");
        match current_status {
            "complete" => { /* done */ }
            "in_progress" => {
                weighted_incomplete += 0.5;
                incomplete.push(format!("{step_name} (in_progress)"));
            }
            _ => {
                weighted_incomplete += 1.0;
                incomplete.push(format!("{step_name} (not_started)"));
            }
        }
    }

    let gap = weighted_incomplete / total;
    (gap.min(1.0), incomplete)
}

/// Collect the top-level keys from a JSON object (for object-gap field
/// comparison). If the value is an array, collects the `name` or `id` field
/// from each element.
fn collect_field_keys(value: &serde_json::Value) -> Vec<String> {
    match value {
        serde_json::Value::Object(map) => map.keys().cloned().collect(),
        serde_json::Value::Array(arr) => arr
            .iter()
            .enumerate()
            .map(|(i, item)| {
                item.get("name")
                    .or_else(|| item.get("id"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or(format!("item_{i}"))
            })
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_calibrate_from_fermi() {
        let input = serde_json::json!({
            "questions": [
                {"question": "a", "estimate": 0.8, "confidence": 0.9},
                {"question": "b", "estimate": 0.2, "confidence": 0.1}
            ]
        });
        let result = dispatch_compute("calibrate_from_fermi", &input).unwrap();
        let calibrated = result.get("calibrated").and_then(|v| v.as_f64()).unwrap();
        assert!((calibrated - 0.74).abs() < 0.01, "weighted average = 0.74");
    }

    #[test]
    fn dispatch_outside_view_adjustment() {
        let input = serde_json::json!({
            "base_rate": 0.7, "inside_estimate": 0.3, "reference_count": 1000
        });
        let result = dispatch_compute("outside_view_adjustment", &input).unwrap();
        let calibrated = result.get("calibrated").and_then(|v| v.as_f64()).unwrap();
        assert!(calibrated > 0.6, "high reference count trusts base rate");
    }

    #[test]
    fn dispatch_bayesian_update() {
        let input = serde_json::json!({
            "prior": 0.3, "evidence_likelihood": 0.9, "evidence_base_rate": 0.3
        });
        let result = dispatch_compute("bayesian_update", &input).unwrap();
        let posterior = result.get("posterior").and_then(|v| v.as_f64()).unwrap();
        assert!((posterior - 0.9).abs() < 0.01, "Bayesian update = 0.9");
    }

    #[test]
    fn dispatch_apply_calibration_adjustment() {
        let input = serde_json::json!({ "prior": 0.9, "overconfidence_bias": 0.3 });
        let result = dispatch_compute("apply_calibration_adjustment", &input).unwrap();
        let adjusted = result.get("adjusted").and_then(|v| v.as_f64()).unwrap();
        assert!(
            adjusted < 0.9 && adjusted > 0.5,
            "overconfident regresses toward 0.5"
        );
    }

    #[test]
    fn dispatch_brier_score() {
        let input = serde_json::json!({ "probability": 1.0, "outcome_occurred": true });
        let result = dispatch_compute("brier_score", &input).unwrap();
        let score = result.get("score").and_then(|v| v.as_f64()).unwrap();
        assert!((score - 0.0).abs() < 1e-9, "perfect forecast = 0 Brier");
    }

    #[test]
    fn dispatch_unknown_ref_errors() {
        let input = serde_json::json!({});
        assert!(dispatch_compute("nonexistent_fn", &input).is_err());
    }

    #[test]
    fn dispatch_lisp_eval_basic() {
        let input = serde_json::json!({
            "form": "(+ 1 2 3)"
        });
        let result = dispatch_compute("lisp.eval", &input).unwrap();
        assert_eq!(result, serde_json::json!(6));
    }

    #[test]
    fn dispatch_lisp_eval_with_env() {
        let input = serde_json::json!({
            "form": "(assoc \"score\" step_1_result)",
            "env": {
                "step_1_result": {"score": 0.85, "findings": ["a", "b"]}
            }
        });
        let result = dispatch_compute("lisp.eval", &input).unwrap();
        assert_eq!(result, serde_json::json!(0.85));
    }

    #[test]
    fn dispatch_lisp_eval_predicate() {
        let input = serde_json::json!({
            "form": "(and (> (length findings) 0) (< composite 0.15))",
            "env": {
                "findings": ["a", "b"],
                "composite": 0.12
            }
        });
        let result = dispatch_compute("lisp.eval", &input).unwrap();
        assert_eq!(result, serde_json::json!(true));
    }

    #[test]
    fn dispatch_lisp_eval_missing_form_errors() {
        let input = serde_json::json!({"env": {}});
        assert!(dispatch_compute("lisp.eval", &input).is_err());
    }

    #[test]
    fn dispatch_lisp_eval_step_limit() {
        let input = serde_json::json!({
            "form": "(begin (define loop (lambda () (loop))) (loop))",
            "max_steps": 100,
            "max_depth": 1000
        });
        assert!(dispatch_compute("lisp.eval", &input).is_err());
    }

    #[test]
    fn dispatch_kata_object_gap_complete() {
        let input = serde_json::json!({
            "current_artifacts": {"title": "My Plan", "obstacles": ["a", "b"], "assessment": "grounded"},
            "target_artifacts": {"title": "", "obstacles": [], "assessment": ""}
        });
        let result = dispatch_compute("kata.object_gap", &input).unwrap();
        let gap = result.get("object_gap").and_then(|v| v.as_f64()).unwrap();
        assert!((gap - 0.0).abs() < 1e-9, "all fields present = gap 0");
    }

    #[test]
    fn dispatch_kata_object_gap_missing_fields() {
        let input = serde_json::json!({
            "current_artifacts": {"title": "My Plan"},
            "target_artifacts": {"title": "", "obstacles": [], "assessment": "", "prediction": ""}
        });
        let result = dispatch_compute("kata.object_gap", &input).unwrap();
        let gap = result.get("object_gap").and_then(|v| v.as_f64()).unwrap();
        // 3 missing out of 4 = 0.75
        assert!(
            (gap - 0.75).abs() < 1e-9,
            "3/4 missing = gap 0.75, got {gap}"
        );
    }

    #[test]
    fn dispatch_kata_object_gap_ungrounded_half_weighted() {
        let input = serde_json::json!({
            "current_artifacts": {"title": "My Plan", "obstacles": [], "assessment": "TODO"},
            "target_artifacts": {"title": "", "obstacles": [], "assessment": ""}
        });
        let result = dispatch_compute("kata.object_gap", &input).unwrap();
        let gap = result.get("object_gap").and_then(|v| v.as_f64()).unwrap();
        // 1 ungrounded (obstacles empty) at 0.5 + 1 ungrounded (assessment=TODO) at 0.5 = 1.0 / 3
        assert!(
            (gap - (1.0 / 3.0)).abs() < 1e-9,
            "2 ungrounded at 0.5 each = 1.0/3, got {gap}"
        );
    }

    #[test]
    fn dispatch_kata_process_gap_all_complete() {
        let input = serde_json::json!({
            "current_procedure": {"steps": [
                {"name": "grasp", "status": "complete"},
                {"name": "target", "status": "complete"},
                {"name": "experiment", "status": "complete"}
            ]},
            "target_procedure": {"steps": [
                {"name": "grasp"},
                {"name": "target"},
                {"name": "experiment"}
            ]}
        });
        let result = dispatch_compute("kata.process_gap", &input).unwrap();
        let gap = result.get("process_gap").and_then(|v| v.as_f64()).unwrap();
        assert!((gap - 0.0).abs() < 1e-9, "all complete = gap 0");
    }

    #[test]
    fn dispatch_kata_process_gap_mixed() {
        let input = serde_json::json!({
            "current_procedure": {"steps": [
                {"name": "grasp", "status": "complete"},
                {"name": "target", "status": "in_progress"},
                {"name": "experiment", "status": "not_started"}
            ]},
            "target_procedure": {"steps": [
                {"name": "grasp"},
                {"name": "target"},
                {"name": "experiment"}
            ]}
        });
        let result = dispatch_compute("kata.process_gap", &input).unwrap();
        let gap = result.get("process_gap").and_then(|v| v.as_f64()).unwrap();
        // 1 complete (0) + 1 in_progress (0.5) + 1 not_started (1.0) = 1.5 / 3 = 0.5
        assert!((gap - 0.5).abs() < 1e-9, "mixed = gap 0.5, got {gap}");
    }

    #[test]
    fn dispatch_kata_hypotenuse() {
        let input = serde_json::json!({ "object_gap": 0.3, "process_gap": 0.4 });
        let result = dispatch_compute("kata.hypotenuse", &input).unwrap();
        let h = result.get("hypotenuse").and_then(|v| v.as_f64()).unwrap();
        assert!((h - 0.5).abs() < 1e-9, "sqrt(0.09 + 0.16) = 0.5, got {h}");
    }

    #[test]
    fn dispatch_kata_prediction_vs_result_correct() {
        let input = serde_json::json!({
            "prediction": {"confidence": 0.9},
            "result": {"occurred": true}
        });
        let result = dispatch_compute("kata.prediction_vs_result", &input).unwrap();
        let brier = result.get("brier").and_then(|v| v.as_f64()).unwrap();
        assert!(
            (brier - 0.01).abs() < 1e-9,
            "(0.9-1.0)^2 = 0.01, got {brier}"
        );
    }

    #[test]
    fn dispatch_kata_prediction_vs_result_wrong() {
        let input = serde_json::json!({
            "prediction": {"confidence": 0.9},
            "result": {"occurred": false}
        });
        let result = dispatch_compute("kata.prediction_vs_result", &input).unwrap();
        let brier = result.get("brier").and_then(|v| v.as_f64()).unwrap();
        assert!(
            (brier - 0.81).abs() < 1e-9,
            "(0.9-0.0)^2 = 0.81, got {brier}"
        );
    }

    #[test]
    fn dispatch_kata_convergence_check_gap_converged() {
        let input = serde_json::json!({
            "hypotenuse": 0.02,
            "hypotenuse_epsilon": 0.05,
            "cauchy_epsilon": 0.03,
            "cauchy_window": 3,
            "brier_history": [0.5],
            "hypotenuse_history": [0.5, 0.02],
            "brier_threshold": 0.15,
            "brier_window": 3,
            "mode": "gap_or_cauchy_or_calibration"
        });
        let result = dispatch_compute("kata.convergence_check", &input).unwrap();
        assert!(result.get("converged").and_then(|v| v.as_bool()).unwrap());
        assert_eq!(result.get("mode").and_then(|v| v.as_str()).unwrap(), "gap");
    }

    #[test]
    fn dispatch_kata_convergence_check_cauchy_converged() {
        let input = serde_json::json!({
            "hypotenuse": 0.30,
            "hypotenuse_epsilon": 0.05,
            "cauchy_epsilon": 0.03,
            "cauchy_window": 3,
            "brier_history": [0.5],
            "hypotenuse_history": [0.30, 0.31, 0.30],
            "brier_threshold": 0.15,
            "brier_window": 3,
            "mode": "gap_or_cauchy_or_calibration"
        });
        let result = dispatch_compute("kata.convergence_check", &input).unwrap();
        assert!(result.get("converged").and_then(|v| v.as_bool()).unwrap());
        assert_eq!(
            result.get("mode").and_then(|v| v.as_str()).unwrap(),
            "cauchy"
        );
    }

    #[test]
    fn dispatch_kata_convergence_check_calibration_converged() {
        let input = serde_json::json!({
            "hypotenuse": 0.30,
            "hypotenuse_epsilon": 0.05,
            "cauchy_epsilon": 0.03,
            "cauchy_window": 3,
            "brier_history": [0.05, 0.05, 0.05],
            "hypotenuse_history": [0.50, 0.30, 0.10],
            "brier_threshold": 0.15,
            "brier_window": 3,
            "mode": "gap_or_cauchy_or_calibration"
        });
        let result = dispatch_compute("kata.convergence_check", &input).unwrap();
        assert!(result.get("converged").and_then(|v| v.as_bool()).unwrap());
        assert_eq!(
            result.get("mode").and_then(|v| v.as_str()).unwrap(),
            "calibration"
        );
    }

    #[test]
    fn dispatch_kata_convergence_check_not_converged() {
        let input = serde_json::json!({
            "hypotenuse": 0.3,
            "hypotenuse_epsilon": 0.05,
            "cauchy_epsilon": 0.03,
            "cauchy_window": 3,
            "brier_history": [0.5, 0.5],
            "hypotenuse_history": [0.5, 0.3],
            "brier_threshold": 0.15,
            "brier_window": 3,
            "mode": "gap_or_cauchy_or_calibration"
        });
        let result = dispatch_compute("kata.convergence_check", &input).unwrap();
        assert!(!result.get("converged").and_then(|v| v.as_bool()).unwrap());
    }

    #[test]
    fn dispatch_missing_input_errors() {
        let input = serde_json::json!({});
        assert!(
            dispatch_compute("bayesian_update", &input).is_err(),
            "missing prior errors"
        );
    }
}
