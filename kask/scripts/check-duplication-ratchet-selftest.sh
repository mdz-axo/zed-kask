#!/usr/bin/env bash
# Self-test for the duplication ratchet (check-duplication-ratchet.sh).
#
# Institutionalizes the "a CI gate must be shown to fail before its status:
# enforced is trusted" discipline for the simplification program's Phase
# 1.2 oracle. Pins the gate's behavior on a synthetic repo whose scan set
# and cluster count are exactly known:
#   1. A tree at its baseline passes.
#   2. A newly duplicated block (a new cluster) FAILS — the RED proof.
#   3. Removing duplication passes — the gate never punishes cleanup.
#   4. Dropping below baseline passes (stale-high, refreshable).
#   5. --refresh re-baselines the tree and it passes again.
#
# Runs in CI via the kask/scripts/check-*-selftest.sh glob.
# Exit codes: 0 = all cases behaved as expected, 1 = the gate is vacuous
# or broken.

set -uo pipefail

GATE="$(cd "$(dirname "$0")" && pwd)/check-duplication-ratchet.sh"

[ -f "$GATE" ] || { echo "FAIL: gate not found at $GATE"; exit 1; }
command -v npx >/dev/null 2>&1 || { echo "FAIL: npx not found — the ratchet selftest requires node/npx."; exit 1; }
command -v jq >/dev/null 2>&1 || { echo "FAIL: jq not found — the ratchet selftest requires jq."; exit 1; }

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
FAILURES=0

expect() { # expect <actual-exit> <expected-exit> <label>
  if [ "$1" -ne "$2" ]; then
    echo "FAIL: $3 (expected exit $2, got $1)"
    FAILURES=$((FAILURES + 1))
  else
    echo "ok: $3"
  fi
}

git init -q "$WORK"
git -C "$WORK" config user.email selftest@local
git -C "$WORK" config user.name selftest
mkdir -p "$WORK/scripts" "$WORK/kask" "$WORK/src"
cp "$GATE" "$WORK/scripts/check-duplication-ratchet.sh"

# Base commit: the gate itself and a unique kask file. The scan-set pin is
# written after the base commit exists (it names the base), then committed
# with the fixture pair so the working tree the gate reads is realistic.
printf 'pub fn seed() -> u32 {\n    7\n}\n' > "$WORK/kask/lib.rs"
git -C "$WORK" add -A
git -C "$WORK" commit -qm base
BASE="$(git -C "$WORK" rev-parse HEAD)"
printf '%s\n' "$BASE" > "$WORK/scripts/duplication-scan-base.txt"

# A duplicated block well over the 70-token floor, shared by two files.
cat > "$WORK/src/a.rs" <<'EOF'
pub fn reconcile_inventory(orders: &[Order], inventory: &mut Inventory) -> Result<Summary, InventoryError> {
    let mut summary = Summary::with_capacity(orders.len());
    for order in orders {
        let reserved = inventory
            .reserve(&order.sku, order.quantity)
            .map_err(InventoryError::Reserve)?;
        let priced = reserved
            .apply_pricing(order.unit_price, order.discount)
            .map_err(InventoryError::Price)?;
        let receipt = Receipt::new(order.id, priced.total(), priced.tax());
        inventory.commit(reserved);
        summary.push(receipt);
    }
    Ok(summary)
}
EOF
sed 's/reconcile_inventory/reconcile_inventory_b/' "$WORK/src/a.rs" > "$WORK/src/b.rs"
git -C "$WORK" add -A
git -C "$WORK" commit -qm duplicated-pair

gate_exit() {
  bash "$WORK/scripts/check-duplication-ratchet.sh" >/dev/null 2>&1
  return $?
}

# Seed the baseline at the current (legitimate) count: the a/b pair.
bash "$WORK/scripts/check-duplication-ratchet.sh" --refresh >/dev/null

# Case 1: at baseline → pass.
gate_exit
expect $? 0 "at-baseline tree passes"

# Case 2: a new duplicated block in a new file pair → FAIL (the RED proof).
cat > "$WORK/src/c.rs" <<'EOF'
pub fn audit_ledger(entries: &[Entry], ledger: &mut Ledger) -> Result<Report, AuditError> {
    let mut report = Report::with_capacity(entries.len());
    for entry in entries {
        let posted = ledger
            .post(&entry.account, entry.amount)
            .map_err(AuditError::Post)?;
        let balanced = posted
            .reconcile(entry.reference, entry.timestamp)
            .map_err(AuditError::Reconcile)?;
        let line = LineItem::new(entry.id, balanced.debit(), balanced.credit());
        ledger.seal(balanced);
        report.push(line);
    }
    Ok(report)
}
EOF
sed 's/audit_ledger/audit_ledger_d/' "$WORK/src/c.rs" > "$WORK/src/d.rs"
git -C "$WORK" add -A
git -C "$WORK" commit -qm second-duplicated-pair
gate_exit
expect $? 1 "a new duplicated block fails"

# Case 3: removing the new pair → pass — cleanup is never punished.
rm "$WORK/src/c.rs" "$WORK/src/d.rs"
git -C "$WORK" add -A
git -C "$WORK" commit -qm remove-second-pair
gate_exit
expect $? 0 "removing the new duplication passes"

# Case 4: below baseline → pass (stale-high, refreshable).
rm "$WORK/src/b.rs"
git -C "$WORK" add -A
git -C "$WORK" commit -qm remove-one-copy
gate_exit
expect $? 0 "below-baseline tree passes"

# Case 5: --refresh re-baselines the tree → pass.
bash "$WORK/scripts/check-duplication-ratchet.sh" --refresh >/dev/null
gate_exit
expect $? 0 "refresh re-baselines the tree"

if [ "$FAILURES" -eq 0 ]; then
  echo ""
  echo "SELFTEST OK: duplication ratchet is alive (RED + cleanup + below-baseline + refresh all pinned)"
  exit 0
fi
echo "" >&2
echo "SELFTEST FAIL: $FAILURES case(s) did not behave as expected — the gate is vacuous or broken." >&2
exit 1
