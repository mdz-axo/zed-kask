#!/usr/bin/env bash
# Exercise check-template-contracts.sh in isolation: a body-consistent
# contract passes; a declared output the body never mentions fails; a
# variable consumed only as the second term of an {% if a and b %}
# condition fails when the contract does not declare it.
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
  '    tone: string' \
  '  output:' '    summary: string' '---' 'Summarize {{ topic }}.' \
  '{% if topic and tone %} Use {{ tone }}.{% endif %}' \
  'Return JSON: {"summary": "..."}' > "$good"

output="$(REPO_ROOT="$fixture" bash "$fixture/kask/scripts/check-template-contracts.sh")"
if [[ "$output" != OK:* ]]; then
  echo "Expected a body-consistent contract (compound condition, both terms declared) to pass: $output" >&2
  exit 1
fi

printf '%s\n' \
  '{# goal: fixture. #}' '[inference]' 'contract:' '  input:' '    topic: string' \
  '  output:' '    summary: string' '    phantom_field: string' '---' 'Summarize {{ topic }}.' \
  'Return JSON: {"summary": "..."}' > "$good"
if output="$(REPO_ROOT="$fixture" bash "$fixture/kask/scripts/check-template-contracts.sh" 2>&1)"; then
  echo "Absent declared output was not rejected: $output" >&2
  exit 1
fi
if [[ "$output" != *'absent-output: phantom_field'* ]]; then
  echo "Absent declared output was not identified: $output" >&2
  exit 1
fi

# Second-term miss: `tone` is consumed only as the second operand of the
# compound condition and is not declared — the gate must flag it.
printf '%s\n' \
  '{# goal: fixture. #}' '[inference]' 'contract:' '  input:' '    topic: string' \
  '  output:' '    summary: string' '---' \
  '{% if topic and tone %}Tone advisory only.{% endif %}' \
  'Return JSON: {"summary": "..."}' > "$good"
if output="$(REPO_ROOT="$fixture" bash "$fixture/kask/scripts/check-template-contracts.sh" 2>&1)"; then
  echo "Undeclared second-term consumption was not rejected: $output" >&2
  exit 1
fi
if [[ "$output" != *'undeclared: tone'* ]]; then
  echo "Undeclared second-term consumption was not identified: $output" >&2
  exit 1
fi

# Raw server-rendered templates keep their documented inference contract
# inside a Jinja comment rather than using the stripped-header separator.
# A preceding comment must not be mistaken for the contract terminator.
printf '%s\n' \
  '{# goal: fixture.' '#}' '{#' '[inference]' 'contract:' \
  '  input:' '    topic: string' '    tone: string' \
  '  output:' '    summary: string' '#}' \
  'Summarize {{ topic }}.' \
  '{% if topic and tone %} Use {{ tone }}.{% endif %}' \
  'Return JSON: {"summary": "..."}' > "$good"
if ! output="$(REPO_ROOT="$fixture" bash "$fixture/kask/scripts/check-template-contracts.sh" 2>&1)"; then
  echo "A body-consistent commented contract was rejected: $output" >&2
  exit 1
fi

# The header itself must not count as consumption of an unused input.
printf '%s\n' \
  '{# goal: fixture.' '#}' '{#' '[inference]' 'contract:' \
  '  input:' '    topic: string' '    phantom_input: string' \
  '  output:' '    summary: string' '#}' \
  'Summarize {{ topic }}.' 'Return JSON: {"summary": "..."}' > "$good"
if output="$(REPO_ROOT="$fixture" bash "$fixture/kask/scripts/check-template-contracts.sh" 2>&1)"; then
  echo "An unused commented-contract input was not rejected: $output" >&2
  exit 1
fi
if [[ "$output" != *'unused-input: phantom_input'* ]]; then
  echo "An unused commented-contract input was not identified: $output" >&2
  exit 1
fi

printf '%s\n' \
  '{# goal: fixture. #}' '{#' '[inference]' 'contract:' \
  '  input:' '    topic: string' \
  '  output:' '    summary: string' '    phantom_output: string' '#}' \
  'Summarize {{ topic }}.' 'Return JSON: {"summary": "..."}' > "$good"
if output="$(REPO_ROOT="$fixture" bash "$fixture/kask/scripts/check-template-contracts.sh" 2>&1)"; then
  echo "An absent commented-contract output was not rejected: $output" >&2
  exit 1
fi
if [[ "$output" != *'absent-output: phantom_output'* ]]; then
  echo "An absent commented-contract output was not identified: $output" >&2
  exit 1
fi

# Nor may an earlier comment make a normal contract count its own header.
printf '%s\n' \
  '{# goal: fixture.' '#}' '[inference]' 'contract:' \
  '  input:' '    topic: string' '    phantom_input: string' \
  '  output:' '    summary: string' '---' \
  'Summarize {{ topic }}.' 'Return JSON: {"summary": "..."}' > "$good"
if output="$(REPO_ROOT="$fixture" bash "$fixture/kask/scripts/check-template-contracts.sh" 2>&1)"; then
  echo "A normal contract counted its header as input consumption: $output" >&2
  exit 1
fi
if [[ "$output" != *'unused-input: phantom_input'* ]]; then
  echo "A normal contract's unused input was not identified: $output" >&2
  exit 1
fi

echo 'OK: normal and commented contracts pass; unused inputs, absent outputs and undeclared condition operands fail the gate.'
