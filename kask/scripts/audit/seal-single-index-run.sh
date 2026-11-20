#!/usr/bin/env bash
# seal-single-index-run.sh — deterministically seal a single-index corpus run
# (e.g. `fine.db` for zk-ref-open) into `run-identity.json` matching the exact
# closed schema_version=3 `RunIdentity` struct in
# `kask/crates/hkask-memory/src/federated_recall.rs:1024-1040`.
#
# Replaces ad-hoc per-revision 3-tier `reseal-*.sh` scripts: binds ONLY the
# shipped index (`indexes: { "<index>": "<sha256>" }`) plus every required
# provenance digest, preserving the previous identity before replacing.
#
# usage: seal-single-index-run.sh <run-dir> <index-name> <actual-model> <revision-label> <interim|final> [run-spec-json]
set -euo pipefail

if [[ $# -lt 5 || $# -gt 6 ]]; then
    echo "usage: $0 <run-dir> <index-name> <actual-model> <revision-label> <interim|final> [run-spec-json]" >&2
    exit 64
fi

run_dir=$1
index_name=$2
actual_model=$3
revision_label=$4
phase=$5
run_spec=${6:-}

if [[ "$phase" != "interim" && "$phase" != "final" ]]; then
    echo "phase must be 'interim' or 'final' (got: $phase)" >&2
    exit 64
fi

for tool in jq sha256sum; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "required command not found: $tool" >&2
        exit 69
    fi
done

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
db_path="$run_dir/${index_name}.db"
rep_dir="$run_dir/representations"
rep_manifest="$rep_dir/manifest.json"
identity_path="$run_dir/run-identity.json"
queries_path="$run_dir/queries.jsonl"
evaluator_path="$script_dir/evaluate-chunk-retrieval.sh"

for f in "$db_path" "$rep_manifest"; do
    if [[ ! -f "$f" ]]; then
        echo "required artifact not found: $f" >&2
        exit 66
    fi
done

sha_file() { sha256sum "$1" | cut -d' ' -f1; }

# Pre-check existing run-identity.json if present.
old_run_id="none"
if [[ -f "$identity_path" ]]; then
    old_run_id=$(jq -r '.run_id' "$identity_path")
    old_computed=$(jq -cS 'del(.run_id)' "$identity_path" | sha256sum | cut -d' ' -f1)
    if [[ "${old_run_id,,}" != "${old_computed,,}" ]]; then
        echo "PRE-CHECK FAILED: existing run-identity.json does not self-verify" >&2
        exit 74
    fi
fi

db_sha=$(sha_file "$db_path")
rep_manifest_sha=$(sha_file "$rep_manifest")
policies_sha=$(jq -cS '{reference:.policies.reference,current:.policies.current,fine:.policies.fine,parent:.policies.parent}' "$rep_manifest" | sha256sum | cut -d' ' -f1)

if [[ -n "$run_spec" && -f "$run_spec" ]]; then
    accepted_sources_sha=$(jq -cS '.accepted_sources' "$run_spec" | sha256sum | cut -d' ' -f1)
    run_spec_sha=$(jq -cS . "$run_spec" | sha256sum | cut -d' ' -f1)
    requested_model=$(jq -r '.embedding_model' "$run_spec")
    retriever_sha=$(jq -cS '.retriever' "$run_spec" | sha256sum | cut -d' ' -f1)
else
    accepted_sources_sha="$rep_manifest_sha"
    run_spec_sha="$rep_manifest_sha"
    requested_model="$actual_model"
    retriever_sha="$rep_manifest_sha"
fi

if [[ -f "$queries_path" ]]; then
    queries_sha=$(sha_file "$queries_path")
else
    queries_sha="$rep_manifest_sha"
fi

if [[ -f "$evaluator_path" ]]; then
    evaluator_sha=$(sha_file "$evaluator_path")
else
    evaluator_sha="$rep_manifest_sha"
fi

# Collect representation shard digests present on disk.
rep_obj="{}"
for pair in "reference:reference.jsonl" "current:current.jsonl" "fine:fine-children.jsonl" "child_parent_map:child-parent-map.jsonl" "parent:parents.jsonl"; do
    key="${pair%%:*}"
    fname="${pair##*:}"
    if [[ -f "$rep_dir/$fname" ]]; then
        fsha=$(sha_file "$rep_dir/$fname")
        rep_obj=$(jq -cn --argjson obj "$rep_obj" --arg k "$key" --arg v "$fsha" '$obj + {($k): $v}')
    fi
done

# Deterministic preseal_run_id over pre-index inputs.
preseal_run_id=$(jq -cnS \
    --arg accepted_sources_sha256 "$accepted_sources_sha" \
    --arg run_spec_sha256 "$run_spec_sha" \
    --arg queries_sha256 "$queries_sha" \
    --arg requested_embedding_model "$requested_model" \
    --arg policies_sha256 "$policies_sha" \
    --arg retriever_sha256 "$retriever_sha" \
    --arg evaluator_sha256 "$evaluator_sha" \
    --arg representations_manifest_sha256 "$rep_manifest_sha" \
    --argjson representations "$rep_obj" '
    {
      schema_version: 3,
      accepted_sources_sha256: $accepted_sources_sha256,
      run_spec_sha256: $run_spec_sha256,
      queries_sha256: $queries_sha256,
      requested_embedding_model: $requested_embedding_model,
      policies_sha256: $policies_sha256,
      retriever_sha256: $retriever_sha256,
      evaluator_sha256: $evaluator_sha256,
      representations_manifest_sha256: $representations_manifest_sha256,
      representations: $representations
    }
' | sha256sum | cut -d' ' -f1)

identity_base=$(mktemp)
trap 'rm -f "$identity_base"' EXIT

jq -n \
    --arg preseal_run_id "$preseal_run_id" \
    --arg accepted_sources_sha256 "$accepted_sources_sha" \
    --arg run_spec_sha256 "$run_spec_sha" \
    --arg queries_sha256 "$queries_sha" \
    --arg requested_embedding_model "$requested_model" \
    --arg actual_embedding_model "$actual_model" \
    --arg policies_sha256 "$policies_sha" \
    --arg retriever_sha256 "$retriever_sha" \
    --arg evaluator_sha256 "$evaluator_sha" \
    --arg representations_manifest_sha256 "$rep_manifest_sha" \
    --argjson representations "$rep_obj" \
    --arg index_name "$index_name" \
    --arg db_sha "$db_sha" '
    {
      schema_version: 3,
      preseal_run_id: $preseal_run_id,
      accepted_sources_sha256: $accepted_sources_sha256,
      run_spec_sha256: $run_spec_sha256,
      queries_sha256: $queries_sha256,
      requested_embedding_model: $requested_embedding_model,
      actual_embedding_model: $actual_embedding_model,
      policies_sha256: $policies_sha256,
      retriever_sha256: $retriever_sha256,
      evaluator_sha256: $evaluator_sha256,
      representations_manifest_sha256: $representations_manifest_sha256,
      representations: $representations,
      indexes: { ($index_name): $db_sha }
    }
' > "$identity_base"

new_run_id=$(jq -cS . "$identity_base" | sha256sum | cut -d' ' -f1)

if [[ -f "$identity_path" ]]; then
    cp "$identity_path" "$run_dir/run-identity.pre-${phase}-${revision_label}.json"
fi

jq -cS --arg run_id "$new_run_id" '. + {run_id: $run_id}' "$identity_base" > "$identity_path"
sha_file "$identity_path" > "${identity_path}.sha256"

# Post-check self-verification.
chk=$(jq -r '.run_id' "$identity_path")
cmp2=$(jq -cS 'del(.run_id)' "$identity_path" | sha256sum | cut -d' ' -f1)
if [[ "${chk,,}" != "${cmp2,,}" ]]; then
    echo "POST-CHECK FAILED: new run-identity.json does not self-verify" >&2
    exit 74
fi

echo "seal-single-index-run: index=$index_name phase=$phase run_id=$old_run_id -> $new_run_id (self-verified OK)"
