#!/usr/bin/env bash
#
# ADVISORY AGENT TOOL — unwired from push/CI enforcement by operator
# mandate 2026-10-31 (the rules-that-don't-improve-code-quality cut).
# Run it voluntarily; nothing blocks a push or fails CI on it. Re-wiring
# it into kask/githooks/pre-push or .github/workflows/kask-invariants.yml
# requires an explicit operator decision.
# Skill-corpus Lisp gate — parse every explicit Lisp candidate through the
# production hkask-lisp parser and reject mechanically certain inert/type-broken
# forms before they reach an agent.
#
# The Rust checker owns extraction and AST linting. Scientific gate regressions
# also evaluate the authored ED/HT forms; parsing alone cannot pin decisions.
# The binary override remains the isolated detector-fixture seam.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SKILLS_DIR="${SKILL_CORPUS_DIR:-$ROOT/.agents/skills}"
TEMPLATES_DIR="${TEMPLATE_CORPUS_DIR:-$ROOT/kask/registry/templates}"

if [ -n "${SKILL_LISP_CHECKER_BIN:-}" ]; then
    exec "$SKILL_LISP_CHECKER_BIN" "$SKILLS_DIR" "$TEMPLATES_DIR"
fi

cd "$ROOT"
cargo run -q -p hkask-lisp --bin check-skill-lisp-forms -- \
    "$SKILLS_DIR" "$TEMPLATES_DIR"
exec bash "$SCRIPT_DIR/cargo-test-nonzero.sh" -q -p hkask-lisp \
    --bin check-skill-lisp-forms scientific_gate_
