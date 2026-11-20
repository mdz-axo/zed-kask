#!/usr/bin/env bash
# CI gate: structural pins for the provider profile layer (kask/registry/providers/).
#
# The provider-audit skill (.agents/skills/provider-audit/SKILL.md) owns the
# semantic reading — data terms, status posture, severity. This gate owns the
# structural contract:
#   1. Every tree-adopted vendor has a profile. The adopted set is DERIVED at
#      run time from four tree anchors — inference descriptor ids
#      (INFERENCE_PROVIDERS, its defining file located at run time), media
#      provider structs, research provider modules, and DATA_SERVICES
#      credential keys — never from a hardcoded list, which would silently
#      miss every addition (.rules, cross-seam test trap: name-pinned checks
#      break on every rename and silently miss every addition). The defining
#      file is not pinned either: the table moved from kask_bridge's
#      inference_providers.rs to hkask-inference's openai_compat.rs
#      (a356ad6ffe, 2026-11-17) and the then-pinned path silently dropped
#      'ollama' from the derived set while the tree still adopted it —
#      caught live at the pre-push gate 2026-11-17.
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
MEDIA_SRC="$KASK_ROOT/crates/hkask-inference/src/media_providers.rs"
RESEARCH_DIR="$KASK_ROOT/mcp-servers/hkask-mcp-research/src/research/providers"
PROFILES_DIR="${PROFILES_DIR:-$KASK_ROOT/registry/providers}"

# The INFERENCE_PROVIDERS table is the canonical inference-id anchor; its
# defining file is LOCATED at run time (see header). Missing = the anchor is
# broken; multiple = two registries, never a guess — both fail visibly.
INFERENCE_SRC="$(grep -rl --include='*.rs' 'pub static INFERENCE_PROVIDERS' "$KASK_ROOT/crates" 2>/dev/null || true)"
if [ -z "$INFERENCE_SRC" ]; then
  echo "FAIL: derivation anchor missing: no 'pub static INFERENCE_PROVIDERS' definition under $KASK_ROOT/crates" >&2
  exit 1
fi
if [ "$(printf '%s\n' "$INFERENCE_SRC" | grep -c .)" -ne 1 ]; then
  echo "FAIL: ambiguous derivation anchor: multiple INFERENCE_PROVIDERS definitions:" >&2
  printf '%s\n' "$INFERENCE_SRC" >&2
  exit 1
fi

# DATA_SERVICES credential-key descriptors stayed in kask_bridge's
# inference_providers.rs when the inference table moved out of it.
DATA_SERVICES_SRC="$KASK_ROOT/crates/kask_bridge/src/inference_providers.rs"

for f in "$DATA_SERVICES_SRC" "$MEDIA_SRC"; do
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
    # keys (passphrase) are not vendors; mapped keys normalize to the
    # vendor id the other anchors already emit.
    sed -nE 's/^[[:space:]]*credential_key: "([^"]+)".*/\1/p' "$DATA_SERVICES_SRC" |
      while IFS= read -r k; do
        case "$k" in
          hkask_db_passphrase) continue ;;
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