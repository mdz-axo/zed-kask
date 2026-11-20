#!/usr/bin/env bash
# Selftest for check-skill-crate-refs.sh.
#
# A zero-finding scan is consistent with both a clean corpus and a broken
# rule, so the detector is pinned with a positive fixture (a synthetic
# dangling reference it MUST catch) before any green reading is trusted
# (.rules: the ast-grep selftest pattern).
#
# Pins, in order:
#   1. a dangling crate in a SKILL.md          -> must FAIL
#   2. a dangling crate in a .j2 template      -> must FAIL
#   3. only existing Cargo crates             -> must PASS
#   4. a crate living under kask/mcp-servers/  -> must PASS (the scan-set
#      false-absence class: omitting that root reports every server crate
#      as dangling)
#   5. package-name and directory-name aliases -> must PASS when Cargo.toml
#      backs both identities
#   6. an empty leftover crate directory       -> must FAIL
#   7. a missing skill/template corpus directory -> must FAIL configuration
#   8. an unbackticked mention of a dead crate -> must PASS (prose, not a ref)

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CHECK="$SCRIPT_DIR/check-skill-crate-refs.sh"

fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT

mkdir -p "$fixture/crates/real_panel" \
         "$fixture/crates/hkask-package-alias-dir" \
         "$fixture/kask/crates/hkask-real-lib" \
         "$fixture/kask/mcp-servers/hkask-mcp-real" \
         "$fixture/skills/demo" \
         "$fixture/templates/demo"
printf '[package]\nname = "real_panel"\n' > "$fixture/crates/real_panel/Cargo.toml"
printf '[package]\nname = "hkask-package-alias"\n' > "$fixture/crates/hkask-package-alias-dir/Cargo.toml"
printf '[package]\nname = "hkask-real-lib"\n' > "$fixture/kask/crates/hkask-real-lib/Cargo.toml"
printf '[package]\nname = "hkask-mcp-real"\n' > "$fixture/kask/mcp-servers/hkask-mcp-real/Cargo.toml"

run_check() {
    SKILL_CORPUS_DIR="$fixture/skills" \
    TEMPLATE_CORPUS_DIR="$fixture/templates" \
    CRATE_ROOT_1="$fixture/crates" \
    CRATE_ROOT_2="$fixture/kask/crates" \
    CRATE_ROOT_3="$fixture/kask/mcp-servers" \
        bash "$CHECK" 2>&1
}

reset_fixture() {
    printf '# Demo\n\nUses `hkask-real-lib` and `real_panel`.\n' > "$fixture/skills/demo/SKILL.md"
    printf '{# goal: demo #}\nScope: `hkask-real-lib`.\n' > "$fixture/templates/demo/demo.j2"
}

# 1. Dangling crate in a SKILL.md must fail.
reset_fixture
printf 'Audit `hkask-ghost-widget` too.\n' >> "$fixture/skills/demo/SKILL.md"
if run_check > "$fixture/out1.txt"; then
    echo "SELFTEST FAIL: dangling crate in SKILL.md was not caught"; cat "$fixture/out1.txt"; exit 1
fi
grep -q 'hkask-ghost-widget' "$fixture/out1.txt" || {
    echo "SELFTEST FAIL: output does not name the dangling crate"; cat "$fixture/out1.txt"; exit 1; }
echo "OK (flags a dangling crate in SKILL.md)"

# 2. Dangling crate in a template must fail.
reset_fixture
printf 'Also `hkask-vanished-widget`.\n' >> "$fixture/templates/demo/demo.j2"
if run_check > "$fixture/out2.txt"; then
    echo "SELFTEST FAIL: dangling crate in .j2 was not caught"; cat "$fixture/out2.txt"; exit 1
fi
grep -q 'hkask-vanished-widget' "$fixture/out2.txt" || {
    echo "SELFTEST FAIL: output does not name the dangling template crate"; cat "$fixture/out2.txt"; exit 1; }
echo "OK (flags a dangling crate in a template)"

# 3. Only existing crates must pass.
reset_fixture
if ! run_check > "$fixture/out3.txt"; then
    echo "SELFTEST FAIL: clean fixture was flagged"; cat "$fixture/out3.txt"; exit 1
fi
echo "OK (passes a clean fixture)"

# 4. An MCP-server crate must resolve (the scan-set false-absence class).
reset_fixture
printf 'Tools live on `hkask-mcp-real`.\n' >> "$fixture/skills/demo/SKILL.md"
if ! run_check > "$fixture/out4.txt"; then
    echo "SELFTEST FAIL: kask/mcp-servers/ crate reported dangling"; cat "$fixture/out4.txt"; exit 1
fi
echo "OK (resolves a kask/mcp-servers/ crate)"

# 5. Cargo package and directory names both resolve when they differ.
reset_fixture
printf 'Aliases `hkask-package-alias` and `hkask-package-alias-dir` are both live.\n' >> "$fixture/skills/demo/SKILL.md"
if ! run_check > "$fixture/out5.txt"; then
    echo "SELFTEST FAIL: package/directory aliases were not both accepted"; cat "$fixture/out5.txt"; exit 1
fi
echo "OK (resolves Cargo package-name and directory-name aliases)"

# 6. A leftover directory without Cargo.toml is not a live crate.
reset_fixture
mkdir -p "$fixture/crates/hkask-empty-residue"
printf 'Audit `hkask-empty-residue`.\n' >> "$fixture/skills/demo/SKILL.md"
if run_check > "$fixture/out6.txt"; then
    echo "SELFTEST FAIL: empty residue directory was accepted as a crate"; cat "$fixture/out6.txt"; exit 1
fi
grep -q 'hkask-empty-residue' "$fixture/out6.txt" || {
    echo "SELFTEST FAIL: output does not name the residue directory"; cat "$fixture/out6.txt"; exit 1; }
echo "OK (rejects a directory without Cargo.toml)"

# 7. A missing corpus directory is a configuration failure, not a clean scan.
reset_fixture
mv "$fixture/skills" "$fixture/skills.missing"
if run_check > "$fixture/out7.txt"; then
    echo "SELFTEST FAIL: missing skills corpus was accepted as clean"; cat "$fixture/out7.txt"; exit 1
fi
grep -q 'skill corpus directory not found' "$fixture/out7.txt" || {
    echo "SELFTEST FAIL: missing-corpus error was not actionable"; cat "$fixture/out7.txt"; exit 1; }
mv "$fixture/skills.missing" "$fixture/skills"
echo "OK (fails visibly when a corpus directory is missing)"

# 8. An unbackticked dead-crate mention is prose, not a reference.
rm -rf "$fixture/crates/hkask-empty-residue"
reset_fixture
printf 'The old hkask-ghost-widget crate was removed in 2026.\n' >> "$fixture/skills/demo/SKILL.md"
if ! run_check > "$fixture/out8.txt"; then
    echo "SELFTEST FAIL: unbackticked prose mention was flagged"; cat "$fixture/out8.txt"; exit 1
fi
echo "OK (ignores an unbackticked prose mention)"

echo "SELFTEST OK: crate-reference detector is alive (4 positive controls, 4 negative)."
