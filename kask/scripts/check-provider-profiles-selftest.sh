#!/usr/bin/env bash
# Selftest: check-provider-profiles.sh fails on synthetic violations (a
# missing profile, a zombie retirement, an active orphan) and passes on a
# clean synthetic tree — a gate that cannot fail is not a gate (.rules,
# ast-grep positive-fixture trap: pin every rule with a synthetic violation
# it must catch before trusting zero).
#
# Exit codes: 0 = the gate behaves (fails on every violation, passes clean),
# 1 = the gate is broken (passed a violation or failed a clean tree).
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
gate="$here/check-provider-profiles.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

root="$tmp/kask"
mkdir -p "$root/crates/kask_bridge/src" \
  "$root/crates/hkask-inference/src" \
  "$root/mcp-servers/hkask-mcp-research/src/research/providers" \
  "$root/registry/providers"

# Synthetic anchors — the four the gate derives from.
cat > "$root/crates/kask_bridge/src/inference_providers.rs" <<'EOF'
pub static INFERENCE_PROVIDERS: &[()] = &[
    id: "TestVendor",
];
pub static DATA_SERVICES: &[()] = &[
    credential_key: "dsone",
];
EOF
cat > "$root/crates/hkask-inference/src/media_providers.rs" <<'EOF'
pub struct TestVendorMediaProvider {}
EOF
touch "$root/mcp-servers/hkask-mcp-research/src/research/providers/alpha.rs"

# Derived synthetic adopted set: testvendor (inference + media),
# alpha (research), dsone (data service).

profile() {
  cat > "$root/registry/providers/$1.yaml" <<EOF
id: $1
name: $1
status: ${2:-active}
roles:
  - test
keychain:
  slot: none
  env_var: ""
consumed_capabilities:
  - capability: test
    source: selftest
deliberate_non_consumption: []
data_terms:
  extracted_at: null
status_surface:
  status_page: none
pricing:
  machine_readable: none
first_audit_pending: true
EOF
}

run_gate() {
  KASK_ROOT="$root" PROFILES_DIR="$root/registry/providers" bash "$gate" >/dev/null 2>&1
}

expect_fail() {
  local label="$1"
  if run_gate; then
    echo "FAIL: gate PASSED a $label — the gate is broken." >&2
    exit 1
  fi
}

expect_pass() {
  local label="$1"
  if ! run_gate; then
    echo "FAIL: gate FAILED a $label — the gate is broken." >&2
    exit 1
  fi
}

# Case 1: clean tree — all three adopted vendors profiled.
profile testvendor
profile alpha
profile dsone
expect_pass "clean synthetic tree"

# Case 2: missing profile — an adopted vendor without a profile must fail.
rm "$root/registry/providers/testvendor.yaml"
expect_fail "missing profile (adopted vendor unprofiled)"
profile testvendor

# Case 3: zombie retirement — a retired profile for a tree-adopted vendor.
profile testvendor retired
expect_fail "zombie retirement (retired profile, tree still adopts)"
profile testvendor

# Case 4: active orphan — an active profile for a vendor the tree does not adopt.
profile ghostvendor
expect_fail "active orphan (active profile, tree does not adopt)"
rm "$root/registry/providers/ghostvendor.yaml"

# Case 5: retired archive of a departed vendor is legitimate.
profile ghostvendor retired
expect_pass "retired archive of a departed vendor"
rm "$root/registry/providers/ghostvendor.yaml"

# Case 6: a required key missing from a profile must fail.
sed -i '/^pricing:/,+1d' "$root/registry/providers/alpha.yaml"
expect_fail "missing required key (pricing)"
profile alpha

echo "OK: check-provider-profiles.sh fails on every synthetic violation and passes clean trees."