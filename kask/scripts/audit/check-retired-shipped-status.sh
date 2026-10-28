#!/usr/bin/env bash
# Read-only: for each named retired skill, report whether its last revision
# carried `shipped: false` (a name that never shipped needs no retirement
# sweep entry) or shipped in a payload (which requires one).
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$repo_root"

for n in "$@"; do
  last="$(git --no-pager log --format=%H -1 --diff-filter=D -- ".agents/skills/$n/SKILL.md")"
  if [ -z "$last" ]; then
    echo "$n: no deletion commit found"
    continue
  fi
  prev="$(git rev-parse "$last^")"
  frontmatter="$(git --no-pager show "$prev:.agents/skills/$n/SKILL.md" 2>/dev/null | sed -n '1,10p')"
  if printf '%s\n' "$frontmatter" | grep -q 'shipped: *false'; then
    echo "$n: shipped:false at last revision -> never in a payload, no sweep entry needed"
  else
    echo "$n: shipped in a payload -> retirement sweep entry REQUIRED"
  fi
done
