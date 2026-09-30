#!/usr/bin/env bash
# Exercise registry-path classification and failure propagation in isolation.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT
mkdir -p "$fixture/kask/scripts" "$fixture/kask/registry/styles/hemingway" "$fixture/kask/registry/templates/example" "$fixture/.agents/skills/example"
cp "$repo_root/kask/scripts/check-skill-crossrefs.sh" "$fixture/kask/scripts/"
printf 'config\n' > "$fixture/kask/registry/styles/hemingway/voice.yaml"
printf 'template\n' > "$fixture/kask/registry/templates/example/prompt.j2"
printf '%s\n' '`hemingway/voice.yaml` `example/prompt.j2`' > "$fixture/.agents/skills/example/SKILL.md"

# The gate derives its former-skill-name list from the retirement record —
# give the fixture a minimal record with one name (the derivation failure
# modes are pinned by the last two cases below).
mkdir -p "$fixture/crates/agent_skills"
cat > "$fixture/crates/agent_skills/agent_skills.rs" <<'EOF'
const RETIRED_SKILL_NAMES: &[&str] = &[
    "retired-example",
];
EOF

output="$(bash "$fixture/kask/scripts/check-skill-crossrefs.sh")"
if [[ "$output" != *'OK: all skill↔template/deleted-skill cross-references resolve.'* ]]; then
  echo "Expected existing style and template references to resolve: $output" >&2
  exit 1
fi

printf '%s\n' '`example/missing.j2`' >> "$fixture/.agents/skills/example/SKILL.md"
if output="$(bash "$fixture/kask/scripts/check-skill-crossrefs.sh")"; then
  echo "Missing template was not rejected: $output" >&2
  exit 1
fi
if [[ "$output" != *'example/missing'* ]]; then
  echo "Missing template was not identified: $output" >&2
  exit 1
fi

printf '%s\n' '`hemingway/missing.yaml`' > "$fixture/.agents/skills/example/SKILL.md"
if output="$(bash "$fixture/kask/scripts/check-skill-crossrefs.sh")"; then
  echo "Missing style config was not rejected: $output" >&2
  exit 1
fi
if [[ "$output" != *'hemingway/missing'* ]]; then
    echo 'Missing style config was not identified: $output' >&2
    exit 1
fi

# A backticked reference to a retired skill must fail (check 1).
printf '%s\n' '`example/prompt.j2` and `retired-example`' > "$fixture/.agents/skills/example/SKILL.md"
if output="$(bash "$fixture/kask/scripts/check-skill-crossrefs.sh")"; then
    echo "Retired-skill reference was not rejected: $output" >&2
    exit 1
fi
if [[ "$output" != *'references deleted skill'* ]]; then
    echo "Retired-skill hit was not identified: $output" >&2
    exit 1
fi

# A missing retirement record must fail loudly, not pass vacuously.
rm "$fixture/crates/agent_skills/agent_skills.rs"
if output="$(bash "$fixture/kask/scripts/check-skill-crossrefs.sh")"; then
    echo "Missing retirement record was accepted: $output" >&2
    exit 1
fi
if [[ "$output" != *'retired-skill record not found'* ]]; then
    echo "Missing-record failure was not identified: $output" >&2
    exit 1
fi

echo 'OK: existing paths resolve; missing paths, retired-skill references, and a missing retirement record all fail the gate.'
