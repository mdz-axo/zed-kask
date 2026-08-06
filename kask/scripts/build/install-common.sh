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

# jq is required for JSONC settings.json parsing/merging. The previous python3
# fallback was removed (no Python shipped with the installer). Fail loudly here
# so a missing jq is caught before any settings write is attempted.
if ! command -v jq >/dev/null 2>&1; then
    log_error "jq is required but not found on PATH. Install jq (e.g. 'apt install jq' or 'brew install jq') and re-run."
    exit 1
fi

# System bin path for optional symlink.
SYSTEM_BIN="/usr/local/bin"

# strip_jsonc_comments — strip JSONC comments from a file and emit clean JSON
# on stdout.
#
# Zed writes settings.json as JSONC (// line comments and /* */ block comments).
# jq is strict JSON, so comments must be removed before jq parses the file.
#
# Approach: a single awk pass that walks the input line-by-line, tracking
# whether the current position is inside a double-quoted string, and strips
# // line comments and /* */ block comments only when outside a string. This
# preserves // inside string values (e.g. "https://example.com"). Escaped
# quotes (\\") and escaped backslashes (\\\\) are handled so a string-internal
# escaped quote does not flip the string-state tracker.
#
# Mirrors the behavior of the deleted kask/scripts/build/jsonc_load.py
# _strip_comments helper (string-aware, block comments spanning newlines are
# replaced with a newline so downstream line numbers stay meaningful). Does
# NOT strip trailing commas — jq 1.6+ rejects them, but Zed's settings.json
# files in this tree do not use trailing commas; if that changes, add a second
# sed pass for ,] and ,} outside strings.
#
# Args: $1 = path to the JSONC file. Emits stripped JSON to stdout.
strip_jsonc_comments() {
    local file="$1"
    if [ ! -f "$file" ]; then
        log_error "strip_jsonc_comments: file not found: $file"
        return 1
    fi
    awk '
        BEGIN { in_string = 0; out = "" }
        {
            line = $0
            n = length(line)
            i = 1
            while (i <= n) {
                ch = substr(line, i, 1)
                if (in_string) {
                    out = out ch
                    if (ch == "\\" && i < n) {
                        # Preserve the escaped character verbatim (handles \", \\, etc.).
                        out = out substr(line, i + 1, 1)
                        i += 2
                        continue
                    }
                    if (ch == "\"") in_string = 0
                    i++
                    continue
                }
                # Not in a string.
                if (ch == "\"") {
                    in_string = 1
                    out = out ch
                    i++
                    continue
                }
                if (ch == "/" && i < n) {
                    nxt = substr(line, i + 1, 1)
                    if (nxt == "/") {
                        # Line comment: skip rest of line (do not consume the newline).
                        i = n + 1
                        continue
                    }
                    if (nxt == "*") {
                        # Block comment: skip to */. May span multiple lines.
                        # Find closing */ on this line first.
                        rest = substr(line, i + 2)
                        pos = index(rest, "*/")
                        if (pos > 0) {
                            # Single-line block comment.
                            i = i + 2 + pos + 1
                            continue
                        } else {
                            # Multi-line block comment: consume lines until */ found.
                            i = n + 1
                            in_block = 1
                            while (in_block) {
                                if ((getline line) <= 0) {
                                    # EOF inside block comment — unterminated.
                                    print "strip_jsonc_comments: unterminated block comment" > "/dev/stderr"
                                    exit 1
                                }
                                n = length(line)
                                pos = index(line, "*/")
                                if (pos > 0) {
                                    # Resume after */ on this line.
                                    i = pos + 2
                                    in_block = 0
                                    break
                                }
                                # Whole line consumed by block comment.
                                line = ""
                                n = 0
                                i = 1
                            }
                            # Emit a newline so downstream line numbers stay sane
                            # (matches jsonc_load.py behavior for multi-line blocks).
                            out = out "\n"
                            continue
                        }
                    }
                }
                out = out ch
                i++
            }
            print out
            out = ""
        }
    ' "$file"
}

# prepare_install_dir — remove stale zed-kask / hkask-mcp-* binaries from
# BIN_DIR before installing fresh ones.
#
# cp would overwrite the current binaries, but stale copies of servers that
# were renamed or removed between releases (and the CLI itself) would linger
# in BIN_DIR otherwise. kask owns the hkask-mcp-* namespace in BIN_DIR, so
# removing every match is safe — there are no user-owned files under that
# prefix.
#
# Idempotent: a fresh install dir produces a no-op with a single log line.
# Args: expects BIN_DIR to be set by the caller.
prepare_install_dir() {
    if [ -z "${BIN_DIR:-}" ]; then
        log_error "prepare_install_dir: BIN_DIR is not set"
        return 1
    fi

    local removed=0
    if [ -f "$BIN_DIR/zed-kask" ]; then
        rm -f "$BIN_DIR/zed-kask"
        log "Removed previous zed-kask binary"
        removed=$((removed + 1))
    fi

    # Glob + guard instead of find: portable across GNU/BSD find and safe
    # when BIN_DIR does not exist yet (fresh install).
    local stale
    for stale in "$BIN_DIR"/hkask-mcp-*; do
        [ -f "$stale" ] || continue
        rm -f "$stale"
        log "Removed stale MCP server binary: $(basename "$stale")"
        removed=$((removed + 1))
    done

    if [ "$removed" -eq 0 ]; then
        log "No previous binaries found in $BIN_DIR"
    else
        log_success "Removed $removed stale binary(ies) from $BIN_DIR"
    fi
}

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

# write_mcp_server_settings — write context_servers entries into the
# zed-kask user settings.json so the 10 built-in MCP servers appear in
# Settings → AI → MCP Servers with absolute command paths.
#
# This is the canonical Zed way to register MCP servers: a `context_servers`
# map in settings.json where each entry has `command` (absolute path), `args`,
# and optional `env`. Without these entries, the servers are registered via
# the ContextServerDescriptorRegistry (KaskMcpDescriptor) which passes a bare
# binary name to std::process::Command — that relies on PATH, which GUI-launched
# apps don't inherit from shell configs. Writing explicit JSON entries with
# absolute paths bypasses the PATH problem entirely.
#
# The child process inherits the zed-kask process env (which loads
# ~/.config/zed-kask/.env at startup for API keys), so `env` is left empty —
# the servers read DEEPINFRA_API_KEY, HKASK_* etc. from their inherited env.
#
# Idempotent: re-running updates the command paths without duplicating entries.
# Preserves any user-added context_servers entries that aren't kask built-ins.
#
# Args: expects BIN_DIR and MCP_SERVERS to be set by the caller.
write_mcp_server_settings() {
    if [ -z "${BIN_DIR:-}" ]; then
        log_error "write_mcp_server_settings: BIN_DIR is not set"
        return 1
    fi
    if [ "${#MCP_SERVERS[@]}" -eq 0 ]; then
        log_error "write_mcp_server_settings: MCP_SERVERS is empty"
        return 1
    fi

    # Resolve the zed-kask config directory (matches paths::config_dir).
    # Linux: $XDG_CONFIG_HOME/zed-kask  (default ~/.config/zed-kask)
    # macOS: ~/Library/Application Support/Zed-Kask
    # The .desktop entry and settings.json live here.
    local config_dir
    if [ -n "${XDG_CONFIG_HOME:-}" ]; then
        config_dir="$XDG_CONFIG_HOME/zed-kask"
    elif [ -n "${HOME:-}" ]; then
        config_dir="$HOME/.config/zed-kask"
    else
        log_error "write_mcp_server_settings: cannot determine config dir (no XDG_CONFIG_HOME or HOME)"
        return 1
    fi

    mkdir -p "$config_dir"
    local settings_file="$config_dir/settings.json"

    # Build the kask context_servers JSON object from MCP_SERVERS.
    # Each entry: "<id>": {"command": "<abs path>", "args": [], "env": {}}
    # The binary name is the package name (e.g. hkask-mcp-codegraph); the
    # server ID is derived by stripping the hkask-mcp- prefix.
    local kask_servers_json='{}'
    local server_id
    for server in "${MCP_SERVERS[@]}"; do
        # Derive the context_servers key from the binary name.
        # hkask-mcp-codegraph → codegraph, hkask-mcp-kata-kanban → kata-kanban
        server_id="${server#hkask-mcp-}"
        local binary_path="$BIN_DIR/$server"
        if [ ! -x "$binary_path" ]; then
            log_warning "MCP server binary not found, skipping settings entry: $binary_path"
            continue
        fi
        # Use jq to build the JSON entry safely (no string interpolation into
        # JSON — avoids quoting bugs). jq is required (checked at source time).
        kask_servers_json=$(jq --arg id "$server_id" --arg path "$binary_path" \
            '. + {($id): {"command": $path, "args": [], "env": {}}}' <<< "$kask_servers_json")
    done

    # Merge kask_servers_json into the existing settings.json's context_servers.
    # - If settings.json doesn't exist, create it with the context_servers block.
    # - If it exists but has no context_servers, add the block.
    # - If it exists with context_servers, merge: overwrite kask server entries
    #   (by id) but preserve any non-kask entries the user added.
    if [ ! -f "$settings_file" ]; then
        # Create a new settings.json with the context_servers block.
        jq -n --argjson servers "$kask_servers_json" '{context_servers: $servers}' > "$settings_file"
        log "Created $settings_file with kask MCP server entries"
    else
        # Merge into existing settings.json. Preserve everything except
        # overwrite kask server entries under context_servers.
        # jq is strict JSON, so strip JSONC comments (// line and /* */ block)
        # first — Zed writes settings.json as JSONC. strip_jsonc_comments is
        # string-aware so // inside string values (e.g. URLs) is preserved.
        local stripped
        stripped=$(strip_jsonc_comments "$settings_file") || return 1
        local tmp
        tmp=$(mktemp)
        jq --argjson kask "$kask_servers_json" \
            '.context_servers = ((.context_servers // {}) + $kask)' \
            <<< "$stripped" > "$tmp" && mv "$tmp" "$settings_file"
        log "Updated $settings_file with kask MCP server entries"
    fi

    log_success "Wrote ${#MCP_SERVERS[@]} kask MCP server entries to settings.json"
}

# remove_mcp_server_settings — remove kask-managed context_servers entries
# from settings.json. Preserves user-added (non-kask) entries.
#
# Identifies kask-managed entries by checking if the command path points at
# a binary named hkask-mcp-* (the kask naming convention). This is safe even
# if BIN_DIR changed between install and uninstall.
remove_mcp_server_settings() {
    local config_dir
    if [ -n "${XDG_CONFIG_HOME:-}" ]; then
        config_dir="$XDG_CONFIG_HOME/zed-kask"
    elif [ -n "${HOME:-}" ]; then
        config_dir="$HOME/.config/zed-kask"
    else
        return 0
    fi
    local settings_file="$config_dir/settings.json"
    if [ ! -f "$settings_file" ]; then
        return 0
    fi

    # Remove entries whose command basename matches hkask-mcp-*.
    # jq is strict JSON, so strip JSONC comments first (Zed writes settings.json
    # as JSONC). jq is required (checked at source time).
    local stripped
    if ! stripped=$(strip_jsonc_comments "$settings_file"); then
        log_warning "Could not parse $settings_file; leaving kask MCP entries in place"
        return 0
    fi
    local tmp
    tmp=$(mktemp)
    if jq '.context_servers |= with_entries(select(.value.command | split("/") | last | startswith("hkask-mcp-") | not))' \
            <<< "$stripped" > "$tmp" 2>/dev/null; then
        mv "$tmp" "$settings_file"
        log "Cleaned kask MCP entries from $settings_file"
    else
        rm -f "$tmp"
        log_warning "Could not clean kask MCP entries from $settings_file (jq parse failed)"
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
