#!/usr/bin/env bash
# Build CPU/RSS trace sampler — the build observes itself.
#
# Started in the background by install.sh's build_hkask, this samples the
# compile processes every interval and appends one CSV row per sample. The
# trace file IS the observability record for build CPU burn: an install that
# pegs the machine leaves per-sample CPU%/RSS numbers behind (the peak is
# the max of the cpu/rss columns — e.g. `sort -t, -k3 -g "$TRACE" | tail -1`),
# not a user report. Pure bash + ps — no dependencies.
#
# Self-terminating: exits when the parent shell (install.sh) is gone, so a
# crashed build never leaks the sampler. install.sh stops the sampler with
# SIGTERM at build end so the trace covers exactly the build; there is no
# exit-time summary epilogue — under that stop protocol one never ran (the
# SIGTERM kills the sampler mid-loop; a former epilogue only executed when
# install.sh itself died, the opposite of a completed install; verified by
# probe 2026-09-27).
#
# Usage: build-monitor.sh <trace-file> [interval-secs]
# Output: CSV at <trace-file>, one row per sample:
#   timestamp,n_compile_procs,total_cpu_pct,total_rss_mb,busiest_proc

set -uo pipefail
# NOT set -e: one bad ps sample must not kill the sampler.

TRACE_FILE="${1:?usage: build-monitor.sh <trace-file> [interval-secs]}"
INTERVAL="${2:-2}"

echo "timestamp,n_compile_procs,total_cpu_pct,total_rss_mb,busiest_proc" > "$TRACE_FILE"

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

    sleep "$INTERVAL"
done
