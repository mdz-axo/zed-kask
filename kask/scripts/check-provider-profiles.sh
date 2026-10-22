#!/usr/bin/env bash
# CI gate: structural pins for the provider profile layer (kask/registry/providers/).
#
# The provider-audit skill (.agents/skills/provider-audit/SKILL.md) owns the
# semantic reading — data terms, status posture, severity. This gate owns the
# structural contract:
#   1. Every tree-adopted vendor has a profile. The adopted set is DERIVED at
#      run time from five tree anchors — inference descriptor ids
#      (INFERENCE_PROVIDERS), media provider structs,
#      research provider modules, and DATA_SERVICES credential keys — never
#      from a hardcoded list, which would silently miss every addition
#      (.rules, cross-seam test trap: name-pinned checks break on every
#      rename and silently miss every addition).
#   2. Every profile carries the required contract keys.
#   3. Profile id matches its filename; status is active|retired.
#   4. No zombie retirement (a retired profile whose vendor the tree still
#      adopts) and no active orphan (an active profile for a vendor the tree
#      does not adopt). Retired archives of departed vendors are legitimate.
#
# Exit codes: 0 = clean, 1 = violation, or the scan itself failed (never a
# silent pass).
#
# Usage: bash kask/scripts/check-provider-profiles.sh
# Selftest overrides: KASK_ROOT=<tree> PROFILES_DIR=<dir> bash kask/scripts/check-provider-profiles.sh

set -euo pipefail

KASK_ROOT="${KASK_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
INFERENCE_SRC="$KASK_ROOT/crates/kask_bridge/src/inference_providers.rs"
MEDIA_SRC="$KASK_ROOT/crates/hkask-inference/src/media_providers.rs"
RESEARCH_DIR="$KASK_ROOT/mcp-servers/hkask-mcp-research/src/research/providers"
PROFILES_DIR="${PROFILES_DIR:-$KASK_ROOT/registry/providers}"

for f in "$INFERENCE_SRC" "$MEDIA_SRC"; do
  [ -f "$f" ] || { echo "FAIL: derivation anchor missing: $f" >&2; exit 1; }
done
[ -d "$RESEARCH_DIR" ] || { echo "FAIL: derivation anchor missing: $RESEARCH_DIR" >&2; exit 1; }
[ -d "$PROFILES_DIR" ] || { echo "FAIL: profile dir missing: $PROFILES_DIR" >&2; exit 1; }

# ── Derive the adopted set from the tree anchors ──
adopted="$(
  {
    # Inference provider descriptor ids (INFERENCE_PROVIDERS).
    sed -nE 's/^[[:space:]]*id: "([^"]+)".*/\1/p' "$INFERENCE_SRC" | tr '[:upper:]' '[:lower:]'
    # Media provider structs (DeepInfraMediaProvider → deepinfra).
    sed -nE 's/^pub struct ([A-Za-z]+)MediaProvider.*/\1/p' "$MEDIA_SRC" | tr '[:upper:]' '[:lower:]'
    # The training-host anchor was removed with the training server
    # (model training is out of scope for the co-researcher mandate).
    # Research provider modules. Module spellings normalize to the
    # credential-key identity: serapi.rs serves SerpAPI, whose DATA_SERVICES
    # credential key is "serpapi" (kask://credentials/serpapi) — one vendor,
    # one profile id.
    for f in "$RESEARCH_DIR"/*.rs; do
      b="$(basename "$f" .rs)"
      [ "$b" = "serapi" ] && b="serpapi"
      echo "$b"
    done
    # Data-service credential keys (DATA_SERVICES) → vendor ids. Internal
    # keys (passphrase, SMTP) are not vendors; mapped keys normalize to the
    # vendor id the other anchors already emit.
    sed -nE 's/^[[:space:]]*credential_key: "([^"]+)".*/\1/p' "$INFERENCE_SRC" |
      while IFS= read -r k; do
        case "$k" in
          hkask_db_passphrase | hkask_smtp_password) continue ;;
          runpod) echo runpod ;;
          *) echo "$k" ;;
        esac
      done
  } | sort -u
)"

[ -n "$adopted" ] || {
  echo "FAIL: derived an empty adopted-vendor set — the anchors are broken (never a silent pass)." >&2
  exit 1
}

FAIL=0
fail() { echo "FAIL: $*" >&2; FAIL=1; }

is_adopted() { printf '%s\n' "$adopted" | grep -qx "$1"; }

# ── Checks 1-3: every adopted vendor has a contract-complete profile ──
for v in $adopted; do
  p="$PROFILES_DIR/$v.yaml"
  if [ ! -f "$p" ]; then
    fail "tree-adopted vendor '$v' has no profile at $p (kask/registry/providers/)"
    continue
  fi
  for key in id name status roles keychain consumed_capabilities deliberate_non_consumption data_terms status_surface pricing; do
    grep -qE "^${key}:" "$p" || fail "$p is missing required key '${key}:'"
  done
  pid="$(sed -nE 's/^id:[[:space:]]*([A-Za-z0-9_-]+).*/\1/p' "$p")"
  [ "$pid" = "$v" ] || fail "$p id '$pid' does not match filename stem '$v'"
  st="$(sed -nE 's/^status:[[:space:]]*(active|retired).*/\1/p' "$p")"
  if [ -z "$st" ]; then
    fail "$p status must be 'active' or 'retired'"
  elif [ "$st" = "retired" ]; then
    fail "$p is retired but the tree still adopts '$v' — a retirement must remove the integration (zombie)"
  fi
done

# ── Check 4: no active orphans; retired archives are legitimate ──
for p in "$PROFILES_DIR"/*.yaml; do
  [ -e "$p" ] || continue
  v="$(basename "$p" .yaml)"
  is_adopted "$v" && continue
  st="$(sed -nE 's/^status:[[:space:]]*(active|retired).*/\1/p' "$p")"
  if [ "$st" = "active" ]; then
    fail "$p is active but the tree does not adopt '$v' — stale profile (retire or delete it)"
  fi
done

if [ "$FAIL" -ne 0 ]; then
  echo "FAIL: provider profile layer violations (see above)." >&2
  exit 1
fi

count="$(printf '%s\n' "$adopted" | wc -l)"
echo "OK: provider profile layer clean — $count adopted vendors derived from the tree, all profiled."