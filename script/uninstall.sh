#!/usr/bin/env bash
# Fork note: Zed-Kask is uninstalled via kask/scripts/build/install.sh --uninstall,
# which removes binaries, the desktop entry, the icon, and the URL-scheme handler.
#
# The upstream uninstall.sh targeted upstream Zed's bundle layout under dev.zed.*
# app IDs — wrong for this fork.

set -eu

repo_root=""
if [ -n "${BASH_SOURCE:-}" ] && [ -f "$(dirname "${BASH_SOURCE[0]}")/../kask/scripts/build/install.sh" ]; then
    repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
elif [ -f "$(dirname "$0")/../kask/scripts/build/install.sh" ]; then
    repo_root="$(cd "$(dirname "$0")/.." && pwd)"
fi

if [ -n "$repo_root" ] && [ -x "$repo_root/kask/scripts/build/install.sh" ]; then
    exec "$repo_root/kask/scripts/build/install.sh" --uninstall "$@"
fi

echo "No local checkout detected; cannot uninstall without the fork's installer." >&2
echo "Clone the repo and run: kask/scripts/build/install.sh --uninstall" >&2
exit 1
