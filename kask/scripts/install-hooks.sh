#!/usr/bin/env bash
# Install the kask git hooks into the local clone's .git/hooks directory.
#
# Git does not track .git/hooks, so each clone must opt in. This script
# symlinks kask/scripts/hooks/* into .git/hooks/ so hook updates are picked
# up automatically on future pulls (no need to re-run this after hook edits).
#
# Usage: bash kask/scripts/install-hooks.sh
# Remove: bash kask/scripts/install-hooks.sh --uninstall

set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null)" || {
  echo "install-hooks: not inside a git repository" >&2
  exit 1
}
HOOKS_DIR="$REPO_ROOT/.git/hooks"
SRC_DIR="$REPO_ROOT/kask/scripts/hooks"

if [ ! -d "$SRC_DIR" ]; then
  echo "install-hooks: no hooks to install at $SRC_DIR" >&2
  exit 1
fi

mkdir -p "$HOOKS_DIR"

UNINSTALL=false
if [ "${1:-}" = "--uninstall" ]; then
  UNINSTALL=true
fi

installed=0
for src_hook in "$SRC_DIR"/*; do
  [ -f "$src_hook" ] || continue
  hook_name="$(basename "$src_hook")"
  dest="$HOOKS_DIR/$hook_name"

  if $UNINSTALL; then
    if [ -L "$dest" ] && [ "$(readlink -f "$dest")" = "$(readlink -f "$src_hook")" ]; then
      rm "$dest"
      echo "  removed: $hook_name"
    fi
    continue
  fi

  # If a real (non-symlink) hook exists, back it up rather than clobbering.
  if [ -e "$dest" ] && [ ! -L "$dest" ]; then
    mv "$dest" "$dest.bak.$(date +%s)"
    echo "  backed up existing $hook_name -> ${dest##*/}.bak.*"
  fi

  ln -sf "$src_hook" "$dest"
  chmod +x "$src_hook"
  echo "  installed: $hook_name -> kask/scripts/hooks/$hook_name"
  installed=$((installed + 1))
done

if $UNINSTALL; then
  echo "install-hooks: uninstalled."
else
  echo "install-hooks: $installed hook(s) symlinked. Bypass with 'git push --no-verify'."
fi