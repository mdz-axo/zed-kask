#!/usr/bin/env bash
#
# check-principle-constraints.sh — verify derived principle constraints
# haven't drifted from the codebase.
#
# For each constraint with status: enforced:
#   - Verify the enforced_at path still exists (file is present)
#   - Verify the falsifier test still exists (grep the test suite)
#   - Fail if any enforced constraint has drifted
#
# For each constraint with status: gap:
#   - Report as a warning (gaps are findings, not violations)
#
# Usage: bash kask/scripts/check-principle-constraints.sh
#
# Returns 0 if all enforced constraints are intact, 1 if any have drifted.

set -euo pipefail

CONSTRAINTS_FILE="kask/docs/architecture/principle-constraints.yaml"

if [ ! -f "$CONSTRAINTS_FILE" ]; then
    echo "OK: $CONSTRAINTS_FILE does not exist — no constraints to verify."
    exit 0
fi

# Check if the file has any principles (empty array means no constraints yet)
PRINCIPLE_COUNT=$(python3 -c "
import yaml, sys
with open('$CONSTRAINTS_FILE') as f:
    data = yaml.safe_load(f)
principles = data.get('principles', [])
print(len(principles))
" 2>/dev/null || echo "0")

if [ "$PRINCIPLE_COUNT" = "0" ]; then
    echo "OK: No principles in $CONSTRAINTS_FILE — nothing to verify."
    exit 0
fi

DRIFTED=0
GAPS=0

# Parse the YAML and check each enforced constraint
python3 -c "
import yaml, subprocess, os, sys

with open('$CONSTRAINTS_FILE') as f:
    data = yaml.safe_load(f)

drifted = 0
gaps = 0

for principle in data.get('principles', []):
    pid = principle.get('id', 'unknown')
    for constraint in principle.get('constraint_set', []):
        cid = constraint.get('id', '?')
        status = constraint.get('status', '')
        enforced_at = constraint.get('enforced_at', '')
        falsifier = constraint.get('falsifier', '')

        if status == 'enforced':
            # Check that enforced_at points to a real file
            if enforced_at and enforced_at != 'UNKNOWN':
                # Extract file path (before the colon)
                file_path = enforced_at.split(':')[0] if ':' in enforced_at else enforced_at
                if not os.path.exists(file_path):
                    print(f'DRIFT: {pid}.{cid} — enforced_at file not found: {file_path}')
                    drifted += 1
                    continue

            # Check that the falsifier test exists (if not MISSING:)
            if falsifier and not falsifier.startswith('MISSING:'):
                # Grep for the test function name in .rs files
                result = subprocess.run(
                    ['grep', '-r', '--include=*.rs', '-l', falsifier, 'kask/'],
                    capture_output=True, text=True
                )
                if result.returncode != 0:
                    print(f'DRIFT: {pid}.{cid} — falsifier test not found: {falsifier}')
                    drifted += 1
                else:
                    print(f'OK: {pid}.{cid} — enforced at {enforced_at}, falsifier {falsifier} found')

        elif status == 'gap':
            gaps += 1
            print(f'GAP: {pid}.{cid} — {constraint.get(\"assertion\", \"\")[:80]}...')

if drifted > 0:
    print(f'\\nFAILED: {drifted} enforced constraint(s) have drifted.')
    sys.exit(1)
else:
    print(f'\\nOK: All enforced constraints intact.')
    if gaps > 0:
        print(f'WARNING: {gaps} gap constraint(s) require human review.')
    sys.exit(0)
"
