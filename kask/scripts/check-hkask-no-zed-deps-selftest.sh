#!/usr/bin/env bash
# Self-test for the hkask-no-zed-deps gate (check-hkask-no-zed-deps.sh).
#
# Institutionalizes the .rules trap "A CI gate must be shown to fail before its
# status: enforced is trusted" — applied to check-hkask-no-zed-deps.sh per CI
# gate sweep follow-up issue #6. The gate's "0 = 0" failure mode (zero hKask
# manifests found → zero violations → trivially passes) was flagged in the
# sweep. This self-test pins these failure modes:
#   1. Violation: an hKask Cargo.toml with a `[dependencies]` section naming
#      a zed-kask-only crate (e.g. `gpui`).
#   2. Empty: a MANIFEST_PATHS matching zero manifests must exit 0 (no
#      violations) without crashing — pinning the silent-disconnection path.
#   3. Rename (inline table): `render_core = { package = "gpui", workspace =
#      true }` — the same inverted dependency under a key the denylist cannot
#      see. Signals 1-2 match the dependency KEY, so this shape passed the
#      gate before the `package = ` signal was added.
#   4. Rename (subtable): the `[dependencies.alias]` form of the same thing.
#   5. Clean control: a manifest with only third-party deps must still exit 0.
#      Without this, a detector that rejects everything would score as
#      "hardened".
#   6. Subtable false positive: `[dependencies.serde]` + `workspace = true`
#      must exit 0. In a per-dependency subtable the keys are FIELDS, not
#      crate names, so reading the field `workspace` as the denylisted crate
#      `workspace` reports a legitimate manifest as a violation. No hKask
#      manifest uses the subtable spelling today, so this was latent — it
#      would have fired on the first one.
#
# Design: temp crate dirs are populated with minimal Cargo.toml files, and
# the gate is invoked with MANIFEST_PATHS pointed at the temp globs. Each
# case asserts the gate exits with the expected code and keyword.
#
# Exit codes:
#   0 — all cases behaved as expected (gate is alive)
#   1 — at least one case did not behave as expected (gate is vacuous or broken)
#
# Usage: bash scripts/check-hkask-no-zed-deps-selftest.sh

set -euo pipefail

KASK_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GATE="$KASK_ROOT/scripts/check-hkask-no-zed-deps.sh"

if [ ! -f "$GATE" ]; then
  echo "FAIL: gate not found at $GATE"
  exit 1
fi

TMPDIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMPDIR"
}
trap cleanup EXIT

failures=0

# ──────────────────────────────────────────────────────────────────────────
# Case 1: violation — an hKask Cargo.toml depending on a zed-kask crate
# (`gpui`). The gate must exit 1 with "VIOLATION".
#
# The gate runs `find $MANIFEST_PATHS` from the kask root (it cd's there
# internally), so MANIFEST_PATHS must be paths the kask-root cwd can resolve.
# We use absolute paths for the temp tree.
# ──────────────────────────────────────────────────────────────────────────
mkdir -p "$TMPDIR/crates/hkask-selftest-bad"
cat > "$TMPDIR/crates/hkask-selftest-bad/Cargo.toml" <<'EOF'
[package]
name = "hkask-selftest-bad"
version = "0.0.0"

[dependencies]
gpui = "1.0"
EOF

set +e
CASE1_OUT=$(MANIFEST_PATHS="$TMPDIR/crates/hkask-*" bash "$GATE" 2>&1)
CASE1_RC=$?
set -e

if [ "$CASE1_RC" -ne 1 ]; then
  echo "FAIL (case 1 — violation): expected exit 1, got $CASE1_RC"
  printf '%s\n' "$CASE1_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
elif ! printf '%s\n' "$CASE1_OUT" | grep -q "VIOLATION"; then
  echo "FAIL (case 1 — violation): exit 1 but 'VIOLATION' not in output"
  printf '%s\n' "$CASE1_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
else
  echo "OK (case 1 — violation): gate detected the zed-kask dependency"
fi

# ──────────────────────────────────────────────────────────────────────────
# Case 2: empty MANIFEST_PATHS — zero matching manifests must exit 0 (no
# violations) without crashing. This pins the silent-disconnection path:
# 0 manifests = 0 violations is the correct verdict here, NOT a vacuous pass.
# ──────────────────────────────────────────────────────────────────────────
mkdir -p "$TMPDIR/empty"

set +e
CASE2_OUT=$(MANIFEST_PATHS="$TMPDIR/empty/*" bash "$GATE" 2>&1)
CASE2_RC=$?
set -e

if [ "$CASE2_RC" -ne 0 ]; then
  echo "FAIL (case 2 — empty): expected exit 0, got $CASE2_RC"
  printf '%s\n' "$CASE2_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
elif ! printf '%s\n' "$CASE2_OUT" | grep -q "OK"; then
  echo "FAIL (case 2 — empty): exit 0 but 'OK' not in output"
  printf '%s\n' "$CASE2_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
else
  echo "OK (case 2 — empty): gate exited 0 on empty manifest paths"
fi

# ──────────────────────────────────────────────────────────────────────────
# Case 3: renamed dep, inline-table form. Cargo's `package = "<name>"` key
# makes the dependency KEY arbitrary while the resolved package stays `gpui`.
# `workspace = true` (not a path dep) is used deliberately so the path-dep
# signal cannot catch it — only the resolved-identity signal can.
# ──────────────────────────────────────────────────────────────────────────
mkdir -p "$TMPDIR/rename/hkask-selftest-rename"
cat > "$TMPDIR/rename/hkask-selftest-rename/Cargo.toml" <<'EOF'
[package]
name = "hkask-selftest-rename"
version = "0.0.0"

[dependencies]
render_core = { package = "gpui", workspace = true }
EOF

set +e
CASE3_OUT=$(MANIFEST_PATHS="$TMPDIR/rename/hkask-*" bash "$GATE" 2>&1)
CASE3_RC=$?
set -e

if [ "$CASE3_RC" -ne 1 ]; then
  echo "FAIL (case 3 — renamed inline): expected exit 1, got $CASE3_RC"
  printf '%s\n' "$CASE3_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
elif ! printf '%s\n' "$CASE3_OUT" | grep -q "VIOLATION"; then
  echo "FAIL (case 3 — renamed inline): exit 1 but 'VIOLATION' not in output"
  printf '%s\n' "$CASE3_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
else
  echo "OK (case 3 — renamed inline): gate detected package = \"gpui\" under an alias key"
fi

# ──────────────────────────────────────────────────────────────────────────
# Case 4: renamed dep, `[dependencies.alias]` subtable form — same inversion,
# different TOML spelling. Pinned separately because the section-tracking awk
# must treat `[dependencies.render_core]` as a dependency section.
# ──────────────────────────────────────────────────────────────────────────
mkdir -p "$TMPDIR/subtable/hkask-selftest-subtable"
cat > "$TMPDIR/subtable/hkask-selftest-subtable/Cargo.toml" <<'EOF'
[package]
name = "hkask-selftest-subtable"
version = "0.0.0"

[dependencies.render_core]
package = "gpui"
workspace = true
EOF

set +e
CASE4_OUT=$(MANIFEST_PATHS="$TMPDIR/subtable/hkask-*" bash "$GATE" 2>&1)
CASE4_RC=$?
set -e

if [ "$CASE4_RC" -ne 1 ]; then
  echo "FAIL (case 4 — renamed subtable): expected exit 1, got $CASE4_RC"
  printf '%s\n' "$CASE4_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
elif ! printf '%s\n' "$CASE4_OUT" | grep -q "VIOLATION"; then
  echo "FAIL (case 4 — renamed subtable): exit 1 but 'VIOLATION' not in output"
  printf '%s\n' "$CASE4_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
else
  echo "OK (case 4 — renamed subtable): gate detected the [dependencies.alias] rename"
fi

# ──────────────────────────────────────────────────────────────────────────
# Case 5: clean control — only third-party deps, including one whose name
# merely CONTAINS a denylisted substring. A gate that fails this is
# over-matching, which is as broken as under-matching.
# ──────────────────────────────────────────────────────────────────────────
mkdir -p "$TMPDIR/clean/hkask-selftest-clean"
cat > "$TMPDIR/clean/hkask-selftest-clean/Cargo.toml" <<'EOF'
[package]
name = "hkask-selftest-clean"
version = "0.0.0"

[dependencies]
serde = "1"
ui_test_helpers = "0.1"
hkask-types = { path = "../hkask-types" }
EOF

set +e
CASE5_OUT=$(MANIFEST_PATHS="$TMPDIR/clean/hkask-*" bash "$GATE" 2>&1)
CASE5_RC=$?
set -e

if [ "$CASE5_RC" -ne 0 ]; then
  echo "FAIL (case 5 — clean control): expected exit 0, got $CASE5_RC"
  printf '%s\n' "$CASE5_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
else
  echo "OK (case 5 — clean control): gate accepted a manifest with no zed-kask deps"
fi

# ──────────────────────────────────────────────────────────────────────────
# Case 6: subtable false positive. `[dependencies.serde]` is a legitimate
# per-dependency table whose keys are FIELDS (version/path/workspace/features),
# not crate names. Matching the denylist against those field names reports
# `workspace = true` as a dependency on the zed crate `workspace`. The gate
# must exit 0 here. Paired with cases 3-4: those prove the subtable form is
# still checked for real renames, so this is not "stop checking subtables".
# ──────────────────────────────────────────────────────────────────────────
mkdir -p "$TMPDIR/subclean/hkask-selftest-subclean"
cat > "$TMPDIR/subclean/hkask-selftest-subclean/Cargo.toml" <<'EOF'
[package]
name = "hkask-selftest-subclean"
version = "0.0.0"

[dependencies.serde]
workspace = true
features = ["derive"]

[dependencies.tokio]
version = "1"
EOF

set +e
CASE6_OUT=$(MANIFEST_PATHS="$TMPDIR/subclean/hkask-*" bash "$GATE" 2>&1)
CASE6_RC=$?
set -e

if [ "$CASE6_RC" -ne 0 ]; then
  echo "FAIL (case 6 — subtable false positive): expected exit 0, got $CASE6_RC"
  printf '%s\n' "$CASE6_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
else
  echo "OK (case 6 — subtable false positive): gate read subtable keys as fields, not crate names"
fi

if [ "$failures" -eq 0 ]; then
  echo ""
  echo "SELFTEST OK: hkask-no-zed-deps gate is alive (violation, empty, both rename forms, clean control, and subtable fields all pinned)"
  exit 0
else
  echo ""
  echo "SELFTEST FAIL: $failures case(s) did not behave as expected"
  echo "The gate is vacuous or broken — see kask/scripts/check-hkask-no-zed-deps.sh."
  exit 1
fi
