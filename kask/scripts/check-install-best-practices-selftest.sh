#!/usr/bin/env bash
# Self-test for the install-best-practices gate
# (build/check-install-best-practices.sh).
#
# Pins the gate's failure modes against a fixture tree (the gate's
# IBP_SCRIPTS_DIR / IBP_REPO_ROOT overrides): each static mutation must fail
# the gate BY NAME, a behavioral mutation (neutralizing the up-to-date
# gate) must fail the behavioral suite, and the pristine fixture must pass
# the full gate. A gate that passes a mutated fixture is vacuous.
#
# Exit codes: 0 = the gate is alive (every probe behaved as expected),
# 1 = the gate is vacuous or broken.
set -euo pipefail

KASK_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GATE="$KASK_ROOT/scripts/build/check-install-best-practices.sh"
[ -f "$GATE" ] || { echo "FAIL: gate not found at $GATE"; exit 1; }

fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT

# Pristine fixture: copies of the real build scripts + a minimal repo root
# (a Cargo.toml stub carrying the real workspace version, and the real
# registry bundle so the integrity walk has a well-formed subject).
pristine_scripts="$fixture/pristine-scripts"
mkdir -p "$pristine_scripts"
cp "$KASK_ROOT/scripts/build/"*.sh "$pristine_scripts/"
cp "$KASK_ROOT/scripts/build/mcp-servers.txt" "$pristine_scripts/"
pristine_repo="$fixture/pristine-repo"
mkdir -p "$pristine_repo/kask/registry"
real_version="$(awk -F'"' '/^version *=* "/{print $2; exit}' "$KASK_ROOT/../Cargo.toml")"
printf 'version = "%s"\n' "$real_version" > "$pristine_repo/Cargo.toml"
cp -r "$KASK_ROOT/registry/zk-ref-open" "$pristine_repo/kask/registry/"

failures=0

# run_static_probe <label> <mutation-sed...> <expected-failure-text>
# Copies the pristine scripts, applies the sed to the named file, runs the
# gate's static section, and asserts the gate fails naming the violation.
run_static_probe() {
    local label="$1" target="$2" mutation="$3" expected="$4"
    local work="$fixture/work-$label"
    mkdir -p "$work"
    cp "$pristine_scripts/"*.sh "$work/"
    cp "$pristine_scripts/mcp-servers.txt" "$work/"
    sed -i "$mutation" "$work/$target"
    set +e
    out="$(IBP_SCRIPTS_DIR="$work" IBP_REPO_ROOT="$pristine_repo" \
        IBP_SKIP_BEHAVIORAL=1 bash "$GATE" 2>&1)"
    rc=$?
    set -e
    if [ "$rc" -eq 0 ]; then
        echo "FAIL ($label): gate exited 0 on a mutated fixture"
        failures=$((failures + 1))
    elif ! grep -q "$expected" <<<"$out"; then
        echo "FAIL ($label): exit $rc but the violation was not identified (expected: $expected)"
        printf '%s\n' "$out" | tail -5
        failures=$((failures + 1))
    else
        echo "OK ($label): gate detected the violation"
    fi
}

# M1. A cargo build loses --locked.
run_static_probe "locked" install.sh \
    's/cargo build --locked --jobs/cargo build --jobs/' \
    "carry --locked"

# M2. The installer's hardcoded version default drifts from the workspace.
run_static_probe "version-sync" install.sh \
    's/HKASK_VERSION="[0-9.]*"/HKASK_VERSION="0.0.0"/' \
    "workspace version"

# M3. A committed bundle.json asset loses its 64-hex sha256.
work="$fixture/work-registry"
mkdir -p "$work"
cp "$pristine_scripts/"*.sh "$work/"
cp "$pristine_scripts/mcp-servers.txt" "$work/"
broken_repo="$fixture/broken-repo"
cp -r "$pristine_repo" "$broken_repo"
jq '(.assets[0]).sha256 = "nothex"' \
    "$broken_repo/kask/registry/zk-ref-open/bundle.json" \
    > "$broken_repo/kask/registry/zk-ref-open/bundle.json.tmp" \
    && mv "$broken_repo/kask/registry/zk-ref-open/bundle.json.tmp" \
          "$broken_repo/kask/registry/zk-ref-open/bundle.json"
set +e
out="$(IBP_SCRIPTS_DIR="$work" IBP_REPO_ROOT="$broken_repo" \
    IBP_SKIP_BEHAVIORAL=1 bash "$GATE" 2>&1)"
rc=$?
set -e
if [ "$rc" -eq 0 ]; then
    echo "FAIL (registry): gate exited 0 on a bundle.json with a non-hex sha256"
    failures=$((failures + 1))
elif ! grep -q "64-hex sha256" <<<"$out"; then
    echo "FAIL (registry): exit $rc but the violation was not identified"
    printf '%s\n' "$out" | tail -5
    failures=$((failures + 1))
else
    echo "OK (registry): gate detected the malformed bundle.json"
fi

# M4. A script loses its fail-fast stance.
run_static_probe "fail-fast" install.sh \
    's/^set -euo pipefail$/set -u/' \
    "fail-fast stance"

# M5. The main-branch fallback silently defaults to true.
run_static_probe "fallback" install.sh \
    's/HKASK_ALLOW_FALLBACK:-false/HKASK_ALLOW_FALLBACK:-true/' \
    "fallback"

# M6. The corpus dispatch becomes fatal again (bare call).
run_static_probe "non-fatal" install.sh \
    's/if ! install_corpus_bundles/if install_corpus_bundles/' \
    "not fatal"

# M7. A shellcheck-flagrant line appears in a build script. The construct
# must be warning-severity (SC2064: a double-quoted trap expands now, not
# when signalled) — SC2086-class advice sits below -S warning and would
# make this probe vacuous.
run_static_probe "shellcheck" install.sh \
    's/^main "\$@"/trap "rm $UNQUOTED_TMPFILE" EXIT\nmain "$@"/' \
    "shellcheck"

# M8. Behavioral aliveness: neutralizing the up-to-date gate must fail the
# behavioral suite (B2's idempotent skip disappears).
work="$fixture/work-behavioral"
mkdir -p "$work"
cp "$pristine_scripts/"*.sh "$work/"
cp "$pristine_scripts/mcp-servers.txt" "$work/"
sed -i 's/registered_run_id" = "\$bundle_run_id/registered_run_id" != "\$bundle_run_id/' \
    "$work/install-common.sh"
set +e
out="$(IBP_SCRIPTS_DIR="$work" IBP_REPO_ROOT="$pristine_repo" bash "$GATE" 2>&1)"
rc=$?
set -e
if [ "$rc" -eq 0 ]; then
    echo "FAIL (behavioral): gate exited 0 with the up-to-date gate neutralized"
    failures=$((failures + 1))
elif ! grep -q "B2" <<<"$out"; then
    echo "FAIL (behavioral): exit $rc but the idempotence probe (B2) was not the failure"
    printf '%s\n' "$out" | tail -5
    failures=$((failures + 1))
else
    echo "OK (behavioral): the idempotence probe caught the neutralized gate"
fi

# M9. The pristine fixture passes the full gate.
work="$fixture/work-pristine"
mkdir -p "$work"
cp "$pristine_scripts/"*.sh "$work/"
cp "$pristine_scripts/mcp-servers.txt" "$work/"
set +e
out="$(IBP_SCRIPTS_DIR="$work" IBP_REPO_ROOT="$pristine_repo" bash "$GATE" 2>&1)"
rc=$?
set -e
if [ "$rc" -ne 0 ] || ! grep -q "^PASS:" <<<"$out"; then
    echo "FAIL (pristine): rc=$rc — the gate fails a clean fixture"
    printf '%s\n' "$out" | tail -5
    failures=$((failures + 1))
else
    echo "OK (pristine): the full gate passes a clean fixture"
fi

if [ "$failures" -eq 0 ]; then
    echo "SELFTEST OK: install-best-practices gate is alive (7 static probes + behavioral + pristine all pinned)"
    exit 0
fi
echo "SELFTEST FAIL: $failures case(s) did not behave as expected"
exit 1
