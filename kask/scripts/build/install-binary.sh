#!/bin/bash
# hKask binary installer — downloads prebuilt binaries from a GitHub Release
# and installs them to ~/.local/bin (or /usr/local/bin with --system).
#
# Falls back to the source-build installer (kask/scripts/build/install.sh)
# if no matching prebuilt archive exists for the current platform.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/mdz-axo/zed-kask/main/kask/scripts/build/install-binary.sh | bash
#   wget -O - https://raw.githubusercontent.com/mdz-axo/zed-kask/main/kask/scripts/build/install-binary.sh | bash
#
# Environment variables:
#   HKASK_VERSION   Pin a release tag (default: latest release from GitHub API)
#                    Set to "nightly" to install the nightly build.
#   HKASK_CHANNEL    Alias for HKASK_VERSION=nightly ("nightly" or "stable")
#   INSTALL_DIR     Install prefix (default: $HOME/.local)
#   HKASK_SYSTEM_INSTALL  Set to "true" to symlink into /usr/local/bin
#   HKASK_REPO      GitHub owner/repo (default: mdz-axo/zed-kask)
#   HKASK_NO_FALLBACK  Set to "true" to skip source-build fallback

set -euo pipefail

# ============================================================================
# Configuration
# ============================================================================

HKASK_REPO="${HKASK_REPO:-mdz-axo/zed-kask}"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local}"
BIN_DIR="${INSTALL_DIR}/bin"
SYSTEM_BIN="/usr/local/bin"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

log()        { echo -e "${BLUE}[INFO]${NC} $1"; }
log_success(){ echo -e "${GREEN}[SUCCESS]${NC} $1"; }
log_warning(){ echo -e "${YELLOW}[WARNING]${NC} $1"; }
log_error()  { echo -e "${RED}[ERROR]${NC} $1" >&2; }

# MCP server binaries that kask spawns as child processes.
# Must stay in sync with kask/crates/hkask-mcp-server/src/lib.rs BUILTIN_SERVERS
# and kask/scripts/build/install.sh MCP_SERVERS.
MCP_SERVERS=(
    "hkask-mcp-condenser"
    "hkask-mcp-research"
    "hkask-mcp-companies"
    "hkask-mcp-media"
    "hkask-mcp-corpus"
    "hkask-mcp-training"
    "hkask-mcp-kata-kanban"
    "hkask-mcp-curator"
    "hkask-mcp-codegraph"
    "hkask-mcp-scenarios"
)

# ============================================================================
# Platform detection
# ============================================================================

detect_target() {
    local os arch
    os="$(uname -s)"
    arch="$(uname -m)"

    case "$os-$arch" in
        Linux-x86_64)  echo "x86_64-unknown-linux-gnu" ;;
        Linux-aarch64|Linux-arm64) echo "aarch64-unknown-linux-gnu" ;;
        Darwin-x86_64) echo "x86_64-apple-darwin" ;;
        Darwin-arm64|Darwin-aarch64) echo "aarch64-apple-darwin" ;;
        MINGW*-x86_64|MSYS*-x86_64|CYGWIN*-x86_64) echo "x86_64-pc-windows-msvc" ;;
        *)
            log_error "Unsupported platform: $os-$arch"
            log_error "Supported: linux-x86_64, linux-aarch64, macos-x86_64, macos-arm64, windows-x86_64"
            return 1
            ;;
    esac
}

archive_name_for_target() {
    local target="$1"
    case "$target" in
        *-pc-windows-msvc) echo "zed-kask-${target}.zip" ;;
        *)                  echo "zed-kask-${target}.tar.gz" ;;
    esac
}

# ============================================================================
# HTTP helper (curl or wget)
# ============================================================================

http_get() {
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$@"
    elif command -v wget >/dev/null 2>&1; then
        wget -qO- "$@"
    else
        log_error "Neither curl nor wget is available"
        return 1
    fi
}

http_download() {
    # http_download <url> <output_path>
    local url="$1" out="$2"
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL -o "$out" "$url"
    elif command -v wget >/dev/null 2>&1; then
        wget -qO "$out" "$url"
    else
        log_error "Neither curl nor wget is available"
        return 1
    fi
}

# ============================================================================
# Resolve release tag
# ============================================================================

resolve_tag() {
    # Explicit version wins.
    if [ -n "${HKASK_VERSION:-}" ]; then
        # "nightly" is a special channel tag, not a version — pass through.
        if [ "$HKASK_VERSION" = "nightly" ]; then
            echo "nightly"
            return
        fi
        # Strip leading 'v' if user passed a bare version, then re-add it.
        local stripped="${HKASK_VERSION#v}"
        echo "v${stripped}"
        return
    fi

    # Channel alias.
    if [ "${HKASK_CHANNEL:-stable}" = "nightly" ]; then
        echo "nightly"
        return
    fi

    log "Resolving latest release tag from ${HKASK_REPO}..."
    local tag
    # Use the /releases/latest endpoint, which excludes prereleases.
    # Nightly builds are marked prerelease, so they won't show up here.
    tag=$(http_get "https://api.github.com/repos/${HKASK_REPO}/releases/latest" \
          | grep -m1 '"tag_name"' \
          | sed -E 's/.*"tag_name"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/')
    if [ -z "$tag" ]; then
        log_error "Could not determine latest release tag"
        return 1
    fi
    echo "$tag"
}

# ============================================================================
# Download and extract
# ============================================================================

download_and_extract() {
    local target="$1" tag="$2"
    local archive archive_url temp_dir

    archive=$(archive_name_for_target "$target")
    archive_url="https://github.com/${HKASK_REPO}/releases/download/${tag}/${archive}"

    temp_dir="$(mktemp -d)"
    trap 'rm -rf "$temp_dir"' RETURN

    log "Downloading ${archive} from ${tag}..."
    http_download "$archive_url" "$temp_dir/$archive" || return 1

    # Verify checksum if SHA256SUMS is published alongside the archive.
    local sums_url="https://github.com/${HKASK_REPO}/releases/download/${tag}/SHA256SUMS"
    local sums_path="$temp_dir/SHA256SUMS"
    if http_download "$sums_url" "$sums_path" 2>/dev/null; then
        log "Verifying checksum..."
        ( cd "$temp_dir" && grep -F "$archive" SHA256SUMS | sha256sum -c - )
    else
        log_warning "No SHA256SUMS published for ${tag} — skipping checksum verification"
    fi

    # Extract into a staging dir
    local staging="$temp_dir/extracted"
    mkdir -p "$staging"
    case "$archive" in
        *.tar.gz)
            tar -xzf "$temp_dir/$archive" -C "$staging"
            ;;
        *.zip)
            if command -v unzip >/dev/null 2>&1; then
                unzip -q "$temp_dir/$archive" -d "$staging"
            else
                log_error "unzip is required to extract $archive on Windows"
                return 1
            fi
            ;;
    esac

    echo "$staging"
}

# ============================================================================
# Install
# ============================================================================

install_binaries() {
    local staging="$1"
    mkdir -p "$BIN_DIR"

    # zed-kask binary (may be zed-kask.exe on Windows)
    local cli_src=""
    for cand in "$staging/zed-kask" "$staging/zed-kask.exe"; do
        if [ -f "$cand" ]; then cli_src="$cand"; break; fi
    done
    if [ -z "$cli_src" ]; then
        log_error "zed-kask binary not found in archive"
        return 1
    fi
    cp "$cli_src" "$BIN_DIR/"
    chmod +x "$BIN_DIR"/zed-kask* 2>/dev/null || true

    # MCP server binaries
    local installed_servers=0
    for server in "${MCP_SERVERS[@]}"; do
        for cand in "$staging/$server" "$staging/$server.exe"; do
            if [ -f "$cand" ]; then
                cp "$cand" "$BIN_DIR/"
                chmod +x "$BIN_DIR"/"$server"* 2>/dev/null || true
                installed_servers=$((installed_servers + 1))
                break
            fi
        done
    done

    log_success "Installed zed-kask + ${installed_servers} MCP server(s) to $BIN_DIR"
}

add_to_path() {
    # Strategy 1: symlink into /usr/local/bin (already in PATH on all Linux/macOS)
    if [ "${HKASK_SYSTEM_INSTALL:-false}" = "true" ] || [ -w "$SYSTEM_BIN" ]; then
        if ln -sf "$BIN_DIR/zed-kask" "$SYSTEM_BIN/zed-kask" 2>/dev/null; then
            log_success "zed-kask linked into $SYSTEM_BIN"
            return 0
        fi
        if command -v sudo >/dev/null 2>&1 && \
           sudo ln -sf "$BIN_DIR/zed-kask" "$SYSTEM_BIN/zed-kask" 2>/dev/null; then
            log_success "zed-kask linked into $SYSTEM_BIN (via sudo)"
            return 0
        fi
        log_warning "Cannot write to $SYSTEM_BIN, falling back to shell config"
    fi

    # Strategy 2: add BIN_DIR to PATH via shell config files.
    local user_shell added=false
    user_shell=$(basename "${SHELL:-/bin/bash}")
    local configs=("$HOME/.profile")
    case "$user_shell" in
        zsh)  configs+=("$HOME/.zshrc" "$HOME/.zprofile") ;;
        bash|sh) configs+=("$HOME/.bashrc") ;;
        *)    configs+=("$HOME/.bashrc") ;;
    esac

    if [[ ":$PATH:" != *":$BIN_DIR:"* ]]; then
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
    fi
}

# ============================================================================
# Source-build fallback
# ============================================================================

fallback_to_source_build() {
    if [ "${HKASK_NO_FALLBACK:-false}" = "true" ]; then
        log_error "No prebuilt binary available and HKASK_NO_FALLBACK=true"
        exit 1
    fi

    log_warning "No prebuilt binary for this platform. Falling back to source build..."
    log "This will clone the repo and build with cargo (requires Rust toolchain)."

    local installer_url="https://raw.githubusercontent.com/${HKASK_REPO}/main/kask/scripts/build/install.sh"
    exec bash -c "$(http_get "$installer_url")"
}

# ============================================================================
# Verify
# ============================================================================

verify_installation() {
    local cli_path="$BIN_DIR/zed-kask"
    [ -f "$cli_path" ] || cli_path="$BIN_DIR/zed-kask.exe"
    if [ ! -f "$cli_path" ]; then
        log_error "Binary not found at $cli_path"
        return 1
    fi

    local size
    size=$(stat -c%s "$cli_path" 2>/dev/null || stat -f%z "$cli_path" 2>/dev/null || echo "unknown")
    log "CLI: $cli_path (${size} bytes)"

    local mcp_count=0
    for server in "${MCP_SERVERS[@]}"; do
        if [ -x "$BIN_DIR/$server" ] || [ -x "$BIN_DIR/$server.exe" ]; then
            mcp_count=$((mcp_count + 1))
        fi
    done
    log "MCP servers: ${mcp_count}/${#MCP_SERVERS[@]} available"

    if command -v zed-kask >/dev/null 2>&1; then
        log_success "zed-kask is in PATH: $(command -v zed-kask)"
    else
        log_warning "zed-kask not yet in PATH for this shell session"
        log "  export PATH=\"$BIN_DIR:\$PATH\""
    fi
}

# ============================================================================
# Main
# ============================================================================

main() {
    echo ""
    echo "╔══════════════════════════════════════════════════════════╗"
    echo "║              hKask Binary Installer                      ║"
    echo "║   Downloads prebuilt binaries from GitHub Releases       ║"
    echo "╚══════════════════════════════════════════════════════════╝"
    echo ""

    local target tag
    target=$(detect_target) || exit 1
    log "Detected target: $target"

    tag=$(resolve_tag) || exit 1
    log "Release tag: $tag"

    local staging
    staging=$(download_and_extract "$target" "$tag") || {
        log_error "Download/extract failed for $target"
        fallback_to_source_build
        exit 1
    }

    install_binaries "$staging"
    add_to_path
    verify_installation

    echo ""
    log_success "Installation complete!"
    echo ""
    echo "To get started:"
    echo "  zed-kask --help"
    echo ""
    if ! command -v zed-kask >/dev/null 2>&1; then
        echo "  Note: start a new shell session for PATH changes to take effect."
        echo ""
    fi
}

main "$@"
