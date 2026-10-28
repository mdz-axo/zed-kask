#!/usr/bin/env bash
# Build CPU/RSS trace sampler — the build observes itself.
#
# Started in the background by install.sh's build_hkask, this samples the
# BUILD-OWNED process tree — the descendants of its parent shell
# (install.sh), minus the sampler's own subtree — every interval and appends
# one CSV row per sample. The trace file IS the observability record for
# build CPU burn: an install that pegs the machine leaves per-sample CPU%/RSS
# numbers behind (the peak is the max of the cpu/rss columns — e.g.
# `sort -t, -k3 -g "$TRACE" | tail -1`), not a user report. Pure bash + ps +
# /proc — no dependencies.
#
# Attribution and units (2026-10-06 rewrite). The former sampler matched
# process NAMES system-wide (counting the editor's rust-analyzer as build
# load) and summed ps pcpu — a process's LIFETIME-average CPU, not its
# utilization in the interval. This sampler instead:
#   - restricts sampling to the build's own process tree, so sibling
#     processes (the editor's rust-analyzer) are never attributed to the
#     build, and tree members are counted whatever they are named;
#   - computes total_cpu_pct as INTERVAL utilization: the per-pid cputime
#     delta (utime+stime from /proc/<pid>/stat) between samples over the
#     elapsed wall time, in percent-of-one-core summed over the tree —
#     comparable to the old summed pcpu, but measured per interval.
# A process that exits mid-interval is undercounted by its final partial
# interval (negligible at the default 2s cadence). A pid that vanishes
# between the tree listing and its stat read (constant under cargo — the
# build spawns short-lived children) is skipped silently: the read silences
# stderr BEFORE the input redirect, because bash applies redirections left
# to right and a failed `< /proc/...` would otherwise print ENOENT to the
# install shell's stderr, interleaving with cargo's output (observed
# 2026-10-06). Column rename: the old n_compile_procs (name-matched) is now
# n_build_procs (tree members) — pre-rewrite traces carry the old column
# name.
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
# Output: CSV at <trace-file>, one row per interval (the first row appears
# after one full interval — deltas need two samples):
#   timestamp,n_build_procs,total_cpu_pct,total_rss_mb,busiest_proc

set -uo pipefail
# NOT set -e: one bad sample must not kill the sampler.

TRACE_FILE="${1:?usage: build-monitor.sh <trace-file> [interval-secs]}"
INTERVAL="${2:-2}"

CLK_TCK="$(getconf CLK_TCK 2>/dev/null || echo 100)"
PAGE_KB=$(( $(getconf PAGESIZE 2>/dev/null || echo 4096) / 1024 ))

echo "timestamp,n_build_procs,total_cpu_pct,total_rss_mb,busiest_proc" > "$TRACE_FILE"

# Per-pid cputime (clock ticks) at the previous sample.
declare -A prev_ticks
prev_now="$(date +%s)"

# sample_tree — the build-owned pids: descendants of $PPID (the install.sh
# shell that started this sampler), minus the sampler's own subtree (the
# sampler and its `sleep` child are tree members but not build load).
sample_tree() {
    ps -eo pid,ppid --no-headers 2>/dev/null | awk -v root="$PPID" -v self="$$" '
        { ppid[$1] = $2; list[n++] = $1 }
        END {
            in_tree[root] = 1
            changed = 1
            while (changed) {
                changed = 0
                for (i = 0; i < n; i++) {
                    p = list[i]
                    if (!(p in in_tree) && (ppid[p] in in_tree)) { in_tree[p] = 1; changed = 1 }
                }
            }
            in_self[self] = 1
            changed = 1
            while (changed) {
                changed = 0
                for (i = 0; i < n; i++) {
                    p = list[i]
                    if (!(p in in_self) && (ppid[p] in in_self)) { in_self[p] = 1; changed = 1 }
                }
            }
            for (i = 0; i < n; i++) {
                p = list[i]
                if ((p in in_tree) && !(p in in_self)) print p
            }
        }'
}

# Exit when the parent is gone (install.sh finished or crashed).
while kill -0 "$PPID" 2>/dev/null; do
    sleep "$INTERVAL"
    kill -0 "$PPID" 2>/dev/null || break

    now="$(date +%s)"
    elapsed=$(( now - prev_now ))
    [ "$elapsed" -gt 0 ] || elapsed=1

    unset cur_ticks
    declare -A cur_ticks
    n_procs=0
    total_delta=0
    total_rss_kb=0
    busiest_pid=""
    busiest_delta=0

    for pid in $(sample_tree); do
        line=""
        # stderr silenced before the input redirect: bash applies
        # redirections left to right, so `< file 2>/dev/null` would leak the
        # shell's ENOENT for a pid that exited after listing.
        read -r line 2>/dev/null < "/proc/$pid/stat" || continue
        line="${line#*) }"          # strip "pid (comm) " — comm may contain spaces
        [ -n "$line" ] || continue
        set -- $line
        ticks=$(( ${12} + ${13} ))  # utime + stime, clock ticks
        cur_ticks[$pid]=$ticks
        n_procs=$((n_procs + 1))
        total_rss_kb=$((total_rss_kb + ${22} * PAGE_KB ))
        delta=$(( ticks - ${prev_ticks[$pid]:-0} ))
        total_delta=$((total_delta + delta))
        if [ "$delta" -gt "$busiest_delta" ]; then
            busiest_delta=$delta
            busiest_pid=$pid
        fi
    done

    busiest="-"
    if [ -n "$busiest_pid" ]; then
        bline=""
        if read -r bline 2>/dev/null < "/proc/$busiest_pid/stat"; then
            bcomm="${bline#*\(}"
            busiest="${bcomm%%\)*}"
        fi
    fi

    # Interval utilization: percent-of-one-core summed over the tree.
    cpu_x10=$(( total_delta * 1000 / (CLK_TCK * elapsed) ))
    cpu_pct="$(printf '%d.%d' $((cpu_x10 / 10)) $((cpu_x10 % 10)))"
    rss_mb="$(printf '%d.%d' $((total_rss_kb / 1024)) $(((total_rss_kb % 1024) * 10 / 1024)))"

    echo "$(date +%H:%M:%S),$n_procs,$cpu_pct,$rss_mb,$busiest" >> "$TRACE_FILE"

    # This sample becomes the next baseline.
    unset prev_ticks
    declare -A prev_ticks
    for pid in "${!cur_ticks[@]}"; do
        prev_ticks[$pid]=${cur_ticks[$pid]}
    done
    prev_now=$now
done
