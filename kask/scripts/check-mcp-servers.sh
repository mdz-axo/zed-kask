#!/usr/bin/env bash
# CI gate: assert kask/scripts/build/mcp-servers.txt matches the runtime
# canonical registry BUILT_IN_MCP_SERVERS in
# kask/crates/kask_bridge/src/mcp_servers.rs, and that both match the set of
# server crates actually on disk under kask/mcp-servers/.
#
# Drift between the install/release surface and the runtime registry causes
# partial installs (missing MCP server binaries) with no compile-time error.
# This check fails CI before such drift can ship.
#
# The disk leg closes a scope gap: comparing the list against the registry
# proves those two agree with each other, but says nothing about the tree. A
# server crate added under kask/mcp-servers/ and wired into neither — or, more
# realistically, a server REMOVED from both while its directory is left behind
# as residue — passed silently. Three sets must agree, not two.
#
# Usage: cd kask && bash scripts/check-mcp-servers.sh
# Exit codes: 0 = match, 1 = drift detected

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
NC='\033[0m'

# Resolve paths relative to the kask/ workspace (script lives at kask/scripts/).
# Each path is overridable via env var so the self-test can point at a temp
# tree; the defaults preserve the production behavior exactly.
KASK_ROOT="${KASK_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
LIST_FILE="${LIST_FILE:-$KASK_ROOT/scripts/build/mcp-servers.txt}"
REGISTRY_FILE="${REGISTRY_FILE:-$KASK_ROOT/crates/kask_bridge/src/mcp_servers.rs}"
SERVERS_DIR="${SERVERS_DIR:-$KASK_ROOT/mcp-servers}"
AST_GREP="$KASK_ROOT/scripts/ast-grep.sh"
REGISTRY_RULE="$KASK_ROOT/ast-grep/rules/builtin-mcp-server-entry.yml"

command -v jq >/dev/null 2>&1 || {
    echo -e "${RED}[ERROR]${NC} jq is required to read parser output."
    exit 1
}
command -v cargo >/dev/null 2>&1 || {
    echo -e "${RED}[ERROR]${NC} cargo is required to resolve server package identities."
    exit 1
}
[ -x "$AST_GREP" ] || {
    echo -e "${RED}[ERROR]${NC} Structural parser not executable: $AST_GREP"
    exit 1
}
[ -f "$REGISTRY_RULE" ] || {
    echo -e "${RED}[ERROR]${NC} Registry extraction rule not found: $REGISTRY_RULE"
    exit 1
}

if [ ! -f "$LIST_FILE" ]; then
    echo -e "${RED}[ERROR]${NC} MCP server list not found: $LIST_FILE"
    exit 1
fi
if [ ! -f "$REGISTRY_FILE" ]; then
    echo -e "${RED}[ERROR]${NC} Runtime registry not found: $REGISTRY_FILE"
    exit 1
fi

# Extract binary names from mcp-servers.txt (skip comments and blank lines).
# Sort for stable comparison, but do NOT deduplicate: multiplicity is part of
# the inventory contract. Two identical declarations are still two entries.
list_names=$(grep -vE '^[[:space:]]*#|^[[:space:]]*$' "$LIST_FILE" \
    | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//' \
    | sort || true)

# Extract binary names from concrete BuiltinMcpServer struct literals. The
# Rust AST excludes comment/string decoys that a textual `binary:` grep reads
# as phantom registrations. Preserve duplicates: multiplicity is checked.
registry_json=$(mktemp)
registry_errors=$(mktemp)
trap 'rm -f "$registry_json" "$registry_errors"' EXIT
set +e
"$AST_GREP" scan --rule "$REGISTRY_RULE" --json=stream "$REGISTRY_FILE" \
    >"$registry_json" 2>"$registry_errors"
registry_status=$?
set -e
if [ "$registry_status" -ne 0 ]; then
    echo -e "${RED}[ERROR]${NC} Could not parse runtime registry: $REGISTRY_FILE"
    cat "$registry_errors" >&2
    exit 1
fi
if ! registry_literals=$(jq -r '.metaVariables.single.BINARY.text' "$registry_json"); then
    echo -e "${RED}[ERROR]${NC} Could not read runtime registry parser output."
    exit 1
fi
registry_names=""
while IFS= read -r literal; do
    [ -n "$literal" ] || continue
    if [[ "$literal" =~ ^\"(hkask-mcp-[a-z0-9_-]+)\"$ ]]; then
        registry_names+="${BASH_REMATCH[1]}"$'\n'
    else
        echo -e "${RED}[ERROR]${NC} Invalid MCP server binary in $REGISTRY_FILE: $literal"
        echo "  Every canonical entry must use an hkask-mcp-* string literal."
        exit 1
    fi
done <<< "$registry_literals"
registry_names=$(printf '%s' "$registry_names" | sort)

if [ -z "$list_names" ] || [ -z "$registry_names" ]; then
    echo -e "${RED}[ERROR]${NC} One or both lists are empty."
    echo "  mcp-servers.txt:"
    sed 's/^/    /' <<< "$list_names"
    echo "  BUILT_IN_MCP_SERVERS:"
    sed 's/^/    /' <<< "$registry_names"
    exit 1
fi

# Reject duplicate declarations before comparing the three inventories. A
# set-only comparison cannot distinguish one registration from two.
list_duplicates=$(printf '%s\n' "$list_names" | uniq -d)
registry_duplicates=$(printf '%s\n' "$registry_names" | uniq -d)
if [ -n "$list_duplicates" ] || [ -n "$registry_duplicates" ]; then
    echo -e "${RED}[ERROR]${NC} Duplicate MCP server declarations found."
    if [ -n "$list_duplicates" ]; then
        echo "  duplicated in $LIST_FILE:"
        sed 's/^/    /' <<< "$list_duplicates"
    fi
    if [ -n "$registry_duplicates" ]; then
        echo "  duplicated binary in $REGISTRY_FILE:"
        sed 's/^/    /' <<< "$registry_duplicates"
    fi
    exit 1
fi

# Diff the two inventories. `diff <(a) <(b)` returns 0 if identical.
if diff_output=$(diff <(echo "$list_names") <(echo "$registry_names")); then
    count=$(echo "$list_names" | wc -l)
    echo -e "${GREEN}[OK]${NC} mcp-servers.txt matches BUILT_IN_MCP_SERVERS ($count servers)"
else
    echo -e "${RED}[ERROR]${NC} Drift between mcp-servers.txt and BUILT_IN_MCP_SERVERS:"
    sed 's/^/    /' <<< "$diff_output"
    echo ""
    echo "Fix: edit $LIST_FILE to match BUILT_IN_MCP_SERVERS in $REGISTRY_FILE"
    exit 1
fi

# Guard: BUILTIN_SERVERS must not be re-introduced in its old location.
# The canonical registry was consolidated into kask_bridge::BUILT_IN_MCP_SERVERS
# (PR #127, "Remove duplicate MCP server registry"). A parallel list in
# hkask_mcp_server.rs would reintroduce the silent drift this check exists to
# prevent — the previous duplicate used id "kanban" while the canonical list
# uses "kata-kanban". See the "Do NOT re-introduce" note in that file.
OLD_REGISTRY_FILE="${OLD_REGISTRY_FILE:-$KASK_ROOT/crates/hkask-mcp-server/src/hkask_mcp_server.rs}"
if grep -nE '^[[:space:]]*(pub[[:space:]]+)?(const|static)[[:space:]]+BUILTIN_SERVERS\b' "$OLD_REGISTRY_FILE" >/dev/null; then
    echo -e "${RED}[ERROR]${NC} BUILTIN_SERVERS was re-introduced in $OLD_REGISTRY_FILE"
    echo "  The canonical registry is BUILT_IN_MCP_SERVERS in $REGISTRY_FILE."
    echo "  Remove the parallel list from $OLD_REGISTRY_FILE."
    exit 1
fi
echo -e "${GREEN}[OK]${NC} BUILTIN_SERVERS not re-introduced in old location"

# Third set: Cargo package identities from manifest-bearing server crates on
# disk. Directory names are not crate identity; a directory/package mismatch
# is itself drift and must not be normalized away.
if [ ! -d "$SERVERS_DIR" ]; then
    echo -e "${RED}[ERROR]${NC} Server crate directory not found: $SERVERS_DIR"
    exit 1
fi

manifest_paths=()
while IFS= read -r -d '' manifest; do
    manifest_paths+=("$manifest")
done < <(find "$SERVERS_DIR" -mindepth 2 -maxdepth 2 -name Cargo.toml -print0 2>/dev/null || true)

if [ "${#manifest_paths[@]}" -eq 0 ]; then
    echo -e "${RED}[ERROR]${NC} No manifest-bearing server crates found under $SERVERS_DIR"
    echo "  A zero-server tree is never a legitimate state for this gate; refusing to pass trivially."
    exit 1
fi

disk_names_file=$(mktemp)
trap 'rm -f "$registry_json" "$registry_errors" "$disk_names_file"' EXIT
for manifest in "${manifest_paths[@]}"; do
    manifest_abs=$(realpath "$manifest")
    metadata_file=$(mktemp)
    metadata_errors=$(mktemp)
    if ! cargo metadata --manifest-path "$manifest_abs" --format-version 1 --no-deps \
        >"$metadata_file" 2>"$metadata_errors"; then
        echo -e "${RED}[ERROR]${NC} Could not resolve server manifest: $manifest"
        cat "$metadata_errors" >&2
        rm -f "$metadata_file" "$metadata_errors"
        exit 1
    fi
    if ! package_name=$(jq -er --arg manifest "$manifest_abs" '
        [.packages[] | select(.manifest_path == $manifest)]
        | if length == 1 then .[0].name else error("expected exactly one package") end
    ' "$metadata_file"); then
        echo -e "${RED}[ERROR]${NC} Cargo metadata did not report exactly one package for $manifest"
        rm -f "$metadata_file" "$metadata_errors"
        exit 1
    fi
    directory_name=$(basename "$(dirname "$manifest_abs")")
    if [ "$package_name" != "$directory_name" ]; then
        echo -e "${RED}[ERROR]${NC} Server directory/package identity drift:"
        echo "    directory: $directory_name"
        echo "    package:   $package_name ($manifest)"
        rm -f "$metadata_file" "$metadata_errors"
        exit 1
    fi
    printf '%s\n' "$package_name" >>"$disk_names_file"
    rm -f "$metadata_file" "$metadata_errors"
done
disk_names=$(sort "$disk_names_file")

if disk_diff=$(diff <(echo "$registry_names") <(echo "$disk_names")); then
    disk_count=$(echo "$disk_names" | wc -l)
    echo -e "${GREEN}[OK]${NC} BUILT_IN_MCP_SERVERS matches the server crates on disk ($disk_count crates)"
else
    echo -e "${RED}[ERROR]${NC} Drift between BUILT_IN_MCP_SERVERS and $SERVERS_DIR:"
    echo "    (< registry only, > on disk only)"
    sed 's/^/    /' <<< "$disk_diff"
    echo ""
    echo "Fix: register the on-disk server in $REGISTRY_FILE (and $LIST_FILE),"
    echo "     or remove the unregistered crate directory under $SERVERS_DIR."
    exit 1
fi

exit 0
