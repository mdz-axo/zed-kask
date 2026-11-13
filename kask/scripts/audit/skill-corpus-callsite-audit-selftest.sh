#!/usr/bin/env bash
# Exercise the call-site audit's detection in isolation: a cross-namespace
# delegation whose required inputs the caller never names must be flagged
# (the 2026-11-12 essentialist→coding-guidelines case the own-namespace-only
# audit missed), optional inputs must not count, an accepted-ledger entry
# must silence a citation-not-invocation, and a complete caller must pass.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
audit="$repo_root/kask/scripts/audit/skill-corpus-callsite-audit.sh"
fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT

skills="$fixture/skills"
templates="$fixture/templates"
accepted="$fixture/accepted.txt"
mkdir -p "$skills/caller" "$templates/caller" "$templates/callee"

cat > "$templates/callee/verify.j2" <<'EOF'
[inference]
contract:
  input:
    task_description: string
    diff_preview: string
    notes: string|null
    scope:
      type: object
      required: false
    budget:
      type: number
      default: 3
  output:
    violations: array
---
{{ task_description }} {{ diff_preview }}
EOF
cat > "$templates/caller/own.j2" <<'EOF'
[inference]
contract:
  input:
    subject: string
  output:
    result: string
---
{{ subject }}
EOF
: > "$accepted"

run() {
    SKILL_CORPUS_DIR="$skills" TEMPLATE_CORPUS_DIR="$templates" CALLSITE_ACCEPTED_FILE="$accepted" \
        bash "$audit" 2>&1 || true
}

# 1. Cross-namespace delegation naming only one of two required inputs: a gap
#    naming the undocumented input, and nothing about the optional ones.
printf '%s\n' 'Render `own.j2` with `subject`. Then delegate to `callee/verify` passing `task_description`.' \
    > "$skills/caller/SKILL.md"
output="$(run)"
if [[ "$output" != *'caller -> callee/verify: callsite-gap: 1 of 2 required inputs undocumented in SKILL.md: diff_preview'* ]]; then
    echo "Cross-namespace gap was not flagged with the missing input named: $output" >&2
    exit 1
fi
if [[ "$output" == *'notes'* || "$output" == *'scope'* || "$output" == *'budget'* ]]; then
    echo "An optional input (|null, required: false, default) was counted as required: $output" >&2
    exit 1
fi
if [[ "$output" != *'checked 2 call sites; 1 flagged;'* ]]; then
    echo "Expected both call sites checked and exactly one flagged: $output" >&2
    exit 1
fi
if SKILL_CORPUS_DIR="$skills" TEMPLATE_CORPUS_DIR="$templates" CALLSITE_ACCEPTED_FILE="$accepted" bash "$audit" >/dev/null 2>&1; then
    echo "A flagged run must exit nonzero" >&2
    exit 1
fi

# 2. Naming none of the required inputs is callsite-blind.
printf '%s\n' 'See the `callee/verify.j2` pattern.' > "$skills/caller/SKILL.md"
output="$(run)"
if [[ "$output" != *'caller -> callee/verify: callsite-blind: 2 required inputs, none documented in SKILL.md: task_description diff_preview'* ]]; then
    echo "Blind delegation was not flagged: $output" >&2
    exit 1
fi

# 3. The accepted ledger silences a citation that is not an invocation.
printf '%s\n' 'caller callee/verify' > "$accepted"
output="$(run)"
if [[ "$output" != *'checked 0 call sites; 0 flagged; 1 accepted (baseline);'* ]]; then
    echo "Accepted entry did not silence the citation: $output" >&2
    exit 1
fi
: > "$accepted"

# 4. A caller naming every required input passes with exit 0.
printf '%s\n' 'Delegate to `callee/verify` with `task_description` and `diff_preview`; render `own.j2` with `subject`.' \
    > "$skills/caller/SKILL.md"
if ! output="$(SKILL_CORPUS_DIR="$skills" TEMPLATE_CORPUS_DIR="$templates" CALLSITE_ACCEPTED_FILE="$accepted" bash "$audit" 2>&1)"; then
    echo "Complete caller was rejected: $output" >&2
    exit 1
fi
if [[ "$output" != 'checked 2 call sites; 0 flagged; 0 accepted (baseline); 0 skipped (no contract)' ]]; then
    echo "Unexpected clean summary: $output" >&2
    exit 1
fi

echo "OK: skill-corpus-callsite-audit selftest passed"
