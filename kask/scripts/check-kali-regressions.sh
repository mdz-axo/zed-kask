#!/usr/bin/env bash
# CI gate: enforce security regression library entries.
#
# Each regression in security/regressions/RR-NNNN.yaml with status: enforced
# and detection.kind: grep is checked against the codebase. If the bug pattern
# re-appears, the gate fails.
#
# detection.kind: reg-span regressions (surface: runtime) are acknowledged but
# not mechanically enforced — they require runtime REG span history infrastructure
# that is not yet implemented.
#
# RATCHETED: regressions with status: pending are warnings only. Once the fix
# lands, flip status to enforced.
#
# Exit codes:
#   0 — all enforced grep regressions pass
#   1 — an enforced grep regression's pattern was found
#
# Usage: bash scripts/check-kali-regressions.sh

set -euo pipefail
cd "$(dirname "$0")/.."

# shellcheck source=scripts/lib-regressions.sh
source "$(dirname "$0")/lib-regressions.sh"

# No surface filter (all surfaces), no include patterns (use per-regression include field),
# deferred kind is "reg-span".
check_regressions "" "" "reg-span"
