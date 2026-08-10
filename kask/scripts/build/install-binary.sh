#!/bin/bash
# zed-kask binary installer. Downloads only a verified flat zed-kask archive.

set -euo pipefail

_HKASK_INSTALL_DIR="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" && pwd)"
# shellcheck source=install-common.sh
source "$_HKASK_INSTALL_DIR/install-common.sh"

HKASK_REPO="${HKASK_REPO:-mdz-axo/zed-kask}"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local}"
BIN_DIR="${INSTALL_DIR}/bin"
UPDATER_DIR="${INSTALL_DIR}/share/zed-kask/install"

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
    local url="$1" output_path="$2"
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL -o "$output_path" "$url"
    elif command -v wget >/dev/null 2>&1; then
        wget -qO "$output_path" "$url"
    else
        log_error "Neither curl nor wget is available"
        return 1
    fi
}

detect_target() {
    case "$(uname -s)-$(uname -m)" in
        Linux-x86_64) echo "x86_64-unknown-linux-gnu" ;;
        *) log_error "Prebuilt zed-kask releases support only Linux x86_64"; return 1 ;;
    esac
}

resolve_tag() {
    if [ -n "${HKASK_VERSION:-}" ]; then
        [ "$HKASK_VERSION" = "weekly" ] && { echo weekly; return; }
        echo "v${HKASK_VERSION#v}"
        return
    fi
    [ "${HKASK_CHANNEL:-stable}" = weekly ] && { echo weekly; return; }
    http_get "https://api.github.com/repos/${HKASK_REPO}/releases/latest" \
        | grep -m1 '"tag_name"' \
        | sed -E 's/.*"tag_name"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/'
}

verify_checksum() {
    local directory="$1" archive="$2" tag="$3"
    local sums_url="https://github.com/${HKASK_REPO}/releases/download/${tag}/SHA256SUMS"
    http_download "$sums_url" "$directory/SHA256SUMS" || {
        log_error "SHA256SUMS is required for every zed-kask update"
        return 1
    }

    local checksum_line
    checksum_line=$(awk -v archive="$archive" '$2 == archive || $2 == "./" archive { print; exit }' "$directory/SHA256SUMS")
    if [ -z "$checksum_line" ]; then
        log_error "SHA256SUMS has no checksum for $archive"
        return 1
    fi
    printf '%s\n' "$checksum_line" | (cd "$directory" && sha256sum -c -)
}

validate_archive() {
    local archive_path="$1"
    command -v tar >/dev/null 2>&1 || { log_error "tar is required"; return 1; }

    local listing
    listing=$(tar -tzf "$archive_path") || return 1
    local entry normalized
    while IFS= read -r entry; do
        normalized="${entry#./}"
        case "$normalized" in
            '' ) continue ;;
            zed|zed.app|zed.app/*|*/zed|*/zed.app|*/zed.app/*|/*|../*|*/../*|*/..)
                log_error "archive contains a forbidden path: $entry"
                return 1
                ;;
            */*)
                log_error "archive contains a nested path: $entry"
                return 1
                ;;
            zed-kask|hkask-mcp-*)
                ;;
            *)
                log_error "archive contains an unexpected file: $entry"
                return 1
                ;;
        esac
    done <<< "$listing"

    local verbose
    verbose=$(tar -tvzf "$archive_path") || return 1
    while IFS= read -r entry; do
        case "${entry:0:1}" in
            -) ;;
            d)
                case "$entry" in
                    *' ./'|*' .') ;;
                    *)
                        log_error "archive contains an unexpected directory: $entry"
                        return 1
                        ;;
                esac
                ;;
            *)
                log_error "archive contains a non-regular entry: $entry"
                return 1
                ;;
        esac
    done <<< "$verbose"
}

download_and_extract() {
    local target="$1" tag="$2" archive="zed-kask-${target}.tar.gz"
    local temp_dir
    temp_dir=$(mktemp -d)
    printf '%s\n' "$temp_dir"

    log "Downloading $archive from $tag..." >&2
    http_download "https://github.com/${HKASK_REPO}/releases/download/${tag}/${archive}" "$temp_dir/$archive"
    verify_checksum "$temp_dir" "$archive" "$tag"
    validate_archive "$temp_dir/$archive"
    mkdir "$temp_dir/extracted"
    tar -xzf "$temp_dir/$archive" -C "$temp_dir/extracted" --no-same-owner --no-same-permissions
}

install_updater_bundle() {
    assert_not_zed_owned_path "$UPDATER_DIR" "updater installation" || return 1
    mkdir -p "$UPDATER_DIR"
    local file
    for file in install-binary.sh install-common.sh update-zed-kask.sh mcp-servers.txt; do
        if [ ! -f "$_HKASK_INSTALL_DIR/$file" ]; then
            log_error "updater bundle source is missing: $_HKASK_INSTALL_DIR/$file"
            return 1
        fi
        cp "$_HKASK_INSTALL_DIR/$file" "$UPDATER_DIR/$file.tmp"
        mv -f "$UPDATER_DIR/$file.tmp" "$UPDATER_DIR/$file"
    done
    chmod 755 "$UPDATER_DIR/install-binary.sh" "$UPDATER_DIR/update-zed-kask.sh"
}

move_staged_binary() {
    mv -f -- "$1" "$2"
}

install_binaries() {
    local staging="$1"
    assert_not_zed_owned_path "$BIN_DIR" "binary installation" || return 1
    assert_kask_binary_destination "$BIN_DIR/zed-kask" || return 1

    local -a binaries=(zed-kask "${MCP_SERVERS[@]}")
    local binary
    for binary in "${binaries[@]}"; do
        assert_kask_binary_destination "$BIN_DIR/$binary" || return 1
        if [ ! -f "$staging/$binary" ]; then
            log_error "required release binary is missing: $binary"
            return 1
        fi
    done

    mkdir -p "$BIN_DIR"
    local transaction_dir
    transaction_dir=$(mktemp -d "$BIN_DIR/.zed-kask-update.XXXXXX") || return 1

    for binary in "${binaries[@]}"; do
        if [ -d "$BIN_DIR/$binary" ]; then
            log_error "binary replacement refused: destination is a directory: $BIN_DIR/$binary"
            rm -rf "$transaction_dir"
            return 1
        fi
        if ! cp "$staging/$binary" "$transaction_dir/$binary.new" \
            || ! chmod 755 "$transaction_dir/$binary.new"; then
            log_error "could not stage replacement binary: $binary"
            rm -rf "$transaction_dir"
            return 1
        fi
        if [ -e "$BIN_DIR/$binary" ] && ! cp -p "$BIN_DIR/$binary" "$transaction_dir/$binary.old"; then
            log_error "could not back up installed binary: $binary"
            rm -rf "$transaction_dir"
            return 1
        fi
    done

    install_updater_bundle || {
        rm -rf "$transaction_dir"
        return 1
    }

    local -a replaced=()
    for binary in "${binaries[@]}"; do
        if ! move_staged_binary "$transaction_dir/$binary.new" "$BIN_DIR/$binary"; then
            log_error "binary replacement failed for $binary; restoring the prior installation"
            local restored
            for restored in "${replaced[@]}"; do
                if [ -f "$transaction_dir/$restored.old" ]; then
                    mv -f "$transaction_dir/$restored.old" "$BIN_DIR/$restored" || log_error "could not restore $restored"
                else
                    rm -f "$BIN_DIR/$restored" || log_error "could not remove partial $restored"
                fi
            done
            rm -rf "$transaction_dir"
            return 1
        fi
        replaced+=("$binary")
    done

    rm -rf "$transaction_dir"
    log_success "Installed zed-kask and ${#MCP_SERVERS[@]} MCP servers to $BIN_DIR"
}

verify_installation() {
    [ -x "$BIN_DIR/zed-kask" ] || { log_error "zed-kask was not installed"; return 1; }
    [ -x "$UPDATER_DIR/update-zed-kask.sh" ] || { log_error "safe updater was not installed"; return 1; }
}

main() {
    print_banner "Verified Binary Installer"
    local target tag temporary_directory
    target=$(detect_target)
    tag=$(resolve_tag)
    [ -n "$tag" ] || { log_error "Could not determine a zed-kask release tag"; exit 1; }
    temporary_directory=$(download_and_extract "$target" "$tag") || exit 1
    trap 'rm -rf "$temporary_directory"' EXIT
    install_binaries "$temporary_directory/extracted"
    add_to_path
    write_mcp_server_settings
    verify_installation
    log_success "Installation complete"
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
    main "$@"
fi
