#!/usr/bin/env bash
# check-no-python-data-dirs.sh — CI gate: zero agent-authored Python in the
# DATA DIRS, past the repo boundary (operator ruling 2026-11-06: no Python
# anywhere an agent authors — the project's languages are Rust, bash, the
# Jinja2 and lisp forms). The repo-surface gate is check-no-python.sh
# (operator ruling 2026-10-13); its scan stops at kask/ and fork-owned
# workflows — exactly the blind spot the 2026-11-06 incident exploited: an
# agent stream built a PyTorch replication harness plus a 21 GB torch venv
# under the artifacts dir and tagged it with a "placement ruling" that read
# the repo gate's scope as permission. A gate that stops at the repo
# boundary cannot see that class; this one does.
#
# Scope: the artifacts dir and the hidden data dir, resolved the way
# hkask_types::agent_paths resolves them — $KASK_ARTIFACTS_DIR /
# $HOME/Documents/zk-data and $KASK_DATA_DIR / $HOME/.local/share/zed-kask.
# KASK_DATA_DIRS (colon-separated absolute paths) overrides both, for the
# selftest. A runner with none of these dirs reports a SURFACED no-op —
# never a silent green; the authoritative run is on the operator's machine.
#
# Externals, recorded not gated (pinned upstream trees and code-as-data —
# the same class the 2026-10-13 rule records for script/, crates/eval_cli/):
#   */private-inference/engines/* and */private-inference/engine/*
#       the pinned llama.cpp clones (upstream's own Python ships in every
#       clone — vendored, never authored here)
#   */corpus-mcp/cache/*
#       cached document text — code-as-data (a corpus may cache .py source
#       files as documents; that is data, not tooling)
#   .git, node_modules, target — never gated
#
# Fail classes: standalone *.py; a virtualenv (.venv*, venv, virtualenv —
# reported once at its root, its subtree pruned); __pycache__; declared
# tooling (pyproject.toml, requirements*.txt, Pipfile, .python-version,
# tox.ini).
#
# Exit codes: 0 = clean (or surfaced no-op), 1 = Python surface found.
# Usage: bash kask/scripts/check-no-python-data-dirs.sh
# Selftest: bash kask/scripts/check-no-python-data-dirs-selftest.sh
set -euo pipefail

if [ -n "${KASK_DATA_DIRS:-}" ]; then
    IFS=':' read -r -a dirs <<< "$KASK_DATA_DIRS"
else
    dirs=("${KASK_ARTIFACTS_DIR:-$HOME/Documents/zk-data}"
          "${KASK_DATA_DIR:-$HOME/.local/share/zed-kask}")
fi

violations=0
scanned=0

for dir in "${dirs[@]}"; do
    if [ ! -d "$dir" ]; then
        echo "[SKIP] data dir not present on this machine: $dir"
        continue
    fi
    scanned=$((scanned + 1))
    # Branch order matters: the externals prune BEFORE the fail prints, so
    # upstream's own Python inside the pinned clones is never flagged; the
    # venv/__pycache__ arms print then prune, so a virtualenv is reported
    # once at its root instead of as 40k site-package files.
    while IFS= read -r -d '' hit; do
        printf '[FAIL] Python surface in the data dirs (operator ruling 2026-11-06): %s\n' "$hit" >&2
        violations=$((violations + 1))
    done < <(find "$dir" \
        \( -name .git -o -name node_modules -o -name target \) -prune -o \
        -path '*/private-inference/engines*' -prune -o \
        -path '*/private-inference/engine*' -prune -o \
        -path '*/corpus-mcp/cache*' -prune -o \
        \( -name '*.py' -o -name pyproject.toml -o -name 'requirements*.txt' \
           -o -name Pipfile -o -name .python-version -o -name tox.ini \) -print0 -o \
        \( \( -name '.venv*' -o -name venv -o -name virtualenv -o -name __pycache__ \) -type d \) -print0 -prune \
        2>/dev/null)
done

if [ "$scanned" -eq 0 ]; then
    echo "[OK] no data dirs present on this machine — SURFACED no-op, never a silent green; the authoritative run is local (operator ruling 2026-11-06)"
    exit 0
fi

[ "$violations" -eq 0 ] || {
    echo "[FAIL] $violations Python surface(s) in the data dirs (operator ruling 2026-11-06: no Python anywhere an agent authors; repo-surface gate: check-no-python.sh)" >&2
    exit 1
}
echo "[OK] zero Python in the data dirs ($scanned scanned; externals recorded, not gated)"
