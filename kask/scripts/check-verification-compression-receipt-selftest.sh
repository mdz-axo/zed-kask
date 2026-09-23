#!/usr/bin/env bash
set -euo pipefail
checker="$(cd "$(dirname "$0")" && pwd)/audit/check-verification-compression-receipt.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
printf 'old\n' > "$tmp/before"
printf 'new\n' > "$tmp/after"
printf 'contract\n' > "$tmp/contract"
printf 'oracle\n' > "$tmp/oracle"
printf 'graph\n' > "$tmp/graph"
printf 'proof\n' > "$tmp/proof"
printf 'log before\n' > "$tmp/log-before"
printf 'log after\n' > "$tmp/log-after"
diff -u --label source/x --label source/x "$tmp/before" "$tmp/after" > "$tmp/diff" || test "$?" -eq 1
receipt() {
  local role phase id path
  : > "$tmp/receipt"
  for spec in 'graph before graph' 'graph after graph' 'source before before' 'source after after' 'contract before contract' 'contract after contract' 'oracle before oracle' 'oracle after oracle' 'proof after proof' 'log before log-before' 'log after log-after' 'candidate_diff after diff'; do
    read -r role phase path <<< "$spec"
    id=singleton
    if [[ $role == source ]]; then id=x; fi
    printf '%s\t%s\t%s\t%s\t%s\n' "$role" "$phase" "$id" "$(sha256sum "$tmp/$path" | cut -d ' ' -f 1)" "$tmp/$path" >> "$tmp/receipt"
  done
}
check() { bash "$checker" "$1" "$tmp/receipt" "$(sha256sum "$tmp/receipt" | cut -d ' ' -f 1)"; }
reject() { if check "$1" > /dev/null 2>&1; then echo "accepted negative control: $2" >&2; exit 1; fi; }
receipt
check execute
reject analyze 'source changed in analyze mode'
# Path and digest are pinned independently: changing either without repinning must fail.
sed -i "s|$tmp/graph|$tmp/proof|" "$tmp/receipt"
reject execute 'spoofed path'
receipt
printf 'tamper\n' >> "$tmp/log-after"
reject execute 'tampered log'
printf 'log after\n' > "$tmp/log-after"
receipt
printf 'wrong\n' > "$tmp/diff"
receipt
reject execute 'unbound candidate diff'
diff -u --label source/x --label source/x "$tmp/before" "$tmp/after" > "$tmp/diff" || test "$?" -eq 1
receipt
printf 'old\n' > "$tmp/after"
receipt
reject execute 'stale diff without source change'
echo 'receipt controls passed'
