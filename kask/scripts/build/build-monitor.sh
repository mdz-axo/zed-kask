#!/usr/bin/env bash
# Build CPU/RSS trace sampler — the build observes itself.
#
# Started in the background by install.sh's build_hkask, this samples the
# compile processes every interval and writes a CSV trace plus a summary
# (peak CPU%, peak RSS, wall time). The trace is the observability surface
# for build CPU burn: an install that pegs the machine leaves a number
# behind, not a vibe. Pure bash + ps — no dependencies.
#
# Self-terminating: exits when the parent shell (install.sh) is gone, so a
# crashed build never leaks the sampler.
#
# Usage: build-monitor.sh <trace-file> [interval-secs]
# Output: CSV at <trace-file>; summary appended + echoed on exit.
#   timestamp,n_compile_procs,total_cpu_pct,total_rss_mb,busiest_proc

set -uo pipefail
# NOT set -e: one bad ps sample must not kill the sampler.

TRACE_FILE="${1:?usage: build-monitor.sh <trace-file> [interval-secs]}"
INTERVAL="${2:-2}"
STARTED_AT=$(date +%s)

echo "timestamp,n_compile_procs,total_cpu_pct,total_rss_mb,busiest_proc" > "$TRACE_FILE"

PEAK_CPU=0
PEAK_RSS=0
SAMPLES=0

# Exit when the parent is gone (install.sh finished or crashed).
while kill -0 "$PPID" 2>/dev/null; do
    # One awk pass over the process table: count compile processes, sum
    # CPU% and RSS, and name the busiest. Matches the toolchain binaries
    # cargo spawns (rustc, clippy-driver, cc/ld for link steps) plus
    # cargo itself and rust-analyzer — the processes that burn a build.
    STATS=$(ps -eo pcpu,rss,comm --no-headers 2>/dev/null | awk '
        $3 ~ /^(rustc|clippy-driver|cargo|rust-analyzer|cc|ld|ld\.gold|mold)$/ {
            cpu += $1; rss += $2; n++
            if ($1 > max_cpu) { max_cpu = $1; max_name = $3 }
        }
        END {
            printf "%d %.1f %.1f %s", n, cpu, rss / 1024, (max_name == "" ? "-" : max_name)
        }')

    N_PROCS=${STATS%% *}
    REST=${STATS#* }
    TOTAL_CPU=${REST%% *}
    REST=${REST#* }
    TOTAL_RSS=${REST%% *}
    BUSIEST=${REST#* }

    echo "$(date +%H:%M:%S),$N_PROCS,$TOTAL_CPU,$TOTAL_RSS,$BUSIEST" >> "$TRACE_FILE"
    SAMPLES=$((SAMPLES + 1))

    PEAK_CPU=$(awk -v a="$TOTAL_CPU" -v b="$PEAK_CPU" 'BEGIN { print (a > b) ? a : b }')
    PEAK_RSS=$(awk -v a="$TOTAL_RSS" -v b="$PEAK_RSS" 'BEGIN { print (a > b) ? a : b }')

    sleep "$INTERVAL"
done

WALL_SECS=$(( $(date +%s) - STARTED_AT ))
{
    echo "# summary"
    echo "# wall_secs=$WALL_SECS samples=$SAMPLES peak_cpu_pct=$PEAK_CPU peak_rss_mb=$PEAK_RSS"
} >> "$TRACE_FILE"
echo "build monitor: ${SAMPLES} samples over ${WALL_SECS}s — peak CPU ${PEAK_CPU}% , peak RSS ${PEAK_RSS}MB; trace: $TRACE_FILE"
