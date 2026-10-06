#!/bin/bash
# zed-kask Installation Script for Linux
#
# Builds zed-kask and the kask MCP servers from source and installs them to
# $HOME/.local/bin (or a custom dir). Source install and --build-only also
# provision the pinned Lean/Lake toolchain via verified Elan; raw cargo build
# remains offline. System dependencies come from ./script/linux; Rust toolchain
# pinning is delegated to rust-toolchain.toml.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/mdz-axo/zed-kask/main/kask/scripts/build/install.sh | bash
#   bash kask/scripts/build/install.sh --fast --skip-deps
#
# Environment variables:
#   HKASK_VERSION       Tag to clone (default: 0.40.1; falls back to main only
#                       if HKASK_ALLOW_FALLBACK=true)
#   HKASK_BUILD_TYPE    release or release-fast (default: release)
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
            HKASK_VERSION="0.40.1"
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
# lives in script/linux (shared with CI via the zed-check job in
# .github/workflows/kask-invariants.yml).
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

# Shared server build for the release and release-fast paths (D46): the MCP
# servers are I/O daemons and always build on the cheap release-mcp profile —
# only the zed binary's profile differs between build types.
build_mcp_servers() {
    local jobs="$1"
    log "Building MCP servers on the release-mcp profile..."
    local server_args=()
    for server in "${MCP_SERVERS[@]}"; do
        server_args+=(--package "$server")
    done
    cargo build --jobs "$jobs" --profile release-mcp "${server_args[@]}"
}

build_hkask() {
    clone_repo
    local workspace_root="$HKASK_SOURCE_DIR"

    log "Building zed-kask in $workspace_root..."
    cd "$workspace_root"

    # Cap concurrent rustc invocations. The release profile compiles the
    # zed binary with codegen-units=1 + thin LTO, so each rustc pins one core
    # for minutes; uncapped, cargo spawns one per core (24 on the dev box)
    # and starves the machine. The cap is min(nproc, 16) — 2/3 of the dev
    # box's cores, full parallelism without starving the foreground.
    # Override with HKASK_BUILD_JOBS.
    local default_jobs
    default_jobs=$(( $(nproc) < 16 ? $(nproc) : 16 ))
    local jobs="${HKASK_BUILD_JOBS:-$default_jobs}"

    # sccache — reuse compiled deps across installs and profile switches.
    # Wired only when the binary is present (script/setup-sccache installs
    # it to ~/.local/lib/kask-sccache/, OUTSIDE target/ so a cargo clean
    # cannot delete the wrapper — the 2026-09-07 silent-uncache defect);
    # a missing wrapper must not break the install, but it must be visible
    # to the operator.
    local sccache_bin="${HKASK_SCCACHE_DIR:-$HOME/.local/lib/kask-sccache}/sccache"
    if [ -x "$sccache_bin" ]; then
        export RUSTC_WRAPPER="$sccache_bin"
        log "sccache enabled: $RUSTC_WRAPPER"
    else
        log_warning "sccache not found at $sccache_bin — building uncached (run script/setup-sccache to enable)"
    fi

    # CPU/RSS trace — the build observes itself (D46). Every install leaves
    # a quantified record of what it did to the machine: a per-sample CSV
    # (compile-proc count, CPU%, RSS, busiest proc) — a burn shows up as
    # sustained high rows in the trace, not a user report. The sampler is
    # stopped with SIGTERM below, so the trace covers exactly the build.
    # Override the trace location with HKASK_BUILD_TRACE.
    local trace_file="${HKASK_BUILD_TRACE:-$workspace_root/target/build-cpu-trace.log}"
    mkdir -p "$(dirname "$trace_file")"
    bash "$(dirname "${BASH_SOURCE[0]}")/build-monitor.sh" "$trace_file" &
    local monitor_pid=$!

    local build_ok=0
    if [ "${HKASK_BUILD_TYPE:-release}" = "release" ]; then
        # Two profiles, two invocations (D46): only the zed binary keeps the
        # full release profile (thin LTO + codegen-units=1 — worth it for
        # the editor). The MCP servers are I/O daemons and build on
        # release-mcp (lto=false, codegen-units=16): parallel-friendly, no
        # one-core-per-crate pin. Building all 11 servers on `release` was
        # the install CPU-burn defect.
        log "Building zed binary in release mode (full LTO)..."
        log "Building with at most $jobs concurrent compile jobs..."
        # zed-kask: upstream parity — no default allocator divergence. The
        # 2026-09-29 D85 mimalloc default was removed 2026-09-30: it landed
        # unmeasured on the performance side while the draw-cost regression
        # (2.3–7.5 ms → 20–38 ms per draw, P7c vs 2026-09-30) appeared only
        # after it. mimalloc remains available as an opt-in feature, exactly
        # as upstream. The glibc arena-ratchet memory finding (2026-09-28) is
        # real; if it recurs, address it without a code divergence
        # (MALLOC_ARENA_MAX in the launcher) and re-measure.
        if cargo build --jobs "$jobs" --release --package zed; then
            build_mcp_servers "$jobs" && build_ok=1
        fi
    else
        # --fast: the release-fast profile (lto=false, codegen-units=16,
        # debug=full, inherits release → debug_assertions OFF) — quick
        # rebuilds with parity flags, and full debug symbols so debugger
        # workflows keep working. Dev-profile installs are removed: they
        # shipped a debug_assertions-on daily-driver (the 2026-10-02
        # "rebuilt and worse" incident, two-symptom findings §7.9).
        log "Building zed binary on the release-fast profile (release flags, no LTO)..."
        log "Building with at most $jobs concurrent compile jobs..."
        if cargo build --jobs "$jobs" --profile release-fast --package zed; then
            build_mcp_servers "$jobs" && build_ok=1
        fi
    fi

    # Stop the sampler before reporting, so the trace covers exactly the
    # build. `kill` is best-effort: a sampler that already exited (parent
    # death) must not fail the install.
    kill "$monitor_pid" 2>/dev/null || true
    wait "$monitor_pid" 2>/dev/null || true

    if [ "$build_ok" -ne 1 ]; then
        log_error "Build failed — CPU/RSS trace left at $trace_file"
        return 1
    fi

    log "CPU/RSS trace written to $trace_file"
    log_success "Build complete"
}

install_binary() {
    local workspace_root="$HKASK_SOURCE_DIR"

    log "Installing zed-kask binaries..."

    assert_not_zed_owned_path "$BIN_DIR" "binary installation" || return 1
    mkdir -p "$BIN_DIR"

    # Profile dirs (D46): release mode takes the zed binary from
    # target/release; --fast (release-fast) takes it from
    # target/release-fast. The MCP servers always come from
    # target/release-mcp.
    local zed_profile_dir profile_dir
    if [ "${HKASK_BUILD_TYPE:-release}" = "release" ]; then
        zed_profile_dir="$workspace_root/target/release"
    else
        zed_profile_dir="$workspace_root/target/release-fast"
    fi
    profile_dir="$workspace_root/target/release-mcp"

    if [ ! -x "$zed_profile_dir/zed-kask" ]; then
        log_error "Built CLI binary not found: $zed_profile_dir/zed-kask"
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
    cp "$zed_profile_dir/zed-kask" "$BIN_DIR/zed-kask"
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

# install_icon — install the zed-kask icon into the hicolor theme so the
# running application's window has a proper icon in the taskbar/dock.
#
# NOTE: the hicolor icon alone is NOT sufficient on GNOME. GNOME Shell
# resolves the taskbar/dock icon by matching the window's app_id to a
# .desktop file (filename + StartupWMClass) and reading its Icon= — for a
# window with no matching .desktop, GNOME shows a GENERIC icon, not a themed
# lookup of the app_id. So install_desktop_entry (below) is what actually
# makes the icon appear; this function supplies the named icon it references.
#
# zed-kask is a CLI development tool and must NOT pollute the app launcher,
# dock, or file associations. The .desktop installed by install_desktop_entry
# is NoDisplay=true with NO MimeType and NO Keywords, so it is invisible to
# the launcher and cannot collide with the user's real Zed install (per the
# .rules "zed-kask .desktop files must never collide with upstream Zed").
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

# install_desktop_entry — install a NoDisplay .desktop entry that binds the
# zed-kask window's Wayland app_id / X11 WM_CLASS to its icon. This is the
# mechanism GNOME Shell actually uses to resolve the taskbar/dock icon: it
# matches the window's app_id to a .desktop file (by filename + StartupWMClass)
# and reads Icon=. Without it GNOME shows a generic icon for the window — the
# hicolor themed-icon-by-app_id fallback is NOT reliable on modern GNOME, so
# the icon installed by install_icon alone never reached the taskbar.
#
# This does NOT register zed-kask in the app launcher/dock or file
# associations: NoDisplay=true hides it from menus, and it declares NO
# MimeType and NO Keywords, so it cannot collide with the user's real Zed
# install (per the .rules "zed-kask .desktop files must never collide with
# upstream Zed" — only x-scheme-handler/zed-kask and Keywords=zed-kask are
# permitted, and we declare neither). It is purely a window→icon binding.
# The filename and StartupWMClass equal the release-channel app_id (see
# ReleaseChannel::app_id in crates/release_channel/src/lib.rs), matching the
# app_id the GPUI window advertises on Wayland (xdg_toplevel.set_app_id) and
# WM_CLASS on X11.
install_desktop_entry() {
    local workspace_root="$HKASK_SOURCE_DIR"

    local data_root
    if [ "${HKASK_SYSTEM_INSTALL:-false}" = "true" ]; then
        data_root="/usr/local/share"
    else
        data_root="${XDG_DATA_HOME:-$HOME/.local/share}"
    fi

    local channel
    if [ -f "$workspace_root/crates/zed/RELEASE_CHANNEL" ]; then
        channel="$(< "$workspace_root/crates/zed/RELEASE_CHANNEL")"
    else
        channel="${RELEASE_CHANNEL:-dev}"
    fi
    local app_id_name
    case "$channel" in
        stable)  app_id_name="dev.zed-kask.Zed-Kask" ;;
        nightly) app_id_name="dev.zed-kask.Zed-Kask-Nightly" ;;
        preview) app_id_name="dev.zed-kask.Zed-Kask-Preview" ;;
        *)       app_id_name="dev.zed-kask.Zed-Kask" ;;
    esac

    local apps_dir="$data_root/applications"
    local desktop_file="$apps_dir/$app_id_name.desktop"
    assert_not_zed_owned_path "$desktop_file" "desktop entry write" || return 1
    mkdir -p "$apps_dir"

    # NoDisplay=true: invisible to launcher/dock/menus (no pollution, no
    # collision with upstream Zed). No MimeType/Keywords: zero file-association
    # surface. Icon + StartupWMClass = app_id: binds the window to the icon.
    cat > "$desktop_file" <<EOF
[Desktop Entry]
Type=Application
Name=Zed-Kask
Comment=Zed-Kask editor
Exec=zed-kask %U
Icon=$app_id_name
StartupWMClass=$app_id_name
Terminal=false
NoDisplay=true
EOF

    # Defensive: the .desktop must never carry upstream-Zed-colliding content.
    if grep -Eq 'text/plain|application/x-zerosize|x-scheme-handler/zed;|Keywords=zed;' "$desktop_file"; then
        log_error "desktop entry contains forbidden (upstream-colliding) content: $desktop_file"
        rm -f "$desktop_file"
        return 1
    fi

    log "Installed desktop entry: $desktop_file"

    # Best-effort: refresh the desktop database so the running GNOME Shell's
    # AppSystem reloads and picks up the new entry (it monitors this dir).
    if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database "$apps_dir" 2>/dev/null || true
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
# Corpus bundles — the distribution stage of the corpus pipeline
# ============================================================================
#
# Every shippable corpus is exported by the `corpus_export_bundle` tool
# (hkask-mcp-corpus) into a package of hash-pinned parts plus metadata,
# described by a bundle.json committed at kask/registry/<corpus-id>/bundle.json.
# The installer downloads each asset, verifies it against the committed
# hashes, reassembles the database, writes the materialization receipt, and
# registers the source for federated recall. The shipped database is locked
# with the PUBLIC bundle key declared in bundle.json (the corpus is openly
# licensed; the encryption is the transport wrapper the database format
# requires) — so installation needs no per-machine rekey and passphrase
# rotation can never orphan a shipped corpus.
#
# A corpus install failure is loud but not fatal to the product install:
# the editor and MCP servers are already in place; re-running the installer
# retries (verified assets are not re-downloaded).
#
# Escape: --skip-corpora stages nothing.

CORPUS_REGISTRY_RELATIVE="kask/registry"
CORPUS_URL_BASE_DEFAULT="https://github.com/mdz-axo/zed-kask/releases/download"

# install_corpus_bundle <corpus-id> <bundle-json-path>
install_corpus_bundle() {
    local corpus_id="$1"
    local bundle_json="$2"
    command -v curl >/dev/null 2>&1 || {
        log_error "corpus '$corpus_id': download requires curl (not found on PATH)"
        return 1
    }
    command -v jq >/dev/null 2>&1 || {
        log_error "corpus '$corpus_id': registration requires jq (not found on PATH)"
        return 1
    }

    local source_id display_name bundle_key bundle_run_id
    source_id="$(jq -r '.source_id' "$bundle_json")"
    display_name="$(jq -r '.display_name' "$bundle_json")"
    bundle_key="$(jq -r '.bundle_key' "$bundle_json")"
    bundle_run_id="$(jq -r '.run_id' "$bundle_json")"
    if [ -z "$source_id" ] || [ "$source_id" = "null" ] \
        || [ -z "$display_name" ] || [ "$display_name" = "null" ] \
        || [ -z "$bundle_key" ] || [ "$bundle_key" = "null" ] \
        || [ -z "$bundle_run_id" ] || [ "$bundle_run_id" = "null" ]; then
        log_error "corpus '$corpus_id': bundle.json is missing source_id, display_name, bundle_key, or run_id"
        return 1
    fi

    local manifest="${XDG_DATA_HOME:-$HOME/.local/share}/zed-kask/agents/curator/federated-sources.json"

    # Up-to-date check: a machine that already holds this corpus must not
    # re-download gigabytes. The run_id seals the run identity including the
    # database digest, so a registration naming the same run with an
    # existing database IS this bundle's content — whether it is a prior
    # install's materialized copy or the build machine's sealed registration.
    # The registration's own admission chain (byte digest or provenance)
    # remains the integrity check; this gate only detects presence.
    if [ -f "$manifest" ]; then
        local registered_identity registered_database registered_run_id
        registered_identity="$(jq -r --arg id "$source_id" \
            '([.sources[]? | select(.id == $id)] | first | .run_identity_path) // empty' \
            "$manifest" 2>/dev/null)" || true
        registered_database="$(jq -r --arg id "$source_id" \
            '([.sources[]? | select(.id == $id)] | first | .database_path) // empty' \
            "$manifest" 2>/dev/null)" || true
        if [ -n "$registered_identity" ] && [ -f "$registered_identity" ] \
            && [ -n "$registered_database" ] && [ -f "$registered_database" ]; then
            registered_run_id="$(jq -r '.run_id // empty' "$registered_identity" 2>/dev/null)" || true
            if [ -n "$registered_run_id" ] && [ "$registered_run_id" = "$bundle_run_id" ]; then
                log "corpus '$corpus_id' already up to date (run ${registered_run_id:0:12}…) — skipping download"
                log "  to force a fresh materialization, remove the '$source_id' entry from $manifest and re-run"
                return 0
            fi
        fi
    fi

    local home="${XDG_DATA_HOME:-$HOME/.local/share}/zed-kask/agents/curator/$corpus_id"
    local staging="$home/.staging"
    mkdir -p "$staging"

    local url_base="${HKASK_CORPUS_URL_BASE:-$CORPUS_URL_BASE_DEFAULT}/corpus-$corpus_id"
    local name hash actual
    while IFS=$'\t' read -r name hash; do
        [ -n "$name" ] || continue
        if [ -f "$staging/$name" ] \
            && [ "$(sha256sum "$staging/$name" | cut -d' ' -f1)" = "$hash" ]; then
            log "corpus '$corpus_id': asset already verified: $name"
            continue
        fi
        rm -f "$staging/$name"
        log "corpus '$corpus_id': downloading $name"
        curl -fL --retry 3 --progress-bar -o "$staging/$name" "$url_base/$name" \
            || { log_error "corpus '$corpus_id': download failed: $url_base/$name"; return 1; }
        actual="$(sha256sum "$staging/$name" | cut -d' ' -f1)"
        if [ "$actual" != "$hash" ]; then
            rm -f "$staging/$name"
            log_error "corpus '$corpus_id': asset '$name' sha256 $actual != bundle pin $hash"
            log_error "Transport corrupted or the release is inconsistent with the committed bundle.json — aborting this corpus."
            return 1
        fi
    done < <(jq -r '.assets[] | "\(.name)\t\(.sha256)"' "$bundle_json")

    # Reassemble the database parts (lexicographic part order is
    # reassembly order — the export's zero-padded suffixes guarantee it).
    local database="$home/$source_id.db"
    : > "$database"
    local part_count=0
    for part in $(jq -r '.assets[].name' "$bundle_json" | grep "^${source_id}\\.db\\.part-" | sort); do
        cat "$staging/$part" >> "$database"
        part_count=$((part_count + 1))
    done
    if [ "$part_count" -eq 0 ]; then
        log_error "corpus '$corpus_id': bundle declares no '${source_id}.db.part-NNN' database parts"
        return 1
    fi

    # Place the metadata assets and the git-pinned manifest.
    cp "$staging/run-identity.json" "$home/run-identity.json"
    cp "$staging/representations-manifest.json" "$home/representations-manifest.json"
    cp "$bundle_json" "$home/bundle.json"

    # The receipt records the verified asset hashes in bundle order and the
    # materialized database path — the provenance chain admission compares.
    jq -n \
        --arg database "$database" \
        --slurpfile bundle "$bundle_json" \
        '{schema_version: 1,
          asset_sha256: $bundle[0].assets | map(.sha256),
          database_path: $database}' > "$home/receipt.json"

    # Register for federated recall: replace any entry with the same id,
    # preserve every other source (the safe jq merge form — no select-as
    # binding, which silently drops unpatched entries). `manifest` was
    # bound by the up-to-date check above.
    local spec
    spec="$(jq -n \
        --arg id "$source_id" \
        --arg name "$display_name" \
        --arg database "$database" \
        --arg identity "$home/run-identity.json" \
        --arg representations "$home/representations-manifest.json" \
        --arg bundle "$home/bundle.json" \
        --arg receipt "$home/receipt.json" \
        '{id: $id,
          display_name: $name,
          database_path: $database,
          run_identity_path: $identity,
          representations_manifest_path: $representations,
          index_name: "fine",
          materialized_provenance: {bundle_manifest_path: $bundle, receipt_path: $receipt}}')"
    if [ -f "$manifest" ]; then
        jq --argjson new "$spec" \
            '.schema_version = (.schema_version // 1) | .sources = ((.sources // []) | map(select(.id != $new.id)) + [$new])' \
            "$manifest" > "$manifest.tmp" && mv "$manifest.tmp" "$manifest"
    else
        jq -n --argjson new "$spec" '{schema_version: 1, sources: [$new]}' > "$manifest"
    fi

    # Verify the registration invariant itself, not the shape: the corpus
    # is present exactly once and the manifest parses.
    jq -e --arg id "$source_id" \
        '(.sources | map(select(.id == $id)) | length) == 1' \
        "$manifest" >/dev/null \
        || { log_error "corpus '$corpus_id': registration merge failed verification"; return 1; }

    rm -rf "$staging"
    log_success "corpus '$corpus_id' installed and registered for federated recall ($part_count database parts)"
    log "It appears as a federated recall option after the next curator-server launch."
    return 0
}

install_corpus_bundles() {
    local registry="$HKASK_SOURCE_DIR/$CORPUS_REGISTRY_RELATIVE"
    if [ ! -d "$registry" ]; then
        log "No corpus registry at $registry — nothing to install"
        return 0
    fi
    local bundle_json corpus_id failed=0
    while IFS= read -r bundle_json; do
        corpus_id="$(basename "$(dirname "$bundle_json")")"
        if ! install_corpus_bundle "$corpus_id" "$bundle_json"; then
            failed=$((failed + 1))
        fi
    done < <(find "$registry" -name bundle.json | sort)
    if [ "$failed" -gt 0 ]; then
        log_error "$failed corpus bundle(s) failed to install — re-run the installer to retry (verified assets are not re-downloaded)"
        return 1
    fi
    return 0
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

    # The NoDisplay .desktop entry is what GNOME actually uses to bind the
    # window's app_id to the icon (see install_desktop_entry). Without it the
    # taskbar shows a generic icon even though the hicolor icon is present.
    local installed_desktop="$icon_data_root/applications/dev.zed-kask.Zed-Kask.desktop"
    if [ ! -s "$installed_desktop" ]; then
        log_error "Desktop entry not found or empty at $installed_desktop"
        return 1
    fi
    log "Desktop entry: $installed_desktop"

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

    local elan_home="${ELAN_HOME:-$HOME/.elan}"
    if [ ! -x "$elan_home/bin/lake" ]; then
        log_error "Pinned Lean/Lake toolchain launcher missing: $elan_home/bin/lake"
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
# uninstall_hkask lives in install-common.sh (shared with the regression
# test kask/scripts/build/check-uninstall-paths.sh, which sources
# install-common.sh to exercise it). The --uninstall dispatch in main() below
# calls it; install.sh itself is not sourceable (it runs `main "$@"` at the
# bottom), so the function had to move to install-common.sh to be testable.

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
    --fast              Build on the release-fast profile (parity flags, no LTO)
    --system            Install system-wide (symlink in /usr/local/bin)
    --skip-deps         Skip system dependency installation
    --skip-rust         Skip Rust installation
    --skip-corpora      Skip installing the corpus bundles (reference libraries)
    --install-dir DIR   Install to custom directory (default: \$HOME/.local)
    --help              Show this help message

Environment Variables:
    HKASK_VERSION         Tag to install (default: derived from workspace
                          Cargo.toml version, or 0.40.1 if unreadable)
    HKASK_BUILD_TYPE      release or release-fast (default: release)
    HKASK_SOURCE_DIR      Use existing source directory instead of cloning
    HKASK_REPO_URL        Git repository URL
    HKASK_ALLOW_FALLBACK  Allow silent fallback to main if tag missing (default: false)
    INSTALL_DIR           Installation directory (default: $HOME/.local)
    HKASK_SYSTEM_INSTALL  Force system-wide install (default: false)
    HKASK_REMOVE_CONFIG   Remove config and data on uninstall (default: false)
    HKASK_CORPUS_URL_BASE Release URL base for corpus bundles
                          (default: the zed-kask GitHub releases; each corpus
                          reads from its corpus-<id> release tag)

Examples:
    # Install hKask (latest release tag)
    curl -fsSL https://raw.githubusercontent.com/mdz-axo/zed-kask/main/kask/scripts/build/install.sh | bash

    # Quick rebuild from an existing checkout (parity flags, no LTO)
    bash kask/scripts/build/install.sh --fast --skip-deps

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
    local skip_corpora=false
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
            --fast)
                HKASK_BUILD_TYPE="release-fast"
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
            --skip-zk-ref)
                # Legacy spelling of --skip-corpora from an unreleased session —
                # accept both so in-flight operator muscle memory works.
                skip_corpora=true
                shift
                ;;
            --skip-corpora)
                skip_corpora=true
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

    # Dev-profile installs are removed (2026-10-02 incident, two-symptom
    # findings §7.9): the installer produces release-class binaries only.
    # --fast is the quick-rebuild path; a dev-profile build is a plain
    # `cargo build` in the repo, not an install.
    case "${HKASK_BUILD_TYPE:-release}" in
        release | release-fast) ;;
        *)
            log_error "HKASK_BUILD_TYPE must be 'release' or 'release-fast' (got '${HKASK_BUILD_TYPE}')"
            log_error "Dev-profile installs are removed — use --fast for quick rebuilds (parity flags, full debug symbols)."
            exit 1
            ;;
    esac

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
            install_lean_toolchain || return 1
            prepare_install_dir
            install_binary
            install_icon
            install_desktop_entry
            setup_environment
            write_mcp_server_settings

            if [ "$skip_corpora" = false ]; then
                install_corpus_bundles
            else
                log "Skipping corpus bundle installation (--skip-corpora)"
            fi

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
            # --uninstall is exempt from assert_not_zed_contaminated_env.
            # That guard prevents a *build/install* from linking zed-kask against
            # upstream Zed's bundled libraries (LD_LIBRARY_PATH / ancestor
            # zed-editor detection). Uninstall does no compilation or linking —
            # it only removes files (rm/sed/jq), so there is nothing to couple.
            # Applying the guard here would block the user from uninstalling
            # zed-kask from Zed's own integrated terminal — the terminal they
            # are most likely sitting in when they decide to remove it —
            # turning a safe, file-only operation into a hard failure.
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
            install_lean_toolchain || return 1
            ;;
    esac
}

main "$@"
