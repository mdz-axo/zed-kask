#!/usr/bin/env bash
# Replace exactly one occurrence of a literal string in a file. Writes nothing unless the count is 1.
# Usage: subst-once.sh <file> <old> <new>
set -euo pipefail
f="$1"; tmp="$(mktemp)"
if jq -Rjs --arg a "$2" --arg b "$3" '(split($a)|length - 1) as $n | if $n == 1 then (split($a)|join($b)) else error("matches: \($n) for: \($a)") end' "$f" > "$tmp"; then
  mv "$tmp" "$f"; echo "ok: $(printf '%s' "$2" | cut -c1-60)"
else rm -f "$tmp"; exit 3; fi
