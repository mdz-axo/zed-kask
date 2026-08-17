#!/usr/bin/env bash
# Self-test for the skill-span-namespace gate (check-skill-span-namespace.sh).
#
# Institutionalizes the .rules trap "A CI gate must be shown to fail before its
# status: enforced is trusted" — applied to check-skill-span-namespace.sh per
# CI gate sweep follow-up issue #6. The gate's "0 = 0" failure mode (zero
# manifests in MANIFEST_DIR → zero violations → trivially passes) was flagged
# in the sweep. This self-test pins both failure modes:
#   1. Namespace drift: a manifest whose `ledger.span_namespace` does NOT
#      match `reg.skill.<manifest.id>`.
#   2. Abolished spans: a manifest carrying a `ledger.spans` list (the
#      abolished form) must also fail.
#   3. Empty: a MANIFEST_DIR with zero manifests must exit 0 (no violations)
#      without crashing — pinning the silent-disconnection path.
#
# Design: temp MANIFEST_DIR is populated with minimal YAML manifests, and the
# gate is invoked with MANIFEST_DIR pointed at the temp dir. Each case asserts
# the gate exits with the expected code and keyword.
#
# Exit codes:
#   0 — all cases behaved as expected (gate is alive)
#   1 — at least one case did not behave as expected (gate is vacuous or broken)
#
# Usage: bash scripts/check-skill-span-namespace-selftest.sh

set -euo pipefail

KASK_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GATE="$KASK_ROOT/scripts/check-skill-span-namespace.sh"

if [ ! -f "$GATE" ]; then
  echo "FAIL: gate not found at $GATE"
  exit 1
fi

TMPDIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMPDIR"
}
trap cleanup EXIT

failures=0

# ──────────────────────────────────────────────────────────────────────────
# Case 1: namespace drift — span_namespace does NOT match reg.skill.<id>.
# The gate must exit 1 with "FAIL".
# ──────────────────────────────────────────────────────────────────────────
mkdir -p "$TMPDIR/manifests-drift"
cat > "$TMPDIR/manifests-drift/selftest-drift.yaml" <<'EOF'
manifest:
  id: selftest-drift
ledger:
  span_namespace: reg.skill.something-else
EOF

set +e
CASE1_OUT=$(MANIFEST_DIR="$TMPDIR/manifests-drift" bash "$GATE" 2>&1)
CASE1_RC=$?
set -e

if [ "$CASE1_RC" -ne 1 ]; then
  echo "FAIL (case 1 — namespace drift): expected exit 1, got $CASE1_RC"
  printf '%s\n' "$CASE1_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
elif ! printf '%s\n' "$CASE1_OUT" | grep -q "FAIL"; then
  echo "FAIL (case 1 — namespace drift): exit 1 but 'FAIL' not in output"
  printf '%s\n' "$CASE1_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
else
  echo "OK (case 1 — namespace drift): gate detected span_namespace drift"
fi

# ──────────────────────────────────────────────────────────────────────────
# Case 2: abolished spans list — a manifest carrying `ledger.spans` must
# fail with "FAIL" (the spans: list is abolished).
# ──────────────────────────────────────────────────────────────────────────
mkdir -p "$TMPDIR/manifests-spans"
cat > "$TMPDIR/manifests-spans/selftest-spans.yaml" <<'EOF'
manifest:
  id: selftest-spans
ledger:
  span_namespace: reg.skill.selftest-spans
  spans:
    - reg.skill.selftest-spans.something
EOF

set +e
CASE2_OUT=$(MANIFEST_DIR="$TMPDIR/manifests-spans" bash "$GATE" 2>&1)
CASE2_RC=$?
set -e

if [ "$CASE2_RC" -ne 1 ]; then
  echo "FAIL (case 2 — abolished spans): expected exit 1, got $CASE2_RC"
  printf '%s\n' "$CASE2_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
elif ! printf '%s\n' "$CASE2_OUT" | grep -q "FAIL"; then
  echo "FAIL (case 2 — abolished spans): exit 1 but 'FAIL' not in output"
  printf '%s\n' "$CASE2_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
else
  echo "OK (case 2 — abolished spans): gate detected the abolished spans: list"
fi

# ──────────────────────────────────────────────────────────────────────────
# Case 3: empty MANIFEST_DIR — zero manifests must exit 0 (no violations)
# without crashing. This pins the silent-disconnection path: 0 manifests = 0
# violations is the correct verdict here, NOT a vacuous pass.
# ──────────────────────────────────────────────────────────────────────────
mkdir -p "$TMPDIR/manifests-empty"

set +e
CASE3_OUT=$(MANIFEST_DIR="$TMPDIR/manifests-empty" bash "$GATE" 2>&1)
CASE3_RC=$?
set -e

if [ "$CASE3_RC" -ne 0 ]; then
  echo "FAIL (case 3 — empty): expected exit 0, got $CASE3_RC"
  printf '%s\n' "$CASE3_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
elif ! printf '%s\n' "$CASE3_OUT" | grep -q "OK"; then
  echo "FAIL (case 3 — empty): exit 0 but 'OK' not in output"
  printf '%s\n' "$CASE3_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
else
  echo "OK (case 3 — empty): gate exited 0 on empty manifest dir"
fi

if [ "$failures" -eq 0 ]; then
  echo ""
  echo "SELFTEST OK: skill-span-namespace gate is alive (drift + spans + empty all pinned)"
  exit 0
else
  echo ""
  echo "SELFTEST FAIL: $failures case(s) did not behave as expected"
  echo "The gate is vacuous or broken — see kask/scripts/check-skill-span-namespace.sh."
  exit 1
fi
