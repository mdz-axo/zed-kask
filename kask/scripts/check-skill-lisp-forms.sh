#!/usr/bin/env bash
# Skill-corpus Lisp gate — parse every explicit Lisp candidate through the
# production hkask-lisp parser and reject mechanically certain inert/type-broken
# forms before they reach an agent.
#
# The Rust checker owns extraction and AST linting. This wrapper only binds the
# live corpus paths and gives fixture selftests an override seam.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SKILLS_DIR="${SKILL_CORPUS_DIR:-$ROOT/.agents/skills}"
TEMPLATES_DIR="${TEMPLATE_CORPUS_DIR:-$ROOT/kask/registry/templates}"

if [ -n "${SKILL_LISP_CHECKER_BIN:-}" ]; then
    exec "$SKILL_LISP_CHECKER_BIN" "$SKILLS_DIR" "$TEMPLATES_DIR"
fi

cd "$ROOT"
exec cargo run -q -p hkask-lisp --bin check-skill-lisp-forms -- \
    "$SKILLS_DIR" "$TEMPLATES_DIR"
