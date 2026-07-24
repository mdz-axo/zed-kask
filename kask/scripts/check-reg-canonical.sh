#!/usr/bin/env bash
# CI gate: every `reg.*` tracing target must be a canonical REG namespace
# (registered in `CANONICAL_NAMESPACES`, directly or via an ancestor).
#
# Rationale: the `reg.*` prefix is reserved for canonical REG spans — the
# essential, ν-event-eligible, `SpanCategory`-categorized, loop-connected spans.
# Performative telemetry MUST use `hkask.*` targets, not `reg.*` (PRINCIPLES §9.1).
# Without this gate, performative logs borrowing the `reg.` prefix silently
# re-grow, recreating the registry/code drift (the 89 stray `reg.*` targets
# cleaned up in the reg-canonical sweep).
#
# This gate checks TWO surfaces:
# 1. Rust code (.rs): `target: "reg.*"` tracing targets in production code
# 2. Jinja2 templates (.j2): `reg.*` namespace references in skill templates
#    (templates instruct agents to emit spans — those references must be
#    canonical too, otherwise the agent would emit non-canonical spans)
#
# Enabled in CI via `.github/workflows/ci.yml` invariants job.
# Run locally: `bash scripts/check-reg-canonical.sh`
#
# Exit codes:
#   0 — every `reg.*` reference in production code AND templates is canonical
#   1 — a non-canonical `reg.*` reference was found (register it or move it to `hkask.*`)

set -euo pipefail
cd "$(dirname "$0")/.."

REGISTRY="crates/hkask-types/src/event.rs"

if [ ! -f "$REGISTRY" ]; then
  echo "FAIL: registry not found at $REGISTRY"
  exit 1
fi

# is_canonical <namespace>: returns 0 if the namespace (or any ancestor, by
# dot-trimming) is registered in CANONICAL_NAMESPACES, else 1. MIRRORS the
# `is_canonical` ancestor-matching rule in crates/hkask-types/src/event.rs —
# update both together.
is_canonical() {
  local cur="$1"
  while [ -n "$cur" ]; do
    if grep -qF -- "\"$cur\"" "$REGISTRY" 2>/dev/null; then
      return 0
    fi
    case "$cur" in
      *.*) cur="${cur%.*}" ;;
      *) cur="" ;;
    esac
  done
  return 1
}

FAIL=0

# ── Surface 1: Rust code (.rs) — `target: "reg.*"` tracing targets ───────
# Collect every distinct `target: "reg.<...>"` in production code
# (exclude tests, examples, build artifacts).
mapfile -t TARGETS < <(
  grep -rhoE 'target: "reg\.[a-z0-9_.]+"' crates/ mcp-servers/ \
    --include='*.rs' \
    --exclude-dir=target \
    --exclude-dir=tests \
    --exclude-dir=examples \
    2>/dev/null \
    | sed -E 's/.*"(reg\.[a-z0-9_.]+)"/\1/' \
    | sort -u
)

for ns in "${TARGETS[@]}"; do
  if ! is_canonical "$ns"; then
    sites=$( { grep -rnF -- "target: \"$ns\"" crates/ mcp-servers/ \
      --include='*.rs' --exclude-dir=target --exclude-dir=tests --exclude-dir=examples \
      2>/dev/null || true; } | head -5 )
    echo "  non-canonical reg.* target (Rust): $ns"
    echo "$sites" | sed 's/^/    /'
    FAIL=1
  fi
done

# ── Surface 2: Jinja2 templates (.j2) — `reg.*` namespace references ──────
# Skill templates (.j2) instruct agents to emit REG spans. Those references
# must be canonical too — otherwise the agent would emit non-canonical spans.
# We scan for `reg.<word>.<word>` patterns (at least two dot-separated segments
# after 'reg.') to avoid false positives from prose like "reg" alone.
#
# RATCHETED: pre-existing non-canonical references are allowlisted below.
# As each is fixed (either registered in CANONICAL_NAMESPACES or retargeted to
# hkask.*), remove it from the allowlist. When the allowlist is empty, the
# standard is fully enforced and cannot regress.
TEMPLATE_ALLOWLIST=(
  # Empty — all references are now canonical or retargeted to hkask.*
)

is_template_allowlisted() {
  local ns="$1"
  for item in "${TEMPLATE_ALLOWLIST[@]}"; do
    [ "$item" = "$ns" ] && return 0
  done
  return 1
}

mapfile -t J2_REFS < <(
  grep -rhoE 'reg\.[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+' registry/templates/ \
    --include='*.j2' \
    2>/dev/null \
    | sort -u
)

template_ratchet=0
for ns in "${J2_REFS[@]}"; do
  if ! is_canonical "$ns"; then
    sites=$( { grep -rnF -- "$ns" registry/templates/ \
      --include='*.j2' \
      2>/dev/null || true; } | head -5 )
    if is_template_allowlisted "$ns"; then
      echo "ratchet: $ns is non-canonical (allowlisted — ${#TEMPLATE_ALLOWLIST[@]} remaining)"
      echo "$sites" | sed 's/^/    /'
      template_ratchet=$((template_ratchet + 1))
    else
      echo "  non-canonical reg.* reference (template): $ns"
      echo "$sites" | sed 's/^/    /'
      FAIL=1
    fi
  fi
done

if [ "$FAIL" -eq 0 ]; then
  echo "OK: every reg.* reference in Rust code and .j2 templates is canonical (registered in CANONICAL_NAMESPACES)."
  if [ "$template_ratchet" -gt 0 ]; then
    echo "ratchet: $template_ratchet non-canonical template reference(s) allowlisted (pre-existing — fix to remove from allowlist)"
  fi
  exit 0
else
  echo ""
  echo "FAIL: non-canonical reg.* references found."
  echo "The reg.* prefix is reserved for canonical REG spans (PRINCIPLES §9.1)."
  echo "Fix: either register the namespace in $REGISTRY (if it drives a cybernetic"
  echo "loop / becomes a ν-event) or retarget the span to hkask.* (if it is"
  echo "performative telemetry)."
  exit 1
fi
