/-
zed-kask trust pin: the trust-model core invariants, parameter-anchored
edition (trust-internalization program; operator rulings 2026-10-31).

ANCHORING (operator ruling: magnitudes anchored on published reference
models, not preferences — see
kask/docs/research/trust-economics-and-model.md, parameter anchoring run
55661b35ad904b63):
- Update-rule FORM: Beta-Bernoulli trust with asymmetric success/failure
  gains — Guo & Yang 2021 (Int. J. Soc. Robot. 13:1899-1909), whose
  published assumption is "Trust is more heavily impacted by negative
  experiences with the automation compared to positive experiences"
  (verified via the Frontiers 2023 review of mathematical models of human
  trust in automation, Front. Neuroergon. 4:1171403, server-recorded this
  run). The linear event fold is the discrete-time form of the same
  proportional-update family (Jonker & Treur 1999 via Akash et al. 2017:
  trust change proportional to the experience-trust gap).
- Asymmetry MAGNITUDE: lambda = 2.25, the loss-aversion coefficient of
  cumulative prospect theory (Tversky & Kahneman 1992, "Advances in
  Prospect Theory", J. Risk Uncertain. 5:297-323), verified this session
  via carriers of the 1992 estimates (lambda = 2.25; alpha = beta = 0.88).
  One violation costs 2.25 full keeps of the same weight.
- Prior T0 = 0.5: the maximum-entropy (uniform) prior on a probability —
  Beta(1,1) — which is also Guo & Yang's conjugate-prior form; consistent
  with the hump-shaped interior trust optimum (Butler-Giuliano-Guiso,
  prior run 54a08fda88a5c74f).
- Resolution and units (DEFINITIONAL, not empirical claims): the trust
  point scale 600 points = T = 1.0, chosen so the anchored asymmetry stays
  exact in fixed-point integers (keep +4·w, violate −9·w; 9/4 = 2.25); a
  full-weight keep is 12/600 = 0.02 — the account's measurement
  resolution. kappa = 1 in C_trust = kappa·w·S is the unit definition of
  the priced cost (the exchange rate is carried by the stake S). The
  rolling window W is the algedonic review cycle — a platform-defined
  instrument, not a preference.
- The recovery-debt state machine of the earlier draft is DELETED: the
  anchored asymmetry (2.25 keeps per violation) carries the
  repair-slowness the debt machine encoded, and no reviewed reference
  model carries a recovery-debt state (essentialist deletion).
- Population-fitted rates (learning speed) are the calibration loop's
  job: every cited model fits its rates to its own data; the platform's
  statistical trackability measures them continuously against resolved
  outcomes, initialized at the definitional resolution above.

This file pins (fixed-point integer arithmetic, no floats):
1. Bounds (I1): T stays in [0, 600] under any event sequence.
2. The anchored asymmetry (I2): one violation costs 2.25 full keeps of the
   same weight (9·w vs 4·w — more than two, exactly nine-fourths).
3. Weight-scaled loss (I3) and the gradient's sign: a violation never
   raises T.
4. Partial repair (I4): a violation followed by two same-weight keeps
   leaves T strictly lower — recovery needs strictly more evidence than
   the violation destroyed (2.25 keeps).
5. THE CLIFF (operator ruling): deception/omission zeroes the reliance
   factor R — goal probability and delivered value collapse to 0 for ANY
   benefit (benefit-independence), and any honest action with positive
   reliance strictly dominates a deceptive one.
6. Priced admissibility (I7b) and renegotiation dominance (I7a).

Statistical trackability: the steering projections P are scored by the
binary Brier score (Brier 1950), whose 0 <= (p-o)^2 <= 1 bounds are pinned
in kask/lean/audit_scoring_invariants.lean (ScoringAuditSpec) — not
duplicated here.

Core Lean 4 only (no Mathlib); checked with the pinned Lake toolchain.
-/

namespace TrustModelSpec

/-- Clamp to the trust interval [0, 600]. 600 points = T = 1.0. -/
def clamp (x : Int) : Int := if x < 0 then 0 else if 600 < x then 600 else x

theorem clamp_lo (x : Int) (h : x < 0) : clamp x = 0 := by
  unfold clamp
  rw [if_pos h]

theorem clamp_hi (x : Int) (h : 600 < x) : clamp x = 600 := by
  unfold clamp
  rw [if_neg (show ¬ (x < 0) by omega), if_pos h]

theorem clamp_of_in_bounds (x : Int) (h1 : 0 ≤ x) (h2 : x ≤ 600) : clamp x = x := by
  unfold clamp
  rw [if_neg (show ¬ (x < 0) by omega), if_neg (show ¬ (600 < x) by omega)]

theorem clamp_bounds (x : Int) : 0 ≤ clamp x ∧ clamp x ≤ 600 := by
  by_cases h1 : x < 0
  · rw [clamp_lo _ h1]; omega
  · by_cases h2 : 600 < x
    · rw [clamp_hi _ h2]; omega
    · rw [clamp_of_in_bounds _ (by omega) (by omega)]; omega

/-- One ledger event: a keep or a violation of weight w in {1, 2, 3}. -/
inductive Event where
  | keep : Int → Event
  | violate : Int → Event

/-- The anchored step: keep +4·w points, violate −9·w points (9/4 = 2.25,
the Tversky-Kahneman loss-aversion coefficient), clamped to [0, 600]. -/
def evolve (t : Int) (e : Event) : Int :=
  match e with
  | Event.keep w => clamp (t + 4 * w)
  | Event.violate w => clamp (t - 9 * w)

theorem evolve_keep (t w : Int) : evolve t (Event.keep w) = clamp (t + 4 * w) := by
  simp only [evolve]

theorem evolve_violate (t w : Int) : evolve t (Event.violate w) = clamp (t - 9 * w) := by
  simp only [evolve]

/-- Fold the event list from the initial trust state. -/
def run (t : Int) : List Event → Int
  | [] => t
  | e :: es => run (evolve t e) es

theorem run_nil (t : Int) : run t [] = t := by
  simp only [run]

theorem run_cons (t : Int) (e : Event) (es : List Event) :
    run t (e :: es) = run (evolve t e) es := by
  simp only [run]

/-- I1: T stays in [0, 600] under any event sequence from a bounded start. -/
theorem run_bounds : ∀ (t : Int) (events : List Event),
    0 ≤ t → t ≤ 600 → 0 ≤ run t events ∧ run t events ≤ 600 := by
  intro t events
  induction events generalizing t with
  | nil => intro h1 h2; exact ⟨h1, h2⟩
  | cons e es ih =>
      intro _ _
      rw [run_cons]
      cases e with
      | keep w => rw [evolve_keep]; exact ih _ (clamp_bounds _).1 (clamp_bounds _).2
      | violate w => rw [evolve_violate]; exact ih _ (clamp_bounds _).1 (clamp_bounds _).2

/-- I2: the anchored asymmetry — one violation costs more than two full
keeps of the same weight (9·w > 2·(4·w) = 8·w), and the ratio is exactly
nine-fourths: 4 violations = 9 keeps at equal weight. -/
theorem asymmetry_anchored (w : Int) (hw : 0 < w) : 2 * (4 * w) < 9 * w := by omega

theorem anchored_ratio_exact (w : Int) : 4 * (9 * w) = 9 * (4 * w) := by omega

/-- I3: the loss scales with the weight of the violated expectation. -/
theorem loss_scales_with_weight (w1 w2 : Int) (h1 : 0 < w1) (h2 : w1 < w2) :
    9 * w1 < 9 * w2 := by omega

/-- The gradient's sign: a violation never raises T. -/
theorem violation_never_raises_trust (t w : Int) (hw : 0 < w) (h0 : 0 ≤ t) :
    evolve t (Event.violate w) ≤ t := by
  rw [evolve_violate]
  by_cases h1 : t - 9 * w < 0
  · rw [clamp_lo _ h1]; omega
  · by_cases h2 : 600 < t - 9 * w
    · rw [clamp_hi _ h2]; omega
    · rw [clamp_of_in_bounds _ (by omega) (by omega)]; omega

/-- I4: partial repair — one violation followed by two same-weight keeps
leaves T strictly lower (net −w points), because one violation costs 2.25
keeps: recovery needs strictly more evidence than the violation destroyed. -/
theorem recovery_needs_more (T w : Int) (hw : 0 < w) (h9 : 9 * w ≤ T) (hT : T ≤ 600) :
    run T [Event.violate w, Event.keep w, Event.keep w] < T := by
  rw [run_cons, evolve_violate, run_cons, evolve_keep, run_cons, evolve_keep, run_nil]
  show clamp (clamp (clamp (T - 9 * w) + 4 * w) + 4 * w) < T
  rw [clamp_of_in_bounds (T - 9 * w) (by omega) (by omega),
    clamp_of_in_bounds (T - 9 * w + 4 * w) (by omega) (by omega),
    clamp_of_in_bounds (T - 9 * w + 4 * w + 4 * w) (by omega) (by omega)]
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
