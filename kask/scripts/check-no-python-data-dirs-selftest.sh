#!/usr/bin/env bash
# check-no-python-data-dirs-selftest.sh — the known-positive selftest for
# the data-dirs zero-Python gate (the check-*-selftest.sh pattern: a "0
# violations" reading is trustworthy only after the gate has been proven to
# FAIL on synthetic violations and PASS the recorded external classes).
#
# Fixtures (all under a temp root, driven through KASK_DATA_DIRS):
#   clean/                nothing            -> exit 0
#   dirty/                stray.py + a .venv-rocm root + pyproject.toml +
#                         __pycache__        -> exit 1, ALL FOUR named
#   external/            upstream .py inside */private-inference/engines/*
#                         and */corpus-mcp/cache/*    -> exit 0 (recorded)
#   corpus-aside/         a .py in corpus-mcp but OUTSIDE cache -> exit 1
#   missing/              absent dir         -> exit 0 with the surfaced no-op
#
# Usage: bash kask/scripts/check-no-python-data-dirs-selftest.sh
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
GATE="$HERE/check-no-python-data-dirs.sh"

root=$(mktemp -d)
trap 'rm -rf "$root"' EXIT

# clean fixture
mkdir -p "$root/clean/notes"
echo "just text" > "$root/clean/notes/a.txt"

# dirty fixture: every fail class
mkdir -p "$root/dirty/a/b" "$root/dirty/.venv-rocm/bin" "$root/dirty/a/__pycache__"
echo "print('agent-authored')" > "$root/dirty/a/stray.py"
echo "not a real venv, the dir NAME is the violation" > "$root/dirty/.venv-rocm/bin/python3"
printf '[project]\nname = "sneak"\n' > "$root/dirty/pyproject.toml"
touch "$root/dirty/a/__pycache__/x.cpython-314.pyc"

# external fixture: upstream Python inside the pinned clones + corpus cache
mkdir -p "$root/external/private-inference/engines/somefork/tools" \
         "$root/external/private-inference/engine/llama.cpp" \
         "$root/external/corpus-mcp/cache/some-corpus"
echo "upstream's own tool" > "$root/external/private-inference/engines/somefork/tools/bench.py"
echo "upstream's own converter" > "$root/external/private-inference/engine/llama.cpp/convert.py"
echo "cached Python SOURCE DOCUMENT — code-as-data" > "$root/external/corpus-mcp/cache/some-corpus/doc.py"

# corpus-aside fixture: Python next to the cache, not inside it
mkdir -p "$root/corpus-aside/corpus-mcp/reports"
echo "print('nope')" > "$root/corpus-aside/corpus-mcp/reports/sneak.py"

run_gate() { # fixture-subdir -> exit code, stdout+stderr captured
    local fixture=$1 out
    if out=$(bash "$GATE" 2>&1); then
        echo "$out"
        return 0
    else
        echo "$out"
        return 1
    fi
}

failed=0

expect_pass() { # name fixture-subdir
    local name=$1 fixture=$2 out rc
    out=$(KASK_DATA_DIRS="$root/$fixture" bash "$GATE" 2>&1) && rc=0 || rc=1
    if [ "$rc" -ne 0 ]; then
        echo "SELFTEST FAIL: $name — expected pass, gate exited 1:" >&2
        printf '%s\n' "$out" | sed 's/^/    /' >&2
        failed=1
    else
        echo "SELFTEST OK: $name"
    fi
}

expect_fail() { # name fixture-subdir required-mentions...
    local name=$1 fixture=$2; shift 2
    local out rc missing=0
    if out=$(KASK_DATA_DIRS="$root/$fixture" bash "$GATE" 2>&1); then
        rc=0
    else
        rc=1
    fi
    if [ "$rc" -ne 1 ]; then
        echo "SELFTEST FAIL: $name — expected exit 1, got $rc" >&2
        printf '%s\n' "$out" | sed 's/^/    /' >&2
        failed=1
        return
    fi
    local needle
    for needle in "$@"; do
        if ! printf '%s' "$out" | grep -qF "$needle"; then
            echo "SELFTEST FAIL: $name — output does not name '$needle'" >&2
            missing=1
            failed=1
        fi
    done
    [ "$missing" -eq 0 ] && echo "SELFTEST OK: $name (all mentions present)"
}

expect_pass "clean tree passes"                       clean
expect_fail "stray .py + venv + pyproject + pycache fail, all named" dirty \
    "stray.py" ".venv-rocm" "pyproject.toml" "__pycache__"
expect_pass "upstream Python in pinned clones and corpus cache is recorded, not gated" external
expect_fail "Python beside the corpus cache (outside it) still fails" corpus-aside \
    "sneak.py"

# the surfaced no-op: a missing dir list must PASS while SAYING so
out=$(KASK_DATA_DIRS="$root/does-not-exist" bash "$GATE" 2>&1) && rc=0 || rc=1
if [ "$rc" -eq 0 ] && printf '%s' "$out" | grep -qF "SURFACED no-op"; then
    echo "SELFTEST OK: missing data dirs pass with the surfaced no-op"
else
    echo "SELFTEST FAIL: missing dirs must pass WITH the surfaced no-op note; rc=$rc" >&2
    printf '%s\n' "$out" | sed 's/^/    /' >&2
    failed=1
fi

[ "$failed" -eq 0 ] || { echo "SELFTEST: FAILURES above" >&2; exit 1; }
echo "OK: check-no-python-data-dirs.sh fails on every synthetic violation, passes the clean and recorded-external trees, and surfaces the no-dir no-op."
