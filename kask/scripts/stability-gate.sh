#!/usr/bin/env bash
# Stability gate for the evolving test harness.
#
# Computes ECR/EIR/Acc from mutation testing (non-degenerate oracle) +
# regression-prevention guard (RR-*.yaml) + classifier (EIR secondary).
# Emits a verdict: proceed, halt_escalate, stalled_escalate,
# regression_violated, or converged.
#
# Usage: stability-gate.sh <run-id-N> [run-id-N-1] [run-id-N-2] [run-id-N-3]
#
# For the Cauchy convergence check and stall detector (W=3), pass the last
# 4 run-ids. For a simple ECR/EIR check, pass 2 run-ids (N and N-1).
#
# Design: kask/docs/plans/evolving-test-harness.md §3.4, §3.6
set -euo pipefail

cd "$(dirname "$0")/.."

TRACE_DIR="${HKASK_TRACE_DIR:-traces}"
REGRESSIONS_DIR="security/regressions"
VERBOSE=0

if [[ "${1:-}" == "--verbose" ]]; then
    VERBOSE=1
    shift
fi

RUN_IDS=("$@")
if [[ ${#RUN_IDS[@]} -lt 2 ]]; then
    echo "Usage: $0 <run-id-N> [run-id-N-1] [run-id-N-2] [run-id-N-3]" >&2
    exit 2
fi

LATEST_RUN="${RUN_IDS[0]}"
PREV_RUN="${RUN_IDS[1]}"

LATEST_TRACE="${TRACE_DIR}/${LATEST_RUN}"
PREV_TRACE="${TRACE_DIR}/${PREV_RUN}"

if [[ ! -d "$LATEST_TRACE" ]]; then
    echo "ERROR: trace dir not found: $LATEST_TRACE" >&2
    exit 2
fi
if [[ ! -d "$PREV_TRACE" ]]; then
    echo "ERROR: trace dir not found: $PREV_TRACE" >&2
    exit 2
fi

# ── Helpers ────────────────────────────────────────────────────────────────

read_metric() {
    local trace_dir="$1"
    local field="$2"
    local metrics="${trace_dir}/metrics.json"
    if [[ -f "$metrics" ]] && command -v jq &>/dev/null; then
        jq -r ".${field} // 0" "$metrics" 2>/dev/null || echo 0
    else
        echo 0
    fi
}

# Reports 1 if the field is present (and non-null) in metrics.json, 0 otherwise.
# Used to refuse convergence when metrics are absent — a missing metric is
# "no signal", not "zero deviation" (the unwrap_or(0) trap).
metric_present() {
    local trace_dir="$1"
    local field="$2"
    local metrics="${trace_dir}/metrics.json"
    if [[ -f "$metrics" ]] && command -v jq &>/dev/null; then
        local val
        val=$(jq -r ".${field} // \"__absent__\"" "$metrics" 2>/dev/null || echo "__absent__")
        [[ "$val" != "__absent__" ]]
    else
        return 1
    fi
}

# ── Regression-prevention guard (RR-*.yaml) ────────────────────────────────

regression_violated=0
if [[ -f "scripts/lib-regressions.sh" ]] && [[ -d "$REGRESSIONS_DIR" ]]; then
    # shellcheck source=scripts/lib-regressions.sh
    source scripts/lib-regressions.sh
    if ! check_regressions "" "" "reg-span" >/dev/null 2>&1; then
        regression_violated=1
    fi
fi

if [[ "$regression_violated" -eq 1 ]]; then
    echo "VERDICT: regression_violated"
    echo "reason: an enforced RR-*.yaml entry flipped pass to fail"
    echo "ECR: N/A"
    echo "EIR: N/A"
    echo "Acc: N/A"
    echo "mutation_score: N/A"
    exit 0
fi

# ── Mutation score (non-degenerate oracle) ─────────────────────────────────

mutation_score=$(read_metric "$LATEST_TRACE" "mutation_score")
prev_mutation_score=$(read_metric "$PREV_TRACE" "mutation_score")

if [[ "$mutation_score" == "0" ]] && command -v cargo-mutants &>/dev/null; then
    if [[ "$VERBOSE" -eq 1 ]]; then
        echo "Running cargo-mutants for mutation score..." >&2
    fi
    cargo mutants --output-format json --in-place \
        -p hkask-types -p hkask-capability -p hkask-templates \
        --timeout-seconds 30 2>/dev/null > /tmp/mutants-out.json || true

    if [[ -f /tmp/mutants-out.json ]] && command -v jq &>/dev/null; then
        total=$(jq 'length' /tmp/mutants-out.json 2>/dev/null || echo 0)
        killed=$(jq '[.[] | select(.status == "killed")] | length' /tmp/mutants-out.json 2>/dev/null || echo 0)
        if [[ "$total" -gt 0 ]]; then
            mutation_score=$(echo "scale=4; $killed / $total" | bc 2>/dev/null || echo 0)
            # Write mutation_score back into metrics.json so subsequent runs (N-1)
            # and the MutationScoreSensor can read it. Without this write-back,
            # prev_mutation_score is always 0 and the ECR/EIR computation is degenerate.
            metrics_file="${LATEST_TRACE}/metrics.json"
            if [[ -f "$metrics_file" ]]; then
                tmp_metrics=$(mktemp)
                if jq --argjson ms "$mutation_score" '. + {mutation_score: $ms}' \
                    "$metrics_file" > "$tmp_metrics" 2>/dev/null; then
                    mv "$tmp_metrics" "$metrics_file"
                else
                    rm -f "$tmp_metrics"
                fi
            fi
        fi
    fi
fi

# ── Coverage and cost from metrics.json ────────────────────────────────────

coverage_pct=$(read_metric "$LATEST_TRACE" "coverage_pct")
prev_coverage_pct=$(read_metric "$PREV_TRACE" "coverage_pct")

# F7: cost axis — wall-clock duration normalized against a configurable ceiling.
# cost_norm = cost_tokens / max_cost_seconds (guard divide-by-zero → cost_norm=0).
# Only contributes to the Cauchy norm when present in BOTH runs (metric_present gate).
HKASK_MAX_COST_SECONDS="${HKASK_MAX_COST_SECONDS:-600}"
cost_tokens=$(read_metric "$LATEST_TRACE" "cost_tokens")
prev_cost_tokens=$(read_metric "$PREV_TRACE" "cost_tokens")
cost_norm=0
prev_cost_norm=0
if [[ "$HKASK_MAX_COST_SECONDS" -gt 0 ]] 2>/dev/null; then
    cost_norm=$(echo "scale=4; $cost_tokens / $HKASK_MAX_COST_SECONDS" | bc 2>/dev/null || echo 0)
    prev_cost_norm=$(echo "scale=4; $prev_cost_tokens / $HKASK_MAX_COST_SECONDS" | bc 2>/dev/null || echo 0)
fi

# ── ECR/EIR/Acc computation ────────────────────────────────────────────────

delta_mutation=$(echo "scale=4; $mutation_score - $prev_mutation_score" | bc 2>/dev/null || echo 0)
delta_coverage=$(echo "scale=4; $coverage_pct - $prev_coverage_pct" | bc 2>/dev/null || echo 0)

if [[ "$prev_mutation_score" != "1" && "$prev_mutation_score" != "1.0" ]]; then
    ecr=$(echo "scale=4; $delta_mutation / (1 - $prev_mutation_score)" | bc 2>/dev/null || echo 0)
else
    ecr=0
fi

if [[ $(echo "$delta_mutation < 0" | bc 2>/dev/null || echo 0) -eq 1 ]]; then
    eir_deterministic=$(echo "scale=4; 0 - $delta_mutation" | bc 2>/dev/null || echo 0)
else
    eir_deterministic=0
fi

# ── EIR classifier component (F5: placeholder) ───────────────────────────
# The classifier EIR counts new flaky tests (is_real_bug: false) from
# qa-triage's classifier.json. This is a NO-OP until qa-triage is wired into
# the harness-evolve-cycle (the failures/ dir is never populated in the
# current loop). The deterministic EIR (mutant regressions, above) is the
# primary signal and works today. When qa-triage is wired in, this section
# will count NEW flaky tests vs N-1 (not all flaky tests — see §9.5 #6).

eir_classifier=0
latest_failures_dir="${LATEST_TRACE}/failures"
prev_failures_dir="${PREV_TRACE}/failures"
if [[ -d "$latest_failures_dir" ]] && command -v jq &>/dev/null; then
    # F5: build a set of failure-test-names from the prev run's failures/ dir.
    # Only NEW non-real-bug failures (present in latest but not prev) are counted.
    # If the prev run has no failures/ dir, count all (first run).
    declare -A prev_failure_names=()
    if [[ -d "$prev_failures_dir" ]]; then
        for prev_fail_path in "$prev_failures_dir"/*/; do
            [[ -d "$prev_fail_path" ]] || continue
            prev_failure_names["$(basename "$prev_fail_path")"]=1
        done
    fi
    for classifier_file in "$latest_failures_dir"/*/classifier.json; do
        [[ -f "$classifier_file" ]] || continue
        fail_name=$(basename "$(dirname "$classifier_file")")
        if [[ -d "$prev_failures_dir" ]] && [[ "${prev_failure_names[$fail_name]:-}" == "1" ]]; then
            continue
        fi
        is_real_bug=$(jq -r '.is_real_bug // false' "$classifier_file" 2>/dev/null || echo false)
        if [[ "$is_real_bug" == "false" ]]; then
            eir_classifier=$((eir_classifier + 1))
        fi
    done
fi

eir_total=$(echo "scale=4; $eir_deterministic + $eir_classifier" | bc 2>/dev/null || echo 0)

# ── Cauchy convergence check (W=3 window) ──────────────────────────────────

converged=0
if [[ ${#RUN_IDS[@]} -ge 4 ]]; then
    cauchy_epsilon="0.03"
    cauchy_count=0
    for i in 0 1 2; do
        run_a="${TRACE_DIR}/${RUN_IDS[$((i+1))]}"
        run_b="${TRACE_DIR}/${RUN_IDS[$i]}"
        # Refuse convergence when metrics are absent — a missing metric is
        # "no signal", not "zero delta". All-zero metrics would otherwise
        # yield norm = 0 < epsilon and spuriously count as converged.
        if ! metric_present "$run_a" "mutation_score" \
            || ! metric_present "$run_b" "mutation_score"; then
            continue
        fi
        ms_a=$(read_metric "$run_a" "mutation_score")
        ms_b=$(read_metric "$run_b" "mutation_score")
        cov_a=$(read_metric "$run_a" "coverage_pct")
        cov_b=$(read_metric "$run_b" "coverage_pct")
        d_ms=$(echo "scale=4; $ms_b - $ms_a" | bc 2>/dev/null || echo 0)
        d_cov=$(echo "scale=4; $cov_b - $cov_a" | bc 2>/dev/null || echo 0)
        # F7: cost term — only added when cost_tokens is present in both runs.
        # d_cost_norm = latest_cost_norm - prev_cost_norm; w_k=0.5.
        cost_term="0"
        if metric_present "$run_a" "cost_tokens" && metric_present "$run_b" "cost_tokens"; then
            ct_a=$(read_metric "$run_a" "cost_tokens")
            ct_b=$(read_metric "$run_b" "cost_tokens")
            if [[ "$HKASK_MAX_COST_SECONDS" -gt 0 ]] 2>/dev/null; then
                cn_a=$(echo "scale=4; $ct_a / $HKASK_MAX_COST_SECONDS" | bc 2>/dev/null || echo 0)
                cn_b=$(echo "scale=4; $ct_b / $HKASK_MAX_COST_SECONDS" | bc 2>/dev/null || echo 0)
                d_cost_norm=$(echo "scale=4; $cn_b - $cn_a" | bc 2>/dev/null || echo 0)
                cost_term=$(echo "scale=4; 0.5 * $d_cost_norm * $d_cost_norm" | bc 2>/dev/null || echo 0)
            fi
        fi
        norm=$(echo "scale=4; sqrt(1.0 * $d_cov * $d_cov + 2.0 * $d_ms * $d_ms + $cost_term)" | bc 2>/dev/null || echo 1)
        if [[ $(echo "$norm < $cauchy_epsilon" | bc 2>/dev/null || echo 0) -eq 1 ]]; then
            cauchy_count=$((cauchy_count + 1))
        fi
    done
    if [[ $cauchy_count -ge 3 ]]; then
        converged=1
    fi
fi

# ── Stall detector (W=3 window) ────────────────────────────────────────────

stalled=0
if [[ ${#RUN_IDS[@]} -ge 4 ]]; then
    stall_count=0
    for i in 0 1 2; do
        run_a="${TRACE_DIR}/${RUN_IDS[$((i+1))]}"
        run_b="${TRACE_DIR}/${RUN_IDS[$i]}"
        ms_a=$(read_metric "$run_a" "mutation_score")
        ms_b=$(read_metric "$run_b" "mutation_score")
        cov_a=$(read_metric "$run_a" "coverage_pct")
        cov_b=$(read_metric "$run_b" "coverage_pct")
        d_ms=$(echo "scale=4; $ms_b - $ms_a" | bc 2>/dev/null || echo 0)
        d_cov=$(echo "scale=4; $cov_b - $cov_a" | bc 2>/dev/null || echo 0)
        if [[ $(echo "$d_cov > 0.02" | bc 2>/dev/null || echo 0) -eq 1 ]] && \
           [[ $(echo "$d_ms <= 0" | bc 2>/dev/null || echo 0) -eq 1 ]]; then
            stall_count=$((stall_count + 1))
        fi
    done
    if [[ $stall_count -ge 3 ]]; then
        stalled=1
    fi
fi

# ── Verdict ────────────────────────────────────────────────────────────────
# EIR > 0 is checked BEFORE convergence: a halt must never be masked by
# spurious convergence on absent/all-zero metrics (design §9.5 #5).

if [[ $(echo "$eir_total > 0" | bc 2>/dev/null || echo 0) -eq 1 ]]; then
    echo "VERDICT: halt_escalate"
    echo "reason: EIR > 0 (test-bug introductions or mutant regressions)"
elif [[ "$converged" -eq 1 ]]; then
    echo "VERDICT: converged"
    echo "reason: Cauchy criterion met (weighted norm < 0.03 for 3 consecutive iterations)"
elif [[ "$stalled" -eq 1 ]]; then
    echo "VERDICT: stalled_escalate"
    echo "reason: coverage climbing while mutation score flat for 3 consecutive iterations"
else
    echo "VERDICT: proceed"
    echo "reason: EIR = 0, no stall, no regression violations"
fi

echo "ECR: $ecr"
echo "EIR: $eir_total"
echo "EIR_deterministic: $eir_deterministic"
echo "EIR_classifier: $eir_classifier"
echo "Acc: $mutation_score"
echo "prev_Acc: $prev_mutation_score"
echo "mutation_score: $mutation_score"
echo "coverage_pct: $coverage_pct"
echo "delta_coverage: $delta_coverage"
echo "delta_mutation: $delta_mutation"
echo "cost_tokens: $cost_tokens"
echo "cost_norm: $cost_norm"