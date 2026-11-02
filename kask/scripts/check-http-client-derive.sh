#!/usr/bin/env bash
# CI gate (DIVERGENCE.md D99): http_client's serde derives must be
# self-declared in its manifest.
#
# The defect (the D80 class, upstream-caused; found at the 2026-11-02 D83
# audit): crates/http_client/Cargo.toml declared bare
# `serde.workspace = true` while src/github.rs uses `#[derive(Deserialize)]`
# — the derive macro is built only when some OTHER crate in the graph
# requests serde's derive feature, so every cargo scope whose graph lacks
# one failed to compile (standalone `cargo check -p http_client` and
# `-p bedrock` were red at HEAD; `--all-features` does not help — the
# feature must come from the graph, and bedrock's has none). Unified
# workspace builds never saw it.
#
# The fix (D99, one line): `serde = { workspace = true, features =
# ["derive"] }` — the same per-crate feature declaration pattern as
# D80's eval_cli pairings. An upstream rebase overwriting this manifest
# reintroduces the red standalone scopes silently; this check fails CI
# first.
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
manifest="${HTTP_CLIENT_MANIFEST:-$root/crates/http_client/Cargo.toml}"

if [[ ! -f "$manifest" ]]; then
    echo "FAIL: http_client manifest not found at $manifest" >&2
    exit 1
fi

if grep -qE '^serde = \{ workspace = true, features = \["derive"\] \}$' "$manifest"; then
    echo "OK: http_client self-declares serde's derive feature (D99)."
else
    echo "FAIL: $manifest must declare" >&2
    echo '  serde = { workspace = true, features = ["derive"] }' >&2
    echo "Without it, http_client's #[derive(Deserialize)] depends on cross-crate feature" >&2
    echo "unification and standalone cargo scopes (-p http_client, -p bedrock) fail to" >&2
    echo "compile (D99, the D80 class)." >&2
    exit 1
fi