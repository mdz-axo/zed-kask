#!/usr/bin/env bash
# Check for `Result<_, String>` anti-pattern in library code.
#
# String error types discard structured error information and prevent
# callers from matching on specific error variants. Use `thiserror` enums
# for library code, `anyhow` for application binaries.
#
# Structural since 2026-10-11 (the tree-sitter adoption proposal's
# accepted follow-up, kask/docs/research/tree-sitter-adoption-proposal.md):
# the match runs via the pinned ast-grep instrument
# (kask/scripts/ast-grep.sh) over the rule
# kask/ast-grep/rules/no-string-errors.yml. The pattern matches the
# generic TYPE node, which replaces the former line-based grep and
# closes both of its documented blind spots:
# - multi-line `Result<..., String>` — a signature split across lines
#   was invisible to a single-line grep; an AST node has no lines.
# - the nested-String false positive — `Result<(String, String), MyError>`
#   matched the old regex; structurally the second generic argument
#   must be exactly the `String` type node, so it does not match.
# Comment lines never match (a comment is not a type node) — the old
# comment-line skip is structural now.
#
# Requires node/npx (the jscpd pinning precedent in
# check-duplication-ratchet.sh; the ast-grep version pin lives in
# kask/scripts/ast-grep.sh, once).
#
# Enabled in CI via `.github/workflows/kask-invariants.yml` check job.
# Run locally: `scripts/check-string-errors.sh`
#
# COVERAGE (known, accepted gaps — an advertised invariant must state what
# it does not check):
# - Scope: every hkask-* crate — the kask subtree (crates/hkask-*,
#   mcp-servers/hkask-*) and the zed-path widget/adapter crates
#   (../crates/hkask-*), widened 2026-09-30 to match
#   check-unsafe-forbid.sh's coverage. Only `kask_bridge` is out of
#   scope: the D8 seam crosses the GPUI/tokio boundary where String
#   errors are accepted. See zed-host-architecture-plan.md §13.1.
# - Exclusions preserved from the line-based pass: tests/ and examples/
#   directories, binary targets, build target/ directories, and
#   `string-error-ok` marker lines (the justification lives in the
#   surrounding doc comment — the same reviewable-suppression pattern
#   as the dead-code baseline).
#   Binary targets are `main.rs` AND `src/bin/*.rs`: Cargo treats both as
#   binaries, and the exclusion exists because binaries legitimately use
#   String/anyhow errors rather than library error enums. Matching only
#   the filename `main.rs` misclassified every `[[bin]]` target declared
#   under `src/bin/` as library code (found 2026-10-18: the gate failed on
#   `hkask-lisp`'s own `src/bin/check_skill_lisp_forms.rs`, a declared
#   `[[bin]]` with `fn main()`).
# - The rule matches `Result<T, String>` exactly: `Box<String>` and
#   `&str` error slots are not matched (the same scope the line-based
#   pass had).

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR/.."

AST_GREP="$SCRIPT_DIR/ast-grep.sh"
RULE_FILE="$SCRIPT_DIR/../ast-grep/rules/no-string-errors.yml"

# Overridable scan roots so the self-test (check-string-errors-selftest.sh)
# can point the gate at a temp tree with a synthetic violation. Defaults to
# the production source roots.
SCAN_DIRS=( ${SCAN_DIRS:-crates/hkask-* mcp-servers/hkask-* ../crates/hkask-*} )

command -v jq >/dev/null 2>&1 || {
  echo "FAIL: jq not found — the gate parses ast-grep's stream JSON with jq." >&2
  exit 1
}
[ -x "$AST_GREP" ] || {
  echo "FAIL: $AST_GREP missing or not executable." >&2
  exit 1
}
[ -f "$RULE_FILE" ] || {
  echo "FAIL: rule file $RULE_FILE missing." >&2
  exit 1
}

# Stream JSON: one match object per line on stdout; the error-severity
# summary goes to stderr and the exit code is 1 when matches exist —
# both expected, hence the redirect and || true (grep reads the temp
# file, never a pipe — a pipe under pipefail SIGPIPEs the writer).
TMPFILE=$(mktemp)
trap 'rm -f "$TMPFILE"' EXIT

"$AST_GREP" scan --rule "$RULE_FILE" --json=stream "${SCAN_DIRS[@]}" 2>/dev/null \
  | jq -r 'select(.ruleId == "no-string-errors")
           | select(.file | test("/(tests|examples|target)/") | not)
           | select(.file | test("(^|/)main\\.rs$") | not)
           | select(.file | test("/src/bin/[^/]+\\.rs$") | not)
           | select(.lines | contains("string-error-ok") | not)
           | "\(.file):\(.range.start.line + 1):\(.text)"' > "$TMPFILE" || true

FAIL=0
while IFS= read -r row; do
  [ -n "$row" ] || continue
  echo "  $row"
  FAIL=1
done < "$TMPFILE"

if [ $FAIL -eq 0 ]; then
  echo "OK: No Result<_, String> patterns found in library code."
  exit 0
else
  echo ""
  echo "FAIL: Result<_, String> patterns found. Replace String error types with thiserror enums."
  echo "See: crates/hkask-keystore/src/error.rs (KeystoreError example)"
  echo "     crates/hkask-inference/src/passage_tagging.rs (PassageTaggingError example)"
  exit 1
fi