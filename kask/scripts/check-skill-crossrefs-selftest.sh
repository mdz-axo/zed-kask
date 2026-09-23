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
  echo "Missing style config was not identified: $output" >&2
  exit 1
fi

echo 'OK: existing style and template paths resolve; missing paths fail the gate.'
