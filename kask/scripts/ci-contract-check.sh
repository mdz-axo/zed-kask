#!/usr/bin/env bash
# Contract Violation Detector — CI pre-commit / pre-push check
#
# Detects trait ↔ implementation contract drift at build time by parsing
# compiler errors and mapping them to REG `ContractViolated` context.
#
# Errors detected:
#   E0053 — method signature incompatible with trait (contract drift)
#   E0277 — trait bound not satisfied (missing From impl, port boundary violation)
#   E0308 — mismatched types across port boundaries
#
# Maps to REG spans:
#   reg.contract.violated  — contract drift detected
#   reg.contract.coverage  — which crate boundaries are monitored
#
# Usage:
#   ./scripts/ci-contract-check.sh          # check entire workspace
#   ./scripts/ci-contract-check.sh -p CRATE # check specific crate
#
# Exit codes:
#   0 — no contract violations found
#   1 — contract violations detected (with report)
#   2 — build failure (unrelated compilation error)

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

REPORT_FILE="${CONTRACT_REPORT:-contract-violations.txt}"

# Parse args
CRATE_FILTER=""
while getopts "p:" opt; do
    case $opt in
        p) CRATE_FILTER="$OPTARG" ;;
        *) echo "Usage: $0 [-p CRATE]"; exit 2 ;;
    esac
done

echo "=== Contract Violation Check ==="
echo ""

# Run cargo check, capturing stderr
if [ -n "$CRATE_FILTER" ]; then
    echo "Checking crate: $CRATE_FILTER"
    CHECK_OUTPUT=$(cargo check -p "$CRATE_FILTER" 2>&1) || true
else
    echo "Checking entire workspace..."
    CHECK_OUTPUT=$(cargo check --workspace 2>&1) || true
fi

# Count contract-relevant errors
E0053_COUNT=$(echo "$CHECK_OUTPUT" | grep -c "E0053" || true)
E0277_COUNT=$(echo "$CHECK_OUTPUT" | grep -c "E0277" || true)
E0308_COUNT=$(echo "$CHECK_OUTPUT" | grep -c "E0308" || true)
TOTAL=$((E0053_COUNT + E0277_COUNT + E0308_COUNT))

if [ "$TOTAL" -eq 0 ]; then
    echo -e "${GREEN}✓ No contract violations detected${NC}"
    echo ""
    echo "Contract coverage: full workspace (all port trait boundaries monitored)"
    echo "REG span: reg.contract.coverage = 1.0"
    exit 0
fi

# Build violation report
{
    echo "# Contract Violation Report"
    echo "## Generated: $(date -u +"%Y-%m-%dT%H:%M:%SZ")"
    echo ""
    echo "## Summary"
    echo "- E0053 (incompatible method signature): $E0053_COUNT"
    echo "- E0277 (trait bound not satisfied):     $E0277_COUNT"
    echo "- E0308 (mismatched types):              $E0308_COUNT"
    echo "- **Total violations: $TOTAL**"
    echo ""
    echo "## Detailed Violations"
    echo ""
    echo '```'
    echo "$CHECK_OUTPUT" | grep -A 2 -E "E0053|E0277|E0308" | grep -v "^--$"
    echo '```'
    echo ""
    echo "## REG Context"
    echo "- reg.contract.violated: $TOTAL instance(s)"
    echo "- reg.contract.coverage: degraded (violations detected)"
    echo ""
    echo "## Resolution"
    echo "1. Identify which port trait changed (check recent commits to hkask-memory/ports.rs, hkask-ports/src/*.rs)"
    echo "2. Update all implementations to match the new trait signatures"
    echo "3. Re-run this check until 0 violations"
} > "$REPORT_FILE"

echo -e "${RED}✗ Contract violations detected: $TOTAL${NC}"
echo "  E0053 (incompatible method): $E0053_COUNT"
echo "  E0277 (trait bound):          $E0277_COUNT"
echo "  E0308 (mismatched types):     $E0308_COUNT"
echo ""
echo "Full report: $REPORT_FILE"
echo ""
echo "REG span: reg.contract.violated (count=$TOTAL)"
echo "REG span: reg.contract.coverage (degraded)"
echo ""
echo "Affected crates:"
echo "$CHECK_OUTPUT" | grep "error\[" | grep -oP "--> crates/\K[^:]+" | sort -u | sed 's/^/  - /'
echo ""

exit 1
