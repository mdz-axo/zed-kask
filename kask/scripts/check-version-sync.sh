#!/usr/bin/env bash
# CI gate: assert kask registry manifests, MCP-server provenance, README
# version lines, and the canonical version banners stay in sync with the
# zed-kask workspace release version in the root Cargo.toml
# `[workspace.package] version`.
#
# Drift surface this guards:
#   - kask/registry/manifests/*.yaml `version:` fields (96 manifests — every
#     release bump must move all of them; a missed manifest silently ships a
#     stale manifest version).
#   - `# [ℏh]Kask v<x>` brand-header comments (when a manifest uses that
#     convention) and `# Manifest version: <x>.` trailing comments.
#   - Hardcoded `"version": "0.x.y"` provenance literals in MCP server .rs
#     files — these must report `env!("CARGO_PKG_VERSION")` (which inherits the
#     workspace version), not a literal that rots between releases.
#   - README `**Version:**` lines across kask/ (crate READMEs, pipeline
#     READMEs) — human-facing current-version markers that drift on missed bumps.
#
# Usage: cd kask && bash scripts/check-version-sync.sh
# Exit codes: 0 = in sync, 1 = drift detected

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
NC='\033[0m'

# kask/ workspace (script lives at kask/scripts/).
# Each path is overridable via env var so the self-test can point at a temp
# tree; the defaults preserve the production behavior exactly.
KASK_ROOT="${KASK_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
# zed-kask repo root (parent of kask/).
REPO_ROOT="${REPO_ROOT:-$(cd "$KASK_ROOT/.." && pwd)}"
CARGO_TOML="${CARGO_TOML:-$REPO_ROOT/Cargo.toml}"
MANIFEST_DIR="${MANIFEST_DIR:-$KASK_ROOT/registry/manifests}"
MCP_SERVERS_DIR="${MCP_SERVERS_DIR:-$KASK_ROOT/mcp-servers}"

if [ ! -f "$CARGO_TOML" ]; then
    echo -e "${RED}[ERROR]${NC} root Cargo.toml not found: $CARGO_TOML"
    exit 1
fi

# Extract the [workspace.package] version. It is the first `version = "..."`
# line after the `[workspace.package]` header.
WORKSPACE_VERSION=$(awk '
    /^\[workspace\.package\]/ { in_pkg=1; next }
    /^\[/ { in_pkg=0 }
    in_pkg && /^version[[:space:]]*=/ {
        line=$0
        sub(/^version[[:space:]]*=[[:space:]]*"/, "", line)
        sub(/"[[:space:]]*$/, "", line)
        print line
        exit
    }
' "$CARGO_TOML")

if [ -z "$WORKSPACE_VERSION" ]; then
    echo -e "${RED}[ERROR]${NC} could not parse [workspace.package] version from $CARGO_TOML"
    exit 1
fi

echo "Workspace release version: $WORKSPACE_VERSION"

status=0
fail() {
    echo -e "${RED}[DRIFT]${NC} $1"
    status=1
}

# 1. Manifest brand-header comments: when a manifest uses the
#    `# [ℏh]Kask v<x>` convention, its version must match the workspace
#    version. Manifests that use a different header style are skipped here
#    (their `version:` field, checked next, is the authoritative version).
while IFS= read -r manifest; do
    header_ver=$(grep -oE '^# [ℏh]Kask v[0-9]+\.[0-9]+\.[0-9]+' "$manifest" \
        | head -1 | sed -E 's/^.*v([0-9]+\.[0-9]+\.[0-9]+)$/\1/' || true)
    if [ -n "$header_ver" ] && [ "$header_ver" != "$WORKSPACE_VERSION" ]; then
        fail "$manifest: brand header is \`# [ℏh]Kask v${header_ver}\`, expected v${WORKSPACE_VERSION}"
    fi
done < <(find "$MANIFEST_DIR" -maxdepth 1 -name '*.yaml' -type f)

# 2. Manifest `version:` field: every manifest must carry the workspace
#    version as its manifest version (2-space indented, quoted or unquoted).
while IFS= read -r manifest; do
    if ! grep -qE "^  version: \"?${WORKSPACE_VERSION//./\\.}\"?([[:space:]]|$|#)" "$manifest"; then
        fail "$manifest: manifest \`version:\` field is not \`$WORKSPACE_VERSION\`"
    fi
done < <(find "$MANIFEST_DIR" -maxdepth 1 -name '*.yaml' -type f)

# 3. `# Manifest version: <x>.` trailing comments must match.
while IFS= read -r line_file; do
    fail "$line_file: \`# Manifest version:\` comment is not \`$WORKSPACE_VERSION\`"
done < <(grep -rnE '# Manifest version: [0-9]+\.[0-9]+\.[0-9]+\.' "$MANIFEST_DIR" \
    | grep -vE "# Manifest version: ${WORKSPACE_VERSION//./\\.}\.")

# 4. MCP server .rs files must not hardcode a numeric provenance version
#    literal. Reporting the crate version requires `env!("CARGO_PKG_VERSION")`,
#    which inherits the workspace version; a literal rots silently between
#    releases (the hkask-mcp-scenarios provenance drift this fixes).
while IFS= read -r hit; do
    fail "$hit: hardcoded provenance version literal — use env!(\"CARGO_PKG_VERSION\")"
done < <(grep -rnE '"version":[[:space:]]*"[0-9]+\.[0-9]+\.[0-9]+"' "$MCP_SERVERS_DIR" || true)

# 5. README `**Version:**` lines across kask/ must match the workspace version.
#    These are human-facing current-version markers (crate READMEs, pipeline
#    READMEs) that drift silently when a release bump misses them.
while IFS= read -r hit; do
    fail "$hit: README \`**Version:**\` is not \`$WORKSPACE_VERSION\`"
done < <(grep -rnE '\*\*Version:\*\*[[:space:]]*v?0\.[0-9]+\.[0-9]+' "$KASK_ROOT" \
    | grep -vE "\*\*Version:\*\*[[:space:]]*v?${WORKSPACE_VERSION//./\\.}([[:space:]]|$)")

if [ "$status" -eq 0 ]; then
    echo -e "${GREEN}[OK]${NC} all manifests, banners, READMEs, and MCP provenance in sync with v${WORKSPACE_VERSION}"
else
    echo ""
    echo "Fix: bump the drifted fields to $WORKSPACE_VERSION (workspace version in $CARGO_TOML)."
    echo "     For MCP provenance literals, replace the string with the crate's SERVER_VERSION const (env!(\"CARGO_PKG_VERSION\"))."
fi
exit $status