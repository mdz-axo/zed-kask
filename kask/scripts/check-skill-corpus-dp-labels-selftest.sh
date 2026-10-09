#!/usr/bin/env bash
# Selftest for the skill-corpus prescreen's D/P checks (5: presence,
# 6: label-vs-tool floor). Drives skill-corpus-prescreen.sh over a fixture
# skills tree via the SKILLS_DIR seam (the LEAN_SPEC_DIR pattern) and
# asserts each fixture's expected flag behavior — the negative controls
# MUST flag (a checker that accepts a negative control is not an oracle)
# and the clean fixtures MUST pass. The template checks run read-only
# against the live registry (already green); assertions target only the
# two D/P summary lines and the per-fixture flag lines.
#
# Fixture classes pinned:
#   1. clean-table  — D/P step table with a D row        → passes 5 and 6
#   2. clean-bold   — bold-paragraph D/P label           → passes 5 and 6
#   3. mislabeled   — a section, but every step labelled P while the
#                     body names lisp_eval               → 6 MUST flag
#   4. no-section   — inline (D) marker, no D/P section   → 5 MUST flag,
#                     6 passes (isolates the two checks)
#   5. bench-only   — cargo bench/criterion vocabulary, no section,
#                     no D marker                         → 5 MUST flag
#                     (proves the widened detector), 6 MUST flag
set -euo pipefail

KASK_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
prescreen="$KASK_ROOT/scripts/audit/skill-corpus-prescreen.sh"
[ -f "$prescreen" ] || { echo "FAIL: prescreen not found at $prescreen"; exit 1; }

fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT
skills="$fixture/skills"
mkdir -p "$skills/clean-table" "$skills/clean-bold" "$skills/mislabeled" \
         "$skills/no-section" "$skills/bench-only"

cat > "$skills/clean-table/SKILL.md" <<'EOF'
---
name: clean-table
description: Fixture — a computation-prescribing skill with a D/P step table.
---

# Clean Table

## Instructions

1. Run the gate with `lisp_eval`.

## D/P labelling

| Step | Type | Oracle / critique |
|------|------|-------------------|
| 1 Run the gate | D | `lisp_eval` |
EOF

cat > "$skills/clean-bold/SKILL.md" <<'EOF'
---
name: clean-bold
description: Fixture — a computation-prescribing skill with a bold-paragraph D/P label.
---

# Clean Bold

## Instructions

1. Compute the composite with `lisp_eval`.

## D/P labelling

The composite computation is D (`lisp_eval`, the form below); step framing is P, critiqued by the operator.
EOF

cat > "$skills/mislabeled/SKILL.md" <<'EOF'
---
name: mislabeled
description: Fixture — names a D oracle but labels every step P.
---

# Mislabeled

## Instructions

1. Compute the ranking with `lisp_eval`.

## D/P labelling

All steps are P — judgment, critiqued by the operator. No deterministic oracle is named.
EOF

cat > "$skills/no-section/SKILL.md" <<'EOF'
---
name: no-section
description: Fixture — names a D oracle inline but carries no labelling section.
---

# No Section

## Instructions

1. Verify the count with `lisp_eval` (D) — the receipt is the evidence.
EOF

cat > "$skills/bench-only/SKILL.md" <<'EOF'
---
name: bench-only
description: Fixture — benchmark vocabulary only, unlabelled.
---

# Bench Only

## Instructions

1. Measure with `cargo bench -p mycrate --bench parse`.
2. The harness is criterion: `criterion_group!` and `criterion_main!`.
EOF

set +e
out="$(SKILLS_DIR="$skills" bash "$prescreen" 2>&1)"
rc=$?
set -e

failures=0
# Grep reads from a herestring, never a pipe: grep -q's early exit SIGPIPEs
# a piped writer under pipefail (the .rules trap; the prescreen itself
# uses the same herestring form).
assert_contains() { # <label> <needle>
    if grep -qF -- "$2" <<< "$out"; then
        echo "OK ($1)"
    else
        echo "FAIL ($1): expected output mentioning: $2"
        failures=$((failures + 1))
    fi
}
assert_absent() { # <label> <needle>
    if grep -qF -- "$2" <<< "$out"; then
        echo "FAIL ($1): unexpected output mentioning: $2"
        failures=$((failures + 1))
    else
        echo "OK ($1)"
    fi
}

# Check 5 (presence): the widened detector admits all five fixtures.
assert_contains "check-5 summary" "checked 5 computation-prescribing SKILL.mds; 3 carry D/P labelling; 2 flagged"
assert_contains "check-5 flags no-section" "no-section/SKILL.md: computation-prescribing SKILL.md carries no D/P labelling section"
assert_contains "check-5 flags bench-only (the widened detector)" "bench-only/SKILL.md: computation-prescribing SKILL.md carries no D/P labelling section"
assert_absent  "check-5 passes clean-table" "clean-table/SKILL.md: computation-prescribing"
assert_absent  "check-5 passes clean-bold" "clean-bold/SKILL.md: computation-prescribing"
assert_absent  "check-5 passes mislabeled (it has a section)" "mislabeled/SKILL.md: computation-prescribing"

# Check 6 (label-vs-tool floor): the mislabeled negative control MUST flag.
assert_contains "check-6 summary" "checked 5 oracle-naming SKILL.mds; 3 carry a D regime marker; 2 flagged"
assert_contains "check-6 flags mislabeled (the negative control)" "mislabeled/SKILL.md: names a D oracle but asserts no D regime"
assert_contains "check-6 flags bench-only" "bench-only/SKILL.md: names a D oracle but asserts no D regime"
assert_absent  "check-6 passes clean-table" "clean-table/SKILL.md: names a D oracle"
assert_absent  "check-6 passes clean-bold" "clean-bold/SKILL.md: names a D oracle"
assert_absent  "check-6 passes no-section (inline (D) marker present)" "no-section/SKILL.md: names a D oracle"

# The prescreen is a reporting tool: flags surface, they do not fail the run.
if [ "$rc" -ne 0 ]; then
    echo "FAIL: prescreen exited $rc under the fixture seam (flags must not fail a reporting tool)"
    failures=$((failures + 1))
else
    echo "OK (prescreen exits 0 under the fixture seam)"
fi

if [ "$failures" -eq 0 ]; then
    echo "SELFTEST OK: prescreen D/P checks are alive (presence + label-vs-tool, both negative controls pinned)"
    exit 0
fi
echo "SELFTEST FAIL: $failures assertion(s) did not behave as expected"
exit 1
