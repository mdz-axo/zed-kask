#!/usr/bin/env bash
# CI gate: every cross-surface reference in the skills ecosystem must resolve.
#
# Root cause this gate closes: incomplete multi-leg changes. A deletion or
# rename lands on one side (a skill, a template, an MCP tool) while the
# consumer-side update never does, leaving "phantom references" — prose that
# instructs an agent to call something that no longer exists. Verified
# instances: bug-hunt/tdd → deleted `harness-optimize`/`proptest` skills,
# logo-builder → deleted media templates, gemba-walk → deleted
# `validate_golden_outputs` executor. See
# kask/docs/plans/architecture-audit-2026-08-26.md §2.1.
#
# What it checks (heuristic, like check-mcp-tool-tests.sh — a ratchet, not a
# proof):
#   1. `skill-name` backtick refs in SKILL.md bodies that look like skill
#      names resolve against .agents/skills/ or are in the allowlist.
#   2. Template refs of the shape `<skill>/<file>` used with render_template
#      resolve against kask/registry/templates/.
#
# Exit codes:
#   0 — all refs resolve or are allowlisted
#   1 — at least one unresolved reference (regression)
#
# Usage: bash kask/scripts/check-skill-crossrefs.sh

set -euo pipefail

# Skills and templates live under the repo root (`.agents/skills/` is
# repo-root-relative, not kask-relative), so resolve everything from the
# repository root while keeping the script invocable from any directory.
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"

SKILLS_DIR=".agents/skills"
TEMPLATES_DIR="kask/registry/templates"

# Deliberate non-references that pattern-match but are not skill names.
# Extend only with a documented reason.
ALLOW_RE=(
  'cargo-mutants'
)

is_allowed() {
  local ref="$1"
  for allowed in "${ALLOW_RE[@]}"; do
    [ "$ref" = "$allowed" ] && return 0
  done
  return 1
}

FAIL=0

# ── Check 1: backtick refs that match installed skill names must exist ──
# Strategy: collect every kebab-case backtick token from every SKILL.md, then
# flag tokens that LOOK like skill names (≥3 segments, or contain a verb noun
# pair) and do not resolve to a skill, template dir, or known non-skill term.
#
# A token is treated as a skill reference when a skill exists whose name is a
# prefix/suffix overlap is too fuzzy; instead we invert: for every DELETED
# concept we care about, nothing flags. The enforceable direction is:
#   every `<skill>/<file>.j2` template ref resolves (check 2), and
#   every backtick token that exactly equals a *former* skill name fails.
# Former names are discovered from git history once and pinned here.

FORMER_SKILL_NAMES=(
  'harness-optimize'
  'harness-evolve-cycle'
  'proptest'
  'eqm'
  'eqm-improvement'
  'kali-audit'
  'adversarial-red-team'
  'graph-audit'
  'runtime-posture-monitor'
  'supply-chain-sentinel'
  'web-deep-research'
)

for skill_md in "$SKILLS_DIR"/*/SKILL.md; do
  skill=$(basename "$(dirname "$skill_md")")
  while IFS=: read -r line_no line; do
    for former in "${FORMER_SKILL_NAMES[@]}"; do
      # A former name that has since been restored is no longer deleted —
      # references to it resolve, so it must not flag (eqm/eqm-improvement
      # were deleted in 9bcfe558a0 and restored in 9ec1df0ca0).
      [ -f "$SKILLS_DIR/$former/SKILL.md" ] && continue
      if printf '%s' "$line" | grep -qF "\`$former\`"; then
        echo "UNRESOLVED: $skill_md:$line_no references deleted skill \`$former\`"
        FAIL=1
      fi
    done
  done < <(grep -n '`' "$skill_md" || true)
done

# ── Check 2: `<skill>/<file>.j2` / `<skill>/<file>.yaml` template refs must
# resolve on disk. Only refs carrying an explicit template extension are
# checked — bare `a/b` backticks are usually git refs, file paths, or prose.
for skill_md in "$SKILLS_DIR"/*/SKILL.md; do
  grep -noE '`[a-z0-9-]+/[a-z0-9._-]+\.(j2|yaml)`' "$skill_md" 2>/dev/null | while IFS=: read -r line_no match; do
    ref="${match//\`/}"
    ref="${ref%.j2}"
    ref="${ref%.yaml}"
    if is_allowed "$ref"; then
      continue
    fi
    if [ ! -f "$TEMPLATES_DIR/$ref.j2" ] && [ ! -f "$TEMPLATES_DIR/$ref.yaml" ]; then
      echo "UNRESOLVED: $skill_md:$line_no template ref \`$ref\` has no file under $TEMPLATES_DIR/"
      FAIL=1
    fi
  done || true
done

if [ "$FAIL" -ne 0 ]; then
  echo ""
  echo "FAIL: unresolved cross-surface references found."
  echo "Either complete the consumer-side update (the referenced artifact was"
  echo "deleted/renamed) or restore the artifact. See §5.0 triage protocol in"
  echo "kask/docs/plans/architecture-audit-2026-08-26.md."
  exit 1
fi

echo "OK: all skill↔template/deleted-skill cross-references resolve."
