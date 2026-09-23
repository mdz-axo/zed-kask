#!/usr/bin/env bash
# Regression cases for the lean-prover SKILL.md workflow (core Lean, no Mathlib).
set -euo pipefail

if [[ $# -ne 1 || ! -x "$1" ]]; then
    echo "usage: $0 <lean-4-binary>" >&2
    exit 64
fi
lean=$1
"$lean" --version
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT

cat > "$scratch/positive.lean" <<'LEAN'
theorem swap (p q : Prop) : p ∧ q → q ∧ p := by
  intro h
  exact ⟨h.2, h.1⟩

theorem append_nil_demo (xs : List Nat) : xs ++ [] = xs := by
  induction xs with
  | nil => rfl
  | cons x xs ih => exact congrArg (List.cons x) ih

theorem exists_demo : ∃ n : Nat, n = 2 := ⟨2, rfl⟩
theorem finite_demo : 2 + 2 ≠ 5 := by decide
#print axioms swap
#print axioms append_nil_demo
#print axioms exists_demo
#print axioms finite_demo
LEAN
"$lean" "$scratch/positive.lean" > "$scratch/positive.log" 2>&1 || {
    cat "$scratch/positive.log" >&2
    exit 1
}
if grep -Eq 'warning:|error:|depends on axioms:' "$scratch/positive.log"; then
    cat "$scratch/positive.log" >&2
    exit 1
fi
for theorem in swap append_nil_demo exists_demo finite_demo; do
    grep -Fq "'$theorem' does not depend on any axioms" "$scratch/positive.log" || {
        cat "$scratch/positive.log" >&2
        exit 1
    }
done
printf 'positive: 4 checked, no warnings or axioms\n'

cat > "$scratch/false.lean" <<'LEAN'
example : 1 = 2 := by decide
LEAN
if "$lean" "$scratch/false.lean" > "$scratch/false.log" 2>&1; then
    echo 'false proposition unexpectedly accepted' >&2
    exit 1
fi
grep -Fq 'proposition' "$scratch/false.log"
grep -Fq 'is false' "$scratch/false.log"
printf 'false proposition: rejected by decide\n'

cat > "$scratch/elim.lean" <<'LEAN'
def extractWitness (h : ∃ n : Nat, n = n) : Nat :=
  match h with
  | ⟨n, _⟩ => n
LEAN
if "$lean" "$scratch/elim.lean" > "$scratch/elim.log" 2>&1; then
    echo 'Exists elimination into Nat unexpectedly accepted' >&2
    exit 1
fi
grep -Fq 'Exists.casesOn' "$scratch/elim.log"
grep -Fq 'can only eliminate into `Prop`' "$scratch/elim.log"
printf 'Exists witness extraction: rejected into Type\n'

cat > "$scratch/trust.lean" <<'LEAN'
theorem hole : False := by sorry
#print axioms hole
theorem compiled : 2 + 2 = 4 := by native_decide
#print axioms compiled
LEAN
"$lean" "$scratch/trust.lean" > "$scratch/trust.log" 2>&1 || {
    cat "$scratch/trust.log" >&2
    exit 1
}
grep -Fq 'warning: declaration uses `sorry`' "$scratch/trust.log"
grep -Eq "'hole' depends on axioms: \[sorryAx\]" "$scratch/trust.log"
grep -Eq "'compiled' depends on axioms: \[.*native_decide" "$scratch/trust.log"
printf 'trust: sorryAx and native_decide dependency surfaced\n'
