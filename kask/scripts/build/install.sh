#!/bin/bash
# hKask Installation Script for Linux
#
# This script installs hKask and its dependencies on Linux systems.
# Supports: Debian/Ubuntu, Fedora/RHEL, Arch Linux, openSUSE, Alpine
#
# Also sets up the Conduit Matrix homeserver (Docker/Podman sidecar) for
# agent-to-agent communication. The Curator registers as the Matrix admin
# and manages account creation, deletion, and moderation on the server.
# Skip with --skip-conduit.
#
# Usage: curl -fsSL https://raw.githubusercontent.com/mdz-axo/hKask/main/scripts/build/install.sh | bash
# Or: wget -O - https://raw.githubusercontent.com/mdz-axo/hKask/main/scripts/build/install.sh | bash

set -euo pipefail

# ============================================================================
# Configuration
# ============================================================================

HKASK_VERSION="${HKASK_VERSION:-0.31.0}"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local}"
BIN_DIR="${INSTALL_DIR}/bin"
SYSTEM_BIN="/usr/local/bin"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# ============================================================================
# Logging Functions
# ============================================================================

log() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

log_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# ============================================================================
# System Detection
# ============================================================================

detect_package_manager() {
    if command -v apt-get &> /dev/null; then
        echo "apt"
    elif command -v dnf &> /dev/null; then
        echo "dnf"
    elif command -v yum &> /dev/null; then
        echo "yum"
    elif command -v pacman &> /dev/null; then
        echo "pacman"
    elif command -v zypper &> /dev/null; then
        echo "zypper"
    elif command -v apk &> /dev/null; then
        echo "apk"
    else
        echo "unknown"
    fi
}

detect_os() {
    if [ -f /etc/os-release ]; then
        . /etc/os-release
        echo "$ID"
    else
        echo "unknown"
    fi
}

# ============================================================================
# Dependency Installation
# ============================================================================

install_system_dependencies() {
    local pkg_mgr os
    pkg_mgr=$(detect_package_manager)
    os=$(detect_os)

    log "Detected package manager: $pkg_mgr"
    log "Detected OS: $os"

    case "$pkg_mgr" in
        apt)
            log "Updating package lists..."
            sudo apt-get update -qq

            log "Installing build dependencies (Debian/Ubuntu)..."
            sudo apt-get install -y -qq \
                build-essential \
                clang \
                mold \
                pkg-config \
                libssl-dev \
                libsqlite3-dev \
                libdbus-1-dev \
                git \
                curl \
                wget \
                jq \
                xz-utils
            ;;
        dnf|yum)
            log "Installing build dependencies (Fedora/RHEL)..."
            sudo "$pkg_mgr" install -y \
                gcc \
                gcc-c++ \
                clang \
                mold \
                make \
                pkg-config \
                openssl-devel \
                sqlite-devel \
                dbus-devel \
                git \
                curl \
                wget \
                jq \
                xz
            ;;
        pacman)
            log "Installing build dependencies (Arch Linux)..."
            sudo pacman -Sy --noconfirm \
                base-devel \
                clang \
                mold \
                openssl \
                sqlite \
                dbus \
                git \
                curl \
                wget \
                jq \
                xz
            ;;
        zypper)
            log "Installing build dependencies (openSUSE)..."
            sudo zypper install -y \
                -t pattern devel_basis \
                clang \
                mold \
                pkg-config \
                libopenssl-devel \
                sqlite3-devel \
                dbus-1-devel \
                git \
                curl \
                wget \
                jq \
                xz
            ;;
        apk)
            log "Installing build dependencies (Alpine)..."
            sudo apk add --no-cache \
                build-base \
                clang \
                mold \
                openssl-dev \
                sqlite-dev \
                dbus-dev \
                git \
                curl \
                wget \
                jq \
                xz
            ;;
        unknown)
            log_warning "Unknown package manager. Please install dependencies manually."
            log "Required: build-essential, clang, mold, pkg-config, libssl-dev, libsqlite3-dev, libdbus-1-dev, git, curl, jq, xz-utils"
            return 1
            ;;
    esac

    log_success "System dependencies installed"
}

install_rust() {
    if command -v rustc &> /dev/null; then
        local rust_version
        rust_version=$(rustc --version)
        log "Rust already installed: $rust_version"

        # Parse version: e.g. "rustc 1.91.0 (...)", extract major.minor
        local rust_major_minor rust_major rust_minor
        rust_major_minor=$(rustc --version | awk '{print $2}' | cut -d. -f1,2)
        rust_major=$(echo "$rust_major_minor" | cut -d. -f1)
        rust_minor=$(echo "$rust_major_minor" | cut -d. -f2)
        if [ -n "$rust_major" ] && [ -n "$rust_minor" ] && { [ "$rust_major" -lt 1 ] || { [ "$rust_major" -eq 1 ] && [ "$rust_minor" -lt 91 ]; }; }; then
            log_warning "Rust version too old (project requires 1.91+). Update with 'rustup update' or install from https://rustup.rs"
        fi
    else
        log "Installing Rust toolchain..."

        if [ "${CI:-}" != "true" ]; then
            curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.91

            if [ -f "$HOME/.cargo/env" ]; then
                source "$HOME/.cargo/env"
            fi
        else
            log "Running in CI environment, skipping Rust installation"
        fi
    fi

    log "Adding Rust components..."
    rustup component add rustfmt clippy rust-src 2>/dev/null || true

    log_success "Rust toolchain ready"
}

# ============================================================================
# Build and Install
# ============================================================================

HKASK_REPO_URL="${HKASK_REPO_URL:-https://github.com/mdz-axo/hKask.git}"
HKASK_SOURCE_DIR="${HKASK_SOURCE_DIR:-}"

clone_repo() {
    if [ -n "$HKASK_SOURCE_DIR" ]; then
        log "Using existing source directory: $HKASK_SOURCE_DIR"
        return 0
    fi

    if [ -f "Cargo.toml" ] && grep -q "hkask-cli" Cargo.toml 2>/dev/null; then
        HKASK_SOURCE_DIR="$(pwd)"
        log "Running from within hKask repo: $HKASK_SOURCE_DIR"
        return 0
    fi

    if [ -n "${BASH_SOURCE[0]:-}" ] && [ -f "$(dirname "${BASH_SOURCE[0]}")/../../Cargo.toml" ]; then
        HKASK_SOURCE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
        log "Detected repo from script location: $HKASK_SOURCE_DIR"
        return 0
    fi

    local clone_dir="${XDG_CACHE_HOME:-$HOME/.cache}/hkask-build"
    log "Cloning hKask repository (v${HKASK_VERSION})..."
    rm -rf "$clone_dir"

    # Try version tag first, fall back to main branch
    local clone_err
    if clone_err=$(git clone --depth 1 --branch "v${HKASK_VERSION}" "$HKASK_REPO_URL" "$clone_dir" 2>&1); then
        log "Checked out tag v${HKASK_VERSION}"
    else
        # Only fall back if the error is "tag not found", not a network failure
        if echo "$clone_err" | grep -qE '(Remote branch.*not found|pathspec.*did not match|could not find remote branch)'; then
            log "Tag v${HKASK_VERSION} not found, cloning main branch"
            git clone --depth 1 "$HKASK_REPO_URL" "$clone_dir"
        else
            log_error "Failed to clone repository:"
            echo "$clone_err" >&2
            exit 1
        fi
    fi
    HKASK_SOURCE_DIR="$clone_dir"
    log_success "Repository cloned to $HKASK_SOURCE_DIR"
}

# MCP server binaries that kask spawns as child processes.
# Must stay in sync with crates/hkask-mcp/src/lib.rs BUILTIN_SERVERS (canonical registry).
MCP_SERVERS=(
    "hkask-mcp-memory"
    "hkask-mcp-condenser"
    "hkask-mcp-research"
    "hkask-mcp-companies"
    "hkask-mcp-communication"
    "hkask-mcp-media"
    "hkask-mcp-docproc"
    "hkask-mcp-training"
    "hkask-mcp-replica"
    "hkask-mcp-kata-kanban"
    "hkask-mcp-skill"
    "hkask-mcp-filesystem"
    "hkask-mcp-curator"
    "hkask-mcp-codegraph"
    "hkask-mcp-scenarios"
)

build_hkask() {
    clone_repo
    local workspace_root="$HKASK_SOURCE_DIR"

    log "Building hKask in $workspace_root..."
    cd "$workspace_root"

    local build_args=()
    if [ "${HKASK_BUILD_TYPE:-release}" = "release" ]; then
        build_args+=(--release)
        log "Building in release mode..."
    else
        log "Building in debug mode..."
    fi

    local package_args=(--package hkask-cli)
    for server in "${MCP_SERVERS[@]}"; do
        package_args+=(--package "$server")
    done

    log "Building CLI and MCP server binaries..."
    cargo build "${build_args[@]}" "${package_args[@]}"

    log_success "Build complete"
}

install_binary() {
    local workspace_root="$HKASK_SOURCE_DIR"

    log "Installing hKask binaries..."

    # Create bin directory if it doesn't exist
    mkdir -p "$BIN_DIR"

    local profile_dir
    if [ "${HKASK_BUILD_TYPE:-release}" = "release" ]; then
        profile_dir="$workspace_root/target/release"
    else
        profile_dir="$workspace_root/target/debug"
    fi

    if [ ! -x "$profile_dir/kask" ]; then
        log_error "Built CLI binary not found: $profile_dir/kask"
        return 1
    fi
    for server in "${MCP_SERVERS[@]}"; do
        if [ ! -x "$profile_dir/$server" ]; then
            log_error "Built MCP server binary not found: $profile_dir/$server"
            return 1
        fi
    done

    # Install CLI binary
    cp "$profile_dir/kask" "$BIN_DIR/kask"
    chmod +x "$BIN_DIR/kask"

    # Strip debug symbols (reduces binary size ~60%, non-fatal if missing)
    if command -v strip &> /dev/null; then
        strip "$BIN_DIR/kask" 2>/dev/null || true
        log "Stripped debug symbols from kask"
    fi

    # Install MCP server binaries
    local installed_servers=0
    for server in "${MCP_SERVERS[@]}"; do
        if [ -f "$profile_dir/$server" ]; then
            cp "$profile_dir/$server" "$BIN_DIR/$server"
            chmod +x "$BIN_DIR/$server"
            if command -v strip &> /dev/null; then
                strip "$BIN_DIR/$server" 2>/dev/null || true
            fi
            installed_servers=$((installed_servers + 1))
        else
            log_warning "MCP server binary not found: $server"
        fi
    done

    log_success "Installed kask + $installed_servers MCP server(s) to $BIN_DIR"
}

# Add kask to PATH. Tries symlink to /usr/local/bin first (system-wide),
# falls back to shell config PATH manipulation (user-local).
add_to_path() {
    # Strategy 1: symlink into /usr/local/bin (already in PATH on all Linux)
    if [ -w "$SYSTEM_BIN" ] || [ "${HKASK_SYSTEM_INSTALL:-false}" = "true" ]; then
        log "Creating symlink at $SYSTEM_BIN/kask → $BIN_DIR/kask"
        if ln -sf "$BIN_DIR/kask" "$SYSTEM_BIN/kask" 2>/dev/null; then
            log_success "kask linked into $SYSTEM_BIN (system PATH)"
            return 0
        fi
        # Symlink failed even with --system — fall through to shell config
        log_warning "Cannot write to $SYSTEM_BIN, falling back to shell config"
    elif command -v sudo &> /dev/null; then
        log "Creating system symlink (requires sudo)..."
        if sudo ln -sf "$BIN_DIR/kask" "$SYSTEM_BIN/kask" 2>/dev/null; then
            log_success "kask linked into $SYSTEM_BIN (system PATH)"
            return 0
        fi
        log_warning "Cannot write to $SYSTEM_BIN, falling back to shell config"
    else
        log "No sudo access — configuring PATH in shell config"
    fi

    # Strategy 2: add BIN_DIR to PATH via shell config files.
    # Detect the user's login shell from $SHELL (not the script's interpreter
    # — $SHELL is set by login(1) and inherits through subprocesses).
    local added=false
    local configs=()
    local user_shell
    user_shell=$(basename "${SHELL:-/bin/bash}")

    # .profile is sourced by bash/zsh/sh login shells (ssh, systemd, tty login)
    configs+=("$HOME/.profile")

    case "$user_shell" in
        zsh)
            configs+=("$HOME/.zshrc")
            # zsh login shells source .zprofile, not .profile, but many
            # zsh configs also source .profile for compatibility.
            configs+=("$HOME/.zprofile")
            ;;
        bash|sh)
            configs+=("$HOME/.bashrc")
            ;;
        *)
            # Unknown shell — add bashrc as best-effort fallback
            configs+=("$HOME/.bashrc")
            ;;
    esac

    # Check if BIN_DIR needs PATH on this system
    # (systemd 0.25+ ships ~/.local/bin in PATH by default)
    local needs_local_path=false
    if [[ ":$PATH:" != *":$BIN_DIR:"* ]]; then
        needs_local_path=true
    fi

    if [ "$needs_local_path" = true ]; then
        for cfg in "${configs[@]}"; do
            if ! grep -qF '# hKask' "$cfg" 2>/dev/null; then
                {
                    echo ""
                    echo "# hKask"
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
    else
        log_warning "Could not add $BIN_DIR to PATH automatically"
        log "Please add this line to your shell config:"
        log "  export PATH=\"$BIN_DIR:\$PATH\""
    fi
}

setup_environment() {
    log "Setting up environment..."

    # Add kask to PATH
    add_to_path

    # Also export for this script's process
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
# Conduit Setup (Matrix Homeserver)
# ============================================================================

# Print OS-specific instructions for installing Docker or Podman.
# Called when setup_conduit() detects no container runtime.
print_container_runtime_guide() {
    local pkg_mgr
    pkg_mgr=$(detect_package_manager)

    echo ""
    log_warning "Conduit (Matrix homeserver) requires Docker or Podman."
    echo ""
    echo "  Install a container runtime for your system:"
    echo ""

    case "$pkg_mgr" in
        apt)
            echo "    # Docker (Debian/Ubuntu):"
            echo "    sudo apt-get install -y docker.io"
            echo "    sudo systemctl enable --now docker"
            echo "    sudo usermod -aG docker \$USER  # log out and back in after this"
            echo ""
            echo "    # Or Podman:"
            echo "    sudo apt-get install -y podman podman-compose"
            ;;
        dnf|yum)
            echo "    # Docker (Fedora/RHEL):"
            echo "    sudo dnf install -y docker docker-compose"
            echo "    sudo systemctl enable --now docker"
            echo "    sudo usermod -aG docker \$USER  # log out and back in after this"
            echo ""
            echo "    # Or Podman:"
            echo "    sudo dnf install -y podman podman-compose"
            ;;
        pacman)
            echo "    # Docker (Arch):"
            echo "    sudo pacman -S --noconfirm docker docker-compose"
            echo "    sudo systemctl enable --now docker"
            echo "    sudo usermod -aG docker \$USER  # log out and back in after this"
            echo ""
            echo "    # Or Podman:"
            echo "    sudo pacman -S --noconfirm podman podman-compose"
            ;;
        zypper)
            echo "    # Docker (openSUSE):"
            echo "    sudo zypper install -y docker docker-compose"
            echo "    sudo systemctl enable --now docker"
            echo "    sudo usermod -aG docker \$USER  # log out and back in after this"
            echo ""
            echo "    # Or Podman:"
            echo "    sudo zypper install -y podman podman-compose"
            ;;
        apk)
            echo "    # Docker (Alpine):"
            echo "    sudo apk add docker docker-compose"
            echo "    sudo rc-update add docker boot"
            echo "    sudo service docker start"
            echo "    sudo addgroup \$USER docker  # log out and back in after this"
            ;;
        *)
            echo "    See: https://docs.docker.com/engine/install/"
            echo "    Or:  https://podman.io/getting-started/installation"
            ;;
    esac

    echo ""
    echo "  After installing, start Conduit:"
    echo "    ./scripts/conduit/conduit-docker.sh start"
    echo ""
}

# Start Conduit Matrix homeserver via the conduit-docker.sh management script.
#
# If a container runtime (Docker/Podman) is available, pulls the Conduit image,
# starts the container, waits for it to become healthy, and registers the
# Curator as the Matrix admin. The Curator manages account creation, deletion,
# and moderation on the Matrix server.
#
# If no runtime is found, prints OS-specific install instructions and
# skips Conduit (non-fatal).
#
# Requires the repo to be cloned (HKASK_SOURCE_DIR must be set).
setup_conduit() {
    local conduit_script="$HKASK_SOURCE_DIR/scripts/conduit/conduit-docker.sh"

    if [ ! -f "$conduit_script" ]; then
        log_warning "conduit-docker.sh not found at $conduit_script — skipping Conduit setup"
        return 0
    fi

    log "Setting up Conduit Matrix homeserver..."

    # conduit-docker.sh handles its own runtime detection and will exit with
    # a clear error if neither Docker nor Podman is available.
    if bash "$conduit_script" start; then
        log_success "Conduit Matrix homeserver is running at http://localhost:8008"

        # Register the Curator admin user for human administration.
        # System bots (hkask-curator, 7R7, etc.) auto-register during hKask bootstrap.
        log "Registering Curator admin user..."
        if bash "$conduit_script" register curator UserSovereignty 2>&1 | grep -q "successfully"; then
            log_success "Curator admin registered: @curator:localhost / UserSovereignty"
        else
            log "Curator admin may already exist — credentials: @curator:localhost / UserSovereignty"
        fi
        log "System bots will auto-register Matrix accounts on first launch."
        CONDUIT_READY=true
    else
        print_container_runtime_guide
        CONDUIT_READY=false
    fi
}

# ============================================================================
# Verification
# ============================================================================

verify_installation() {
    log "Verifying installation..."

    # Check the binary file exists
    if [ ! -f "$BIN_DIR/kask" ]; then
        log_error "Binary not found at $BIN_DIR/kask"
        return 1
    fi

    local version
    version=$("$BIN_DIR/kask" --version 2>&1 || echo "unknown")
    log "CLI: $BIN_DIR/kask ($version)"

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
    if [ -L "$SYSTEM_BIN/kask" ]; then
        log "Symlink: $SYSTEM_BIN/kask → $(readlink "$SYSTEM_BIN/kask")"
    fi

    # Check if kask is reachable via PATH
    if command -v kask &> /dev/null; then
        local resolved
        resolved=$(command -v kask)
        log_success "kask is in PATH: $resolved ($version)"
    else
        log_warning "kask command not yet in PATH for this shell session"
        log "The PATH will take effect in new shell sessions. For now:"
        log "  export PATH=\"$BIN_DIR:\$PATH\""
    fi

    # Check Conduit health if it was set up
    if [ "${CONDUIT_READY:-false}" = "true" ]; then
        if curl -s "http://localhost:8008/_matrix/client/versions" > /dev/null 2>&1; then
            log_success "Conduit Matrix homeserver: healthy at http://localhost:8008"
        else
            log_warning "Conduit Matrix homeserver: not responding — check logs with ./scripts/conduit/conduit-docker.sh logs"
        fi
    fi
}

# ============================================================================
# Uninstall
# ============================================================================

uninstall_hkask() {
    log "Uninstalling hKask..."

    # Remove system symlink
    if [ -L "$SYSTEM_BIN/kask" ]; then
        sudo rm -f "$SYSTEM_BIN/kask" 2>/dev/null || rm -f "$SYSTEM_BIN/kask" 2>/dev/null || true
        log "Removed symlink: $SYSTEM_BIN/kask"
    fi

    # Remove CLI binary and MCP server binaries
    if [ -f "$BIN_DIR/kask" ]; then
        rm -f "$BIN_DIR/kask"
        log "Removed $BIN_DIR/kask"
    fi
    for server in "${MCP_SERVERS[@]}"; do
        if [ -f "$BIN_DIR/$server" ]; then
            rm -f "$BIN_DIR/$server"
        fi
    done
    log "Removed MCP server binaries"

    # Remove PATH entries from shell configs
    for cfg in "$HOME/.bashrc" "$HOME/.zshrc" "$HOME/.zprofile" "$HOME/.profile"; do
        if [ -f "$cfg" ] && grep -q '# hKask' "$cfg" 2>/dev/null; then
            sed -i '/# hKask/d' "$cfg"
            sed -i "/export PATH.*$BIN_DIR/d" "$cfg"
            log "Cleaned PATH entry from $cfg"
        fi
    done

    # Remove config (optional)
    if [ "${HKASK_REMOVE_CONFIG:-false}" = "true" ]; then
        local config_dir="${XDG_CONFIG_HOME:-$HOME/.config}/hkask"
        rm -rf "$config_dir"
        log "Removed config directory: $config_dir"

        local data_dir="${XDG_DATA_HOME:-$HOME/.local/share}/hkask"
        rm -rf "$data_dir"
        log "Removed data directory: $data_dir"
    fi

    log_success "hKask uninstalled"
}

# ============================================================================
# Help
# ============================================================================

show_help() {
    cat << EOF
hKask Installation Script

Usage: $0 [OPTIONS]

Options:
    --install           Install hKask (default)
    --uninstall         Remove hKask
    --build-only        Build without installing
    --debug             Build in debug mode
    --system            Install system-wide (symlink in /usr/local/bin)
    --skip-deps         Skip system dependency installation
    --skip-rust         Skip Rust installation
    --skip-conduit      Skip Conduit Matrix homeserver setup
    --matrix-lan        Enable TLS + well-known for phone access over LAN
    --install-dir DIR   Install to custom directory (default: \$HOME/.local)
    --help              Show this help message

Environment Variables:
    HKASK_VERSION       Version to install (default: 0.31.0)
    HKASK_BUILD_TYPE    Build type: release or debug (default: release)
    INSTALL_DIR         Installation directory (default: \$HOME/.local)
    CARGO_HOME          Cargo installation directory (default: \$HOME/.cargo)
    HKASK_SYSTEM_INSTALL Force system-wide install (default: false)
    HKASK_REMOVE_CONFIG Remove config and data on uninstall (default: false)
    HKASK_SOURCE_DIR    Use existing source directory instead of cloning
    HKASK_REPO_URL      Git repository URL (default: https://github.com/mdz-axo/hKask.git)

Examples:
    # Install hKask
    curl -fsSL https://raw.githubusercontent.com/mdz-axo/hKask/main/scripts/build/install.sh | bash

    # Install with custom directory
    INSTALL_DIR=/opt/hkask bash install.sh

    # Debug build
    HKASK_BUILD_TYPE=debug bash install.sh

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
    local skip_conduit=false
    local matrix_lan=false

    # Parse arguments
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
                HKASK_SYSTEM_INSTALL="true"
                INSTALL_DIR="/usr/local/libexec/hkask"
                BIN_DIR="/usr/local/libexec/hkask/bin"
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
            --skip-conduit)
                skip_conduit=true
                shift
                ;;
            --matrix-lan)
                matrix_lan=true
                shift
                ;;
            --install-dir)
                if [ $# -lt 2 ]; then
                    log_error "--install-dir requires a directory argument"
                    exit 1
                fi
                INSTALL_DIR="$2"
                BIN_DIR="${INSTALL_DIR}/bin"
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

    # Post-process: --system takes precedence over --install-dir.
    # If the user passed both, --system wins (system-wide paths are fixed).
    if [ "${HKASK_SYSTEM_INSTALL:-false}" = "true" ]; then
        INSTALL_DIR="/usr/local/libexec/hkask"
        BIN_DIR="${INSTALL_DIR}/bin"
    fi

    echo ""
    echo "╔══════════════════════════════════════════════════════════╗"
    echo "║                    hKask Installer                      ║"
    echo "║        ℏKask - A Minimal Viable Container for UserPods    ║"
    echo "╚══════════════════════════════════════════════════════════╝"
    echo ""

    case "$action" in
        install)
            log "Starting hKask installation..."

            if [ "$skip_deps" = false ]; then
                install_system_dependencies
            else
                log "Skipping system dependency installation"
            fi

            if [ "$skip_rust" = false ]; then
                install_rust
            else
                log "Skipping Rust installation"
            fi

            build_hkask
            install_binary
            setup_environment

            if [ "$skip_conduit" = false ]; then
                setup_conduit
                if [ "$matrix_lan" = true ] && [ "${CONDUIT_READY:-false}" = "true" ]; then
                    log "Setting up LAN access for phone connections..."
                    bash "$HKASK_SOURCE_DIR/scripts/conduit/conduit-docker.sh" setup-lan
                fi
            else
                log "Skipping Conduit setup (--skip-conduit)"
            fi

            verify_installation

            echo ""
            log_success "Installation complete!"
            echo ""
            echo "To get started:"
            echo "  1. Run hKask:"
            echo "     kask --help"
            echo ""
            echo "  2. Start interactive chat:"
            echo "     kask chat"
            echo ""
            if [ "${CONDUIT_READY:-false}" = "true" ]; then
                echo "  Matrix communication is ready at http://localhost:8008"
                echo "  Manage Conduit: ./scripts/conduit/conduit-docker.sh {status|stop|logs|reset}"
                echo ""
            elif [ "$skip_conduit" = false ]; then
                echo "  Matrix communication not available — Conduit not running."
                echo "  Install Docker/Podman, then: ./scripts/conduit/conduit-docker.sh start"
                echo ""
            fi
            if ! command -v kask &> /dev/null; then
                echo "  Note: Start a new shell session for PATH changes to take effect."
                echo ""
            fi
            ;;
        uninstall)
            uninstall_hkask
            ;;
        build-only)
            if [ "$skip_deps" = false ]; then
                install_system_dependencies
            fi
            if [ "$skip_rust" = false ]; then
                install_rust
            fi
            build_hkask
            ;;
    esac
}

# Run main function
main "$@"
