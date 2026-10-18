#!/usr/bin/env bash
# Read-only: derive the retirement set by the record's OWN stated method —
# every name whose `.agents/skills/<name>/SKILL.md` was deleted, existed
# without `shipped: false` in at least one revision (so some payload carried
# it), and is absent from HEAD — then diff it against the curated
# RETIRED_SKILL_NAMES array. A name in the derived set but not the array is a
# retirement that skipped its required append.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$repo_root"

record="crates/agent_skills/agent_skills.rs"

# Curated array, parsed exactly as check-skill-crossrefs.sh does.
awk '/^const RETIRED_SKILL_NAMES/{f=1;next} f&&/^\];/{f=0} f' "$record" \
  | grep -oE '"[a-z0-9-]+"' | tr -d '"' | sort -u > /tmp/retired-curated.txt

# Derived set: every deleted SKILL.md path in history.
git --no-pager log --diff-filter=D --name-only --format='' -- '.agents/skills/*/SKILL.md' \
  | grep -E '^\.agents/skills/[^/]+/SKILL\.md$' \
  | sed -E 's|^\.agents/skills/([^/]+)/SKILL\.md$|\1|' | sort -u > /tmp/retired-deleted-ever.txt

: > /tmp/retired-derived.txt
while IFS= read -r name; do
  [ -z "$name" ] && continue
  # Absent from HEAD (a restored skill is not retired).
  [ -f ".agents/skills/$name/SKILL.md" ] && continue
  # Did it ever exist without `shipped: false`? Then a payload carried it.
  ever_shipped=0
  while IFS= read -r rev; do
    [ -z "$rev" ] && continue
    body="$(git --no-pager show "$rev:.agents/skills/$name/SKILL.md" 2>/dev/null || true)"
    [ -z "$body" ] && continue
    if ! printf '%s\n' "$body" | sed -n '1,10p' | grep -q 'shipped: *false'; then
      ever_shipped=1
      break
    fi
  done < <(git --no-pager log --format=%H -- '.agents/skills/'"$name"'/SKILL.md')
  [ "$ever_shipped" -eq 1 ] && echo "$name" >> /tmp/retired-derived.txt
done < /tmp/retired-deleted-ever.txt

sort -u /tmp/retired-derived.txt -o /tmp/retired-derived.txt

echo "curated: $(wc -l < /tmp/retired-curated.txt) names"
echo "derived: $(wc -l < /tmp/retired-derived.txt) names"
echo "--- retired but MISSING from the curated record ---"
comm -23 /tmp/retired-derived.txt /tmp/retired-curated.txt
echo "--- curated but not derived (record may carry older/manual names) ---"
comm -13 /tmp/retired-derived.txt /tmp/retired-curated.txt | head -40
