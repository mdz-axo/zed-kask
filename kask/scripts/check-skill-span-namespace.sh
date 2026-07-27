#!/usr/bin/env bash
# CI gate: every skill manifest's ledger.span_namespace must be reg.skill.<manifest.id>
#
# Enforces the unified feedback standard (P9 §9.1):
#   - Every skill with a ledger section MUST set span_namespace to reg.skill.<id>
#   - The <id> must match the manifest's manifest.id field (with / → - sanitization)
#   - The spans: list is abolished — its presence is a failure
#
# Skills without a ledger section are skipped (not all manifests are skills).
#
# Run locally: bash scripts/check-skill-span-namespace.sh
# Exit codes: 0 = all conform, 1 = violations found

set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

MANIFEST_DIR="registry/manifests"
FAIL=0
CHECKED=0
SKIPPED=0

for manifest in "$MANIFEST_DIR"/*.yaml; do
  [ -f "$manifest" ] || continue

  # Extract manifest.id
  skill_id=$(python3 -c "
import yaml, sys
try:
    with open('$manifest') as f:
        m = yaml.safe_load(f)
    print(m.get('manifest', {}).get('id', ''))
except Exception:
    sys.exit(1)
" 2>/dev/null)

  if [ -z "$skill_id" ]; then
    SKIPPED=$((SKIPPED + 1))
    continue
  fi

  # Sanitize: replace / with -
  expected_ns="reg.skill.${skill_id//\//-}"

  # Extract span_namespace
  actual_ns=$(python3 -c "
import yaml, sys
try:
    with open('$manifest') as f:
        m = yaml.safe_load(f)
    print(m.get('ledger', {}).get('span_namespace', ''))
except Exception:
    sys.exit(1)
" 2>/dev/null)

  # Skip if no ledger section
  if [ -z "$actual_ns" ]; then
    SKIPPED=$((SKIPPED + 1))
    continue
  fi

  CHECKED=$((CHECKED + 1))

  if [ "$actual_ns" != "$expected_ns" ]; then
    echo "FAIL: $manifest"
    echo "  span_namespace: $actual_ns (expected $expected_ns)"
    FAIL=1
    continue
  fi

  # Check for abolished spans: list
  has_spans=$(python3 -c "
import yaml, sys
try:
    with open('$manifest') as f:
        m = yaml.safe_load(f)
    print('yes' if m.get('ledger', {}).get('spans') else 'no')
except Exception:
    print('no')
" 2>/dev/null)

  if [ "$has_spans" = "yes" ]; then
    echo "FAIL: $manifest has abolished spans: list in ledger"
    FAIL=1
  fi
done

if [ "$FAIL" -eq 0 ]; then
  echo "OK: $CHECKED skill manifests conform to reg.skill.<id> standard ($SKIPPED skipped — no ledger)."
  exit 0
else
  echo ""
  echo "FAIL: skill span namespace violations found."
  echo "Standard: ledger.span_namespace must be reg.skill.<manifest.id>"
  echo "The spans: list is abolished — remove it."
  exit 1
fi
