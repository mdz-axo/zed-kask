#!/usr/bin/env bash
# D56: exercise the three build scripts affected by D7 version inheritance.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."

cargo check --locked -p remote_server -p eval_cli -p edit_prediction_cli

# The generated compliance workflow is not executed by cargo check.
grep -q 'workspace\.package' tooling/xtask/src/tasks/workflows/compliance_check.rs
printf '%s\n' 'Build checks passed; compliance workspace-version reference present (not an executed workflow test).'
