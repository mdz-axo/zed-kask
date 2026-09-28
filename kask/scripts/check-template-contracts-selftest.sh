#!/usr/bin/env bash
# Exercise check-template-contracts.sh in isolation: a body-consistent
# contract passes; a declared output the body never mentions fails.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT
mkdir -p "$fixture/kask/scripts/audit" "$fixture/kask/registry/templates/example"
cp "$repo_root/kask/scripts/check-template-contracts.sh" "$fixture/kask/scripts/"
cp "$repo_root/kask/scripts/audit/skill-corpus-contract-audit.sh" "$fixture/kask/scripts/audit/"

good="$fixture/kask/registry/templates/example/good.j2"
printf '%s\n' \
  '{# goal: fixture. #}' '[inference]' 'contract:' '  input:' '    topic: string' \
  '  output:' '    summary: string' '---' 'Summarize {{ topic }}.' 'Return JSON: {"summary": "..."}' > "$good"

output="$(REPO_ROOT="$fixture" bash "$fixture/kask/scripts/check-template-contracts.sh")"
if [[ "$output" != OK:* ]]; then
  echo "Expected a body-consistent contract to pass: $output" >&2
  exit 1
fi

printf '%s\n' \
  '{# goal: fixture. #}' '[inference]' 'contract:' '  input:' '    topic: string' \
  '  output:' '    summary: string' '    phantom_field: string' '---' 'Summarize {{ topic }}.' 'Return JSON: {"summary": "..."}' > "$good"
if output="$(REPO_ROOT="$fixture" bash "$fixture/kask/scripts/check-template-contracts.sh" 2>&1)"; then
  echo "Absent declared output was not rejected: $output" >&2
  exit 1
fi
if [[ "$output" != *'absent-output: phantom_field'* ]]; then
  echo "Absent declared output was not identified: $output" >&2
  exit 1
fi

echo 'OK: consistent contracts pass; a declared output the body never produces fails the gate.'
