#!/usr/bin/env bash
# Run a crate's suite through cargo-test-nonzero.sh with the operator-facing environment replaced:
# XDG_RUNTIME_DIR (bridge discovery), HKASK_DATA_DIR (gallery.db, voice_profiles.db, faces, models),
# HKASK_ARTIFACTS_DIR and TMPDIR all point into one temp directory; the curator/regulation variables are unset.
# HOME stays (cargo and rustup need it). Usage (repo root): run-tests.sh <log> <cargo-test args...>
set -uo pipefail
log="$1"; shift
tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/data" "$tmp/artifacts" "$tmp/scratch"
env -u HKASK_CURATOR_DB -u HKASK_CURATOR_WEBID -u HKASK_REG_CONFIG -u HKASK_INFERENCE_SOCKET -u HKASK_MEDIA_DB \
  -u OPENROUTER_API_KEY -u DEEPINFRA_API_KEY -u HKASK_SERPAPI_API_KEY \
  XDG_RUNTIME_DIR="$tmp" HKASK_DATA_DIR="$tmp/data" HKASK_ARTIFACTS_DIR="$tmp/artifacts" TMPDIR="$tmp/scratch" \
  bash kask/scripts/cargo-test-nonzero.sh "$@" > "$log" 2>&1
rc=$?
echo "exit $rc"
grep -E '^test result:|^error(\[|:)|FAILED|panicked at|failures:$' "$log" | cut -c1-200 | head -40
echo "socket-path file in temp runtime dir: $([ -e "$tmp/kask/inference-socket-path" ] && echo YES || echo no); files left in temp data dir: $(find "$tmp/data" -type f | wc -l)"
exit $rc
