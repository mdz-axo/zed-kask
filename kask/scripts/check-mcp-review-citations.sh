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
if ! jq -e . "$ARTIFACT" > /dev/null 2>&1; then
  echo "error: artifact is not valid JSON: $ARTIFACT" >&2
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
while IFS=$'\x1f' read -r file line excerpt; do
  check_site "$file" "$line" "$excerpt" "finding"
done < <(jq -r '.findings[]?.citations[]? | [.file, (.line | tostring), (.excerpt // "")] | join("\u001f")' "$ARTIFACT")

# Tool registration citations: file + line (existence + range).
while IFS=$'\x1f' read -r file line; do
  check_site "$file" "$line" "" "tool"
done < <(jq -r '.tools[]? | [.file, (.line | tostring)] | join("\u001f")' "$ARTIFACT")

echo "----"
echo "citations checked: $((pass + fail))  passed: $pass  failed: $fail"
[ "$fail" -eq 0 ]