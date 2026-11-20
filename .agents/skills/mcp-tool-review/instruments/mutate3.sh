#!/usr/bin/env bash
# mutate3.sh <table.tsv>: one mutation per row, the pin must FAIL. Differences from mutate2.sh (PE-13, PE-14):
#  - the table is read on fd 3 so no child process can consume it;
#  - a 5th optional column is a 1-based line number: the substitution is made only on that line
#    (for lines that occur more than once); otherwise the line must occur exactly once;
#  - git log -1 and the staged index are recorded before and after every row, and the row aborts the run
#    if either changed while the file was mutated (someone else may have captured the broken tree).
# Columns: file | exact line (trimmed) | replacement | test filter | [line number]. Run from the repo root.
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"; S=kask/mcp-servers/hkask-mcp-media/src
table="$1"; out="${table%.tsv}.out"; : > "$out"
state() { printf '%s|%s' "$(git --no-pager log -1 --format=%h)" "$(git --no-optional-locks diff --cached --name-only | sha256sum | cut -c1-12)"; }
while IFS=$'\t' read -r file line repl filter lineno <&3; do
  [ -z "$file" ] && continue
  f="$S/$file"; sha="$(sha256sum "$f" | cut -d' ' -f1)"; bak="$(mktemp)"; cp "$f" "$bak"
  before="$(state)"
  if [ -n "${lineno:-}" ]; then
    actual="$(sed -n "${lineno}p" "$f" | sed 's/^[ \t]*//')"
    if [ "$actual" != "$line" ]; then printf 'SKIP\t%s\t%s\tline %s is not the target\n' "$filter" "$file" "$lineno" | tee -a "$out"; rm -f "$bak"; continue; fi
    awk -v n="$lineno" -v r="$repl" 'NR==n {print r; next} {print}' "$bak" > "$f"
  else
    n="$(awk -v t="$line" '{s=$0; sub(/^[ \t]+/,"",s); if (s==t) c++} END{print c+0}' "$f")"
    if [ "$n" != 1 ]; then printf 'SKIP\t%s\t%s\tline occurs %s times\n' "$filter" "$file" "$n" | tee -a "$out"; rm -f "$bak"; continue; fi
    awk -v t="$line" -v r="$repl" '{s=$0; sub(/^[ \t]+/,"",s); if (s==t) print r; else print}' "$bak" > "$f"
  fi
  cmp -s "$bak" "$f" && { printf 'SKIP\t%s\t%s\tmutation is a no-op\n' "$filter" "$file" | tee -a "$out"; rm -f "$bak"; continue; }
  log="$here/mutation.$filter.$(echo "$line" | tr -c 'A-Za-z0-9' '_' | cut -c1-40).log"
  bash "$here/run-tests.sh" "$log" -p hkask-mcp-media --lib -- "$filter" >/dev/null 2>&1 </dev/null; rc=$?
  verdict="PIN-DID-NOT-FAIL"
  if grep -q "^test .*$filter.* FAILED" "$log"; then verdict="RED"; elif grep -q '^error' "$log"; then verdict="COMPILE-ERROR"; fi
  cp "$bak" "$f"; rm -f "$bak"
  [ "$(sha256sum "$f" | cut -d' ' -f1)" = "$sha" ] && restored=restored || restored=NOT-RESTORED
  after="$(state)"
  printf '%s\t%s\t%s\trc=%s\t%s\ttree %s -> %s\n' "$verdict" "$filter" "$file" "$rc" "$restored" "$before" "$after" | tee -a "$out"
  if [ "$before" != "$after" ]; then echo "TREE CHANGED UNDER THE MUTATION: stopping" | tee -a "$out"; exit 3; fi
done 3< "$table"
