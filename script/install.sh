#!/usr/bin/env bash
# Fork note: Zed-Kask is built from source. Delegate to the fork's installer at
# kask/scripts/build/install.sh, which installs only the zed-kask command,
# MCP server commands, zed-kask settings, and zed-kask icons.
#
# The upstream install.sh downloaded a tarball from cloud.zed.dev and installed
# upstream Zed under dev.zed.* app IDs — wrong for this fork.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/mdz-axo/zed-kask/main/script/install.sh | bash
#   bash script/install.sh [OPTIONS]
#
# Options are forwarded to kask/scripts/build/install.sh. See that script for
# the full list (--debug, --system, --skip-deps, --uninstall, etc.).

set -eu

# Resolve the repo root from the script's own location so this works whether
# invoked from a checkout or piped via curl (in which case BASH_SOURCE is empty
# and the fork's installer handles the clone itself).
repo_root=""
if [ -n "${BASH_SOURCE:-}" ] && [ -f "$(dirname "${BASH_SOURCE[0]}")/../kask/scripts/build/install.sh" ]; then
    repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
elif [ -f "$(dirname "$0")/../kask/scripts/build/install.sh" ]; then
    repo_root="$(cd "$(dirname "$0")/.." && pwd)"
fi

if [ -n "$repo_root" ] && [ -x "$repo_root/kask/scripts/build/install.sh" ]; then
    exec "$repo_root/kask/scripts/build/install.sh" "$@"
fi

# Piped-via-curl path: no local checkout, so clone and run.
echo "No local checkout detected. Cloning zed-kask and running the installer..."
temp="$(mktemp -d)"
trap 'rm -rf "$temp"' EXIT
git clone --depth 1 https://github.com/mdz-axo/zed-kask.git "$temp"
exec "$temp/kask/scripts/build/install.sh" "$@"
