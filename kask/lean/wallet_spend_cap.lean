/-
zed-kask spec pin (PRF-01 — D110 wallet amount & rolling spend-cap invariants).

The Rust side:
- `kask/crates/hkask-wallet-core/src/amount.rs` (`Millisatoshis::to_satoshis_exact`,
  `Millisatoshis::to_satoshis_ceil`, `MSAT_PER_SAT = 1000`)
- `kask/crates/hkask-wallet-core/src/policy.rs` (`RollingSpendLedger::{prune,
  spent_in_window, check_policy, reserve, release}`, `ROLLING_WINDOW_SECS = 86400`)

This file pins the pure mathematical contract over unbounded `Nat` with explicit
`U64_MAX` overflow guards:
1. Unit conversion soundness (`toSatoshisExact`, `toSatoshisCeil`).
2. `prune` observational equivalence for all future timestamps `t' ≥ t`.
3. `reserve` daily-cap preservation (`spentInWindow l' now ≤ cap`) and exact
   remaining-budget accounting (`spentInWindow l' now + rem = cap`).
4. `release` monotonicity (`spentInWindow (release l id) now ≤ spentInWindow l now`).

Check: `lean_check` on this file (pinned Lean 4.34.0); the paired
`wallet_spend_cap_negative_control.lean` must FAIL the same check.
-/

namespace WalletSpendCap

def MSAT_PER_SAT : Nat := 1000
def ROLLING_WINDOW_SECS : Nat := 86400
def U64_MAX : Nat := 2 ^ 64 - 1

/-- Checked u64 addition model. -/
def checkedAdd (a b : Nat) : Option Nat :=
  if a + b ≤ U64_MAX then some (a + b) else none

/-- Exact msat → sat conversion (`Millisatoshis::to_satoshis_exact`). -/
def toSatoshisExact (msat : Nat) : Option Nat :=
  if msat % MSAT_PER_SAT = 0 then some (msat / MSAT_PER_SAT) else none

/-- Conservative ceiling msat → sat conversion (`Millisatoshis::to_satoshis_ceil`). -/
def toSatoshisCeil (msat : Nat) : Option Nat :=
  match checkedAdd msat (MSAT_PER_SAT - 1) with
  | some bumped => some (bumped / MSAT_PER_SAT)
  | none => none

theorem toSatoshisExact_iff (m s : Nat) :
    toSatoshisExact m = some s ↔ m = MSAT_PER_SAT * s := by
  unfold toSatoshisExact MSAT_PER_SAT
  constructor
  · intro h
    split at h
    · next hmod =>
        injection h with hs
        omega
    · simp at h
  · intro hm
    subst hm
    have hmod : (1000 * s) % 1000 = 0 := by omega
    have hdiv : (1000 * s) / 1000 = s := by omega
    simp [hmod, hdiv]

theorem toSatoshisCeil_bounds {m s : Nat} (h : toSatoshisCeil m = some s) :
    m ≤ MSAT_PER_SAT * s ∧ MSAT_PER_SAT * s < m + MSAT_PER_SAT := by
  unfold toSatoshisCeil checkedAdd at h
  unfold MSAT_PER_SAT at h ⊢
  by_cases hle : m + (1000 - 1) ≤ U64_MAX
  · simp [hle] at h
    omega
  · simp [hle] at h

theorem toSatoshisExact_implies_ceil {m s : Nat}
    (hexact : toSatoshisExact m = some s)
    (hbound : m + (MSAT_PER_SAT - 1) ≤ U64_MAX) :
    toSatoshisCeil m = some s := by
  have hm := (toSatoshisExact_iff m s).mp hexact
  unfold toSatoshisCeil checkedAdd MSAT_PER_SAT at *
  simp [hbound]
  omega

-- ── RollingSpendLedger model ────────────────────────────────────────────────

structure SpendReservation where
  intentId : String
  totalSats : Nat
  timestampSecs : Nat
deriving DecidableEq

/-- Whether a reservation falls inside the 24h rolling window at `now`. -/
def inWindow (now ts : Nat) : Bool :=
  decide (now - ts < ROLLING_WINDOW_SECS)

/-- Drop reservations older than 24h relative to `now`. -/
def prune (rs : List SpendReservation) (now : Nat) : List SpendReservation :=
  rs.filter (fun r => inWindow now r.timestampSecs)

/-- Sum of satoshis reserved within the 24h window at `now`. -/
def spentInWindow (rs : List SpendReservation) (now : Nat) : Nat :=
  (rs.filter (fun r => inWindow now r.timestampSecs)).foldl (fun acc r => acc + r.totalSats) 0

private theorem foldl_add_init (l : List SpendReservation) (init : Nat) :
    l.foldl (fun acc r => acc + r.totalSats) init =
    init + l.foldl (fun acc r => acc + r.totalSats) 0 := by
  induction l generalizing init with
  | nil => simp
  | cons r rs ih =>
      simp only [List.foldl_cons, Nat.zero_add]
      rw [ih (init + r.totalSats), ih r.totalSats]
      omega

theorem spentInWindow_cons (r : SpendReservation) (rs : List SpendReservation) (now : Nat) :
    spentInWindow (r :: rs) now =
      (if inWindow now r.timestampSecs then r.totalSats else 0) + spentInWindow rs now := by
  unfold spentInWindow
  cases h : inWindow now r.timestampSecs
  · simp [h]
  · simp only [List.filter_cons, h, ↓reduceIte, List.foldl_cons, Nat.zero_add]
    exact foldl_add_init (rs.filter (fun r => inWindow now r.timestampSecs)) r.totalSats

/-- Monotonicity of window membership: once expired at `t`, a reservation stays
expired at every `t' ≥ t`. -/
theorem inWindow_anti_mono {t t' ts : Nat} (hle : t ≤ t')
    (h : inWindow t' ts = true) : inWindow t ts = true := by
  unfold inWindow at *
  simp only [decide_eq_true_eq] at *
  omega

/-- `prune` is observationally equivalent for `spentInWindow` at any `t' ≥ t`. -/
theorem prune_preserves_spent (rs : List SpendReservation) {t t' : Nat} (hle : t ≤ t') :
    spentInWindow (prune rs t) t' = spentInWindow rs t' := by
  induction rs with
  | nil => rfl
  | cons r rs ih =>
      unfold prune
      rw [List.filter_cons]
      cases ht : inWindow t r.timestampSecs
      · simp only [Bool.false_eq_true, ↓reduceIte]
        have ht' : inWindow t' r.timestampSecs = false := by
          cases h : inWindow t' r.timestampSecs
          · rfl
          · have := inWindow_anti_mono hle h
            rw [ht] at this
            exact Bool.noConfusion this
        rw [spentInWindow_cons, ht']
        simpa [prune] using ih
      · simp only [↓reduceIte]
        rw [spentInWindow_cons, spentInWindow_cons]
        simpa [prune] using congrArg (fun x => (if inWindow t' r.timestampSecs then r.totalSats else 0) + x) ih

/-- Pure reserve transition modeling `RollingSpendLedger::reserve`. -/
def reserve (rs : List SpendReservation) (dailyCap amount maxFee now : Nat)
    (intentId : String) : Option (List SpendReservation × Nat) :=
  let pruned := prune rs now
  match checkedAdd amount maxFee with
  | none => none
  | some requested =>
      let spent := spentInWindow pruned now
      match checkedAdd spent requested with
      | none => none
      | some projected =>
          if projected ≤ dailyCap then
            let remaining := dailyCap - projected
            let entry : SpendReservation :=
              { intentId := intentId, totalSats := requested, timestampSecs := now }
            some (entry :: pruned, remaining)
          else
            none

/-- Daily-cap preservation & exact remaining-budget conservation:
whenever `reserve` admits an intent, the new ledger's 24h spend is `≤ dailyCap`
and `spentInWindow new now + remaining = dailyCap`. -/
theorem reserve_preserves_daily_cap
    {rs rs' : List SpendReservation} {dailyCap amount maxFee now rem : Nat} {id : String}
    (h : reserve rs dailyCap amount maxFee now id = some (rs', rem)) :
    spentInWindow rs' now ≤ dailyCap ∧ spentInWindow rs' now + rem = dailyCap := by
  unfold reserve checkedAdd at h
  by_cases h1 : amount + maxFee ≤ U64_MAX
  · simp only [h1, ↓reduceIte] at h
    by_cases h2 : spentInWindow (prune rs now) now + (amount + maxFee) ≤ U64_MAX
    · simp only [h2, ↓reduceIte] at h
      by_cases h3 : spentInWindow (prune rs now) now + (amount + maxFee) ≤ dailyCap
      · simp only [h3, ↓reduceIte, Option.some.injEq, Prod.mk.injEq] at h
        rcases h with ⟨hrs, hrem⟩
        subst hrs hrem
        rw [spentInWindow_cons]
        have hself : inWindow now now = true := by
          unfold inWindow ROLLING_WINDOW_SECS
          simp
        rw [hself]
        simp only [↓reduceIte]
        omega
      · simp [h3] at h
    · simp [h2] at h
  · simp [h1] at h

/-- Pure release transition modeling `RollingSpendLedger::release`. -/
def release (rs : List SpendReservation) (intentId : String) : List SpendReservation :=
  rs.filter (fun r => r.intentId != intentId)

/-- Releasing a reservation can never increase 24h spend. -/
theorem release_monotone (rs : List SpendReservation) (intentId : String) (now : Nat) :
    spentInWindow (release rs intentId) now ≤ spentInWindow rs now := by
  induction rs with
  | nil => simp [release, spentInWindow]
  | cons r rs ih =>
      unfold release
      rw [List.filter_cons]
      cases hkeep : (r.intentId != intentId)
      · simp only [Bool.false_eq_true, ↓reduceIte]
        rw [spentInWindow_cons]
        have : spentInWindow (release rs intentId) now ≤ spentInWindow rs now := ih
        unfold release at this
        omega
      · simp only [↓reduceIte]
        rw [spentInWindow_cons, spentInWindow_cons]
        have : spentInWindow (release rs intentId) now ≤ spentInWindow rs now := ih
        unfold release at this
        omega

end WalletSpendCap

#print axioms WalletSpendCap.toSatoshisExact_iff
#print axioms WalletSpendCap.toSatoshisCeil_bounds
#print axioms WalletSpendCap.prune_preserves_spent
#print axioms WalletSpendCap.reserve_preserves_daily_cap
#print axioms WalletSpendCap.release_monotone
