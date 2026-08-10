#!/bin/bash
# Runs the installer bundle that was installed alongside zed-kask.
# The prefix is derived from this script's installed location, never from PATH.

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
install_dir="$(cd "$script_dir/../../.." && pwd)"

if [ ! -x "$script_dir/install-binary.sh" ]; then
    echo "zed-kask updater is incomplete: $script_dir/install-binary.sh is missing or not executable" >&2
    exit 1
fi

if command -v flock >/dev/null 2>&1; then
    exec 9>"$install_dir/.zed-kask-update.lock"
    if ! flock -n 9; then
        echo "A zed-kask update is already running." >&2
        exit 1
    fi
fi

export INSTALL_DIR="$install_dir"
exec "$script_dir/install-binary.sh" "$@"
