#!/usr/bin/env bash
#
# ADVISORY AGENT TOOL — unwired from push/CI enforcement by operator
# mandate 2026-10-31 (the rules-that-don't-improve-code-quality cut).
# Run it voluntarily; nothing blocks a push or fails CI on it. Re-wiring
# it into kask/githooks/pre-push or .github/workflows/kask-invariants.yml
# requires an explicit operator decision.
# Skill-corpus crate-reference check — resolves kask-owned crate names cited
# in SKILL.md bodies and .j2 templates against the crates that actually exist.
#
# The deletion-residue gate. When a crate is removed, the removal sweeps its
# code and usually its SKILL.md mentions, but crate names embedded in template
# PROSE survive: `check-skill-crossrefs.sh` validates skill<->template refs,
# the contract audit validates contract<->body agreement, and neither resolves
# a crate name inside a template's scope list. Two dangling refs survived a
# fully green corpus that way (2026-10-17 audit):
#   - `hkask-portfolio-widget`  removed 12cfcb680c (2026-10-15), still named
#     in kask-seam-audit/audit-ui.j2's strict audit scope.
#   - `hkask-conversation-injector` removed fa95c2b8c7 (2026-09-28), same list.
# A "strict scope" list is followed literally, so a dangling entry sends an
# auditor hunting a crate that does not exist.
#
# Scope: backticked tokens matching the kask-owned crate shapes — `hkask-*`
# and the fork's GPUI surfaces (`*_panel`, `*_widget`, `*_ui`, `*_ui_common`).
# Upstream Zed crate names are NOT checked (the fork does not own their
# lifecycle and they are cited in many non-ownership senses).
#
# The crate scan set spans every Cargo.toml under all three homes a kask
# crate can live in. Both the package name and its containing directory name
# are accepted: most match, while a few workspace crates intentionally differ.
# A leftover directory without Cargo.toml is NOT a crate and cannot satisfy a
# reference.
#   crates/            (GPUI-dependent, kask-owned per the §13.1 invariant)
#   kask/crates/       (library crates)
#   kask/mcp-servers/  (MCP server crates)
# Omitting kask/mcp-servers/ reports every server crate as dangling — a
# false-absence class caught during this check's own construction.
#
# Usage: ./check-skill-crate-refs.sh
# Exit 0 when every referenced crate resolves; 1 with the dangling list.

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

SKILLS_DIR="${SKILL_CORPUS_DIR:-$ROOT/.agents/skills}"
TEMPLATES_DIR="${TEMPLATE_CORPUS_DIR:-$ROOT/kask/registry/templates}"
CRATE_ROOTS=(
    "${CRATE_ROOT_1:-$ROOT/crates}"
    "${CRATE_ROOT_2:-$ROOT/kask/crates}"
    "${CRATE_ROOT_3:-$ROOT/kask/mcp-servers}"
)

if [ ! -d "$SKILLS_DIR" ]; then
    echo "skill corpus directory not found: $SKILLS_DIR" >&2
    exit 1
fi
if [ ! -d "$TEMPLATES_DIR" ]; then
    echo "template corpus directory not found: $TEMPLATES_DIR" >&2
    exit 1
fi

crates_file="$(mktemp)"
refs_file="$(mktemp)"
trap 'rm -f "$crates_file" "$refs_file"' EXIT

# Existing crates: package and directory names backed by a Cargo.toml under
# every crate root. A stale empty directory must not make a deleted crate look
# live. Package-name extraction is deliberately limited to the [package]
# section; dependency `name =` fields are not crate identities.
for root in "${CRATE_ROOTS[@]}"; do
    [ -d "$root" ] || continue
    while IFS= read -r -d '' manifest; do
        basename "$(dirname "$manifest")"
        sed -n '/^\[package\]$/,/^\[/s/^name = "\([^"]*\)"/\1/p' "$manifest" | sed -n '1p'
    done < <(find "$root" -mindepth 2 -maxdepth 2 -type f -name Cargo.toml -print0)
done | sed '/^$/d' | sort -u > "$crates_file"

if [ ! -s "$crates_file" ]; then
    echo "no crates found under: ${CRATE_ROOTS[*]}" >&2
    exit 1
fi

# Referenced crate-shaped tokens in backticks. A bare (unbackticked) mention is
# prose, not a reference; requiring backticks keeps the check on identifiers.
{
    find "$SKILLS_DIR" -name SKILL.md -print0 | xargs -0 -r \
        grep -hoE '`(hkask-[a-z0-9-]+|[a-z0-9_]+_(panel|widget|ui|ui_common))`' || true
    find "$TEMPLATES_DIR" -name '*.j2' -print0 | xargs -0 -r \
        grep -hoE '`(hkask-[a-z0-9-]+|[a-z0-9_]+_(panel|widget|ui|ui_common))`' || true
} | tr -d '`' | sort -u > "$refs_file"

checked=0
flagged=0
while read -r crate; do
    [ -n "$crate" ] || continue
    checked=$((checked + 1))
    if ! grep -qxF -- "$crate" "$crates_file"; then
        flagged=$((flagged + 1))
        echo "DANGLING CRATE REFERENCE: \`$crate\` is cited but no such crate exists"
        # Feed grep from a file, never a pipe: `printf | grep -q` SIGPIPEs the
        # writer under pipefail (.rules).
        {
            if [ -d "$SKILLS_DIR" ]; then
                grep -rn --include=SKILL.md -- "\`$crate\`" "$SKILLS_DIR" || true
            fi
            if [ -d "$TEMPLATES_DIR" ]; then
                grep -rn --include='*.j2' -- "\`$crate\`" "$TEMPLATES_DIR" || true
            fi
        } | sed "s|$ROOT/||" | sed 's/^/    /'
    fi
done < "$refs_file"

if [ "$flagged" -gt 0 ]; then
    echo
    echo "FAIL: checked $checked referenced crates; $flagged dangling."
    echo "A removed crate's name left in skill or template prose is active misinformation:"
    echo "remove the reference in the same change that removes the crate."
    exit 1
fi

echo "OK: checked $checked referenced crates against $(wc -l < "$crates_file") existing; 0 dangling."
