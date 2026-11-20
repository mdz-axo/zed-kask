#!/usr/bin/env bash
# CI gate: zero Python under kask/ and fork-owned workflows (operator
# ruling 2026-10-13, tightening the 2026-09-09 heredoc exception). Python
# is dynamically typed and interpreted — no static guarantees, an
# interpreter dependency inside CI gates, mediocre LLM codegen quality —
# so repo scripting is bash, and what bash cannot do (parsing) belongs in
# a Rust test or bin on workspace deps (toml, serde_yaml_neo), never an
# embedded interpreter.
#
# Detectors are INVOCATION SHAPES, never the bare word "python": the word
# false-positives on 62 template guardrail notes ("Do not execute
# arbitrary Python code in Jinja2 expressions") and on documented
# non-Python contrasts. Data, not code, is out of scope: vendored ontology
# sources and literary style caches.
#
# Exit codes: 0 = clean, 1 = Python surface outside the allowlist.
# Usage: bash kask/scripts/check-no-python.sh
# Selftest: bash kask/scripts/check-no-python-selftest.sh
# Selftest overrides: KASK_ROOT, WORKFLOWS_DIR, ALLOWLIST.

set -euo pipefail

KASK_ROOT_EXPLICIT=0
[ -n "${KASK_ROOT+x}" ] && KASK_ROOT_EXPLICIT=1
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
KASK_ROOT="${KASK_ROOT:-$(cd "$HERE/.." && pwd)}"
WORKFLOWS_DIR="${WORKFLOWS_DIR:-$(cd "$KASK_ROOT/.." && pwd)/.github/workflows}"
ALLOWLIST="${ALLOWLIST:-$HERE/no-python-allowlist.txt}"
REPO_ROOT="$(cd "$KASK_ROOT/.." && pwd)"
SKILLS_ROOT="${SKILLS_ROOT:-}"
if [ -z "$SKILLS_ROOT" ] && [ "$KASK_ROOT_EXPLICIT" -eq 0 ] && [ -d "$REPO_ROOT/.agents/skills" ]; then
    SKILLS_ROOT="$REPO_ROOT/.agents/skills"
fi

fail() { echo "[FAIL] $*" >&2; exit 1; }

[ -d "$KASK_ROOT" ] || fail "KASK_ROOT directory not found: $KASK_ROOT"

# The gate's own detector text and the selftest's fixture text are not
# Python invocations — self-excluded from the scan, never allowlisted.
is_gate_self() {
    [ "$1" = "$HERE/check-no-python.sh" ] || [ "$1" = "$HERE/check-no-python-selftest.sh" ]
}

# Allowlist: repo-relative paths, one per line, '#' comments ignored. EMPTY
# by default — every entry is a recorded operator decision, never silent.
allowlisted() {
    [ -f "$ALLOWLIST" ] && grep -qxF "$1" "$ALLOWLIST"
}

# Scope: every file under kask/ (plus .agents/skills/ on full-repo runs) except the data exemptions.
scan_tree() {
    local roots=("$KASK_ROOT")
    [ -n "$SKILLS_ROOT" ] && [ -d "$SKILLS_ROOT" ] && roots+=("$SKILLS_ROOT")
    find "${roots[@]}" \
        \( -path '*/hkask-bridge-ontology/sources' \
        -o -path '*/registry/styles' \
        -o -name 'target' \) -prune \
        -o -type f -print0
}

# Standalone Python and declared Python tooling, by filename. setup.py is
# subsumed by *.py (a shadowed case pattern is a shellcheck warning).
is_python_surface_name() {
    case "${1##*/}" in
        *.py | pyproject.toml | requirements*.txt | tox.ini | Pipfile | .python-version) return 0 ;;
        *) return 1 ;;
    esac
}

PY_SHAPES='(^|[;&|[:space:]`(])python([23](\.[0-9]+)?)?[[:space:]]+(-|<<|["'"'"'$./~A-Za-z0-9_-]*\.py([^A-Za-z0-9_]|$)|\$[A-Za-z_{])|^#!.*python|\|[[:space:]]*python([23](\.[0-9]+)?)?[[:space:]]*$|(^|[;&|[:space:]`(])uv[[:space:]]+run[[:space:]]+[^#\n]*\.py([^A-Za-z0-9_]|$)|(^|[;&|[:space:]`(])pytest[[:space:]]+(-q|-k|--|[A-Za-z0-9_./-]+\.py([^A-Za-z0-9_]|$))'

violations=0
scanned_files=0

SCAN_LIST=$(mktemp)
trap 'rm -f "$SCAN_LIST"' EXIT
scan_tree > "$SCAN_LIST"

# 1. Standalone Python and declared Python tooling: files in scope.
while IFS= read -r -d '' f; do
    scanned_files=$((scanned_files + 1))
    is_gate_self "$f" && continue
    is_python_surface_name "$f" || continue
    rel="${f#"$REPO_ROOT"/}"
    if allowlisted "$rel"; then
        echo "[ALLOWED] Python surface: $rel (recorded operator decision — kask/scripts/no-python-allowlist.txt)"
    else
        echo "[FAIL] Python surface out of bounds (operator ruling 2026-10-13): $rel" >&2
        violations=$((violations + 1))
    fi
done < "$SCAN_LIST"

[ "$scanned_files" -gt 0 ] || fail "zero files scanned under $KASK_ROOT — refusing to pass vacuously"

# 2. Embedded invocations: interpreter shapes inside in-scope text files.
while IFS= read -r f; do
    [ -n "$f" ] || continue
    is_gate_self "$f" && continue
    rel="${f#"$REPO_ROOT"/}"
    if allowlisted "$rel"; then
        echo "[ALLOWED] embedded-invocation scan: $rel (recorded operator decision)"
        continue
    fi
    echo "[FAIL] Python invocation shape in $rel:" >&2
    grep -InE "$PY_SHAPES" "$f" | sed 's/^/    /' >&2
    violations=$((violations + 1))
done < <(xargs -0 -r grep -IlE "$PY_SHAPES" < "$SCAN_LIST" || true)

# 3. Fork-owned workflows: interpreter shapes + setup-python actions.
for wf in "$WORKFLOWS_DIR"/kask-*.yml; do
    [ -e "$wf" ] || continue
    rel="${wf#"$REPO_ROOT"/}"
    if allowlisted "$rel"; then
        echo "[ALLOWED] fork-owned workflow scan: $rel (recorded operator decision)"
        continue
    fi
    if grep -IqE "$PY_SHAPES|setup-python" "$wf"; then
        echo "[FAIL] Python in fork-owned workflow $rel:" >&2
        grep -InE "$PY_SHAPES|setup-python" "$wf" | sed 's/^/    /' >&2
        violations=$((violations + 1))
    fi
done

[ "$violations" -eq 0 ] || fail "$violations Python surface(s) outside the allowlist (operator ruling 2026-10-13: zero Python under kask/ and fork-owned workflows)"
echo "[OK] zero Python: no standalone scripts, embedded invocations, or declared tooling under kask/ or fork-owned workflows"