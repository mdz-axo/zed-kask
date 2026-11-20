#!/usr/bin/env bash
# check-mcp-review-citations.sh — mcp-tool-review's citation-existence oracle.
# Verifies every citation in a findings artifact against the current tree:
#   1. the cited file exists (repo-relative to the git root),
#   2. the cited line is within the file's line count,
#   3. the excerpt (when present) appears within ±3 lines of the cited line.
# Also verifies each tools[].file/line registration citation (existence + range).
# Usage: check-mcp-review-citations.sh <findings-artifact.json>
# Exits 0 when every citation passes; 1 when any fails; 2 on usage/parse errors.
set -euo pipefail

if [ $# -ne 1 ]; then
  echo "usage: $0 <findings-artifact.json>" >&2
  exit 2
fi

ARTIFACT=$1
if [ ! -f "$ARTIFACT" ]; then
  echo "error: artifact not found: $ARTIFACT" >&2
  exit 2
fi
if ! jq -e 'type == "object" and (((.findings? // null) | type == "array") or ((.tools? // null) | type == "array"))' "$ARTIFACT" > /dev/null 2>&1; then
  echo "error: artifact is not a valid review-citations JSON object (expected .findings[] and/or .tools[]): $ARTIFACT" >&2
  exit 2
fi

ROOT=$(git rev-parse --show-toplevel)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
pass=0
fail=0

# UC-3 (tree-sitter adoption proposal,
# kask/docs/research/tree-sitter-adoption-proposal.md): mechanical
# re-anchor suggestion for a failed citation — advisory output only, the
# gate's exit contract is unchanged. Preference order: the excerpt's
# current verbatim line when unique, else the enclosing definition's
# current start line resolved structurally (resolve-citation-anchor.sh
# over tree-sitter).
suggest_reanchor() {
  local file=$1 line=$2 excerpt=$3
  local path="$ROOT/$file"
  [ -f "$path" ] || return 0
  if [ -n "$excerpt" ]; then
    local n
    n=$(grep -Fc -- "$excerpt" "$path" || true)
    if [ "$n" -eq 1 ]; then
      local re_line
      re_line=$(grep -Fn -- "$excerpt" "$path" | cut -d: -f1)
      echo "     re-anchor: excerpt found verbatim at $file:$re_line"
      return 0
    fi
  fi
  local def
  def=$("$SCRIPT_DIR/resolve-citation-anchor.sh" "$path" "$line" 2>/dev/null) || return 0
  local def_name def_line
  def_name=$(printf '%s' "$def" | cut -f1)
  def_line=$(printf '%s' "$def" | cut -f2)
  [ -n "$def_name" ] || return 0
  echo "     re-anchor: enclosing definition '$def_name' currently at $file:$def_line"
}

check_site() {
  local file=$1 line=$2 excerpt=$3 kind=$4
  local path="$ROOT/$file"
  if [ -z "$file" ] || [[ ! "$line" =~ ^[0-9]+$ ]]; then
    echo "FAIL [$kind] $file:$line — missing file or non-numeric line"
    fail=$((fail + 1))
    return
  fi
  if [ "$kind" = "finding" ] && [ -z "$excerpt" ]; then
    echo "FAIL [$kind] $file:$line — finding citation requires a non-empty excerpt"
    fail=$((fail + 1))
    return
  fi
  if [ ! -f "$path" ]; then
    echo "FAIL [$kind] $file:$line — file not found"
    fail=$((fail + 1))
    return
  fi
  local max_line
  max_line=$(wc -l < "$path")
  if [ "$line" -lt 1 ] || [ "$line" -gt "$max_line" ]; then
    echo "FAIL [$kind] $file:$line — line out of range (file has $max_line lines)"
    suggest_reanchor "$file" "$line" "$excerpt"
    fail=$((fail + 1))
    return
  fi
  if [ -n "$excerpt" ]; then
    local from=$((line - 3))
    [ "$from" -lt 1 ] && from=1
    local to=$((line + 3))
    if ! sed -n "${from},${to}p" "$path" | grep -F -- "$excerpt" > /dev/null 2>&1; then
      echo "FAIL [$kind] $file:$line — excerpt not found within ±3 lines"
      suggest_reanchor "$file" "$line" "$excerpt"
      fail=$((fail + 1))
      return
    fi
  fi
  echo "PASS [$kind] $file:$line"
  pass=$((pass + 1))
}

# Findings citations: file + line + excerpt (excerpt checked verbatim, ±3 lines).
# Enumerate into a variable first so jq failures never hide behind bash
# process-substitution `< <(...)` (ce066584: a malformed element died mid-stream
# and let the loop exit 0 on a truncated count).
if ! finding_rows=$(jq -c '
  (.findings // []) |
  if type != "array" then error("expected .findings to be an array") else . end |
  .[] |
  if type != "object" then error("expected each .findings[] entry to be an object") else . end |
  (.citations // []) |
  if type != "array" then error("expected .findings[].citations to be an array") else . end |
  .[] |
  if type != "object" then error("expected each .findings[].citations[] entry to be an object") else . end
' "$ARTIFACT"); then
  echo "error: failed to enumerate .findings[].citations[] in $ARTIFACT" >&2
  exit 2
fi
while IFS= read -r row; do
  [ -n "$row" ] || continue
  file=$(jq -r '.file // ""' <<<"$row")
  line=$(jq -r '(.line // "") | tostring' <<<"$row")
  excerpt=$(jq -r '.excerpt // ""' <<<"$row")
  check_site "$file" "$line" "$excerpt" "finding"
done <<<"$finding_rows"

# Tool registration citations: file + line (existence + range).
if ! tool_rows=$(jq -c '
  (.tools // []) |
  if type != "array" then error("expected .tools to be an array") else . end |
  .[] |
  if type != "object" then error("expected each .tools[] entry to be an object") else . end
' "$ARTIFACT"); then
  echo "error: failed to enumerate .tools[] in $ARTIFACT" >&2
  exit 2
fi
while IFS= read -r row; do
  [ -n "$row" ] || continue
  file=$(jq -r '.file // ""' <<<"$row")
  line=$(jq -r '(.line // "") | tostring' <<<"$row")
  check_site "$file" "$line" "" "tool"
done <<<"$tool_rows"

total=$((pass + fail))
echo "----"
echo "citations checked: $total  passed: $pass  failed: $fail"
if [ "$total" -eq 0 ]; then
  echo "error: zero citations checked in $ARTIFACT — refusing to pass vacuously" >&2
  exit 1
fi
[ "$fail" -eq 0 ]