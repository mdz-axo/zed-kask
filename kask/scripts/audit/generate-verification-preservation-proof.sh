#!/usr/bin/env bash
# Generate one Lean proof instance from a reconciled, five-field signal graph.
# Inputs are untrusted; never substitute an agent-written proof for this output.
set -euo pipefail

if [[ $# -ne 2 ]]; then
    echo "usage: $0 <graph.json> <new-proof.lean>" >&2
    exit 64
fi
graph=$1
proof=$2
for tool in jq sha256sum mktemp mv; do
    command -v "$tool" >/dev/null || { echo "missing required tool: $tool" >&2; exit 69; }
done
[[ -f "$graph" ]] || { echo "graph is missing: $graph" >&2; exit 66; }
[[ ! -e "$proof" ]] || { echo "refusing to overwrite proof: $proof" >&2; exit 73; }
[[ -d "$(dirname "$proof")" ]] || { echo "proof parent does not exist: $proof" >&2; exit 66; }

if ! jq -e '
  def key: [.expectation_id,.falsifier_id,.oracle_kind,.failure_class,.provenance_tier] | @json;
  def valid_signal:
    type == "object" and
    (keys == ["expectation_id","failure_class","falsifier_id","oracle_kind","provenance_tier"]) and
    all(.[]; type == "string" and length > 0);
  def valid_entry:
    type == "object" and keys == ["artifact_id","signal"] and
    (.artifact_id | type == "string" and length > 0) and (.signal | valid_signal);
  def valid_mapping:
    type == "object" and keys == ["removed_artifact","retained_artifact","signal"] and
    (.removed_artifact | type == "string" and length > 0) and
    (.retained_artifact | type == "string" and length > 0) and (.signal | valid_signal);
  . as $g |
  ($g.schema_version == 1) and
  ($g | keys == ["after","before","removed_mappings","required","schema_version"]) and
  ($g.required | type == "array" and length > 0 and all(.[]; valid_signal)) and
  ($g.before | type == "array" and length > 0 and all(.[]; valid_entry)) and
  ($g.after | type == "array" and length > 0 and all(.[]; valid_entry)) and
  ($g.removed_mappings | type == "array" and all(.[]; valid_mapping)) and
  (([$g.required[] | key] | unique) | length == ($g.required | length)) and
  (([$g.before[].signal | key] | unique) as $before |
   ([$g.after[].signal | key] | unique) as $after |
   ([$g.required[] | key] | unique) as $required |
   ($required - $before | length == 0) and
   ($before - $required | length == 0) and
   ($before - $after | length == 0)) and
  all($g.before[]; . as $old |
    any($g.after[]; .artifact_id == $old.artifact_id and (.signal | key) == ($old.signal | key)) or
    any($g.removed_mappings[]; . as $mapping |
      $mapping.removed_artifact == $old.artifact_id and
      ($mapping.signal | key) == ($old.signal | key) and
      any($g.after[]; .artifact_id == $mapping.retained_artifact and
        (.signal | key) == ($mapping.signal | key)))) and
  all($g.removed_mappings[]; . as $mapping |
    any($g.before[]; .artifact_id == $mapping.removed_artifact and
      (.signal | key) == ($mapping.signal | key)) and
    any($g.after[]; .artifact_id == $mapping.retained_artifact and
      (.signal | key) == ($mapping.signal | key)))
' "$graph" >/dev/null; then
    echo "verification signal graph is incomplete or has an unbound removal" >&2
    exit 65
fi

# Emit the complete tuple, not an opaque label. jq tojson escapes Lean string
# fields; key order does not affect the tuple's field order.
tmp=$(mktemp "${proof}.tmp.XXXXXX")
trap 'rm -f "$tmp"' EXIT
{
    cat <<'LEAN'
structure SignalKey where
  expectation : String
  falsifier : String
  oracleKind : String
  failureClass : String
  provenanceTier : String
  deriving DecidableEq, Repr

LEAN
    for field in required before after; do
        printf 'def %s : List SignalKey := [\n' "$field"
        jq -r --arg field "$field" '
          .[$field][] | (.signal // .) |
          "  ⟨" + (.expectation_id | tojson) + ", " + (.falsifier_id | tojson) + ", " +
          (.oracle_kind | tojson) + ", " + (.failure_class | tojson) + ", " +
          (.provenance_tier | tojson) + "⟩,"
        ' "$graph"
        printf ']\n\n'
    done
    cat <<'LEAN'
theorem required_is_covered : required.all (fun signal => decide (signal ∈ after)) = true := by
  decide

theorem no_baseline_signal_lost : before.all (fun signal => decide (signal ∈ after)) = true := by
  decide
LEAN
} > "$tmp"
if grep -qE '\b(sorry|axiom)\b' "$tmp"; then
    echo "proof contains an unproved escape" >&2
    exit 65
fi
mv "$tmp" "$proof"
trap - EXIT
printf 'graph_sha256=%s proof_sha256=%s proof=%s\n' \
    "$(sha256sum "$graph" | cut -d' ' -f1)" \
    "$(sha256sum "$proof" | cut -d' ' -f1)" "$proof"
