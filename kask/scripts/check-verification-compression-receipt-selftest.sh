#!/usr/bin/env bash
set -euo pipefail
script_dir="$(cd "$(dirname "$0")" && pwd)"
checker="$script_dir/audit/check-verification-compression-receipt.sh"
generator="$script_dir/audit/generate-verification-preservation-proof.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
printf 'old\n' > "$tmp/before"
printf 'new\n' > "$tmp/after"
printf 'contract\n' > "$tmp/contract"
printf 'oracle\n' > "$tmp/oracle"
cat > "$tmp/graph" <<'JSON'
{"schema_version":1,"required":[{"expectation_id":"E1","falsifier_id":"F1","oracle_kind":"tool","failure_class":"silent","provenance_tier":"verified"}],"before":[{"artifact_id":"focused","signal":{"expectation_id":"E1","falsifier_id":"F1","oracle_kind":"tool","failure_class":"silent","provenance_tier":"verified"}},{"artifact_id":"suite","signal":{"expectation_id":"E1","falsifier_id":"F1","oracle_kind":"tool","failure_class":"silent","provenance_tier":"verified"}}],"after":[{"artifact_id":"suite","signal":{"expectation_id":"E1","falsifier_id":"F1","oracle_kind":"tool","failure_class":"silent","provenance_tier":"verified"}}],"removed_mappings":[{"removed_artifact":"focused","retained_artifact":"suite","signal":{"expectation_id":"E1","falsifier_id":"F1","oracle_kind":"tool","failure_class":"silent","provenance_tier":"verified"}}]}
JSON
cp "$tmp/graph" "$tmp/graph-after"
bash "$generator" "$tmp/graph-after" "$tmp/proof" > /dev/null
printf 'log before\n' > "$tmp/log-before"
printf 'log after\n' > "$tmp/log-after"
printf '%s\n' '{"scope":"warm","cold":{"status":"not_run","samples_ms":[]},"warm":{"status":"measured","samples_ms":[200,210]}}' > "$tmp/timing-before"
printf '%s\n' '{"scope":"warm","cold":{"status":"not_run","samples_ms":[]},"warm":{"status":"measured","samples_ms":[180,190]}}' > "$tmp/timing-after"
diff -u --label source/x --label source/x "$tmp/before" "$tmp/after" > "$tmp/diff" || test "$?" -eq 1
receipt() {
  local role phase id path
  : > "$tmp/receipt"
  for spec in 'graph before graph' 'graph after graph-after' 'source before before' 'source after after' 'contract before contract' 'contract after contract' 'oracle before oracle' 'oracle after oracle' 'proof after proof' 'log before log-before' 'log after log-after' 'timing before timing-before' 'timing after timing-after' 'candidate_diff after diff'; do
    read -r role phase path <<< "$spec"
    id=singleton
    if [[ $role == source ]]; then id=x; fi
    printf '%s\t%s\t%s\t%s\t%s\n' "$role" "$phase" "$id" "$(sha256sum "$tmp/$path" | cut -d ' ' -f 1)" "$tmp/$path" >> "$tmp/receipt"
  done
}
approved=$(sha256sum "$tmp/diff" | cut -d ' ' -f 1)
check() { local authorization=${2:-$approved}; bash "$checker" "$1" "$tmp/receipt" "$(sha256sum "$tmp/receipt" | cut -d ' ' -f 1)" "$authorization"; }
reject() { if check "$1" "${3:-$approved}" > /dev/null 2>&1; then echo "accepted negative control: $2" >&2; exit 1; fi; }
receipt
check execute | jq -e '.verified == true and .timing.scope == "warm" and .timing.warm.before_ms == [200,210] and .timing.warm.after_ms == [180,190]' >/dev/null
printf '%s\n' '{"scope":"warm","cold":{"status":"not_run","samples_ms":[]},"warm":{"status":"measured","samples_ms":[-10,190]}}' > "$tmp/timing-after"
receipt
reject execute 'nonpositive pinned timing sample'
printf '%s\n' '{"scope":"warm","cold":{"status":"not_run","samples_ms":[]},"warm":{"status":"measured","samples_ms":[180,190]}}' > "$tmp/timing-after"
receipt
reject execute 'unapproved diff despite valid pinned receipt' "$(printf 'other operator-approved diff\n' | sha256sum | cut -d ' ' -f 1)"
reject analyze 'source changed in analyze mode' '-' 
printf 'forged proof\n' > "$tmp/proof"
receipt
reject execute 'proof does not derive from after graph'
rm -f "$tmp/proof"
bash "$generator" "$tmp/graph-after" "$tmp/proof" > /dev/null
jq '(.required[] | .expectation_id) = "E2" | (.before[].signal | .expectation_id) = "E2" | (.after[].signal | .expectation_id) = "E2" | (.removed_mappings[].signal | .expectation_id) = "E2"' "$tmp/graph" > "$tmp/graph-after"
rm -f "$tmp/proof"
bash "$generator" "$tmp/graph-after" "$tmp/proof" > /dev/null
receipt
reject execute 'after graph substitutes baseline signals'
cp "$tmp/graph" "$tmp/graph-after"
rm -f "$tmp/proof"
bash "$generator" "$tmp/graph-after" "$tmp/proof" > /dev/null
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
reject execute 'unbound candidate diff' "$(sha256sum "$tmp/diff" | cut -d ' ' -f 1)"
diff -u --label source/x --label source/x "$tmp/before" "$tmp/after" > "$tmp/diff" || test "$?" -eq 1
receipt
printf 'old\n' > "$tmp/after"
receipt
reject execute 'stale diff without source change'
echo 'receipt controls passed'
