#!/usr/bin/env bash
# ast-grep.sh — the pinned structural-analysis instrument (UC-1 of the
# tree-sitter adoption proposal, kask/docs/research/tree-sitter-adoption-proposal.md).
#
# tree-sitter parsing + query language via ast-grep (sg), pinned to
# AST_GREP_VERSION below — the jscpd/JSCPD_VERSION pinning precedent in
# check-duplication-ratchet.sh. The version lives here and nowhere else;
# callers invoke this wrapper and never npx directly, so one bump moves the
# whole instrument. Rules live in kask/ast-grep/rules/ (the index is
# kask/ast-grep/sgconfig.yml); all arguments pass through to ast-grep.
#
# Requires node/npx (network fetch on first use, cached after).
#
# Examples:
#   kask/scripts/ast-grep.sh scan --config kask/ast-grep/sgconfig.yml --json=stream <paths>
#   kask/scripts/ast-grep.sh -p '$X.unwrap()' -l rust --json=pretty <paths>
#
# Stream JSON is the stable machine shape: one match object per line on
# stdout, diagnostics on stderr, exit 1 when error-severity diagnostics
# exist (expected — callers redirect and filter by ruleId).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
AST_GREP_VERSION=0.50.0

command -v npx >/dev/null 2>&1 || {
  echo "FAIL: npx not found — the structural instrument requires node/npx (ast-grep @${AST_GREP_VERSION}, the jscpd pinning precedent)." >&2
  exit 1
}

exec npx --yes --package "@ast-grep/cli@${AST_GREP_VERSION}" ast-grep "$@"