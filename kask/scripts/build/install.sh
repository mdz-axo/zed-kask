#!/bin/bash
# zed-kask Installation Script for Linux
#
# Builds zed-kask and the kask MCP servers from source and installs them to
# $HOME/.local/bin (or a custom dir). System dependencies are installed via
# the canonical ./script/linux (shared with CI); Rust toolchain pinning is
# delegated to rust-toolchain.toml.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/mdz-axo/zed-kask/main/kask/scripts/build/install.sh | bash
#   bash kask/scripts/build/install.sh --debug --skip-deps
#
# Environment variables:
#   HKASK_VERSION       Tag to clone (default: 0.32.0; falls back to main only
#                       if HKASK_ALLOW_FALLBACK=true)
#   HKASK_BUILD_TYPE    release or debug (default: release)
#   HKASK_SOURCE_DIR    Use an existing source directory instead of cloning
#   HKASK_REPO_URL      Git URL (default: https://github.com/mdz-axo/zed-kask.git)
#   HKASK_ALLOW_FALLBACK  Set to "true" to allow silent fallback to main when
#                       the requested tag is missing (default: false — hard fail)
#   INSTALL_DIR         Install prefix (default: $HOME/.local)
#   HKASK_SYSTEM_INSTALL  Set to "true" to symlink into /usr/local/bin
#   HKASK_REMOVE_CONFIG  Remove config and data on uninstall (default: false)

set -euo pipefail

# ============================================================================
# Shared helpers (log functions, MCP_SERVERS, add_to_path, print_banner)
# ============================================================================
_HKASK_INSTALL_DIR="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" && pwd)"
# shellcheck source=install-common.sh
source "$_HKASK_INSTALL_DIR/install-common.sh"

# ============================================================================
# Configuration
# ============================================================================

HKASK_VERSION="${HKASK_VERSION:-}"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local}"
BIN_DIR="${INSTALL_DIR}/bin"

# ============================================================================
# Repository
# ============================================================================

HKASK_REPO_URL="${HKASK_REPO_URL:-https://github.com/mdz-axo/zed-kask.git}"
HKASK_SOURCE_DIR="${HKASK_SOURCE_DIR:-}"

clone_repo() {
    if [ -n "$HKASK_SOURCE_DIR" ]; then
        log "Using existing source directory: $HKASK_SOURCE_DIR"
        return 0
    fi

    # The root workspace Cargo.toml is a pure [workspace] manifest — the
    # `zed` package lives in crates/zed/Cargo.toml, so `grep 'name = "zed"'`
    # on the root fails. Identify the repo root by [workspace] + crates/zed/.
    if [ -f "Cargo.toml" ] && grep -q '\[workspace\]' Cargo.toml 2>/dev/null && [ -d "crates/zed" ]; then
        HKASK_SOURCE_DIR="$(pwd)"
        log "Running from within zed-kask repo: $HKASK_SOURCE_DIR"
        return 0
    fi

    # Script lives at <root>/kask/scripts/build/install.sh, so the root
    # workspace is three levels up (../../..), not two (../..). Going up only
    # two lands in kask/ — a sub-workspace that inherits deps from the root
    # and cannot build in isolation.
    # When piped via curl|bash, BASH_SOURCE[0] is empty — fall through to clone.
    if [ -n "${BASH_SOURCE[0]:-}" ] && [ -f "$(dirname "${BASH_SOURCE[0]}")/../../../Cargo.toml" ]; then
        HKASK_SOURCE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
        log "Detected repo from script location: $HKASK_SOURCE_DIR"
        return 0
    fi

    # Resolve the version to install: explicit HKASK_VERSION wins, otherwise
    # derive from the local workspace Cargo.toml (single source of truth),
    # falling back to the hardcoded default only if Cargo.toml is unreadable.
    if [ -z "$HKASK_VERSION" ]; then
        local local_root
        local_root="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")/../../.." 2>/dev/null && pwd)"
        if [ -n "$local_root" ] && [ -f "$local_root/Cargo.toml" ]; then
            HKASK_VERSION="$(awk -F'"' '/^version *=* "/{print $2; exit}' "$local_root/Cargo.toml")"
        fi
        if [ -z "$HKASK_VERSION" ]; then
            HKASK_VERSION="0.32.0"
            log_warning "Could not derive version from Cargo.toml — using default $HKASK_VERSION"
        fi
    fi

    local clone_dir="${XDG_CACHE_HOME:-$HOME/.cache}/hkask-build"
    log "Cloning zed-kask repository (v${HKASK_VERSION})..."
    rm -rf "$clone_dir"

    # Try the requested tag. If it doesn't exist, fail hard unless the user
    # explicitly opted into fallback — silent fallback breaks reproducibility
    # (two users running the same HKASK_VERSION get different binaries).
    local clone_err
    if clone_err=$(git clone --depth 1 --branch "v${HKASK_VERSION}" "$HKASK_REPO_URL" "$clone_dir" 2>&1); then
        log "Checked out tag v${HKASK_VERSION}"
    else
        if echo "$clone_err" | grep -qE '(Remote branch.*not found|pathspec.*did not match|could not find remote branch)'; then
            if [ "${HKASK_ALLOW_FALLBACK:-false}" = "true" ]; then
                log_warning "Tag v${HKASK_VERSION} not found — HKASK_ALLOW_FALLBACK=true, cloning main branch"
                git clone --depth 1 "$HKASK_REPO_URL" "$clone_dir"
            else
                log_error "Tag v${HKASK_VERSION} not found in $HKASK_REPO_URL"
                log_error "Set HKASK_ALLOW_FALLBACK=true to fall back to main, or check the tag name."
                echo "$clone_err" >&2
                exit 1
            fi
        else
            log_error "Failed to clone repository:"
            echo "$clone_err" >&2
            exit 1
        fi
    fi
    HKASK_SOURCE_DIR="$clone_dir"
    log_success "Repository cloned to $HKASK_SOURCE_DIR"
}

# ============================================================================
# System dependencies — delegate to the canonical ./script/linux
# ============================================================================
#
# install.sh does NOT maintain its own dependency list. The canonical list
# lives in script/linux (shared with CI via .github/workflows/kask-ci.yml).
# A second list here would drift, as it did before this rewrite — the prior
# inline list omitted libasound2-dev, libfontconfig-dev, libxkbcommon-x11-dev,
# libvulkan1, libwayland-dev, and other libs required to build zed/GPUI,
# causing fresh-system source builds to fail.
#
# script/linux handles: Debian/Ubuntu, Fedora/RHEL, openSUSE, Arch, Void,
# Gentoo. It is idempotent (apt-get install is a no-op for already-installed
# packages) and tolerant of pre-existing version conflicts.

install_system_dependencies() {
    local workspace_root="$1"
    if [ ! -x "$workspace_root/script/linux" ]; then
        log_error "script/linux not found or not executable: $workspace_root/script/linux"
        return 1
    fi
    log "Installing system dependencies via script/linux (canonical)..."
    (cd "$workspace_root" && ./script/linux)
    log_success "System dependencies installed"
}

# ============================================================================
# Rust toolchain
# ============================================================================
#
# rust-toolchain.toml (at the repo root) is the canonical pin. We do NOT
# hardcode a version here — rustup's default toolchain is overridden by the
# repo's rust-toolchain.toml on the first cargo invocation inside the repo.

install_rust() {
    if command -v rustc >/dev/null 2>&1; then
        log "Rust already installed: $(rustc --version)"
    else
        log "Installing Rust toolchain via rustup..."
        if [ "${CI:-}" != "true" ]; then
            curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
            if [ -f "$HOME/.cargo/env" ]; then
                # shellcheck disable=SC1091
                source "$HOME/.cargo/env"
            fi
        else
            log "Running in CI environment, skipping Rust installation"
            return 1
        fi
    fi

    # rust-toolchain.toml auto-installs the pinned components (rustfmt, clippy,
    # rust-src) on the first cargo invocation. No explicit `rustup component
    # add` needed — and silently swallowing errors with `|| true` hides real
    # failures (violates project .rules).
    log_success "Rust toolchain ready"
}

# ============================================================================
# Build and Install
# ============================================================================

build_hkask() {
    clone_repo
    local workspace_root="$HKASK_SOURCE_DIR"

    log "Building zed-kask in $workspace_root..."
    cd "$workspace_root"

    local build_args=()
    if [ "${HKASK_BUILD_TYPE:-release}" = "release" ]; then
        build_args+=(--release)
        log "Building in release mode..."
    else
        log "Building in debug mode..."
    fi

    # Build the zed-kask CLI + every MCP server listed in mcp-servers.txt.
    local package_args=(--package zed)
    for server in "${MCP_SERVERS[@]}"; do
        package_args+=(--package "$server")
    done

    log "Building CLI and MCP server binaries..."
    cargo build "${build_args[@]}" "${package_args[@]}"

    log_success "Build complete"
}

install_binary() {
    local workspace_root="$HKASK_SOURCE_DIR"

    log "Installing zed-kask binaries..."

    assert_not_zed_owned_path "$BIN_DIR" "binary installation" || return 1
    mkdir -p "$BIN_DIR"

    local profile_dir
    if [ "${HKASK_BUILD_TYPE:-release}" = "release" ]; then
        profile_dir="$workspace_root/target/release"
    else
        profile_dir="$workspace_root/target/debug"
    fi

    if [ ! -x "$profile_dir/zed-kask" ]; then
        log_error "Built CLI binary not found: $profile_dir/zed-kask"
        return 1
    fi
    for server in "${MCP_SERVERS[@]}"; do
        if [ ! -x "$profile_dir/$server" ]; then
            log_error "Built MCP server binary not found: $profile_dir/$server"
            return 1
        fi
    done

    # Install CLI binary
    assert_kask_binary_destination "$BIN_DIR/zed-kask" || return 1
    cp "$profile_dir/zed-kask" "$BIN_DIR/zed-kask"
    chmod +x "$BIN_DIR/zed-kask"

    # Strip debug symbols (reduces binary size ~60%, non-fatal if missing)
    if command -v strip >/dev/null 2>&1; then
        strip "$BIN_DIR/zed-kask" 2>/dev/null || true
        log "Stripped debug symbols from zed-kask"
    fi

    # Install MCP server binaries
    local installed_servers=0
    for server in "${MCP_SERVERS[@]}"; do
        assert_kask_binary_destination "$BIN_DIR/$server" || return 1
        cp "$profile_dir/$server" "$BIN_DIR/$server"
        chmod +x "$BIN_DIR/$server"
        if command -v strip >/dev/null 2>&1; then
            strip "$BIN_DIR/$server" 2>/dev/null || true
        fi
        installed_servers=$((installed_servers + 1))
    done

    log_success "Installed zed-kask + $installed_servers MCP server(s) to $BIN_DIR"
}

install_updater_bundle() {
    local updater_dir="$INSTALL_DIR/share/zed-kask/install"
    assert_not_zed_owned_path "$updater_dir" "updater installation" || return 1
    mkdir -p "$updater_dir"

    local file
    for file in install-binary.sh install-common.sh update-zed-kask.sh mcp-servers.txt; do
        if [ ! -f "$_HKASK_INSTALL_DIR/$file" ]; then
            log_error "Updater bundle source is missing: $_HKASK_INSTALL_DIR/$file"
            return 1
        fi
        cp "$_HKASK_INSTALL_DIR/$file" "$updater_dir/$file.tmp"
        mv -f "$updater_dir/$file.tmp" "$updater_dir/$file"
    done
    chmod 755 "$updater_dir/install-binary.sh" "$updater_dir/update-zed-kask.sh"
    log_success "Installed safe updater bundle to $updater_dir"
}

# install_icon — install the zed-kask icon into the hicolor theme so the
# running application's window has a proper icon in the taskbar/dock.
#
# zed-kask is a CLI development tool, NOT a desktop application. It must NOT
# have a .desktop file — that would register it in the app launcher, dock,
# and file associations, where it collides with the user's real Zed install.
# The icon is needed only for the window manager to display the correct icon
# when zed-kask is running.
#
# On Wayland the compositor (GNOME Shell, KDE) resolves the taskbar/dock icon
# by looking up the xdg_toplevel app_id as an icon name in the hicolor theme.
# The app_id is dev.zed-kask.Zed-Kask (see ReleaseChannel::app_id in
# crates/release_channel/src/lib.rs), so the icon MUST be installed under that
# name — a bare "zed-kask" name is never looked up and the WM falls back to a
# generic icon. We also install the friendly "zed-kask" alias for human
# inspection; it is not load-bearing.
install_icon() {
    local workspace_root="$HKASK_SOURCE_DIR"

    local data_root
    if [ "${HKASK_SYSTEM_INSTALL:-false}" = "true" ]; then
        data_root="/usr/local/share"
    else
        data_root="${XDG_DATA_HOME:-$HOME/.local/share}"
    fi

    # Install icons at multiple resolutions so the WM always finds a crisp
    # variant. Some desktop shells (GNOME on HiDPI, KDE) prefer the 1024x1024
    # entry and fall back to 512x512.
    local icon_dir_512="$data_root/icons/hicolor/512x512/apps"
    local icon_dir_1024="$data_root/icons/hicolor/1024x1024/apps"
    mkdir -p "$icon_dir_512" "$icon_dir_1024"

    local channel
    if [ -f "$workspace_root/crates/zed/RELEASE_CHANNEL" ]; then
        channel="$(< "$workspace_root/crates/zed/RELEASE_CHANNEL")"
    else
        channel="${RELEASE_CHANNEL:-dev}"
    fi
    local icon_suffix=""
    if [ "$channel" != "stable" ]; then
        icon_suffix="-$channel"
    fi
    local src_icon="$workspace_root/kask/assets/icons/app-icon${icon_suffix}.png"
    local src_icon_2x="$workspace_root/kask/assets/icons/app-icon${icon_suffix}@2x.png"
    if [ ! -f "$src_icon" ]; then
        src_icon="$workspace_root/kask/assets/icons/app-icon.png"
        src_icon_2x="$workspace_root/kask/assets/icons/app-icon@2x.png"
    fi
    if [ ! -f "$src_icon" ]; then
        log_error "No source icon found at $src_icon"
        return 1
    fi

    # app_id must match ReleaseChannel::app_id() in crates/release_channel.
    local app_id_name
    case "$channel" in
        stable)  app_id_name="dev.zed-kask.Zed-Kask" ;;
        nightly) app_id_name="dev.zed-kask.Zed-Kask-Nightly" ;;
        preview) app_id_name="dev.zed-kask.Zed-Kask-Preview" ;;
        *)       app_id_name="dev.zed-kask.Zed-Kask" ;;
    esac

    local name
    for name in "$app_id_name" "zed-kask"; do
        assert_not_zed_owned_path "$icon_dir_512/$name.png" "icon write" || return 1
        assert_not_zed_owned_path "$icon_dir_1024/$name.png" "icon write" || return 1

        cp "$src_icon" "$icon_dir_512/$name.png"
        if ! cmp -s "$src_icon" "$icon_dir_512/$name.png"; then
            log_error "Installed icon does not match its source: $icon_dir_512/$name.png"
            return 1
        fi
        log "Installed icon: $icon_dir_512/$name.png"

        if [ -f "$src_icon_2x" ]; then
            cp "$src_icon_2x" "$icon_dir_1024/$name.png"
            if ! cmp -s "$src_icon_2x" "$icon_dir_1024/$name.png"; then
                log_error "Installed icon does not match its source: $icon_dir_1024/$name.png"
                return 1
            fi
            log "Installed icon: $icon_dir_1024/$name.png"
        fi
    done

    # Best-effort icon cache refresh.
    local hicolor_root="$data_root/icons/hicolor"
    if [ -d "$hicolor_root" ]; then
        gtk-update-icon-cache -f "$hicolor_root" 2>/dev/null || true
    fi
}

setup_environment() {
    log "Setting up environment..."

    # Add zed-kask to PATH (delegates to install-common.sh).
    add_to_path

    # Also export for this script's process.
    export PATH="$BIN_DIR:$PATH"

    # Create config directory
    local config_dir="${XDG_CONFIG_HOME:-$HOME/.config}/hkask"
    if [ ! -d "$config_dir" ]; then
        mkdir -p "$config_dir"
        log "Created config directory: $config_dir"
    fi

    # Create data directory
    local data_dir="${XDG_DATA_HOME:-$HOME/.local/share}/hkask"
    if [ ! -d "$data_dir" ]; then
        mkdir -p "$data_dir"
        log "Created data directory: $data_dir"
    fi

    log_success "Environment configured"
}

# ============================================================================
# Verification
# ============================================================================

verify_installation() {
    log "Verifying installation..."

    if [ ! -f "$BIN_DIR/zed-kask" ]; then
        log_error "Binary not found at $BIN_DIR/zed-kask"
        return 1
    fi

    # zed-kask is the Zed editor binary — it has no --version flag, so we
    # report the file size as a sanity check that the binary is non-empty.
    local binary_size
    binary_size=$(stat -c%s "$BIN_DIR/zed-kask" 2>/dev/null || echo "unknown")
    log "CLI: $BIN_DIR/zed-kask (${binary_size} bytes)"

    local icon_data_root
    if [ "${HKASK_SYSTEM_INSTALL:-false}" = "true" ]; then
        icon_data_root="/usr/local/share"
    else
        icon_data_root="${XDG_DATA_HOME:-$HOME/.local/share}"
    fi
    # The app_id-named icon is the one the Wayland compositor resolves for the
    # taskbar/dock (see install_icon). Verify it exists; the friendly
    # "zed-kask" alias is installed alongside but is not load-bearing.
    local installed_icon="$icon_data_root/icons/hicolor/512x512/apps/dev.zed-kask.Zed-Kask.png"
    if [ ! -s "$installed_icon" ]; then
        log_error "Icon not found or empty at $installed_icon"
        return 1
    fi
    log "Icon: $installed_icon ($(stat -c%s "$installed_icon" 2>/dev/null || echo "unknown") bytes)"

    # Check MCP server binaries
    local mcp_count=0
    for server in "${MCP_SERVERS[@]}"; do
        if [ -x "$BIN_DIR/$server" ]; then
            mcp_count=$((mcp_count + 1))
        else
            log_error "MCP server missing or not executable: $server"
        fi
    done
    log "MCP servers: $mcp_count/${#MCP_SERVERS[@]} available"
    if [ "$mcp_count" -ne "${#MCP_SERVERS[@]}" ]; then
        return 1
    fi

    # Check symlink in /usr/local/bin
    if [ -L "$SYSTEM_BIN/zed-kask" ]; then
        log "Symlink: $SYSTEM_BIN/zed-kask → $(readlink "$SYSTEM_BIN/zed-kask")"
    fi

    # Check if zed-kask is reachable via PATH
    if command -v zed-kask >/dev/null 2>&1; then
        log_success "zed-kask is in PATH: $(command -v zed-kask)"
    else
        log_warning "zed-kask command not yet in PATH for this shell session"
        log "The PATH will take effect in new shell sessions. For now:"
        log "  export PATH=\"$BIN_DIR:\$PATH\""
    fi
}

# ============================================================================
# Uninstall
# ============================================================================

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

    # Remove any stale .desktop entry from prior installs (pre-0.34, when
    # zed-kask erroneously installed a .desktop file). Also remove the icon.
    # zed-kask is a CLI tool — it must not have a .desktop file.
    local app_id="dev.zed-kask.Zed-Kask"
    local data_root
    for data_root in "${XDG_DATA_HOME:-$HOME/.local/share}" "/usr/local/share"; do
        local desktop_file="$data_root/applications/$app_id.desktop"
        if [ -f "$desktop_file" ]; then
            rm -f "$desktop_file"
            log "Removed desktop entry: $desktop_file"
        fi
        # Remove both icon names installed by install_icon: the app_id name
        # (load-bearing on Wayland) and the friendly "zed-kask" alias.
        local icon_name
        for icon_name in "$app_id" "zed-kask"; do
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

    # Remove config (optional)
    if [ "${HKASK_REMOVE_CONFIG:-false}" = "true" ]; then
        local config_dir="${XDG_CONFIG_HOME:-$HOME/.config}/hkask"
        rm -rf "$config_dir"
        log "Removed config directory: $config_dir"

        local data_dir="${XDG_DATA_HOME:-$HOME/.local/share}/hkask"
        rm -rf "$data_dir"
        log "Removed data directory: $data_dir"
    fi

    log_success "zed-kask uninstalled"
}

# ============================================================================
# Help
# ============================================================================

show_help() {
    cat << EOF
hKask Installation Script

Builds zed-kask and the kask MCP servers from source and installs them.

Usage: $0 [OPTIONS]

Options:
    --install           Install hKask (default)
    --uninstall         Remove hKask
    --build-only        Build without installing
    --debug             Build in debug mode
    --system            Install system-wide (symlink in /usr/local/bin)
    --skip-deps         Skip system dependency installation
    --skip-rust         Skip Rust installation
    --install-dir DIR   Install to custom directory (default: \$HOME/.local)
    --help              Show this help message

Environment Variables:
    HKASK_VERSION         Tag to install (default: derived from workspace
                          Cargo.toml version, or 0.32.0 if unreadable)
    HKASK_BUILD_TYPE      release or debug (default: release)
    HKASK_SOURCE_DIR      Use existing source directory instead of cloning
    HKASK_REPO_URL        Git repository URL
    HKASK_ALLOW_FALLBACK  Allow silent fallback to main if tag missing (default: false)
    HKASK_MARKETPLACE_URL URL of the kask skill marketplace API
                          (default: http://localhost:3000 for local dev).
                          Set to your kask-aware collab server in production.
                          Decoupled from server_url (which points at Zed's
                          cloud for login/collab/telemetry).
    INSTALL_DIR           Installation directory (default: $HOME/.local)
    HKASK_SYSTEM_INSTALL  Force system-wide install (default: false)
    HKASK_REMOVE_CONFIG   Remove config and data on uninstall (default: false)

Examples:
    # Install hKask (latest release tag)
    curl -fsSL https://raw.githubusercontent.com/mdz-axo/zed-kask/main/kask/scripts/build/install.sh | bash

    # Debug build from an existing checkout
    bash kask/scripts/build/install.sh --debug --skip-deps

    # Install with custom directory
    INSTALL_DIR=/opt/hkask bash install.sh

    # Uninstall
    bash install.sh --uninstall

    # Uninstall with config
    HKASK_REMOVE_CONFIG=true bash install.sh --uninstall

EOF
}

# ============================================================================
# Main
# ============================================================================

main() {
    local action="install"
    local skip_deps=false
    local skip_rust=false
    local saw_system=false
    local saw_install_dir=false
    local install_dir_arg=""

    while [[ $# -gt 0 ]]; do
        case $1 in
            --install)
                action="install"
                shift
                ;;
            --uninstall)
                action="uninstall"
                shift
                ;;
            --build-only)
                action="build-only"
                shift
                ;;
            --debug)
                HKASK_BUILD_TYPE="debug"
                shift
                ;;
            --system)
                saw_system=true
                HKASK_SYSTEM_INSTALL="true"
                shift
                ;;
            --skip-deps)
                skip_deps=true
                shift
                ;;
            --skip-rust)
                skip_rust=true
                shift
                ;;
            --install-dir)
                if [ $# -lt 2 ]; then
                    log_error "--install-dir requires a directory argument"
                    exit 1
                fi
                saw_install_dir=true
                install_dir_arg="$2"
                shift 2
                ;;
            --help)
                show_help
                exit 0
                ;;
            *)
                log_error "Unknown option: $1"
                show_help
                exit 1
                ;;
        esac
    done

    # --system and --install-dir are mutually exclusive: --system installs to
    # fixed system paths (/usr/local/libexec/hkask); --install-dir installs
    # to a user-specified prefix. Combining them is a user error — reject it
    # rather than silently discarding one (BH-19).
    if [ "$saw_system" = true ] && [ "$saw_install_dir" = true ]; then
        log_error "--system and --install-dir are mutually exclusive"
        log_error "  --system installs to /usr/local/libexec/hkask (system PATH)"
        log_error "  --install-dir installs to a custom prefix"
        exit 1
    fi

    if [ "$saw_system" = true ]; then
        INSTALL_DIR="/usr/local/libexec/hkask"
        BIN_DIR="${INSTALL_DIR}/bin"
    elif [ "$saw_install_dir" = true ]; then
        INSTALL_DIR="$install_dir_arg"
        BIN_DIR="${INSTALL_DIR}/bin"
    fi

    print_banner "Source Build Installer"

    case "$action" in
        install)
            log "Starting hKask installation..."

            # Refuse to build/install from inside the upstream Zed (or Flatpak
            # Zed) terminal — a contaminated LD_LIBRARY_PATH couples the
            # build to upstream Zed's libraries. See assert_not_zed_contaminated_env.
            assert_not_zed_contaminated_env "install" || exit 1

            # Resolve the source dir early so install_system_dependencies can
            # find script/linux (which lives at the repo root).
            clone_repo

            if [ "$skip_deps" = false ]; then
                install_system_dependencies "$HKASK_SOURCE_DIR"
            else
                log "Skipping system dependency installation"
            fi

            if [ "$skip_rust" = false ]; then
                install_rust
            else
                log "Skipping Rust installation"
            fi

            build_hkask
            prepare_install_dir
            install_binary
            install_updater_bundle
            install_icon
            setup_environment
            write_mcp_server_settings

            verify_installation

            echo ""
            log_success "Installation complete!"
            echo ""
            echo "To get started:"
            echo "  1. Run zed-kask:"
            echo "     zed-kask --help"
            echo ""
            if ! command -v zed-kask >/dev/null 2>&1; then
                echo "  Note: Start a new shell session for PATH changes to take effect."
                echo ""
            fi
            ;;
        uninstall)
            assert_not_zed_contaminated_env "uninstall" || exit 1
            uninstall_hkask
            ;;
        build-only)
            assert_not_zed_contaminated_env "build" || exit 1
            clone_repo
            if [ "$skip_deps" = false ]; then
                install_system_dependencies "$HKASK_SOURCE_DIR"
            fi
            if [ "$skip_rust" = false ]; then
                install_rust
            fi
            build_hkask
            ;;
    esac
}

main "$@"
