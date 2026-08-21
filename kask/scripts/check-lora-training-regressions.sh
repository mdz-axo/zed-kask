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
# Usage: bash kask/scripts/check-lora-training-regressions.sh (from any directory)

set -euo pipefail

# Resolve the script directory BEFORE the cd — `$0` is relative to the caller's
# working directory, so dereferencing it afterwards resolved to kask/kask/scripts
# and the gate died on a missing source file instead of running.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/.."

# shellcheck source=scripts/lib-regressions.sh
source "$SCRIPT_DIR/lib-regressions.sh"

# Filter to surface: training, grep against training config file types,
# deferred kind is "runtime-assert".
TRAINING_INCLUDE="--include=*.py --include=*.yaml --include=*.yml --include=*.json --include=*.toml"
check_regressions "training" "$TRAINING_INCLUDE" "runtime-assert"
