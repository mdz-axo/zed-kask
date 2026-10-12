#!/bin/bash
# Conformance gate: install/build best practices.
#
# Anchored to kask/docs/research/install-build-ci-reference-models.md —
# the published practices (Google Shell Style Guide, ShellCheck, rustup's
# installer design, curl's documented resume behavior, the Cargo Book's CI
# guidance) behind the operator's 2026-10-11 requirement: the installer
# must check presence and status of large assets before doing expensive
# work, and that conformance must be checked, tested, and pinned — so
# waste like re-downloading a multi-GB corpus the machine already holds
# is a class of error the tree guards against, not a one-off patch.
#
# Two sections:
#   A. Static conformance — ShellCheck clean, explicit fail-fast stances,
#      --locked cargo builds, pinned-version sync (the installer's
#      hardcoded HKASK_VERSION default == the workspace version), no
#      silent main-branch fallback, non-fatal corpus dispatch, and
#      registry integrity (every committed bundle.json is well-formed).
#   B. Behavioral corpus suite — exercises the REAL install_corpus_bundle
#      (install-common.sh) against a file:// fixture release: fresh
#      install, idempotent same-run skip, different-run re-download,
#      partial resume (curl -C - observed through a recording wrapper),
#      corrupt-partial recovery, transport failure (partial kept, the
#      working registration preserved), and atomic assembly (an injected
#      cat failure preserves the registered database).
#
# Overrides for the selftest (check-install-best-practices-selftest.sh):
#   IBP_SCRIPTS_DIR       scripts dir under test (default: this dir)
#   IBP_REPO_ROOT         repo root for Cargo.toml + registry (default: ../../..)
#   IBP_SKIP_BEHAVIORAL=1 skip section B (static-mutation probes)
#
# Run locally: bash kask/scripts/build/check-install-best-practices.sh

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
IBP_SCRIPTS_DIR="${IBP_SCRIPTS_DIR:-$script_dir}"
IBP_REPO_ROOT="${IBP_REPO_ROOT:-$(cd "$script_dir/../../.." && pwd)}"

errors=0
fail() { echo "FAIL: $1" >&2; errors=$((errors + 1)); }

# ============================================================================
# A. Static conformance
# ============================================================================

# A1. ShellCheck clean at warning severity over every build script — the
# published enforcement instrument for shell practice.
if command -v shellcheck >/dev/null 2>&1; then
    sc_out="$(shellcheck -S warning "$IBP_SCRIPTS_DIR"/*.sh 2>&1)" || true
    if [ -n "$sc_out" ]; then
        fail "shellcheck findings (warning+) under $IBP_SCRIPTS_DIR:"
        printf '%s\n' "$sc_out" | head -30 >&2
    fi
else
    fail "shellcheck not found on PATH — required by this gate"
fi

# A2. Every build script carries an explicit fail-fast stance, derived from
# the tree (not a filename list):
#   - `set -euo pipefail` (the default discipline), or
#   - `set -uo pipefail` with an adjacent NOT-set-e justification comment
#     (a sampler must survive bad samples — the documented deviation), or
#   - the HKASK_INSTALL_COMMON_LOADED double-source guard (a sourced
#     library governed by the sourcing script's errexit).
for f in "$IBP_SCRIPTS_DIR"/*.sh; do
    base="$(basename "$f")"
    if grep -q '^set -euo pipefail' "$f"; then
        continue
    fi
    if grep -q '^set -uo pipefail' "$f" && grep -q 'NOT set -e' "$f"; then
        continue
    fi
    if grep -q 'HKASK_INSTALL_COMMON_LOADED' "$f"; then
        continue
    fi
    fail "$base carries no explicit fail-fast stance (set -euo pipefail, a commented set -uo pipefail deviation, or the sourced-library guard)"
done

# A3. Every cargo build invocation in install.sh is --locked — the Cargo
# Book's reproducibility practice: a stale Cargo.lock fails loudly instead
# of silently re-resolving.
total_builds="$(grep -cE '^[[:space:]]*cargo build' "$IBP_SCRIPTS_DIR/install.sh" || true)"
locked_builds="$(grep -cE '^[[:space:]]*cargo build --locked' "$IBP_SCRIPTS_DIR/install.sh" || true)"
if [ "$total_builds" != "$locked_builds" ]; then
    fail "install.sh: $total_builds cargo build invocation(s), only $locked_builds carry --locked (reproducible-build practice)"
fi

# A4. Pinned-version sync: the installer's hardcoded HKASK_VERSION fallback
# (the version the curl|bash path pins when Cargo.toml is unreadable) must
# equal the workspace version — otherwise every release silently ships a
# stale default until someone remembers to edit it.
ws_version="$(awk -F'"' '/^version *=* "/{print $2; exit}' "$IBP_REPO_ROOT/Cargo.toml" || true)"
default_literal="$(grep -oE 'HKASK_VERSION="[0-9]+\.[0-9]+\.[0-9]+"' "$IBP_SCRIPTS_DIR/install.sh" | head -1 || true)"
installer_default="${default_literal#HKASK_VERSION=\"}"
installer_default="${installer_default%\"}"
if [ -z "$ws_version" ] || [ -z "$installer_default" ]; then
    fail "version sync unreadable: workspace='$ws_version' installer default='$installer_default'"
elif [ "$ws_version" != "$installer_default" ]; then
    fail "install.sh: hardcoded HKASK_VERSION default ($installer_default) != workspace version ($ws_version) — the curl|bash path pins this default; keep them in sync"
fi

# A5. No silent fallback to a floating branch: the main-branch fallback must
# default to false (two users running the same HKASK_VERSION must get the
# same binaries).
if ! grep -q 'HKASK_ALLOW_FALLBACK:-false' "$IBP_SCRIPTS_DIR/install.sh"; then
    fail "install.sh: the main-branch fallback must default to false (HKASK_ALLOW_FALLBACK:-false) — reproducibility"
fi

# A6. Non-fatal corpus dispatch: a corpus failure is loud but NOT fatal to
# the product install (the stage's documented contract) — the editor and
# MCP servers are already in place when it runs.
if ! grep -q 'if ! install_corpus_bundles' "$IBP_SCRIPTS_DIR/install.sh"; then
    fail "install.sh: install_corpus_bundles must be dispatched under 'if !' — a corpus failure is loud but not fatal to the product install (documented contract)"
fi

# A7. Registry integrity: every committed bundle.json is well-formed —
# required fields, safe asset names, 64-hex sha256 pins, at least one
# database part, and exactly one of each metadata asset. The installer
# trusts these manifests enough to fetch gigabytes against them.
for bundle in "$IBP_REPO_ROOT"/kask/registry/*/bundle.json; do
    [ -f "$bundle" ] || continue
    for field in source_id display_name bundle_key run_id; do
        if [ -z "$(jq -r ".$field // empty" "$bundle" 2>/dev/null)" ]; then
            fail "$bundle: missing/empty $field"
        fi
    done
    sid="$(jq -r '.source_id' "$bundle" 2>/dev/null || true)"
    if [ "$(jq '.assets | length' "$bundle" 2>/dev/null || echo 0)" -eq 0 ]; then
        fail "$bundle: no assets"
        continue
    fi
    if ! jq -e '([.assets[] | select((.name | test("^[A-Za-z0-9._-]+$")) and (.sha256 | test("^[a-f0-9]{64}$")))] | length) == (.assets | length)' \
        "$bundle" >/dev/null 2>&1; then
        fail "$bundle: every asset needs a safe name (^[A-Za-z0-9._-]+$) and a 64-hex sha256"
    fi
    if ! jq -e --arg sid "$sid" \
        '([.assets[] | select(.name | startswith($sid + ".db.part-"))] | length) >= 1' \
        "$bundle" >/dev/null 2>&1; then
        fail "$bundle: no '${sid}.db.part-*' database part assets"
    fi
    for meta in run-identity.json representations-manifest.json; do
        if ! jq -e --arg m "$meta" '([.assets[] | select(.name == $m)] | length) == 1' \
            "$bundle" >/dev/null 2>&1; then
            fail "$bundle: needs exactly one '$meta' metadata asset"
        fi
    done
done

# ============================================================================
# B. Behavioral corpus suite
# ============================================================================
#
# Exercises the real install_corpus_bundle against a file:// "release":
# curl handles file:// with -C - resume and fails loudly (exit 37) on a
# missing file, so the suite needs no network and no HTTP server.

if [ "${IBP_SKIP_BEHAVIORAL:-0}" = "1" ]; then
    if [ "$errors" -gt 0 ]; then
        echo "REGRESSION: $errors install-best-practice violation(s) detected (static section)." >&2
        exit 1
    fi
    echo "PASS: install/build scripts conform (static section; behavioral skipped)."
    exit 0
fi

sandbox="$(mktemp -d)"
trap 'rm -rf "$sandbox"' EXIT

export HOME="$sandbox/home"
export XDG_DATA_HOME="$sandbox/home/.local/share"
mkdir -p "$HOME" "$XDG_DATA_HOME"
export MCP_SERVERS_LIST_FILE="$IBP_SCRIPTS_DIR/mcp-servers.txt"

# shellcheck source=install-common.sh
source "$IBP_SCRIPTS_DIR/install-common.sh"

CID="ibp-fixture"
SID="ibp-fixture"
RUN_A="aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
RUN_B="bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
RUN_C="cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"

release="$sandbox/release/corpus-$CID"
repo="$sandbox/repo"
mkdir -p "$release" "$repo/kask/registry/$CID"

part0="$release/$SID.db.part-000"
part1="$release/$SID.db.part-001"
head -c 262144 /dev/urandom > "$part0"
head -c 262144 /dev/urandom > "$part1"
cp "$part0" "$sandbox/part0.pristine"
h0="$(sha256sum "$part0" | cut -d' ' -f1)"
h1="$(sha256sum "$part1" | cut -d' ' -f1)"
printf '{"run_id":"%s"}\n' "$RUN_A" > "$release/run-identity.json"
printf '{"schema_version":1,"index":"fine"}\n' > "$release/representations-manifest.json"

bundle="$repo/kask/registry/$CID/bundle.json"
write_bundle_run() { # <run-id> — (re)write the fixture bundle for a run
    local run="$1"
    printf '{"run_id":"%s"}\n' "$run" > "$release/run-identity.json"
    local hi hr
    hi="$(sha256sum "$release/run-identity.json" | cut -d' ' -f1)"
    hr="$(sha256sum "$release/representations-manifest.json" | cut -d' ' -f1)"
    jq -n --arg sid "$SID" --arg run "$run" --arg h0 "$h0" --arg h1 "$h1" \
        --arg hi "$hi" --arg hr "$hr" \
        '{source_id: $sid, display_name: "IBP fixture", bundle_key: "ibp-fixture-key", run_id: $run,
          assets: [
            {name: ($sid + ".db.part-000"), sha256: $h0},
            {name: ($sid + ".db.part-001"), sha256: $h1},
            {name: "run-identity.json", sha256: $hi},
            {name: "representations-manifest.json", sha256: $hr}
          ]}' > "$bundle"
}
write_bundle_run "$RUN_A"

export HKASK_CORPUS_URL_BASE="file://$sandbox/release"
export HKASK_SOURCE_DIR="$repo"

corpus_home="$XDG_DATA_HOME/zed-kask/agents/curator/$CID"
db="$corpus_home/$SID.db"
staging="$corpus_home/.staging"
# Deliberate resolution order: B7 below defines a cat() fault-injection
# mock (define, inject one failure, unset -f cat) AFTER this top-level call,
# so this cat runs the real binary by design — a same-named function
# existing later is the injection mechanism, not a use-before-definition
# bug. The mock definition carries its own SC2329 disable.
# shellcheck disable=SC2218
cat "$part0" "$part1" > "$sandbox/expected.db"

# registered_run_of <source-id> — the registered run_id, or empty. Guarded:
# an unregistered/missing identity must produce a reportable failure, not
# abort the suite mid-case.
registered_run_of() {
    local ident
    ident="$(corpus_registered_field "$1" run_identity_path)"
    if [ -n "$ident" ] && [ -f "$ident" ]; then
        jq -r '.run_id // empty' "$ident" 2>/dev/null || true
    fi
}

# --- B1. Fresh install: downloads, verifies, assembles, registers ---------
set +e
out="$(install_corpus_bundle "$CID" "$bundle" 2>&1)"
rc=$?
set -e
if [ "$rc" -ne 0 ]; then
    fail "B1 fresh install: rc=$rc"
    printf '%s\n' "$out" | tail -5 >&2
fi
if [ ! -s "$db" ]; then
    fail "B1: database not materialized at $db"
fi
if ! cmp -s "$db" "$sandbox/expected.db"; then
    fail "B1: assembled database != concatenated parts"
fi
if [ ! -f "$corpus_home/receipt.json" ]; then
    fail "B1: receipt missing"
fi
if [ -d "$staging" ]; then
    fail "B1: staging not cleaned after success"
fi
reg_db="$(corpus_registered_field "$SID" database_path)"
if [ "$reg_db" != "$db" ]; then
    fail "B1: registration database_path mismatch ($reg_db)"
fi
reg_run="$(registered_run_of "$SID")"
if [ "$reg_run" != "$RUN_A" ]; then
    fail "B1: registered run identity != bundle run"
fi

# --- B2. Idempotence: same run → skip, zero transfers, DB untouched --------
db_mtime_before="$(stat -c%Y "$db")"
set +e
out="$(install_corpus_bundle "$CID" "$bundle" 2>&1)"
rc=$?
set -e
if [ "$rc" -ne 0 ]; then
    fail "B2 idempotent skip: rc=$rc"
fi
if ! grep -q "already up to date" <<<"$out"; then
    fail "B2: no 'already up to date' skip message — the presence/status gate is gone or broken"
fi
if [ "$(stat -c%Y "$db")" != "$db_mtime_before" ]; then
    fail "B2: database was rewritten on an up-to-date skip"
fi

# --- B3. Different run: the gate must NOT over-skip — new content fetches -
write_bundle_run "$RUN_B"
set +e
out="$(install_corpus_bundle "$CID" "$bundle" 2>&1)"
rc=$?
set -e
if [ "$rc" -ne 0 ]; then
    fail "B3 different-run install: rc=$rc"
    printf '%s\n' "$out" | tail -5 >&2
fi
if grep -q "already up to date" <<<"$out"; then
    fail "B3: skipped a DIFFERENT run — the gate over-skips"
fi
reg_run="$(registered_run_of "$SID")"
if [ "$reg_run" != "$RUN_B" ]; then
    fail "B3: registration not updated to the new run"
fi

# --- B4. Resume: a partial part continues via curl -C - --------------------
# (Removing the DB also pins the presence half of the gate: a registration
# whose files are missing falls through to download, not skip.)
rm -f "$db"
mkdir -p "$staging"
head -c 1000 "$part0" > "$staging/$SID.db.part-000"
rm -f "$staging/$SID.db.part-001" "$staging/run-identity.json" "$staging/representations-manifest.json"
curl_log="$sandbox/curl-invocations.txt"
: > "$curl_log"
# shellcheck disable=SC2329
curl() {
    printf '%s\n' "$*" >> "$curl_log"
    command curl "$@"
}
set +e
out="$(install_corpus_bundle "$CID" "$bundle" 2>&1)"
rc=$?
set -e
unset -f curl
if [ "$rc" -ne 0 ]; then
    fail "B4 resume install: rc=$rc"
    printf '%s\n' "$out" | tail -5 >&2
fi
if ! grep -q "resuming partial download" <<<"$out"; then
    fail "B4: no resume notice for the staged partial"
fi
if ! grep -q -- "-C -" "$curl_log"; then
    fail "B4: the part download did not carry curl -C - (no physical resume)"
fi
if ! cmp -s "$db" "$sandbox/expected.db"; then
    fail "B4: resumed assembly != concatenated parts"
fi

# --- B5. Corrupt partial: hash failure → one clean re-download recovers ----
rm -f "$db"
mkdir -p "$staging"
head -c 1000 /dev/urandom > "$staging/$SID.db.part-000" # NOT a prefix of part0
rm -f "$staging/$SID.db.part-001" "$staging/run-identity.json" "$staging/representations-manifest.json"
set +e
out="$(install_corpus_bundle "$CID" "$bundle" 2>&1)"
rc=$?
set -e
if [ "$rc" -ne 0 ]; then
    fail "B5 corrupt-partial recovery: rc=$rc"
    printf '%s\n' "$out" | tail -5 >&2
fi
if ! grep -q "re-downloading from scratch" <<<"$out"; then
    fail "B5: no clean re-download after a hash failure"
fi
if ! cmp -s "$db" "$sandbox/expected.db"; then
    fail "B5: recovered database != concatenated parts"
fi

# --- B6. Transport failure: partial kept, working registration preserved ---
write_bundle_run "$RUN_C"
rm -f "$release/$SID.db.part-000" # the remote part goes missing
mkdir -p "$staging"
head -c 1000 "$sandbox/part0.pristine" > "$staging/$SID.db.part-000"
rm -f "$staging/$SID.db.part-001" "$staging/run-identity.json" "$staging/representations-manifest.json"
good_hash="$(sha256sum "$db" | cut -d' ' -f1)"
set +e
out="$(install_corpus_bundle "$CID" "$bundle" 2>&1)"
rc=$?
set -e
if [ "$rc" -eq 0 ]; then
    fail "B6: install succeeded despite a missing remote part"
fi
if ! grep -q "partial kept in staging for resume" <<<"$out"; then
    fail "B6: transport failure did not keep the partial for resume"
fi
if [ ! -f "$staging/$SID.db.part-000" ] \
    || [ "$(stat -c%s "$staging/$SID.db.part-000")" != "1000" ]; then
    fail "B6: the partial was not preserved at its interrupted size"
fi
if [ "$(sha256sum "$db" | cut -d' ' -f1)" != "$good_hash" ]; then
    fail "B6: the registered database was disturbed by a failed install"
fi
reg_run="$(registered_run_of "$SID")"
if [ "$reg_run" != "$RUN_B" ]; then
    fail "B6: registration was replaced by a failed install"
fi

# --- B7. Atomic assembly: an injected cat failure preserves the DB -------
cp "$sandbox/part0.pristine" "$part0" # restore the remote
cat_fail_part="$staging/$SID.db.part-001"
# shellcheck disable=SC2329
cat() {
    if [ "$1" = "$cat_fail_part" ]; then
        echo "injected cat failure" >&2
        return 1
    fi
    command cat "$@"
}
set +e
out="$(install_corpus_bundle "$CID" "$bundle" 2>&1)"
rc=$?
set -e
unset -f cat
if [ "$rc" -eq 0 ]; then
    fail "B7: assembly succeeded despite an injected cat failure"
fi
if ! grep -q "failed assembling part" <<<"$out"; then
    fail "B7: assembly failure was not reported loudly"
fi
if [ "$(sha256sum "$db" | cut -d' ' -f1)" != "$good_hash" ]; then
    fail "B7: the registered database was truncated by a failed assembly"
fi
if [ -e "$db.assembling" ]; then
    fail "B7: the .assembling temp was left behind"
fi

if [ "$errors" -gt 0 ]; then
    echo "REGRESSION: $errors install-best-practice violation(s) detected." >&2
    exit 1
fi

echo "PASS: install/build scripts conform (static + behavioral corpus suite)."
