/-
zed-kask trust pin: the trust-model core invariants
(trust-internalization program, operator ruling 2026-10-31).

The model (kask/docs/research/trust-economics-and-model.md, Part II, as
amended by the operator ruling of 2026-10-31):
- Trust points: 300 points = T = 1.0. Fixed-point integer arithmetic —
  no floats anywhere, matching the intended implementation.
- Weights w in {1, 2, 3}: implicit norm, committed expectation, explicit
  instruction.
- Keep gain = 2·w, halved to w while recovery debt is positive.
- Violate loss = 10·w — the 5:1 asymmetry at equal weight, grounded in
  Slovic's trust-destruction asymmetry, Bohnet-Zeckhauser betrayal aversion
  and Kim et al. 2004's integrity asymmetry (sources verified in research
  runs 0d53b71a0ce67e5d, f6a6cae2c971be88, 1add6122a28638cb).
- THE CLIFF (operator ruling): trust is priced, not a hard constraint —
  but the cost of deception or omission is -100%: the reliance factor R
  goes to 0, so the goal's achievement probability P = R · P_complete is 0
  (certainty of failure) and the delivered value V = R · B is 0 regardless
  of benefit. Honest violations are priced (κ·w·S); deception and omission
  are goal-kills.

This file pins:
1. Bounds (I1): T stays in [0, 300] under any event sequence.
2. The 5:1 asymmetry (I2): one violation costs more than four full keeps of
   the same weight; and the loss scales with weight (I3).
3. The gradient's sign: a violation never raises T.
4. Partial repair (I4/I5/I6): a violation followed by two same-weight keeps
   leaves T strictly lower — recovery needs strictly more evidence than the
   violation destroyed (Kim et al. 2004's partial-repair finding).
5. The cliff: R = 0 zeroes goal probability and delivered value for ANY
   benefit and ANY completion probability (benefit-independence is the
   "gradient off a cliff" content), and any honest action with positive
   reliance strictly dominates a deceptive one.
6. Priced admissibility (I7b) and renegotiation dominance (I7a): when the
   priced trust cost exceeds the task saving, the honest path beats the
   deceptive shortcut, and renegotiation (ask-overhead a) beats deception
   whenever a is bounded by the trust cost.

Statistical trackability: the steering projections P are scored by the
binary Brier score (Brier 1950), whose 0 <= (p-o)^2 <= 1 bounds are pinned
in kask/lean/audit_scoring_invariants.lean (ScoringAuditSpec) and served by
hkask_forecast::brier_score — not duplicated here.

Core Lean 4 only (no Mathlib); checked with the pinned Lake toolchain.
-/

namespace TrustModelSpec

/-- Clamp to the trust interval [0, 300]. -/
def clamp (x : Int) : Int := if x < 0 then 0 else if 300 < x then 300 else x

theorem clamp_lo (x : Int) (h : x < 0) : clamp x = 0 := by
  unfold clamp
  rw [if_pos h]

theorem clamp_hi (x : Int) (h : 300 < x) : clamp x = 300 := by
  unfold clamp
  rw [if_neg (show ¬ (x < 0) by omega), if_pos h]

theorem clamp_of_in_bounds (x : Int) (h1 : 0 ≤ x) (h2 : x ≤ 300) : clamp x = x := by
  unfold clamp
  rw [if_neg (show ¬ (x < 0) by omega), if_neg (show ¬ (300 < x) by omega)]

theorem clamp_bounds (x : Int) : 0 ≤ clamp x ∧ clamp x ≤ 300 := by
  by_cases h1 : x < 0
  · rw [clamp_lo _ h1]; omega
  · by_cases h2 : 300 < x
    · rw [clamp_hi _ h2]; omega
    · rw [clamp_of_in_bounds _ (by omega) (by omega)]; omega

/-- One ledger event: a keep or a violation of weight w. -/
inductive Event where
  | keep : Int → Event
  | violate : Int → Event

/-- One fold step over the state (T, recovery debt). Positive debt halves
keep gains until it is paid down by keeps; a violation adds to it. -/
def evolve : Int × Int → Event → Int × Int
  | (t, d), Event.keep w =>
      if d > 0 then (clamp (t + w), d - w) else (clamp (t + 2 * w), d)
  | (t, d), Event.violate w => (clamp (t - 10 * w), d + w)

theorem evolve_keep_pos (t d w : Int) (h : d > 0) :
    evolve (t, d) (Event.keep w) = (clamp (t + w), d - w) := by
  simp only [evolve]
  rw [if_pos h]

theorem evolve_keep_nonpos (t d w : Int) (h : ¬ (d > 0)) :
    evolve (t, d) (Event.keep w) = (clamp (t + 2 * w), d) := by
  simp only [evolve]
  rw [if_neg h]

theorem evolve_violate (t d w : Int) :
    evolve (t, d) (Event.violate w) = (clamp (t - 10 * w), d + w) := by
  simp only [evolve]

/-- Fold the event list from the initial state. -/
def run (st : Int × Int) (events : List Event) : Int × Int :=
  events.foldl evolve st

/-- I1: T stays in [0, 300] under any event sequence from a bounded start. -/
theorem run_bounds : ∀ (st : Int × Int) (events : List Event),
    0 ≤ st.1 → st.1 ≤ 300 → 0 ≤ (run st events).1 ∧ (run st events).1 ≤ 300 := by
  intro st events
  induction events generalizing st with
  | nil => intro h1 h2; exact ⟨h1, h2⟩
  | cons e es ih =>
      intro _ _
      obtain ⟨t, d⟩ := st
      simp only [run, List.foldl_cons]
      cases e with
      | keep w =>
          by_cases hd : d > 0
          · rw [evolve_keep_pos _ _ _ hd]
            exact ih _ (clamp_bounds _).1 (clamp_bounds _).2
          · rw [evolve_keep_nonpos _ _ _ hd]
            exact ih _ (clamp_bounds _).1 (clamp_bounds _).2
      | violate w =>
          rw [evolve_violate]
          exact ih _ (clamp_bounds _).1 (clamp_bounds _).2

/-- I2: the 5:1 asymmetry — one violation costs more than four full keeps
of the same weight (10·w vs 4·(2·w) = 8·w). -/
theorem asymmetry_five_to_one (w : Int) (hw : 0 < w) :
    4 * (2 * w) < 10 * w := by omega

/-- I3: the loss scales with the weight of the violated expectation. -/
theorem loss_scales_with_weight (w1 w2 : Int) (h1 : 0 < w1) (h2 : w1 < w2) :
    10 * w1 < 10 * w2 := by omega

/-- The gradient's sign: a violation never raises T. -/
theorem violation_never_raises_trust (t d w : Int) (hw : 0 < w) (h0 : 0 ≤ t) :
    (evolve (t, d) (Event.violate w)).1 ≤ t := by
  rw [evolve_violate]
  by_cases h1 : t - 10 * w < 0
  · rw [clamp_lo _ h1]; omega
  · by_cases h2 : 300 < t - 10 * w
    · rw [clamp_hi _ h2]; omega
    · rw [clamp_of_in_bounds _ (by omega) (by omega)]; omega

/-- I4/I5/I6: partial repair — one violation followed by two same-weight
keeps leaves T strictly lower (T drops by 7·w net, and the first keep is
halved while debt is positive). Recovery needs strictly more evidence than
the violation destroyed. -/
theorem recovery_needs_more (T w : Int) (hw : 0 < w) (h10 : 10 * w ≤ T) (hT : T ≤ 300) :
    (run (T, 0) [Event.violate w, Event.keep w, Event.keep w]).1 < T := by
  show (List.foldl evolve (T, 0)
      [Event.violate w, Event.keep w, Event.keep w]).1 < T
  rw [List.foldl_cons, evolve_violate, List.foldl_cons,
    evolve_keep_pos _ _ _ (show (0 + w) > 0 by omega), List.foldl_cons,
    evolve_keep_nonpos _ _ _ (show ¬ ((0 + w) - w > 0) by omega), List.foldl_nil]
  show clamp (clamp (clamp (T - 10 * w) + w) + 2 * w) < T
  rw [clamp_of_in_bounds (T - 10 * w) (by omega) (by omega),
    clamp_of_in_bounds (T - 10 * w + w) (by omega) (by omega),
    clamp_of_in_bounds (T - 10 * w + w + 2 * w) (by omega) (by omega)]
  omega

/-! The cliff (operator ruling 2026-10-31): the cost of deception or
omission is -100%. Reliance R (in percent, 0..100) is the probability the
user can rely on the work; honest paths draw it from the trust state T;
deception and omission set it to 0 — certainty of failure for the goal. -/

/-- Goal achievement probability: reliance times completion probability. -/
def goalProbability (R Pc : Int) : Int := R * Pc

theorem cliff_zeroes_goal_probability (Pc : Int) : goalProbability 0 Pc = 0 := by
  simp [goalProbability]

theorem honest_goal_probability_positive (R Pc : Int) (hR : 0 < R) (hPc : 0 < Pc) :
    0 < goalProbability R Pc := by
  simp only [goalProbability]
  exact Int.mul_pos hR hPc

/-- Delivered value: reliance times benefit. -/
def deliveredValue (R B : Int) : Int := R * B

/-- The cliff is benefit-independent: no task benefit survives reliance
zero. This is the formal content of "the gradient goes off a cliff" — all
downstream task progress is multiplied by 0. -/
theorem cliff_is_benefit_independent (B1 B2 : Int) :
    deliveredValue 0 B1 = deliveredValue 0 B2 := by
  simp [deliveredValue]

theorem honest_action_dominates (R B : Int) (hR : 0 < R) (hB : 0 < B) :
    deliveredValue 0 B < deliveredValue R B := by
  simp only [deliveredValue]
  have h0 : (0 : Int) * B = 0 := by omega
  rw [h0]
  exact Int.mul_pos hR hB

/-! Priced admissibility and renegotiation dominance. Honest violations are
priced at κ·w·S (operator ruling: priced, not hard-constrained); deception
and omission carry the cliff instead. -/

/-- I7b — the premise, priced: violating an explicit expectation is
inadmissible whenever the priced trust cost exceeds the task saving. -/
theorem priced_violation_inadmissible (C_honest savings k w S : Int)
    (hdom : savings < k * w * S) :
    C_honest < C_honest - savings + k * w * S := by omega

/-- I7a — renegotiation dominance: the ask-overhead a is bounded by the
trust cost of the deceptive alternative, so asking the principal always
beats deceiving the principal when honoring is expensive. -/
theorem renegotiate_beats_deception (C_honest savings a k w S : Int)
    (hdom : a < k * w * S) :
    C_honest - savings + a < C_honest - savings + k * w * S := by omega

end TrustModelSpec
