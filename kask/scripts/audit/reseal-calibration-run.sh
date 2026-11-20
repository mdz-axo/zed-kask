#!/usr/bin/env bash
# Reseal a sealed calibration run after a hmems schema migration.
#
# The seal binds run-identity.json's `indexes` digests to the database
# bytes and `run_id` to the identity file's own canonical serialization
# (jq -cS, one trailing newline — the producer's pipeline,
# hkask-memory/src/federated_recall.rs `sealed_run_id`). A schema
# migration that touches a sealed database (R5 2026-10-10: the hmems
# `provenance` column) changes the bytes, so the seal must be re-bound or
# every federated admission of the source fails the byte-digest check.
#
# This script performs the same migration the storage layer runs on a
# read-write open (hkask-storage `migrate_hmems_provenance`: PRAGMA
# table_info probe, then ALTER TABLE ADD COLUMN with the fail-safe
# DEFAULT 'model_inference'), checkpoints the WAL to the sealed state
# (admission requires an empty WAL), re-computes every index digest, and
# re-derives `run_id`. The original identity is preserved beside the run
# as run-identity.pre-reseal-<date>.json on the first pass.
#
# The SQLCipher key is read from a running hkask-mcp-corpus /
# hkask-mcp-curator process environment (/proc/<pid>/environ) — the same
# source the host-call script uses — and is never printed or persisted.
#
# usage: reseal-calibration-run.sh <run-dir> [<manifest.json> <source-id>]
#   <run-dir>     sealed calibration run directory (run-identity.json +
#                 reference.db / current.db / fine.db)
#   <manifest>    optional: the curator federated-sources manifest naming
#                 a source whose database lives under <run-dir>; when
#                 given (with its source id), the script ends by running
#                 the real admission path (hkask-memory
#                 examples/verify_federated_source) over the re-sealed
#                 artifacts — the live verification, not an assertion.
set -euo pipefail

if [[ $# -ne 1 && $# -ne 3 ]]; then
    echo "usage: $0 <run-dir> [<manifest.json> <source-id>]" >&2
    exit 64
fi

run_dir=$1
manifest_arg=${2:-}
source_id_arg=${3:-}

for tool in sqlcipher jq sha256sum; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "required command not found: $tool" >&2
        exit 69
    fi
done

if [[ ! -f "$run_dir/run-identity.json" ]]; then
    echo "run identity does not exist: $run_dir/run-identity.json" >&2
    exit 66
fi

# ── The SQLCipher key, from a running server's environment ─────────────────
# Never printed, never written to disk. Fail closed without a live server.
db_passphrase=
for pattern in '^hkask-mcp-corpus$' '^hkask-mcp-curator$'; do
    for pid in $(pgrep -f "$pattern" 2>/dev/null || true); do
        db_passphrase=$(tr '\0' '\n' < "/proc/$pid/environ" 2>/dev/null | sed -n 's/^HKASK_DB_PASSPHRASE=//p' | head -1)
        if [[ -n "$db_passphrase" ]]; then
            break 2
        fi
    done
done
if [[ -z "$db_passphrase" ]]; then
    echo "HKASK_DB_PASSPHRASE not found in any running hkask-mcp-corpus/curator environment —" >&2
    echo "start the editor (the governed servers carry the key) or export HKASK_DB_PASSPHRASE" >&2
    exit 69
fi
# Escape single quotes for the PRAGMA key literal (the storage layer's
# own escaping rule: ' -> '').
key_escaped=$(printf '%s' "$db_passphrase" | sed "s/'/''/g")

# Run one statement batch against a sealed database. This sqlcipher build
# acks `PRAGMA key` with an `ok` line on STDOUT — strip it so command
# substitution captures only the statement output (a capture of
# "ok\n1" != "1" silently fails every count check — observed live
# 2026-10-10: the migration landed and the post-verify read it as absent).
sq() {
    sqlcipher "$1" "PRAGMA key='$key_escaped'; $2" 2>/dev/null | tail -n +2
}

# ── Pre-check: the identity's own run_id must verify before anything moves ──
# A broken self-digest means the seal was already tampered with — repairing
# over it would launder the tamper. Abort instead.
pre_run_id=$(jq -r '.run_id' "$run_dir/run-identity.json")
pre_computed=$(jq -cS 'del(.run_id)' "$run_dir/run-identity.json" | sha256sum | cut -d' ' -f1)
if [[ "${pre_run_id,,}" != "${pre_computed,,}" ]]; then
    echo "pre-check FAILED: run-identity.json run_id does not verify against its own content —" >&2
    echo "the seal is already broken; refusing to re-seal over it (expected $pre_run_id, computed $pre_computed)" >&2
    exit 74
fi
echo "pre-check ok: run_id self-verifies"

# ── Migrate every present index database ─────────────────────────────────────
# The same migration the storage layer runs on a read-write open. The
# ALTER is metadata-only (no row rewrite); the DEFAULT backfills legacy
# rows as model_inference — the fail-safe direction (an unstamped memory
# never reads tool_observed).
indexes=(reference current fine)
declare -A before_digest
for index in "${indexes[@]}"; do
    db="$run_dir/$index.db"
    [[ -f "$db" ]] || continue
    before_digest[$index]=$(sha256sum "$db" | cut -d' ' -f1)
    has_column=$(sq "$db" "SELECT count(*) FROM pragma_table_info('hmems') WHERE name = 'provenance';")
    if [[ "$has_column" != "1" ]]; then
        echo "migrating $index.db: adding hmems.provenance"
        sq "$db" "ALTER TABLE hmems ADD COLUMN provenance TEXT NOT NULL DEFAULT 'model_inference';" >/dev/null
    else
        echo "migrating $index.db: provenance already present (idempotent)"
    fi
    # The sealed state is a checkpointed WAL (admission refuses a nonempty
    # WAL) — truncate, then drop the sidecars, matching the producer.
    sq "$db" "PRAGMA wal_checkpoint(TRUNCATE);" >/dev/null
    for suffix in "-wal" "-shm"; do
        sidecar="$db$suffix"
        [[ -f "$sidecar" ]] && rm -f "$sidecar"
    done
done

# ── Preserve the pre-reseal identity (first pass only) ───────────────────────
date_tag=$(date +%Y-%m-%d)
preseal_copy="$run_dir/run-identity.pre-reseal-$date_tag.json"
if [[ ! -f "$preseal_copy" ]]; then
    cp "$run_dir/run-identity.json" "$preseal_copy"
    echo "preserved prior identity: $preseal_copy"
fi

# ── Re-bind the seal: indexes digests, then the run_id self-digest ───────────
identity="$run_dir/run-identity.json"
for index in "${indexes[@]}"; do
    db="$run_dir/$index.db"
    [[ -f "$db" ]] || continue
    digest=$(sha256sum "$db" | cut -d' ' -f1)
    tmp_identity=$(mktemp)
    jq --arg digest "$digest" ".indexes.$index = \$digest" "$identity" > "$tmp_identity"
    mv "$tmp_identity" "$identity"
    echo "re-bound indexes.$index: ${before_digest[$index]:-?} -> $digest"
done
new_run_id=$(jq -cS 'del(.run_id)' "$identity" | sha256sum | cut -d' ' -f1)
tmp_identity=$(mktemp)
jq --arg run_id "$new_run_id" '.run_id = $run_id' "$identity" > "$tmp_identity"
mv "$tmp_identity" "$identity"
echo "re-derived run_id: $pre_run_id -> $new_run_id"

# ── Post-verify: schema, digests, self-digest ───────────────────────────────
for index in "${indexes[@]}"; do
    db="$run_dir/$index.db"
    [[ -f "$db" ]] || continue
    columns=$(sq "$db" "SELECT count(*) FROM pragma_table_info('hmems');")
    provenance=$(sq "$db" "SELECT count(*) FROM pragma_table_info('hmems') WHERE name = 'provenance';")
    if [[ "$provenance" != "1" ]]; then
        echo "post-verify FAILED: $index.db hmems lacks the provenance column" >&2
        exit 74
    fi
    digest=$(sha256sum "$db" | cut -d' ' -f1)
    sealed=$(jq -r ".indexes.$index" "$identity")
    if [[ "${digest,,}" != "${sealed,,}" ]]; then
        echo "post-verify FAILED: $index.db digest $digest != sealed $sealed" >&2
        exit 74
    fi
    echo "post-verify ok: $index.db hmems columns=$columns provenance present, digest matches the seal"
done
post_run_id=$(jq -r '.run_id' "$identity")
post_computed=$(jq -cS 'del(.run_id)' "$identity" | sha256sum | cut -d' ' -f1)
if [[ "${post_run_id,,}" != "${post_computed,,}" ]]; then
    echo "post-verify FAILED: re-derived run_id does not self-verify" >&2
    exit 74
fi
echo "post-verify ok: run_id self-verifies"

# ── Live verification through the real admission path ───────────────────────
# Optional: with a manifest + source id, run the production admission
# (ReadOnlyPassageSource::open — run-identity shape, run_id self-digest,
# byte digest, current schema, embedding identity) plus one probe search
# over the re-sealed artifacts. The key passes through the environment to
# the example child; it is never printed.
if [[ -n "$manifest_arg" ]]; then
    repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
    echo "verifying admission: source '$source_id_arg' via $manifest_arg"
    HKASK_DB_PASSPHRASE="$db_passphrase" cargo run --quiet --manifest-path "$repo_root/Cargo.toml" -p hkask-memory --example verify_federated_source -- "$manifest_arg" "$source_id_arg"
fi

echo "re-sealed: $run_dir"