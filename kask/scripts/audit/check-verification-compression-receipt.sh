#!/usr/bin/env bash
# Verify a trusted, externally pinned receipt against live immutable snapshots.
set -euo pipefail
if [[ $# -ne 3 || ( $1 != analyze && $1 != execute ) || ! $3 =~ ^[0-9a-f]{64}$ ]]; then
  echo 'usage: checker analyze|execute RECEIPT PINNED_SHA256' >&2
  exit 2
fi
mode=$1 receipt=$2 pin=$3
[[ -f $receipt && ! -L $receipt ]] || { echo 'missing or linked receipt' >&2; exit 1; }
[[ $(sha256sum "$receipt" | cut -d ' ' -f 1) == "$pin" ]] || { echo 'receipt pin mismatch' >&2; exit 1; }
declare -A hashes paths
while IFS=$'\t' read -r role phase id digest path extra; do
  [[ -z ${extra:-} && -n $role && -n $phase && -n $id && $id =~ ^[A-Za-z0-9_.-]+$ && $digest =~ ^[0-9a-f]{64}$ && $path == /* && -f $path && ! -L $path ]] || { echo 'invalid receipt row' >&2; exit 1; }
  [[ $(realpath -e -- "$path") == "$path" ]] || { echo 'noncanonical path' >&2; exit 1; }
  case "$role/$phase/$id" in
    graph/before/singleton|graph/after/singleton|contract/before/singleton|contract/after/singleton|oracle/before/singleton|oracle/after/singleton|proof/after/singleton|log/before/singleton|log/after/singleton|candidate_diff/after/singleton|source/before/*|source/after/*) ;;
    *) echo 'unexpected receipt role' >&2; exit 1 ;;
  esac
  key="$role/$phase/$id"
  [[ ! -v hashes[$key] ]] || { echo 'duplicate receipt row' >&2; exit 1; }
  hashes[$key]=$digest paths[$key]=$path
  [[ $(sha256sum "$path" | cut -d ' ' -f 1) == "$digest" ]] || { echo "file hash mismatch: $key" >&2; exit 1; }
done < "$receipt"
for key in graph/before/singleton graph/after/singleton contract/before/singleton contract/after/singleton oracle/before/singleton oracle/after/singleton proof/after/singleton log/before/singleton log/after/singleton candidate_diff/after/singleton; do
  [[ -v hashes[$key] ]] || { echo "missing receipt role: $key" >&2; exit 1; }
done
for role in contract oracle; do
  [[ ${hashes[$role/before/singleton]} == "${hashes[$role/after/singleton]}" ]] || { echo "$role changed" >&2; exit 1; }
done
sources=0
for key in "${!hashes[@]}"; do
  if [[ $key == source/before/* ]]; then
    id=${key#source/before/}
    [[ -v hashes[source/after/$id] ]] || { echo 'missing after source' >&2; exit 1; }
    ((sources+=1))
  elif [[ $key == source/after/* ]]; then
    id=${key#source/after/}
    [[ -v hashes[source/before/$id] ]] || { echo 'missing before source' >&2; exit 1; }
  fi
done
(( sources > 0 )) || { echo 'no sources' >&2; exit 1; }
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
: > "$work/diff"
while IFS= read -r id; do
  before="source/before/$id" after="source/after/$id"
  if [[ $mode == analyze && ${hashes[$before]} != "${hashes[$after]}" ]]; then
    echo 'analyze mode changed source' >&2; exit 1
  fi
  rc=0
  diff -u --label "source/$id" --label "source/$id" "${paths[$before]}" "${paths[$after]}" >> "$work/diff" || rc=$?
  [[ $rc -le 1 ]] || { echo 'diff failed' >&2; exit 1; }
done < <(for key in "${!hashes[@]}"; do [[ $key == source/before/* ]] && printf '%s\n' "${key#source/before/}"; done | LC_ALL=C sort)
cmp -s "$work/diff" "${paths[candidate_diff/after/singleton]}" || { echo 'candidate diff does not match source snapshots' >&2; exit 1; }
echo 'receipt and candidate diff verified'
