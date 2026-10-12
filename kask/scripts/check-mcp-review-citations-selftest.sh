#!/usr/bin/env bash
# Self-test for the mcp-review citation gate (check-mcp-review-citations.sh).
#
# Pins the gate's contract (file existence, line range, excerpt within ±3
# lines) and the UC-3 re-anchor suggestion on the failure paths:
#   1. A current citation passes (exit 0).
#   2. A line-shifted excerpt fails (exit 1 — the suggestion is advisory,
#      never a pass) and suggests the excerpt's current verbatim line.
#   3. An excerpt that no longer exists fails and suggests the enclosing
#      definition's current start line (the resolver's structural path).
#   4. An out-of-range line fails (exit 1) without crashing.
# Also pins the resolver directly: enclosing mode, --find mode, and the
# ambiguity exit — the gate's suggestion path consumes all three — plus
# the 2026-10-11 widening: const/use/impl-span enclosing candidates, and
# impl-qualified --find (Type::method) disambiguation.
#
# Exit codes: 0 = every case behaved as expected, 1 = a case misbehaved.
set -uo pipefail

KASK_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GATE="$KASK_ROOT/scripts/check-mcp-review-citations.sh"
RESOLVER="$KASK_ROOT/scripts/resolve-citation-anchor.sh"

[ -f "$GATE" ] || { echo "FAIL: gate not found at $GATE"; exit 1; }
[ -f "$RESOLVER" ] || { echo "FAIL: resolver not found at $RESOLVER"; exit 1; }

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
git init -q "$WORK"
git -C "$WORK" config user.email selftest@local
git -C "$WORK" config user.name selftest

FAILURES=0

mkdir -p "$WORK/src"
# Lines: 1 struct, 3-5 alpha, 7-9 beta.
cat > "$WORK/src/widget.rs" <<'EOF'
pub struct Widget { pub n: u32 }

pub fn alpha() -> u32 {
    Widget { n: 1 }.n
}

pub fn beta() -> u32 {
    alpha() + 1
}
EOF
cat > "$WORK/src/twin.rs" <<'EOF'
pub struct A {}
pub struct B {}
impl A { pub fn gamma() -> u32 { 1 } }
impl B { pub fn gamma() -> u32 { 2 } }
EOF
# Lines: 1 use, 3 const, 5 struct, 7-10 impl Holder (9 = fetch),
# 12 struct Other, 14-16 impl Other (15 = fetch).
cat > "$WORK/src/extra.rs" <<'EOF'
use std::collections::HashMap;

const LIMIT: u32 = 3;

pub struct Holder { pub n: u32 }

impl Holder {
    /// fetches
    pub fn fetch(&self) -> u32 { self.n }
}

pub struct Other { pub m: u32 }

impl Other {
    pub fn fetch(&self) -> u32 { self.m }
}
EOF

# ── Case 1: current citation passes ───────────────────────────────────────
cat > "$WORK/artifact-pass.json" <<'EOF'
{"findings":[{"citations":[{"file":"src/widget.rs","line":3,"excerpt":"pub fn alpha() -> u32 {"}]}]}
EOF
out=$(cd "$WORK" && bash "$GATE" artifact-pass.json 2>&1); rc=$?
if [ $rc -eq 0 ] && grep -q 'PASS \[finding\] src/widget.rs:3' <<<"$out"; then
  echo "case 1 (current citation passes): OK"
else
  echo "case 1: FAIL (rc=$rc)"; printf '%s\n' "$out"; FAILURES=$((FAILURES + 1))
fi

# ── Resolver direct pins (pre-drift) ──────────────────────────────────────
out=$(bash "$RESOLVER" "$WORK/src/widget.rs" 4 2>&1); rc=$?
if [ $rc -eq 0 ] && grep -q '^alpha	3$' <<<"$out"; then
  echo "case r1 (enclosing mode, innermost definition): OK"
else
  echo "case r1: FAIL (rc=$rc)"; printf '%s\n' "$out"; FAILURES=$((FAILURES + 1))
fi
out=$(bash "$RESOLVER" --find "$WORK/src/widget.rs" beta 2>&1); rc=$?
if [ $rc -eq 0 ] && [ "$out" = "7" ]; then
  echo "case r2 (--find mode): OK"
else
  echo "case r2: FAIL (rc=$rc)"; printf '%s\n' "$out"; FAILURES=$((FAILURES + 1))
fi
out=$(bash "$RESOLVER" --find "$WORK/src/twin.rs" gamma 2>&1); rc=$?
if [ $rc -eq 2 ] && grep -q 'ambiguous' <<<"$out"; then
  echo "case r3 (ambiguous name exits 2): OK"
else
  echo "case r3: FAIL (rc=$rc)"; printf '%s\n' "$out"; FAILURES=$((FAILURES + 1))
fi

# ── Resolver widening pins (2026-10-11 follow-up) ─────────────────────
out=$(bash "$RESOLVER" "$WORK/src/extra.rs" 3 2>&1); rc=$?
if [ $rc -eq 0 ] && grep -q '^LIMIT	3$' <<<"$out"; then
  echo "case r4 (enclosing const_item): OK"
else
  echo "case r4: FAIL (rc=$rc)"; printf '%s\n' "$out"; FAILURES=$((FAILURES + 1))
fi
out=$(bash "$RESOLVER" "$WORK/src/extra.rs" 1 2>&1); rc=$?
if [ $rc -eq 0 ] && grep -q '^std::collections::HashMap	1$' <<<"$out"; then
  echo "case r5 (enclosing use_declaration, full-path key): OK"
else
  echo "case r5: FAIL (rc=$rc)"; printf '%s\n' "$out"; FAILURES=$((FAILURES + 1))
fi
out=$(bash "$RESOLVER" "$WORK/src/extra.rs" 8 2>&1); rc=$?
if [ $rc -eq 0 ] && grep -q '^impl Holder	7$' <<<"$out"; then
  echo "case r6 (doc comment between impl methods resolves to the impl, re-findable form): OK"
else
  echo "case r6: FAIL (rc=$rc)"; printf '%s\n' "$out"; FAILURES=$((FAILURES + 1))
fi
out=$(bash "$RESOLVER" "$WORK/src/extra.rs" 9 2>&1); rc=$?
if [ $rc -eq 0 ] && grep -q '^Holder::fetch	9$' <<<"$out"; then
  echo "case r7 (method inside impl returns the qualified re-findable name): OK"
else
  echo "case r7: FAIL (rc=$rc)"; printf '%s\n' "$out"; FAILURES=$((FAILURES + 1))
fi
out=$(bash "$RESOLVER" --find "$WORK/src/extra.rs" 'Holder::fetch' 2>&1); rc=$?
if [ $rc -eq 0 ] && [ "$out" = "9" ]; then
  echo "case r8 (impl-qualified --find): OK"
else
  echo "case r8: FAIL (rc=$rc)"; printf '%s\n' "$out"; FAILURES=$((FAILURES + 1))
fi
out=$(bash "$RESOLVER" --find "$WORK/src/extra.rs" 'Other::fetch' 2>&1); rc=$?
if [ $rc -eq 0 ] && [ "$out" = "15" ]; then
  echo "case r9 (impl-qualified --find, second impl): OK"
else
  echo "case r9: FAIL (rc=$rc)"; printf '%s\n' "$out"; FAILURES=$((FAILURES + 1))
fi
out=$(bash "$RESOLVER" --find "$WORK/src/extra.rs" 'fetch' 2>&1); rc=$?
if [ $rc -eq 2 ]; then
  echo "case r10 (unqualified ambiguous name still exits 2): OK"
else
  echo "case r10: FAIL (rc=$rc)"; printf '%s\n' "$out"; FAILURES=$((FAILURES + 1))
fi
out=$(bash "$RESOLVER" --find "$WORK/src/extra.rs" 'impl Holder' 2>&1); rc=$?
if [ $rc -eq 0 ] && [ "$out" = "7" ]; then
  echo "case r11 (--find 'impl Type' re-finds the impl block): OK"
else
  echo "case r11: FAIL (rc=$rc)"; printf '%s\n' "$out"; FAILURES=$((FAILURES + 1))
fi
out=$(bash "$RESOLVER" --find "$WORK/src/extra.rs" 'std::collections::HashMap' 2>&1); rc=$?
if [ $rc -eq 0 ] && [ "$out" = "1" ]; then
  echo "case r12 (single-:: use path falls back to plain find): OK"
else
  echo "case r12: FAIL (rc=$rc)"; printf '%s\n' "$out"; FAILURES=$((FAILURES + 1))
fi

# ── Case 2: drifted excerpt → FAIL + verbatim re-anchor ───────────────────
for i in 1 2 3 4 5; do echo "// drift line $i"; done | cat - "$WORK/src/widget.rs" > "$WORK/tmp.rs"
mv "$WORK/tmp.rs" "$WORK/src/widget.rs"
# After the 5-line insert: alpha at 8-10, beta at 12-14.
cat > "$WORK/artifact-drift.json" <<'EOF'
{"findings":[{"citations":[{"file":"src/widget.rs","line":3,"excerpt":"pub fn alpha() -> u32 {"}]}]}
EOF
out=$(cd "$WORK" && bash "$GATE" artifact-drift.json 2>&1); rc=$?
if [ $rc -eq 1 ] && grep -q 'excerpt not found within ±3 lines' <<<"$out" \
   && grep -q 're-anchor: excerpt found verbatim at src/widget.rs:8' <<<"$out"; then
  echo "case 2 (drifted excerpt fails, suggests current verbatim line): OK"
else
  echo "case 2: FAIL (rc=$rc)"; printf '%s\n' "$out"; FAILURES=$((FAILURES + 1))
fi

# ── Case 3: vanished excerpt → FAIL + structural re-anchor ────────────────
cat > "$WORK/artifact-gone.json" <<'EOF'
{"findings":[{"citations":[{"file":"src/widget.rs","line":12,"excerpt":"NO_SUCH_TOKEN"}]}]}
EOF
out=$(cd "$WORK" && bash "$GATE" artifact-gone.json 2>&1); rc=$?
if [ $rc -eq 1 ] && grep -q 'excerpt not found within ±3 lines' <<<"$out" \
   && grep -q "re-anchor: enclosing definition 'beta' currently at src/widget.rs:12" <<<"$out"; then
  echo "case 3 (vanished excerpt fails, suggests enclosing definition): OK"
else
  echo "case 3: FAIL (rc=$rc)"; printf '%s\n' "$out"; FAILURES=$((FAILURES + 1))
fi

# ── Case 4: out-of-range line → FAIL, no crash ────────────────────────────
cat > "$WORK/artifact-range.json" <<'EOF'
{"tools":[{"file":"src/widget.rs","line":999}]}
EOF
out=$(cd "$WORK" && bash "$GATE" artifact-range.json 2>&1); rc=$?
if [ $rc -eq 1 ] && grep -q 'line out of range' <<<"$out"; then
  echo "case 4 (out-of-range line exits 1): OK"
else
  echo "case 4: FAIL (rc=$rc)"; printf '%s\n' "$out"; FAILURES=$((FAILURES + 1))
fi

echo "----"
if [ "$FAILURES" -eq 0 ]; then
  echo "mcp-review-citations selftest: all 16 cases behaved as expected"
else
  echo "mcp-review-citations selftest: $FAILURES case(s) misbehaved"
fi
[ "$FAILURES" -eq 0 ]