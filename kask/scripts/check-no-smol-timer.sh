#!/usr/bin/env bash
# CI gate: no `smol::Timer` on the fork's GPUI-side surface (the zed-path
# hkask crates + kask_bridge). smol timers are not tracked by GPUI's
# scheduler and break `run_until_parked()` in GPUI tests (.rules, "Timers
# in tests") — any smol timer in GPUI-side code is that trap. The kask
# tree's MCP servers are out of scope: they run the smol runtime without
# GPUI, where smol timers are legitimate.
#
# Exit codes: 0 = clean, 1 = a hit (or the scan errored — grep rc 2 —
# never a silent pass).
set -euo pipefail
cd "$(dirname "$0")/.."

SCAN_ROOTS=( ${SCAN_ROOTS:-../crates/hkask-* crates/kask_bridge} )

# grep exit codes: 0 = hits, 1 = clean zero, 2 = error — only 2 is a
# failure of the scan itself; the distinction is the fail-closed capture.
set +e
hits="$(grep -rn 'smol::Timer' "${SCAN_ROOTS[@]}" --include='*.rs' 2>/dev/null)"
rc=$?
set -e
if [ "$rc" -eq 2 ]; then
    echo "FAIL: the smol::Timer scan errored (grep rc=2) — a scan failure is never a silent pass." >&2
    exit 1
fi
if [ -n "$hits" ]; then
    printf '%s\n' "$hits" | sed 's/^/FAIL: /' >&2
    echo "FAIL: smol::Timer on the GPUI-side fork surface — use cx.background_executor().timer(duration) in GPUI code (.rules, Timers in tests)." >&2
    exit 1
fi
echo "OK: no smol::Timer on the fork's GPUI-side surface."
