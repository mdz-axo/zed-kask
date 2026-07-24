#!/usr/bin/env bash
# CI gate: enforce LoRA/QLoRA training-config regression library entries.
#
# Each regression in security/regressions/RR-NNNN.yaml with surface: training
# and status: enforced is checked against training config files.
#
# detection.kind: runtime-assert regressions are acknowledged but not
# mechanically enforced — they require runtime instrumentation during training.
#
# RATCHETED: regressions with status: pending are warnings only.
#
# Exit codes:
#   0 — all enforced grep regressions pass
#   1 — an enforced grep regression's pattern was found
#
# Usage: bash scripts/check-lora-training-regressions.sh

set -euo pipefail
cd "$(dirname "$0")/.."

# shellcheck source=scripts/lib-regressions.sh
source "$(dirname "$0")/lib-regressions.sh"

# Filter to surface: training, grep against training config file types,
# deferred kind is "runtime-assert".
TRAINING_INCLUDE="--include=*.py --include=*.yaml --include=*.yml --include=*.json --include=*.toml"
check_regressions "training" "$TRAINING_INCLUDE" "runtime-assert"
