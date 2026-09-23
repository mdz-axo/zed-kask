#!/usr/bin/env bash
# Isolated contract test for the verification-signal graph → Lean proof seam.
set -euo pipefail
if [[ $# -ne 1 || ! -x "$1" ]]; then
    echo "usage: $0 <lean-binary>" >&2
    exit 64
fi
lean=$1
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
generator="$root/kask/scripts/audit/generate-verification-preservation-proof.sh"
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
cat > "$scratch/graph.json" <<'JSON'
{"schema_version":1,"required":[{"expectation_id":"E1","falsifier_id":"F1","oracle_kind":"public_tool","failure_class":"silent_failure","provenance_tier":"oracle_verified"}],"before":[{"artifact_id":"focused","signal":{"expectation_id":"E1","falsifier_id":"F1","oracle_kind":"public_tool","failure_class":"silent_failure","provenance_tier":"oracle_verified"}},{"artifact_id":"suite","signal":{"expectation_id":"E1","falsifier_id":"F1","oracle_kind":"public_tool","failure_class":"silent_failure","provenance_tier":"oracle_verified"}}],"after":[{"artifact_id":"suite","signal":{"expectation_id":"E1","falsifier_id":"F1","oracle_kind":"public_tool","failure_class":"silent_failure","provenance_tier":"oracle_verified"}}],"removed_mappings":[{"removed_artifact":"focused","retained_artifact":"suite","signal":{"expectation_id":"E1","falsifier_id":"F1","oracle_kind":"public_tool","failure_class":"silent_failure","provenance_tier":"oracle_verified"}}]}
JSON
bash "$generator" "$scratch/graph.json" "$scratch/preserves.lean" > "$scratch/generator.log"
"$lean" "$scratch/preserves.lean" > "$scratch/lean.log" 2>&1
awk '/^def after : List SignalKey := \[/ { in_after=1; print; next } in_after && /^  ⟨/ { in_after=0; next } { print }' "$scratch/preserves.lean" > "$scratch/lean-omitted.lean"
if "$lean" "$scratch/lean-omitted.lean" > "$scratch/lean-omitted.log" 2>&1; then
    echo "Lean accepted an instance with the retained signal omitted" >&2
    exit 1
fi
jq 'del(.removed_mappings[0])' "$scratch/graph.json" > "$scratch/unmapped.json"
if bash "$generator" "$scratch/unmapped.json" "$scratch/unmapped.lean" > /dev/null 2>&1; then
    echo "missing removed-to-retained edge was accepted" >&2
    exit 1
fi
jq 'del(.after[0])' "$scratch/graph.json" > "$scratch/no-after.json"
if bash "$generator" "$scratch/no-after.json" "$scratch/no-after.lean" > /dev/null 2>&1; then
    echo "missing retained signal was accepted" >&2
    exit 1
fi
jq '.required[0].oracle_kind = "different_oracle"' "$scratch/graph.json" > "$scratch/unmatched-requirement.json"
if bash "$generator" "$scratch/unmatched-requirement.json" "$scratch/unmatched-requirement.lean" > /dev/null 2>&1; then
    echo "unmatched full-tuple requirement was accepted" >&2
    exit 1
fi
jq '.after[0].signal.failure_class = "other_failure"' "$scratch/graph.json" > "$scratch/changed-failure.json"
if bash "$generator" "$scratch/changed-failure.json" "$scratch/changed-failure.lean" > /dev/null 2>&1; then
    echo "downgraded failure class was accepted" >&2
    exit 1
fi
jq '.before += [{"artifact_id":"suite","signal":(.required[0] + {"expectation_id":"E2"})}] | .after += [{"artifact_id":"suite","signal":(.required[0] + {"expectation_id":"E2"})}]' "$scratch/graph.json" > "$scratch/omitted-requirement.json"
if bash "$generator" "$scratch/omitted-requirement.json" "$scratch/omitted-requirement.lean" > /dev/null 2>&1; then
    echo "baseline signal omitted from required set was accepted" >&2
    exit 1
fi
if bash "$generator" "$scratch/graph.json" "$scratch/preserves.lean" > /dev/null 2>&1; then
    echo "proof overwrite was accepted" >&2
    exit 1
fi
printf 'verification_proof_contract=pass positive=1 negative=7 lean=%s\n' "$lean"
