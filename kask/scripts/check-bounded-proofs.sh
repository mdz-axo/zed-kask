#!/usr/bin/env bash
# Explicit local proof run, not a promotion/approval gate. No automatic installs.
# Usage: check-bounded-proofs.sh NEW_ARTIFACT_DIRECTORY [CRATE]
#   CRATE selects the harness manifest; it defaults to hkask-mcp-training
#   (the original, backward-compatible invocation). Add new proof-bearing
#   crates by extending the case statement with their sources, harnesses, and
#   scope line — never by loosening the shared budget discipline below.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
if [ "$#" -ne 1 ] && [ "$#" -ne 2 ]; then
    echo 'usage: check-bounded-proofs.sh NEW_ARTIFACT_DIRECTORY [CRATE]' >&2
    exit 2
fi
CRATE="${2:-hkask-mcp-training}"
cd "$ROOT"
case "$CRATE" in
    hkask-mcp-training)
        SOURCES=(
            kask/mcp-servers/hkask-mcp-training/src/lora_validation/param_gates.rs
            kask/mcp-servers/hkask-mcp-training/src/providers/types.rs
            kask/mcp-servers/hkask-mcp-training/src/hkask_mcp_training.rs
            Cargo.lock
        )
        HARNESSES=(
            gm3_refuse_iff_degenerate_scaling
            gm4_findings_follow_rank_thresholds
            gm1_clean_iff_noop_init
            safe_region_has_no_refusals
            gm2_warns_iff_bias_breaks_merge
        )
        SCOPE='Scope: shared G-M1..G-M4 decision core only; diagnostics are regression-tested.'
        ;;
    hkask-types)
        SOURCES=(
            kask/crates/hkask-types/src/json_extract.rs
            kask/crates/hkask-types/Cargo.toml
            Cargo.lock
        )
        HARNESSES=(
            balanced_scan_never_panics_on_valid_utf8
            balanced_result_is_container_delimited
        )
        SCOPE='Scope: hkask-types JSON-extraction scan core only (find_balanced_json); the fence-stripping wrapper is covered by the property and example layers.'
        ;;
    hkask-forecast)
        SOURCES=(
            kask/crates/hkask-forecast/src/hkask_forecast.rs
            kask/crates/hkask-forecast/Cargo.toml
            Cargo.lock
        )
        HARNESSES=(
            marginalize_two_parents_stays_in_the_unit_hull
        )
        SCOPE='Scope: hkask-forecast marginalization core only (marginalize, 2-parent bound); Fermi/Brier/Wilson surfaces are covered by the property layer.'
        ;;
    *)
        echo "unknown crate '$CRATE' — add a manifest in check-bounded-proofs.sh" >&2
        exit 2
        ;;
esac
version=$(cargo kani --version)
if [[ "$version" != *'Kani Rust Verifier 0.68.0'* ]]; then
    echo "Expected pinned Kani 0.68.0; got: $version" >&2
    exit 1
fi
command -v timeout >/dev/null
command -v prlimit >/dev/null
mkdir "$1" # refuse to overwrite a prior run's evidence
OUT="$(cd "$1" && pwd)"
sha256sum "${SOURCES[@]}" > "$OUT/source-sha256.txt"
git diff HEAD -- "${SOURCES[@]}" > "$OUT/source.diff"
{
    git rev-parse HEAD
    printf '%s\n' "$version"
    printf 'Crate: %s\n' "$CRATE"
    printf '%s\n' "$SCOPE"
    printf 'Each run: 2 GiB address space, 120s wall limit, unwind=12; default safety/unwind checks.\n'
} > "$OUT/manifest.txt"
overall=0
for harness in "${HARNESSES[@]}"; do
    date -u +%FT%TZ > "$OUT/$harness.started"
    command=(cargo kani -p "$CRATE" --lib --harness "$harness" --default-unwind 12)
    if [ -n "${CARGO_TARGET_DIR:-}" ]; then
        command+=(--target-dir "$CARGO_TARGET_DIR")
    fi
    {
        printf 'CARGO_BUILD_JOBS=2 timeout -k 5s 120s prlimit --as=2147483648 -- '
        printf '%q ' "${command[@]}"
        printf '\n'
    } >> "$OUT/manifest.txt"
    status=0
    CARGO_BUILD_JOBS=2 timeout -k 5s 120s prlimit --as=2147483648 -- "${command[@]}" \
        > "$OUT/$harness.log" 2>&1 || status=$?
    printf '%s\n' "$status" > "$OUT/$harness.exit"
    date -u +%FT%TZ > "$OUT/$harness.finished"
    if [ "$status" -eq 0 ] && grep -q '^Complete - 1 successfully verified harnesses, 0 failures, 1 total\.' "$OUT/$harness.log"; then
        printf '%s: verified decision-core obligation\n' "$harness"
    else
        printf '%s: NOT VERIFIED (exit %s); inspect raw log, failure need not be a counterexample\n' "$harness" "$status" >&2
        overall=1
    fi
done
if ! sha256sum --check "$OUT/source-sha256.txt" > "$OUT/source-recheck.log" 2>&1; then
    echo 'Source changed during verification; results are stale' >&2
    overall=1
fi
(cd "$OUT" && sha256sum -- *.log *.exit *.started *.finished source-sha256.txt source.diff manifest.txt > SHA256SUMS)
exit "$overall"