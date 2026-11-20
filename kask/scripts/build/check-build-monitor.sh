#!/bin/bash
# Regression test: build-monitor.sh attribution and units.
#
# Pins the 2026-10-06 rewrite's three contracts:
#   1. ATTRIBUTION: only the build-owned tree (descendants of the monitor's
#      parent shell) is sampled. A CPU-burning decoy named `rust-analyzer`
#      OUTSIDE the tree — the exact process class the former name-matching
#      sampler misattributed — must not be counted.
#   2. UNITS: total_cpu_pct is interval utilization (per-pid cputime delta
#      over elapsed wall time), so CPU-burning children yield > 0 on every
#      row.
#   3. SILENCE: a tree pid that vanishes between the listing and its
#      /proc/<pid>/stat read (constant under cargo — short-lived children)
#      is skipped without stderr noise. The sampler's stderr is inherited
#      by the install shell, so a leaked ENOENT interleaves with cargo's
#      compile output (observed live 2026-10-06).
#
# The stop protocol mirrors install.sh's: the monitor is SIGTERMed first,
# then the build shell — so no race row can be emitted after the kill.
# Runtime: ~9s.

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
errors=0
fail() { echo "FAIL: $1" >&2; errors=$((errors + 1)); }

sandbox="$(mktemp -d)"
trap 'rm -rf "$sandbox"' EXIT
trace="$sandbox/trace.csv"

# The "install shell": starts the monitor (as install.sh does), then runs two
# CPU burners as its own children (the "build"), recording pids for
# deterministic cleanup.
parent_script="$sandbox/parent.sh"
cat > "$parent_script" <<EOF
#!/bin/bash
bash "$script_dir/build-monitor.sh" "$trace" 1 &
echo \$! > "$sandbox/monitor.pid"
while [ \$SECONDS -lt 30 ]; do :; done &
echo \$! > "$sandbox/burner1.pid"
while [ \$SECONDS -lt 30 ]; do :; done &
echo \$! > "$sandbox/burner2.pid"
wait
EOF
bash "$parent_script" &
parent_pid=$!

# The decoy: a CPU burner named rust-analyzer, a SIBLING of the install
# shell (a child of this test), not a descendant — the editor's language
# server is never part of the build's process tree.
decoy="$sandbox/rust-analyzer"
printf '#!/bin/bash\nwhile [ $SECONDS -lt 30 ]; do :; done\n' > "$decoy"
chmod +x "$decoy"
"$decoy" &
decoy_pid=$!

sleep 4

# install.sh's stop protocol: monitor first, then the build shell.
kill "$(cat "$sandbox/monitor.pid")" 2>/dev/null || true
sleep 0.3
kill "$parent_pid" "$decoy_pid" 2>/dev/null || true
wait "$parent_pid" "$decoy_pid" 2>/dev/null || true
for f in "$sandbox"/burner*.pid; do
    [ -f "$f" ] && kill "$(cat "$f")" 2>/dev/null || true
done

# --- assertions ------------------------------------------------------------
[ -s "$trace" ] || { echo "FAIL: no trace rows emitted"; exit 1; }
[ "$(head -1 "$trace")" = "timestamp,n_build_procs,total_cpu_pct,total_rss_mb,busiest_proc" ] \
    || fail "unexpected CSV header: $(head -1 "$trace")"

rows=$(($(wc -l < "$trace") - 1))
[ "$rows" -ge 2 ] || fail "expected >= 2 sample rows, got $rows"

# The tree is exactly: the install shell + 2 burners (the monitor's own
# subtree is excluded). The out-of-tree decoy must never appear — if it
# did, n_build_procs would read 4.
while IFS=, read -r _ts n cpu _rss busiest; do
    [ "$n" = "3" ] || fail "expected 3 build-tree procs (install shell + 2 burners), got $n"
    awk -v v="$cpu" 'BEGIN { exit !(v > 50) }' || fail "interval CPU% not positive under burn: $cpu"
    [ "$busiest" != "-" ] || fail "busiest_proc missing under burn"
done < <(tail -n +2 "$trace")

# --- contract 3: a vanished pid is skipped silently -------------------------
#
# The listing→read race, constructed rather than hoped for: the tree is
# padded with 800 sleepers so the monitor's per-sample window (ps scan ~23ms
# + awk + read loop ~25ms) is tens of ms wide. The 24 sacrificial members
# are spawned last (highest pids → listed and read last), and a row-synced
# sweep of kills at ~6ms spacing covers the window: each kill lands between
# a sacrificial pid's listing and its read, so its /proc/<pid>/stat is gone
# at read time — the exact race a real build hits constantly (short-lived
# rustc/cc children; observed live 2026-10-06). The monitor's stderr is
# captured to a file; it must stay empty. A kill that misses the window is a
# harmless no-op (the pid is either not listed or already read), so this
# check can never false-fail.
race_trace="$sandbox/race-trace.csv"
race_parent_script="$sandbox/race-parent.sh"
cat > "$race_parent_script" <<EOF
#!/bin/bash
trap 'kill \$(jobs -p) 2>/dev/null' EXIT
bash "$script_dir/build-monitor.sh" "$race_trace" 1 2> "$sandbox/race.stderr" &
echo \$! > "$sandbox/race-monitor.pid"
for i in \$(seq 800); do sleep 12 & done
for i in \$(seq 24); do sleep 12 & echo \$! >> "$sandbox/race-sac.pid"; done
wait
EOF
bash "$race_parent_script" &
race_parent_pid=$!

# Row-sync to the monitor: after the 2nd row lands, the next sample's ps
# starts ~1s later. Sweep the kills across that sample's window.
sleep 1.9
for _ in $(seq 200); do
    [ "$(wc -l < "$race_trace")" -ge 3 ] && break
    sleep 0.005
done
sleep 1.0
for spid in $(cat "$sandbox/race-sac.pid"); do
    kill "$spid" 2>/dev/null || true
    sleep 0.005
done
sleep 1

kill "$(cat "$sandbox/race-monitor.pid")" 2>/dev/null || true
sleep 0.3
kill "$race_parent_pid" 2>/dev/null || true
wait "$race_parent_pid" 2>/dev/null || true

[ -s "$race_trace" ] || fail "race scenario: no trace rows emitted"
if [ -s "$sandbox/race.stderr" ]; then
    fail "monitor leaked stderr when a listed pid vanished before its read (first line: $(head -1 "$sandbox/race.stderr"))"
fi

if [ "$errors" -gt 0 ]; then
    echo "REGRESSION: $errors build-monitor failure(s) detected." >&2
    exit 1
fi

echo "PASS: build-monitor samples the build-owned tree with interval utilization; out-of-tree decoys are not attributed; vanished pids are skipped silently."
