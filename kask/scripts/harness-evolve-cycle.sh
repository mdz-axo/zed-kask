#!/usr/bin/env bash
# Harness evolve cycle — orchestrates the test harness self-improvement loop.
#
# BROKEN since 4304db290f (2026-08-04): this script calls `./scripts/test --trace`
# at L52, but `kask/scripts/test` was deleted in that commit. The cycle will fail
# at step 1 with "no such file" until `scripts/test` is rebuilt per
# kask/docs/plans/evolving-test-harness.md §2.2. The `hkask-test-harness` crate
# and `kask/scripts/stability-gate.sh` survive and remain functional. See the
# plan doc's 2026-08-04 status revision for the revival path.
#
# This script is the runner for the harness-evolve-cycle. It handles the
# deterministic parts (run tests, run stability gate, branch, loop) and
# outputs instructions for the agent when the harness-optimize skill should
# be invoked.
#
# Usage: harness-evolve-cycle.sh [--max-iterations N] [--trace-dir DIR]
#
# Design: kask/docs/plans/evolving-test-harness.md §3.7
# Manifest: kask/registry/manifests/harness-evolve-cycle.yaml
set -euo pipefail

cd "$(dirname "$0")/.."

MAX_ITERATIONS=5
TRACE_DIR="${HKASK_TRACE_DIR:-traces}"
ITERATION=0
RUN_HISTORY=()  # accumulates run-ids for the stability gate
HISTORY_FILE="${TRACE_DIR}/.run-history"  # persists across invocations

while [[ $# -gt 0 ]]; do
    case "$1" in
        --max-iterations) MAX_ITERATIONS="$2"; shift 2 ;;
        --trace-dir) TRACE_DIR="$2"; shift 2 ;;
        *) break ;;
    esac
done

export HKASK_TRACE_DIR="$TRACE_DIR"

# Load persisted run history so the loop survives cross-invocation handoffs
# (the proceed branch exits for agent action; the agent re-invokes this script).
if [[ -f "$HISTORY_FILE" ]]; then
    mapfile -t RUN_HISTORY < "$HISTORY_FILE"
fi
# Iteration count reflects total runs across invocations (not in-process only).
ITERATION=${#RUN_HISTORY[@]}

echo "=== Harness Evolve Cycle ==="
echo "Max iterations: $MAX_ITERATIONS"
echo "Trace dir: $TRACE_DIR"
echo ""

while true; do
    ITERATION=$((ITERATION + 1))
    echo "--- Iteration $ITERATION / $MAX_ITERATIONS ---"

    # Step 1: Run tests with trace
    echo "[step 1] Running tests with --trace..."
    if ! ./scripts/test --trace >/dev/null 2>&1; then
        echo "[step 1] Tests failed (some tests did not pass) — continuing to stability gate"
    fi

    # Find the latest run-id (newest directory in trace dir)
    LATEST_RUN=$(ls -t "$TRACE_DIR" 2>/dev/null | head -1 || echo "")
    if [[ -z "$LATEST_RUN" ]]; then
        echo "ERROR: no trace directory found after test run" >&2
        exit 1
    fi
    RUN_HISTORY=("$LATEST_RUN" "${RUN_HISTORY[@]}")
    # Persist so the next invocation has N-1 available (F4: bootstrap N-1).
    mkdir -p "$TRACE_DIR"
    printf '%s\n' "${RUN_HISTORY[@]}" > "$HISTORY_FILE"

    # F5: qa-triage failure-splitting step.
    # Parse the nextest JSON for failed tests and split each failure into
    # failures/<test-name>/output.txt so the classifier manifest step and the
    # stability gate's eir_classifier loop can find them. This is the SHELL part
    # only — the LLM classification (classifier.json) is a manifest step.
    LATEST_TRACE="${TRACE_DIR}/${LATEST_RUN}"
    NEXTEST_JSON="${LATEST_TRACE}/nextest-output.json"
    if [[ -f "$NEXTEST_JSON" ]] && command -v jq &>/dev/null; then
        while IFS=$'\t' read -r test_name test_output; do
            [[ -n "$test_name" ]] || continue
            # Sanitize test name for use as a directory name (replace path separators).
            safe_name=$(echo "$test_name" | tr '/\\:' '___')
            fail_dir="${LATEST_TRACE}/failures/${safe_name}"
            mkdir -p "$fail_dir"
            printf '%s\n' "$test_output" > "${fail_dir}/output.txt"
        done < <(jq -r '
            select(.type == "test" and .event == "finished") |
            select(.status == "failed" or (.status | type == "object" and has("Failed"))) |
            .name as $name |
            (
                if (.status | type == "object") and (.status | has("Failed"))
                then (.status.Failed.stdout // "") + "\n" + (.status.Failed.stderr // "")
                else (.stdout // "") + "\n" + (.stderr // "")
                end
            ) as $output |
            $name + "\t" + $output
        ' "$NEXTEST_JSON" 2>/dev/null || true)
    fi

    # Step 2: Run stability gate (requires >=2 run-ids; bootstrap on first run)
    echo "[step 2] Running stability gate..."
    GATE_OUTPUT=""
    if [[ ${#RUN_HISTORY[@]} -lt 2 ]]; then
        GATE_OUTPUT="VERDICT: proceed
reason: bootstrap — first run, no N-1 to compare yet"
    else
        GATE_ARGS=()
        for run_id in "${RUN_HISTORY[@]:0:4}"; do
            GATE_ARGS+=("$run_id")
        done

        GATE_OUTPUT=$(./scripts/stability-gate.sh "${GATE_ARGS[@]}" 2>&1 || true)
    fi
    echo "$GATE_OUTPUT"
    echo ""

    # Re-read the verdict (either from the gate or the bootstrap branch above).
    VERDICT=$(echo "$GATE_OUTPUT" | grep '^VERDICT:' | cut -d' ' -f2)
    if [[ -z "$VERDICT" ]]; then
        VERDICT="proceed"
    fi

    # Step 3: Branch on verdict
    case "$VERDICT" in
        converged)
            echo "[step 5] CONVERGED — Cauchy criterion met."
            echo "The test harness has stabilized. No further improvement needed."
            exit 0
            ;;

        stalled_escalate)
            echo "[step 4] STALLED — coverage climbing while mutation score flat."
            echo "ALGEDONIC: escalating to human operator."
            echo "The harness is generating tests that pass but don't catch bugs."
            echo "Action needed: review the trace data and adjust the improvement strategy."
            exit 1
            ;;

        halt_escalate)
            echo "[step 4] HALT — EIR > 0 (test-bug introductions or mutant regressions)."
            echo "ALGEDONIC: escalating to human operator."
            echo "The last revision introduced new test failures or weakened the suite."
            echo "Action needed: review the EIR breakdown and authorize or reject."
            exit 1
            ;;

        regression_violated)
            echo "[step 4] REGRESSION VIOLATED — an enforced RR-*.yaml entry broke."
            echo "ALGEDONIC: escalating to human operator."
            echo "The last revision broke an existing security regression test."
            echo "Action needed: fix the regression before continuing."
            exit 1
            ;;

        proceed)
            if [[ $ITERATION -ge $MAX_ITERATIONS ]]; then
                echo "[step 7] ITERATION CAP REACHED ($MAX_ITERATIONS)."
                echo "ALGEDONIC: iteration cap reached without Cauchy convergence."
                echo "The harness has not converged after $MAX_ITERATIONS iterations."
                echo "Action needed: review the improvement trajectory and decide whether to continue."
                exit 1
            fi

            # Step 3: Invoke harness-optimize skill
            PREV_RUN="${RUN_HISTORY[1]:-}"
            echo "[step 3] PROCEED — invoke harness-optimize skill."
            echo ""
            echo ">>> AGENT ACTION REQUIRED <<<"
            echo "Invoke the 'harness-optimize' skill with these inputs:"
            echo "  trace_dir_n: ${TRACE_DIR}/${LATEST_RUN}"
            if [[ -n "$PREV_RUN" ]]; then
                echo "  trace_dir_n_minus_1: ${TRACE_DIR}/${PREV_RUN}"
            else
                echo "  trace_dir_n_minus_1: ${TRACE_DIR}/${LATEST_RUN}"
            fi
            echo "  task: 'Improve the test suite based on the trace data. Focus on"
            echo "    under-tested functions with surviving mutants.'"
            echo ""
            echo "After harness-optimize proposes a diff:"
            echo "  1. Apply the proposed test changes"
            echo "  2. Re-run this script to continue the loop"
            echo ""
            exit 0
            ;;

        *)
            echo "ERROR: unknown verdict from stability gate: $VERDICT" >&2
            exit 2
            ;;
    esac
done