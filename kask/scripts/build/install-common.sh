#!/bin/bash
# Shared helpers for zed-kask install scripts.
#
# Sourced by install.sh and install-binary.sh. Provides:
#   - log/log_success/log_warning/log_error
#   - MCP_SERVERS array (loaded from mcp-servers.txt — single source of truth)
#   - add_to_path (symlink-then-shell-config strategy)
#   - print_banner
#
# The runtime canonical registry is BUILT_IN_MCP_SERVERS in
# kask/crates/kask_bridge/src/mcp_servers.rs; mcp-servers.txt is the
# build/CI surface, verified by kask/scripts/check-mcp-servers.sh.

# Avoid double-sourcing.
if [ -n "${HKASK_INSTALL_COMMON_LOADED:-}" ]; then
    return 0 2>/dev/null
fi
HKASK_INSTALL_COMMON_LOADED=1

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

log()         { echo -e "${BLUE}[INFO]${NC} $1"; }
log_success() { echo -e "${GREEN}[SUCCESS]${NC} $1"; }
log_warning() { echo -e "${YELLOW}[WARNING]${NC} $1"; }
log_error()   { echo -e "${RED}[ERROR]${NC} $1" >&2; }

# Load MCP server binary names from the canonical list file.
# Resolves the list path relative to this script (kask/scripts/build/).
_HKASK_COMMON_DIR="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" && pwd)"
MCP_SERVERS_LIST_FILE="${MCP_SERVERS_LIST_FILE:-$_HKASK_COMMON_DIR/mcp-servers.txt}"

if [ ! -f "$MCP_SERVERS_LIST_FILE" ]; then
    log_error "MCP server list not found: $MCP_SERVERS_LIST_FILE"
    exit 1
fi

mapfile -t MCP_SERVERS < <(grep -vE '^\s*#|^\s*$' "$MCP_SERVERS_LIST_FILE" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')

if [ "${#MCP_SERVERS[@]}" -eq 0 ]; then
    log_error "MCP server list is empty: $MCP_SERVERS_LIST_FILE"
    exit 1
fi

# System bin path for optional symlink.
SYSTEM_BIN="/usr/local/bin"

# add_to_path — make BIN_DIR reachable from the user's shell.
#
# Strategy 1: symlink $SYSTEM_BIN/zed-kask → $BIN_DIR/zed-kask (already in PATH
# on all Linux/macOS). Tries without sudo first, then with sudo.
#
# Strategy 2: append `export PATH="$BIN_DIR:$PATH"` to the user's shell config
# files (.profile + shell-specific rc). Idempotent — skips if the marker is
# already present.
#
# Args: expects BIN_DIR to be set by the caller.
add_to_path() {
    if [ -z "${BIN_DIR:-}" ]; then
        log_error "add_to_path: BIN_DIR is not set"
        return 1
    fi

    # Strategy 1: symlink into /usr/local/bin.
    local made_symlink=false
    if [ "${HKASK_SYSTEM_INSTALL:-false}" = "true" ] || [ -w "$SYSTEM_BIN" ]; then
        if ln -sf "$BIN_DIR/zed-kask" "$SYSTEM_BIN/zed-kask" 2>/dev/null; then
            log_success "zed-kask linked into $SYSTEM_BIN (system PATH)"
            made_symlink=true
        fi
    fi
    if [ "$made_symlink" = false ] && command -v sudo >/dev/null 2>&1; then
        if sudo ln -sf "$BIN_DIR/zed-kask" "$SYSTEM_BIN/zed-kask" 2>/dev/null; then
            log_success "zed-kask linked into $SYSTEM_BIN (system PATH, via sudo)"
            made_symlink=true
        fi
    fi
    if [ "$made_symlink" = false ]; then
        log "No write access to $SYSTEM_BIN — configuring PATH in shell config"
    fi

    # Strategy 2: add BIN_DIR to PATH via shell config files.
    # Detect the user's login shell from $SHELL (set by login(1)).
    local user_shell
    user_shell=$(basename "${SHELL:-/bin/bash}")

    # .profile is sourced by bash/zsh/sh login shells (ssh, systemd, tty login).
    local configs=("$HOME/.profile")
    case "$user_shell" in
        zsh)
            configs+=("$HOME/.zshrc" "$HOME/.zprofile")
            ;;
        bash|sh)
            configs+=("$HOME/.bashrc")
            ;;
        *)
            # Unknown shell — add bashrc as best-effort fallback.
            configs+=("$HOME/.bashrc")
            ;;
    esac

    # Check if BIN_DIR is already on PATH for this process.
    local needs_local_path=false
    if [[ ":$PATH:" != *":$BIN_DIR:"* ]]; then
        needs_local_path=true
    fi

    local added=false
    if [ "$needs_local_path" = true ]; then
        for cfg in "${configs[@]}"; do
            if ! grep -qF '# zed-kask' "$cfg" 2>/dev/null; then
                {
                    echo ""
                    echo "# zed-kask"
                    echo "export PATH=\"$BIN_DIR:\$PATH\""
                } >> "$cfg"
                log "Added PATH entry to $cfg"
                added=true
            fi
        done
    fi

    if [ "$added" = true ]; then
        log_success "PATH configured in shell profile(s)"
        log "Restart your shell or run: source ~/.profile"
    elif [ "$made_symlink" = false ] && [ "$needs_local_path" = true ]; then
        log_warning "Could not add $BIN_DIR to PATH automatically"
        log "Please add this line to your shell config:"
        log "  export PATH=\"$BIN_DIR:\$PATH\""
    fi
}

# print_banner — standard installer header.
# Args: $1 = subtitle line.
print_banner() {
    local subtitle="${1:-zed-kask Installer}"
    echo ""
    echo "╔══════════════════════════════════════════════════════════╗"
    echo "║                  zed-kask Installer                      ║"
    echo "║   Zed × Kask — integrating Zed with the Kask agentic AI  ║"
    echo "║        $subtitle"
    echo "╚══════════════════════════════════════════════════════════╝"
    echo ""
}
