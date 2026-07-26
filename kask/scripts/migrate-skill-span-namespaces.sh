#!/usr/bin/env bash
# Migrate skill manifests to the unified feedback standard.
#
# Every skill manifest's ledger.span_namespace is set to:
#   reg.skill.<manifest.id>
#
# The `spans:` list (if present) is removed — it was ambiguous and unused by
# the executor. Performative telemetry is preserved via `telemetry_namespace:
# hkask.template.<manifest.id>` for skills that previously used hkask.template.*
# as their span_namespace.
#
# Idempotent: safe to run multiple times. Skills already conforming are unchanged.
#
# Usage: bash scripts/migrate-skill-span-namespaces.sh [--dry-run]
set -uo pipefail
cd "$(dirname "$0")/.."

DRY_RUN=0
[ "${1:-}" = "--dry-run" ] && DRY_RUN=1

MANIFEST_DIR="registry/manifests"
COUNT=0
SKIPPED=0
MIGRATED=0

for manifest in "$MANIFEST_DIR"/*.yaml; do
  [ -f "$manifest" ] || continue
  COUNT=$((COUNT + 1))

  # Extract the skill id from the manifest's `manifest.id` field.
  skill_id=$(python3 -c "
import yaml, sys
with open('$manifest') as f:
    m = yaml.safe_load(f)
print(m.get('manifest', {}).get('id', ''))
" 2>/dev/null)

  if [ -z "$skill_id" ]; then
    echo "SKIP (no manifest.id): $manifest"
    SKIPPED=$((SKIPPED + 1))
    continue
  fi

  expected_ns="reg.skill.$skill_id"
  telemetry_ns="hkask.template.$skill_id"

  # Read current span_namespace and check if migration is needed.
  current_ns=$(python3 -c "
import yaml
with open('$manifest') as f:
    m = yaml.safe_load(f)
print(m.get('ledger', {}).get('span_namespace', ''))
" 2>/dev/null)

  has_spans=$(python3 -c "
import yaml
with open('$manifest') as f:
    m = yaml.safe_load(f)
print('yes' if m.get('ledger', {}).get('spans') else 'no')
" 2>/dev/null)

  if [ "$current_ns" = "$expected_ns" ] && [ "$has_spans" = "no" ]; then
    SKIPPED=$((SKIPPED + 1))
    continue
  fi

  if [ $DRY_RUN -eq 1 ]; then
    echo "WOULD MIGRATE: $manifest"
    echo "  span_namespace: $current_ns → $expected_ns"
    [ "$has_spans" = "yes" ] && echo "  spans: (remove list)"
    [ "${current_ns#hkask.template.}" != "$current_ns" ] && echo "  telemetry_namespace: $telemetry_ns"
    MIGRATED=$((MIGRATED + 1))
    continue
  fi

  # Perform the migration in-place with Python (preserves YAML formatting better than sed).
  python3 -c "
import yaml, sys

path = '$manifest'
with open(path) as f:
    content = f.read()

# Parse to get structure
doc = yaml.safe_load(content)
if not doc or 'ledger' not in doc:
    sys.exit(0)

ledger = doc['ledger']
old_ns = ledger.get('span_namespace', '')
expected = '$expected_ns'
telemetry = '$telemetry_ns'

# Set span_namespace to reg.skill.<id>
ledger['span_namespace'] = expected

# Remove the spans: list (abolished — ambiguous, unused by executor)
ledger.pop('spans', None)

# If the old namespace was hkask.template.*, preserve it as telemetry_namespace
if old_ns.startswith('hkask.template.') and 'telemetry_namespace' not in ledger:
    ledger['telemetry_namespace'] = telemetry

# Re-serialize. yaml.safe_dump preserves structure but may reorder keys;
# use sort_keys=False to keep insertion order.
with open(path, 'w') as f:
    yaml.safe_dump(doc, f, sort_keys=False, default_flow_style=False, width=100)
" 2>/dev/null

  echo "MIGRATED: $manifest ($current_ns → $expected_ns)"
  MIGRATED=$((MIGRATED + 1))
done

echo ""
echo "Summary: $COUNT manifests, $MIGRATED migrated, $SKIPPED already conforming."
[ $DRY_RUN -eq 1 ] && echo "(dry-run — no changes written)"
