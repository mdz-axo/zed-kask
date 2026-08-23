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

# Reject every destination owned by upstream Zed. This is the installer
# membrane: zed-kask may coexist with Zed, but may never write into Zed's app,
# data, config, launcher, or command paths. `readlink -m` resolves existing
# symlinks and canonicalizes nonexistent leaf paths without creating them.
assert_not_zed_owned_path() {
    local requested_path="$1"
    local operation="${2:-write}"
    if [ -z "${HOME:-}" ]; then
        log_error "$operation refused: HOME is not set"
        return 1
    fi

    local canonical_path canonical_home
    canonical_path=$(readlink -m -- "$requested_path") || return 1
    canonical_home=$(readlink -m -- "$HOME") || return 1

    case "$canonical_path" in
        "$canonical_home/.local/zed.app"|"$canonical_home/.local/zed.app"/*|\
        "$canonical_home/.local"/zed-*.app|"$canonical_home/.local"/zed-*.app/*|\
        "$canonical_home/.local/bin/zed"|\
        "$canonical_home/.local/share/zed"|"$canonical_home/.local/share/zed"/*|\
        "$canonical_home/.config/zed"|"$canonical_home/.config/zed"/*|\
        "$canonical_home/.local/share/applications/dev.zed.Zed.desktop"|\
        "$canonical_home/.local/share/applications/dev.zed.Zed-"*.desktop)
            log_error "$operation refused: destination is owned by upstream Zed: $canonical_path"
            return 1
            ;;
    esac
}

assert_kask_binary_destination() {
    local destination="$1"
    assert_not_zed_owned_path "$destination" "binary write" || return 1

    case "$(basename "$destination")" in
        zed-kask|hkask-mcp-*)
            ;;
        *)
            log_error "binary write refused: destination name is not zed-kask-owned: $destination"
            return 1
            ;;
    esac
}

# assert_not_zed_contaminated_env — refuse to build or install zed-kask when
# the environment is contaminated by the upstream Zed editor. zed-kask must be
# built and installed from a clean shell; a build run inside the upstream
# Zed's integrated terminal inherits Zed's LD_LIBRARY_PATH (pointing at its
# bundled-lib dir, e.g. the Flatpak Zed's files/lib) and produces a binary
# coupled to upstream Zed's libraries — exactly the collision zed-kask exists
# to avoid. The same contamination also makes gtk-update-icon-cache / gdbus
# silently load Zed's mismatched libs (the icon-cache refresh then fails
# silently, so the running gnome-shell never picks up the installed icon).
#
# This is the environmental counterpart of assert_not_zed_owned_path: that
# guards *paths* from colliding with upstream Zed; this guards the *build
# and install environment* from being coupled to upstream Zed. zed-kask's own
# binary is named "zed-kask" (see crates/zed/Cargo.toml [[bin]] name), so
# detecting an ancestor named "zed-editor" unambiguously identifies upstream
# Zed, never zed-kask itself.
#
# Detection: (1) LD_LIBRARY_PATH pointing at an upstream Zed bundled-lib
# dir (Flatpak Zed or an app bundle), or (2) an ancestor process is the
# upstream Zed editor (comm "zed-editor") — i.e. the script is running inside
# Zed's integrated terminal. Hard-fails so the operator runs from a clean
# shell (Ptyxis / a plain terminal / the app launcher), not upstream Zed's
# terminal.
assert_not_zed_contaminated_env() {
    local operation="${1:-build}"

    # (1) LD_LIBRARY_PATH poisoned by upstream Zed's bundled libraries.
    if [ -n "${LD_LIBRARY_PATH:-}" ]; then
        case "$LD_LIBRARY_PATH" in
            *flatpak/app/dev.zed*|*/dev.zed.Zed*/files/lib*)
                log_error "$operation refused: LD_LIBRARY_PATH is contaminated by the upstream Zed editor:"
                log_error "    $LD_LIBRARY_PATH"
                log_error "A build/install run here would couple zed-kask to upstream Zed's libraries."
                log_error "Run from a clean shell (Ptyxis / a plain terminal / the app launcher),"
                log_error "NOT from inside the upstream Zed (or Flatpak Zed) integrated terminal."
                return 1
                ;;
        esac
    fi

    # (2) Running inside the upstream Zed editor's integrated terminal. Walk
    # ancestors; an ancestor named "zed-editor" is upstream Zed. zed-kask's own
    # binary is "zed-kask", so this never false-positives on zed-kask itself.
    local pid=$$ depth=0
    while [ -n "${pid:-}" ] && [ "$pid" != "0" ] && [ "$pid" != "1" ] && [ "$depth" -lt 12 ]; do
        local comm=""
        comm=$(ps -o comm= -p "$pid" 2>/dev/null | tr -d '[:space:]') || comm=""
        if [ "$comm" = "zed-editor" ]; then
            log_error "$operation refused: running inside the upstream Zed editor (ancestor pid $pid is 'zed-editor')."
            log_error "Building or installing zed-kask from inside Zed's integrated terminal couples it to upstream Zed."
            log_error "Run from a clean shell (Ptyxis / a plain terminal / the app launcher), not inside Zed."
            return 1
        fi
        pid=$(ps -o ppid= -p "$pid" 2>/dev/null | tr -d '[:space:]') || pid=""
        depth=$((depth + 1))
    done

    return 0
}

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

    assert_not_zed_owned_path "$BIN_DIR" "install directory preparation" || return 1

    local removed=0
    assert_kask_binary_destination "$BIN_DIR/zed-kask" || return 1
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
        assert_kask_binary_destination "$stale" || return 1
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

    assert_not_zed_owned_path "$BIN_DIR" "PATH setup" || return 1
    assert_kask_binary_destination "$BIN_DIR/zed-kask" || return 1
    assert_kask_binary_destination "$SYSTEM_BIN/zed-kask" || return 1

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
# the servers read OPENROUTER_API_KEY, DEEPINFRA_API_KEY, HKASK_* etc. from their inherited env.
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
    # The zed-kask settings.json lives here.
    local config_dir
    if [ -n "${XDG_CONFIG_HOME:-}" ]; then
        config_dir="$XDG_CONFIG_HOME/zed-kask"
    elif [ -n "${HOME:-}" ]; then
        config_dir="$HOME/.config/zed-kask"
    else
        log_error "write_mcp_server_settings: cannot determine config dir (no XDG_CONFIG_HOME or HOME)"
        return 1
    fi

    assert_not_zed_owned_path "$config_dir" "settings write" || return 1
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
    assert_not_zed_owned_path "$config_dir" "settings removal" || return 1
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

# uninstall_hkask — remove everything install.sh / install-binary.sh deploy.
#
# Lives in the shared helpers file (not install.sh) so it is in scope when
# install-common.sh is sourced by the regression test
# (kask/scripts/build/check-uninstall-paths.sh), the same way
# prepare_install_dir / add_to_path / remove_mcp_server_settings are tested.
# install.sh's --uninstall dispatch calls this; it is not called by
# install-binary.sh (which has no uninstall path).
#
# Caller must set BIN_DIR, INSTALL_DIR, SYSTEM_BIN, MCP_SERVERS (the first
# three are set by the installer's main(); MCP_SERVERS is loaded by sourcing
# install-common.sh). HKASK_REMOVE_CONFIG=true additionally removes the
# zed-kask config and data directories (named `zed-kask`, matching the app id
# used by remove_mcp_server_settings and the runtime — NOT `hkask`, which is a
# stale name that never existed on disk).
uninstall_hkask() {
    log "Uninstalling zed-kask..."

    # Remove system symlink. Capture the result so we log accurately.
    if [ -L "$SYSTEM_BIN/zed-kask" ]; then
        if sudo rm -f "$SYSTEM_BIN/zed-kask" 2>/dev/null || rm -f "$SYSTEM_BIN/zed-kask" 2>/dev/null; then
            log "Removed symlink: $SYSTEM_BIN/zed-kask"
        else
            log_error "Failed to remove $SYSTEM_BIN/zed-kask (may need sudo)"
        fi
    fi

    # Remove CLI binary and MCP server binaries
    assert_not_zed_owned_path "$BIN_DIR" "binary removal" || return 1
    assert_kask_binary_destination "$BIN_DIR/zed-kask" || return 1
    if [ -f "$BIN_DIR/zed-kask" ]; then
        rm -f "$BIN_DIR/zed-kask"
        log "Removed $BIN_DIR/zed-kask"
    fi
    for server in "${MCP_SERVERS[@]}"; do
        if [ -f "$BIN_DIR/$server" ]; then
            rm -f "$BIN_DIR/$server"
        fi
    done
    log "Removed MCP server binaries"

    local updater_dir="$INSTALL_DIR/share/zed-kask/install"
    assert_not_zed_owned_path "$updater_dir" "updater removal" || return 1
    if [ -d "$updater_dir" ]; then
        rm -rf "$updater_dir"
        log "Removed updater bundle: $updater_dir"
    fi

    # Remove the NoDisplay .desktop entry installed by install_desktop_entry
    # (all release channels — dev/stable, nightly, preview) plus the icon.
    # This .desktop is the window→icon binding GNOME uses for the taskbar; it
    # is NoDisplay=true with no MimeType/Keywords, so it never collided with
    # upstream Zed. Also remove any stale entry from pre-0.34 installs.
    local data_root
    for data_root in "${XDG_DATA_HOME:-$HOME/.local/share}" "/usr/local/share"; do
        local desktop_app_id
        for desktop_app_id in dev.zed-kask.Zed-Kask dev.zed-kask.Zed-Kask-Nightly dev.zed-kask.Zed-Kask-Preview; do
            local desktop_file="$data_root/applications/$desktop_app_id.desktop"
            if [ -f "$desktop_file" ]; then
                rm -f "$desktop_file"
                log "Removed desktop entry: $desktop_file"
            fi
        done
        # Remove both icon names installed by install_icon: the app_id name
        # (load-bearing on Wayland) and the friendly "zed-kask" alias.
        local icon_name
        for icon_name in dev.zed-kask.Zed-Kask dev.zed-kask.Zed-Kask-Nightly dev.zed-kask.Zed-Kask-Preview zed-kask; do
            local icon_file_512="$data_root/icons/hicolor/512x512/apps/$icon_name.png"
            local icon_file_1024="$data_root/icons/hicolor/1024x1024/apps/$icon_name.png"
            if [ -f "$icon_file_512" ]; then
                rm -f "$icon_file_512"
                log "Removed icon: $icon_file_512"
            fi
            if [ -f "$icon_file_1024" ]; then
                rm -f "$icon_file_1024"
                log "Removed icon: $icon_file_1024"
            fi
        done
        gtk-update-icon-cache -f "$data_root/icons/hicolor" 2>/dev/null || true
    done
    if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database "${XDG_DATA_HOME:-$HOME/.local/share}/applications" 2>/dev/null || true
    fi

    # Remove PATH entries from shell configs. Match both the current
    # `# zed-kask` marker and the legacy `# hKask` marker so users who
    # installed under the old name get cleaned up too.
    #
    # Escape `/` in $BIN_DIR before interpolating into the sed regex (sed uses
    # `/` as its delimiter). Without this, a BIN_DIR like
    # /home/user/.local/bin produces
    #   sed: -e expression #1, char N: extra characters after command
    # which aborts the uninstaller before remove_mcp_server_settings runs,
    # leaving stale context_servers entries in settings.json.
    local bin_dir_re
    bin_dir_re=$(printf '%s' "$BIN_DIR" | sed 's|/|\\/|g')
    for cfg in "$HOME/.bashrc" "$HOME/.zshrc" "$HOME/.zprofile" "$HOME/.profile"; do
        if [ -f "$cfg" ] && grep -qE '# (zed-kask|hKask)' "$cfg" 2>/dev/null; then
            sed -i -E '/# (zed-kask|hKask)/d' "$cfg"
            sed -i "/export PATH.*${bin_dir_re}/d" "$cfg"
            log "Cleaned PATH entry from $cfg"
        fi
    done

    # Remove kask-managed context_servers entries from settings.json.
    # Preserves any user-added (non-kask) context_servers entries.
    # Only removes entries whose command path points at the (now-removed)
    # BIN_DIR, so a re-install to a different BIN_DIR doesn't lose user data.
    remove_mcp_server_settings

    # Remove config (optional). The runtime dirs are named `zed-kask`
    # (matching the app id used everywhere else — settings.json path in
    # remove_mcp_server_settings, the data dir, logs, db, threads). An earlier
    # version of this block targeted `hkask`, which does not exist on disk, so
    # HKASK_REMOVE_CONFIG silently no-op'd and left real config/data behind.
    if [ "${HKASK_REMOVE_CONFIG:-false}" = "true" ]; then
        local config_dir="${XDG_CONFIG_HOME:-$HOME/.config}/zed-kask"
        rm -rf "$config_dir"
        log "Removed config directory: $config_dir"

        local data_dir="${XDG_DATA_HOME:-$HOME/.local/share}/zed-kask"
        rm -rf "$data_dir"
        log "Removed data directory: $data_dir"
    fi

    log_success "zed-kask uninstalled"
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
