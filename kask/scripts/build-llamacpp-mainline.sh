#!/usr/bin/env bash
# build-llamacpp-mainline.sh — the canonical llama.cpp build recipe (P1 of
# the consolidation plan REV 2, 2026-11-06).
#
# ONE project engine surface: the L1-pinned mainline (v0.9.0-dev, commit
# 9e2a04a — kask/registry/private-inference/llama-cpp-mainline.toml). The
# pin is the manifest's own; this script reads it from there and never
# hardcodes a commit (requirement 3: the drift-absorption layer owns any
# future re-pin — a re-pin is a manifest edit, and this script follows).
#
# Reproduces the build from the repo alone:
#   checkout the pin → cmake HIP gfx1102 with the manifest-conformant flags
#   → build llama-server → the startup probe (exact-commit + flag-vocabulary
#   check, the same semantics as hkask-private-inference's
#   local_leg::probe::probe_binary).
#
# Idempotent against an existing tree: a tree already at the pin is
# re-configured (cmake updates the cache in place) and rebuilt
# incrementally (a no-op when up to date), then probed. A full from-scratch
# verification is the same script against an absent or re-pinned
# ENGINE_ROOT — never a manual cmake invocation.
#
# Artifacts land at exactly one path:
#   {artifacts}/private-inference/engines/llama-cpp-mainline/build-hip/
# the path placement::engine_root() resolves (the artifacts root chain:
# HKASK_ARTIFACTS_DIR → $XDG_DOCUMENTS_DIR/zk-data → ~/Documents/zk-data).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
MANIFEST="$REPO_ROOT/kask/registry/private-inference/llama-cpp-mainline.toml"

# --- the pin (the manifest's own; never a literal here) ---
pin_value() {
  awk '/^\[version_pin\]/{f=1;next} /^\[/{f=0} f && $1 == "'"$1"'"{print}' "$MANIFEST" \
    | sed -E 's/.*"([^"]+)".*/\1/'
}
PIN_COMMIT="$(pin_value commit)"
PIN_VERSION="$(pin_value version)"
if [ -z "$PIN_COMMIT" ] || [ -z "$PIN_VERSION" ]; then
  echo "REFUSED: could not read [version_pin] commit/version from $MANIFEST" >&2
  exit 1
fi

# --- the canonical engine root (the artifacts-root chain) ---
if [ -n "${HKASK_ARTIFACTS_DIR:-}" ] && { [ "${HKASK_ARTIFACTS_DIR:0:1}" = "/" ] || [ "${HKASK_ARTIFACTS_DIR:0:1}" = "." ]; }; then
  ARTIFACTS="$HKASK_ARTIFACTS_DIR"
elif [ -n "${XDG_DOCUMENTS_DIR:-}" ]; then
  ARTIFACTS="$XDG_DOCUMENTS_DIR/zk-data"
else
  ARTIFACTS="$HOME/Documents/zk-data"
fi
ENGINE_ROOT="$ARTIFACTS/private-inference/engines/llama-cpp-mainline"
BUILD_DIR="$ENGINE_ROOT/build-hip"
BINARY="$BUILD_DIR/bin/llama-server"

echo "[pin]      manifest commit=$PIN_COMMIT version=$PIN_VERSION"
echo "[artifacts] $ARTIFACTS"
echo "[engine]   $ENGINE_ROOT"

# --- source at the pin ---
resolve_pin() {
  # The manifest pin is short (7-hex); resolve it to the full sha against
  # the tree's own objects, deepening the upstream fetch history if the
  # commit object is not present yet. Never a guess — an unresolvable pin
  # is a typed refusal.
  if git -C "$ENGINE_ROOT" rev-parse --verify --quiet "$PIN_COMMIT^{commit}" >/dev/null 2>&1; then
    git -C "$ENGINE_ROOT" rev-parse "$PIN_COMMIT^{commit}"
    return 0
  fi
  local depth=200
  while [ "$depth" -le 25600 ]; do
    git -C "$ENGINE_ROOT" fetch --depth "$depth" origin master >/dev/null 2>&1 \
      || git -C "$ENGINE_ROOT" fetch --depth "$depth" origin main >/dev/null 2>&1 \
      || true
    if git -C "$ENGINE_ROOT" rev-parse --verify --quiet "$PIN_COMMIT^{commit}" >/dev/null 2>&1; then
      echo "[fetch]    deepened to depth $depth to resolve the pin" >&2
      git -C "$ENGINE_ROOT" rev-parse "$PIN_COMMIT^{commit}"
      return 0
    fi
    depth=$((depth * 4))
  done
  echo "REFUSED: could not resolve the manifest pin $PIN_COMMIT against ggml-org/llama.cpp (history fetched to depth $depth) — the drift-absorption layer owns re-pins; edit the manifest, never this script" >&2
  return 1
}
if [ ! -d "$ENGINE_ROOT/.git" ]; then
  echo "[clone]    ggml-org/llama.cpp -> $ENGINE_ROOT"
  git clone https://github.com/ggml-org/llama.cpp "$ENGINE_ROOT"
fi
if [ -n "$(git -C "$ENGINE_ROOT" status --porcelain)" ]; then
  echo "REFUSED: $ENGINE_ROOT has uncommitted changes — a dirty engine tree is never rebuilt over" >&2
  git -C "$ENGINE_ROOT" status --porcelain >&2
  exit 1
fi
HEAD_FULL="$(git -C "$ENGINE_ROOT" rev-parse HEAD)"
PIN_FULL="$(resolve_pin)"
if [ "$HEAD_FULL" != "$PIN_FULL" ]; then
  echo "[checkout] $HEAD_FULL -> $PIN_FULL (the manifest pin)"
  git -C "$ENGINE_ROOT" checkout --detach "$PIN_FULL"
fi
echo "[source]   at pin $PIN_FULL"

# --- configure + build (the manifest-conformant HIP flags, verified live
#     2026-11-06 against the canonical build's CMakeCache) ---
cmake -S "$ENGINE_ROOT" -B "$BUILD_DIR" \
  -DGGML_HIP=ON \
  -DAMDGPU_TARGETS=gfx1102 \
  -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_HIP_PLATFORM=amd \
  -DGGML_HIP_NO_VMM=ON \
  -DGGML_HIP_GRAPHS=ON \
  -DGGML_HIP_MMQ_MFMA=ON \
  -DGGML_HIP_RCCL=OFF
cmake --build "$BUILD_DIR" --target llama-server -j"$(nproc)"

# --- the startup probe: exact-commit + the flag vocabulary
#     (the same semantics as local_leg::probe::probe_binary — the manifest
#     is never trusted alone) ---
VERSION_OUT="$("$BINARY" --version 2>&1 || true)"
VLINE="$(grep -m1 '^version: ' <<<"$VERSION_OUT" || true)"
BIN_COMMIT="$(sed -n 's/.*commit \([0-9a-fA-F]*\)).*/\1/p' <<<"$VLINE")"
BIN_VERSION="$(sed -n 's/^version: \([^ ]*\).*/\1/p' <<<"$VLINE")"
if [ -z "$BIN_COMMIT" ] || [ -z "$BIN_VERSION" ]; then
  echo "REFUSED: unparsable --version output: $VERSION_OUT" >&2
  exit 1
fi
if [ "$BIN_COMMIT" != "$PIN_COMMIT" ]; then
  echo "REFUSED: engine commit mismatch — binary $BIN_COMMIT is not the pinned $PIN_COMMIT (exact-commit refusal; a different commit is an unverified surface, newer included)" >&2
  exit 1
fi

HELP="$("$BINARY" --help 2>&1 || true)"
MISSING=()
while IFS= read -r spelling; do
  [ -n "$spelling" ] || continue
  if ! grep -qE -- "${spelling}([^a-zA-Z0-9_-]|$)" <<<"$HELP"; then
    MISSING+=("$spelling")
  fi
done < <(awk '/^\[flags\]/{f=1;next} /^\[/{f=0} f && $1 !~ /^#/ && $1 != "binary"{print}' "$MANIFEST" \
  | grep -oE '"[^"]+"' | tr -d '"')
if [ "${#MISSING[@]}" -gt 0 ]; then
  echo "REFUSED: the engine binary lacks flag spellings the pinned surface requires: ${MISSING[*]} (manifest commit $PIN_COMMIT)" >&2
  exit 1
fi

# --- receipt ---
echo "[probe]    $VLINE"
echo "[probe]    STARTUP PROBE ACCEPTS: version $BIN_VERSION == $PIN_VERSION, commit $BIN_COMMIT == $PIN_COMMIT, vocabulary complete"
sha256sum "$BINARY"
echo "[done]     canonical build verified at $BUILD_DIR"
