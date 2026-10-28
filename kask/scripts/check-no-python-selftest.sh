#!/usr/bin/env bash
# Selftest: check-no-python.sh fails on synthetic violations — a standalone
# .py, a declared pyproject.toml, an embedded python3 heredoc, a
# setup-python action in a fork-owned workflow — and passes a clean tree
# that CARRIES the guardrail sentence and the "no Python fallback"
# comment (the false-positive pin), an allowlisted transition entry, and
# the real repo. A gate that cannot fail is not a gate (.rules, ast-grep
# positive-fixture trap: pin every rule with a synthetic violation it must
# catch before trusting zero).
#
# Exit codes: 0 = the gate behaves (fails on every violation, passes
# clean), 1 = the gate is broken (passed a violation or failed a clean
# tree).
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
gate="$here/check-no-python.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

make_tree() {
    rm -rf "$tmp/kask" "$tmp/workflows" "$tmp/allowlist.txt"
    mkdir -p "$tmp/kask/scripts" "$tmp/workflows"
}

run_gate() {
    KASK_ROOT="$tmp/kask" WORKFLOWS_DIR="$tmp/workflows" \
        ALLOWLIST="$tmp/allowlist.txt" bash "$gate" >/dev/null 2>&1
}

expect_fail() {
    if run_gate; then
        echo "FAIL: gate PASSED a $1 — the gate is broken." >&2
        exit 1
    fi
}

expect_pass() {
    if ! run_gate; then
        echo "FAIL: gate FAILED a $1 — the gate is broken." >&2
        exit 1
    fi
}

# Case 1: standalone .py → fails.
make_tree
echo 'print(1)' > "$tmp/kask/scripts/evil.py"
expect_fail "standalone .py"

# Case 2: declared Python tooling (pyproject.toml) → fails.
make_tree
printf '[project]\nname = "x"\n' > "$tmp/kask/scripts/pyproject.toml"
expect_fail "declared pyproject.toml"

# Case 3: embedded python3 heredoc → fails. The fixture text is assembled
# from printf arguments so the invocation shape never appears literally in
# this selftest file (the gate self-excludes its own detector text and this
# fixture text; keep both exclusions honest — do not inline the shape).
make_tree
{
    echo '#!/usr/bin/env bash'
    printf 'python3 - <<%sPY%s\nprint(1)\nPY\n' "'" "'"
} > "$tmp/kask/scripts/probe.sh"
expect_fail "embedded python3 heredoc"

# Case 4: the false-positive pin — a clean tree carrying the guardrail
# sentence and the no-Python-fallback comment passes.
make_tree
cat > "$tmp/kask/scripts/clean.sh" <<'EOF'
# Do not execute arbitrary Python code in Jinja2 expressions (sandboxed execution).
# jq is required for the settings merge (no Python fallback ships with the installer).
EOF
expect_pass "clean tree carrying the guardrail sentences"

# Case 5: an allowlisted transition entry passes.
make_tree
echo 'print(1)' > "$tmp/kask/scripts/transition.py"
printf '# selftest fixture: a recorded transition decision\nkask/scripts/transition.py\n' > "$tmp/allowlist.txt"
expect_pass "allowlisted transition entry"

# Case 6: setup-python in a fork-owned workflow → fails.
make_tree
printf 'steps:\n  - uses: actions/setup-python@v5\n' > "$tmp/workflows/kask-test.yml"
expect_fail "setup-python action in a fork-owned workflow"

# Case 7: the real repo passes (transition entries ride the allowlist).
if ! bash "$gate" >/dev/null 2>&1; then
    echo "FAIL: gate FAILED the real repo — gate output follows." >&2
    bash "$gate" || true
    exit 1
fi

echo "OK: check-no-python.sh fails on every synthetic violation and passes the clean, allowlisted, and real trees."