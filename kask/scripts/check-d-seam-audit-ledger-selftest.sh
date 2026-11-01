#!/usr/bin/env bash
# Self-test for the D-seam audit ledger gate
# (check-d-seam-audit-ledger.sh): identity coverage (a ledger row missing
# fails), counts reconciliation (a preamble that lies about active or
# retired counts fails; a missing removal-records convention fails loudly,
# never silently), and the paired-entry retired-number counting
# ("**D17, D19**" counts two numbers).
set -euo pipefail

KASK_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GATE="$KASK_ROOT/scripts/check-d-seam-audit-ledger.sh"
[ -f "$GATE" ] || { echo "FAIL: gate not found at $GATE"; exit 1; }

fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT

write_fixtures() {
    cat > "$fixture/register.md" <<'EOF'
# Fixture register

## Status at a glance

- **Release 0.0.0** — fixture.
- **Divergence surface: D1–D92** — 2 active seams in the table below (plus the three removal records D90–D92); 5 retired (see Retired seams — numbers are never reused).

## The divergence surface (fixture)

| D   | Surface | Files | Wired |
| --- | --- | --- | --- |
| D1  | alpha | f | w |
| D2  | beta | f | w |
| D90 | record one | f | w |
| D91 | record two | f | w |
| D92 | record three | f | w |

### Retired seams (numbers are never reused)

**D4** — one. **D17, D19** — a paired entry counting two numbers. **D57** — three. **D88** — four. Numbers are never reused.

## Other modified files

- fixture
EOF
    cat > "$fixture/ledger.md" <<'EOF'
# Fixture ledger

| D1 | audited | retain | pin |
| D2 | audited | retain | pin |
| D90 | pending | record | - |
| D91 | pending | record | - |
| D92 | pending | record | - |
EOF
}

failures=0
run_gate() {
    set +e
    out="$(bash "$GATE" "$fixture/register.md" "$fixture/ledger.md" 2>&1)"
    rc=$?
    set -e
}

# Case 1: consistent fixtures pass, reporting the reconciled counts.
write_fixtures
run_gate
if [ "$rc" -ne 0 ] || ! printf '%s\n' "$out" | grep -q "counts reconciled (2 active + 3 removal records = 5 rows; 5 retired numbers)"; then
    echo "FAIL (case 1 — happy path): rc=$rc"
    printf '%s\n' "$out" | tail -4
    failures=$((failures + 1))
else
    echo "OK (case 1 — happy path): identity + counts reconciled"
fi

# Case 2: a preamble that lies about the active count fails naming the
# drift — the exact class that read green before the counts extension.
write_fixtures
sed -i 's/2 active seams/3 active seams/' "$fixture/register.md"
run_gate
if [ "$rc" -eq 0 ] || ! printf '%s\n' "$out" | grep -q "counts disagree with the table"; then
    echo "FAIL (case 2 — active-count lie): rc=$rc"
    printf '%s\n' "$out" | tail -4
    failures=$((failures + 1))
else
    echo "OK (case 2 — active-count lie): gate failed naming the counts drift"
fi

# Case 3: a retired-list entry removed while the claim stays fails (the
# paired **D17, D19** entry still counts two numbers).
write_fixtures
sed -i 's/\*\*D57\*\* — three\. //' "$fixture/register.md"
run_gate
if [ "$rc" -eq 0 ] || ! printf '%s\n' "$out" | grep -q "retired count disagrees"; then
    echo "FAIL (case 3 — retired-list drift): rc=$rc"
    printf '%s\n' "$out" | tail -4
    failures=$((failures + 1))
else
    echo "OK (case 3 — retired-list drift): gate failed naming the retired drift"
fi

# Case 4: a missing removal-records convention fails loudly as a parse
# failure — never a silent zero.
write_fixtures
sed -i 's/ (plus the three removal records D90–D92)//' "$fixture/register.md"
run_gate
if [ "$rc" -eq 0 ] || ! printf '%s\n' "$out" | grep -q "cannot parse"; then
    echo "FAIL (case 4 — missing records convention): rc=$rc"
    printf '%s\n' "$out" | tail -4
    failures=$((failures + 1))
else
    echo "OK (case 4 — missing records convention): loud parse failure"
fi

# Case 5: the identity contract still holds — a ledger row missing fails.
write_fixtures
sed -i '/^| D92 |/d' "$fixture/ledger.md"
run_gate
if [ "$rc" -eq 0 ] || ! printf '%s\n' "$out" | grep -q "does not cover exactly the live D-seams"; then
    echo "FAIL (case 5 — identity gap): rc=$rc"
    printf '%s\n' "$out" | tail -4
    failures=$((failures + 1))
else
    echo "OK (case 5 — identity gap): gate failed on the ledger identity gap"
fi

if [ "$failures" -eq 0 ]; then
    echo "SELFTEST OK: d-seam ledger gate is alive (happy/active-lie/retired-drift/parse-failure/identity-gap all pinned)"
    exit 0
fi
echo "SELFTEST FAIL: $failures case(s) did not behave as expected"
exit 1