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

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
KASK_ROOT="${KASK_ROOT:-$(cd "$HERE/.." && pwd)}"
WORKFLOWS_DIR="${WORKFLOWS_DIR:-$(cd "$KASK_ROOT/.." && pwd)/.github/workflows}"
ALLOWLIST="${ALLOWLIST:-$HERE/no-python-allowlist.txt}"
REPO_ROOT="$(cd "$KASK_ROOT/.." && pwd)"

fail() { echo "[FAIL] $*" >&2; exit 1; }

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

# Scope: every file under kask/ except the data exemptions.
scan_tree() {
    find "$KASK_ROOT" \
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

PY_SHAPES='python3?[[:space:]]+(-|<<)|^#!.*python|\|[[:space:]]*python3?[[:space:]]*$'

violations=0

# 1. Standalone Python and declared Python tooling: files in scope.
while IFS= read -r -d '' f; do
    is_gate_self "$f" && continue
    is_python_surface_name "$f" || continue
    rel="${f#"$REPO_ROOT"/}"
    if allowlisted "$rel"; then
        echo "[ALLOWED] Python surface: $rel (recorded operator decision — kask/scripts/no-python-allowlist.txt)"
    else
        echo "[FAIL] Python surface out of bounds (operator ruling 2026-10-13): $rel" >&2
        violations=$((violations + 1))
    fi
done < <(scan_tree)

# 2. Embedded invocations: interpreter shapes inside in-scope text files.
# The input is grep -l output — NEWLINE-separated — so the loop reads
# newline-delimited (loop 1 reads find -print0 NUL-delimited; mixing the
# two leaves the while body unexecuted at EOF).
while IFS= read -r f; do
    is_gate_self "$f" && continue
    rel="${f#"$REPO_ROOT"/}"
    if allowlisted "$rel"; then
        echo "[ALLOWED] embedded-invocation scan: $rel (recorded operator decision)"
        continue
    fi
    echo "[FAIL] Python invocation shape in $rel:" >&2
    grep -InE "$PY_SHAPES" "$f" | sed 's/^/    /' >&2
    violations=$((violations + 1))
done < <(scan_tree | xargs -0 -r grep -IlE "$PY_SHAPES" 2>/dev/null || true)

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