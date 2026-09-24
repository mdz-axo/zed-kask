#!/bin/bash
# Offline contract for the shared Lean dependency and its installer call sites.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
source "$here/install-common.sh"

scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
export ELAN_HOME="$scratch/elan"
export ELAN_TEST_LOG="$scratch/calls"
mkdir -p "$ELAN_HOME/bin"
cat > "$ELAN_HOME/bin/elan" <<'ELAN'
#!/bin/bash
printf '%s\n' "$*" >> "$ELAN_TEST_LOG"
case "$1 $2 $3" in
    'toolchain install leanprover/lean4:v4.34.0') exit 0 ;;
    'run leanprover/lean4:v4.34.0 lean') printf '%s\n' 'Lean (version 4.34.0, test, Release)'; exit 0 ;;
    'run leanprover/lean4:v4.34.0 lake') printf '%s\n' 'Lake version test (Lean version 4.34.0)'; exit 0 ;;
esac
exit 1
ELAN
chmod 700 "$ELAN_HOME/bin/elan"
install_lean_toolchain
install_lean_toolchain
[ "$(wc -l < "$ELAN_TEST_LOG")" -eq 6 ]
[ "$(grep -c '^toolchain install leanprover/lean4:v4.34.0$' "$ELAN_TEST_LOG")" -eq 2 ]

ELAN_HOME='relative/elan' && if install_lean_toolchain >/dev/null 2>&1; then
    echo 'relative ELAN_HOME was accepted' >&2
    exit 1
fi
ELAN_HOME="$scratch/empty"
curl() {
    # A successful transport carrying corrupted bytes must fail checksum
    # before any downloaded executable is run.
    local previous='' arg
    for arg in "$@"; do
        if [ "$previous" = '-o' ]; then printf 'not an archive' > "$arg"; return 0; fi
        previous="$arg"
    done
    return 1
}
if install_lean_toolchain >/dev/null 2>&1; then
    echo 'corrupted Elan archive was accepted' >&2
    exit 1
fi
[ ! -e "$ELAN_HOME/bin/elan" ]

bash -n "$here/install-common.sh" "$here/install.sh" "$here/install-binary.sh"
[ "$(grep -c '^[[:space:]]*install_lean_toolchain || return 1$' "$here/install.sh")" -eq 2 ]
[ "$(grep -c '^[[:space:]]*install_lean_toolchain || return 1$' "$here/install-binary.sh")" -eq 1 ]

# Call the binary installer's real main with a failed provisioning stage.
# A failed dependency must leave the already-installed editor untouched even
# when main is called from a conditional (where Bash disables `set -e`).
(
    source "$here/install-binary.sh"
    detect_target() { printf '%s\n' x86_64-unknown-linux-gnu; }
    resolve_tag() { printf '%s\n' v0.40.0; }
    download_and_extract() { mkdir -p "$scratch/download"; printf '%s\n' "$scratch/download"; }
    install_lean_toolchain() { printf '%s\n' provision >> "$scratch/order"; return 1; }
    install_binaries() { printf '%s\n' replaced >> "$scratch/order"; }
    if main > "$scratch/binary-install.log" 2>&1; then
        echo 'binary installer accepted a failed Lean provision' >&2
        exit 1
    fi
    [ "$(cat "$scratch/order")" = provision ] || {
        echo 'binary installer replaced the editor after Lean provision failed' >&2
        exit 1
    }
)
[ ! -e "$scratch/download" ] || { echo 'binary installer left a staging directory' >&2; exit 1; }
printf '%s\n' 'Lean installer contract: cached setup, invalid root, checksum gate and binary-update failure gate passed'
