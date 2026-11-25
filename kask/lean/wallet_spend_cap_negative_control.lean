/-
Negative control for `wallet_spend_cap.lean` (PRF-01).

Removes the `projected ≤ dailyCap` guard from `reserveBuggy` and attempts to
prove `reserve_preserves_daily_cap`. Must FAIL `lean_check` — if this file ever
passed, the spec pin would be vacuous.
-/

namespace WalletSpendCapNegControl

def ROLLING_WINDOW_SECS : Nat := 86400

structure SpendReservation where
  intentId : String
  totalSats : Nat
  timestampSecs : Nat

def inWindow (now ts : Nat) : Bool :=
  decide (now - ts < ROLLING_WINDOW_SECS)

def spentInWindow (rs : List SpendReservation) (now : Nat) : Nat :=
  (rs.filter (fun r => inWindow now r.timestampSecs)).foldl (fun acc r => acc + r.totalSats) 0

/-- Buggy reserve: omits the `projected ≤ dailyCap` check. -/
def reserveBuggy (rs : List SpendReservation) (dailyCap amount maxFee now : Nat)
    (intentId : String) : Option (List SpendReservation × Nat) :=
  let entry : SpendReservation :=
    { intentId := intentId, totalSats := amount + maxFee, timestampSecs := now }
  some (entry :: rs, dailyCap - (spentInWindow rs now + amount + maxFee))

theorem buggy_preserves_cap_must_fail
    {rs rs' : List SpendReservation} {dailyCap amount maxFee now rem : Nat} {id : String}
    (h : reserveBuggy rs dailyCap amount maxFee now id = some (rs', rem)) :
    spentInWindow rs' now ≤ dailyCap := by
  unfold reserveBuggy at h
  injection h with hp
  omega

end WalletSpendCapNegControl
