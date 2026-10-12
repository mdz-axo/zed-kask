#!/usr/bin/env bash
# Self-test for the docs-citations gate (check-docs-citations.sh).
#
# Institutionalizes the .rules trap "A CI gate must be shown to fail before
# its status: enforced is trusted": a broken ladder rung or inverted verdict
# would silently vacate the gate. Pins every verdict class over a temp git
# repo with DOCS_ROOT pointed at per-case docs dirs:
#   1. Clean: every citation resolves (root rung) → exit 0.
#   2. Rotted: the cited file resolves nowhere → exit 1 with ::error::.
#   3. Ambiguous bare name (two repo matches) → warning, exit 0.
#   4. Line drift (line beyond the resolved file's end) → warning, exit 0.
#   5. Doc-relative rung: a crate-relative path resolves from the doc's own
#      directory → exit 0.
#   6. Unique-suffix rung: a partial path resolves against exactly one
#      tracked .rs file → exit 0.
#
# Exit codes: 0 = every case behaved as expected, 1 = a case misbehaved.
#
# Usage: bash scripts/check-docs-citations-selftest.sh

set -uo pipefail

KASK_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GATE="$KASK_ROOT/scripts/check-docs-citations.sh"

[ -f "$GATE" ] || { echo "FAIL: gate not found at $GATE"; exit 1; }

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
git init -q "$WORK"
git -C "$WORK" config user.email selftest@local
git -C "$WORK" config user.name selftest

# Fixture code files (tracked — the ladder's suffix/basename rungs read
# git ls-files).
mkdir -p "$WORK/src" "$WORK/src/a" "$WORK/src/b" "$WORK/crates/nested/src"
printf 'pub fn root_widget() -> u32 { 0 }\n' > "$WORK/src/widget.rs"
printf 'pub fn a_widget() -> u32 { 0 }\n' > "$WORK/src/a/widget.rs"
printf 'pub fn b_widget() -> u32 { 0 }\n' > "$WORK/src/b/widget.rs"
printf 'pub fn deep() -> u32 { 0 }\n' > "$WORK/crates/nested/src/inner.rs"
git -C "$WORK" add -A

FAILURES=0

run_case() {
  # $1 case docs dir (relative to the temp repo), $2.. doc lines on stdin.
  local dir=$1
  mkdir -p "$WORK/$dir"
  cat > "$WORK/$dir/doc.md"
}

expect() {
  # $1 expected rc, $2 case label, $3 docs dir, $4.. required output substrings.
  local want_rc=$1 label=$2 dir=$3
  shift 3
  local out rc
  out=$(cd "$WORK" && DOCS_ROOT="$dir" bash "$GATE" 2>&1); rc=$?
  if [ "$rc" -ne "$want_rc" ]; then
    echo "FAIL ($label): expected exit $want_rc, got $rc"
    printf '%s\n' "$out" | sed 's/^/    /'
    FAILURES=$((FAILURES + 1))
    return
  fi
  local sub
  for sub in "$@"; do
    if ! printf '%s\n' "$out" | grep -qF -- "$sub"; then
      echo "FAIL ($label): exit $want_rc but output missing: $sub"
      printf '%s\n' "$out" | sed 's/^/    /'
      FAILURES=$((FAILURES + 1))
      return
    fi
  done
  echo "OK ($label)"
}

# Case 1: clean — root-rung resolution.
run_case docs-clean <<'EOF'
See `src/widget.rs:1` for the root widget.
EOF
expect 0 "case 1 — clean citation resolves at root" docs-clean "1 resolved"

# Case 2: rotted — resolves on no rung.
run_case docs-rot <<'EOF'
See `src/gone.rs:5` for the deleted surface.
EOF
expect 1 "case 2 — rotted citation fails" docs-rot "::error::" "src/gone.rs:5"

# Case 3: ambiguous bare name — two repo matches, warns, never fails.
run_case docs-ambig <<'EOF'
See `widget.rs:1` for the widget.
EOF
expect 0 "case 3 — ambiguous bare name warns" docs-ambig "ambiguous" "widget.rs:1"

# Case 4: line drift — resolved file, out-of-range line, warns.
run_case docs-drift <<'EOF'
See `src/widget.rs:9999` for the drifted line.
EOF
expect 0 "case 4 — line drift warns" docs-drift "out of range" "src/widget.rs:9999"

# Case 5: doc-relative rung — a crate-relative path resolves from the doc's
# own directory (docs-live-in-the-crate convention).
mkdir -p "$WORK/docs-rel/src"
printf 'pub fn rel_widget() -> u32 { 0 }\n' > "$WORK/docs-rel/src/inner.rs"
git -C "$WORK" add -A
run_case docs-rel <<'EOF'
See `src/inner.rs:1` — relative to this doc's crate.
EOF
expect 0 "case 5 — doc-relative rung resolves" docs-rel "1 resolved"

# Case 6: unique-suffix rung — a partial path matches exactly one tracked file.
run_case docs-suffix <<'EOF'
See `nested/src/inner.rs:1` via the unique suffix.
EOF
expect 0 "case 6 — unique-suffix rung resolves" docs-suffix "1 resolved"

echo "----"
if [ "$FAILURES" -eq 0 ]; then
  echo "docs-citations selftest: all 6 cases behaved as expected"
else
  echo "docs-citations selftest: $FAILURES case(s) misbehaved"
fi
[ "$FAILURES" -eq 0 ]