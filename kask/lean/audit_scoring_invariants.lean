/-
zed-kask audit pin: scoring-equation invariants (multi-skill scoring audit, 2026-10-09).

The Rust side, transcribed during the audit:
- `hkask_forecast::brier_score` (kask/crates/hkask-forecast/src/hkask_forecast.rs:216-218):
  brier p o = (p - o) * (p - o) with o in {0, 1} — the binary Brier score
  (Brier 1950), served to every Brier consumer in the MCP surface
  (scenario_score, scenario_calibration, market_calibration, forecast_record,
  the experimentation claim Brier, the goal-prediction calibration).
- Brinson-Fachler per-group effects
  (kask/mcp-servers/hkask-mcp-portfolio/src/analysis.rs:610-618):
  allocation   = (wp - wb) * (rb - RB)
  selection    = wb * (rp - rb)
  interaction  = (wp - wb) * (rp - rb)
  with wp/wb the portfolio/benchmark group weights, rp/rb the group returns,
  RB the total benchmark return. `attribution` (analysis.rs:241-285) reports
  reconciliation_residual = active_return - (A + S + I).

This file pins:
1. Brier bounds: for p in [0,1] and o in {0,1}, 0 <= (p-o)^2 <= 1.
2. The Brinson-Fachler per-group identity:
     allocation + selection + interaction
       = wp*rp - wb*rb - RB*(wp - wb)
   Under weights summing to one on both sides with Rp = SUM wp*rp and
   Rb = SUM wb*rb, summing over groups gives A + S + I = Rp - Rb — the
   active return — so a nonzero reconciliation_residual is measurement
   error, not an unexplained remainder of the model. (The summed form is
   list bookkeeping over this per-group identity; the identity is the
   conservation core.)

Core Lean 4 only (no Mathlib); checked with the pinned Lake toolchain.
-/

namespace ScoringAuditSpec

/-- An ordered semifield: the stated assumptions, as standard facts (the
axiom set of event_tree_marginalization.lean with explicit binders, plus
zero_mul, so rw can instantiate each fact at specific arguments). -/
class OrdField (F : Type) where
  add : F → F → F
  mul : F → F → F
  sub : F → F → F
  zero : F
  one : F
  le : F → F → Prop
  add_comm : ∀ (a b : F), add a b = add b a
  add_assoc : ∀ (a b c : F), add (add a b) c = add a (add b c)
  add_zero : ∀ (a : F), add a zero = a
  add_right_cancel : ∀ (a b c : F), add b a = add c a → b = c
  mul_comm : ∀ (a b : F), mul a b = mul b a
  mul_assoc : ∀ (a b c : F), mul (mul a b) c = mul a (mul b c)
  mul_one : ∀ (a : F), mul a one = a
  one_mul : ∀ (a : F), mul one a = a
  mul_zero : ∀ (a : F), mul a zero = zero
  zero_mul : ∀ (a : F), mul zero a = zero
  left_distrib : ∀ (a b c : F), mul a (add b c) = add (mul a b) (mul a c)
  sub_add_cancel : ∀ (a b : F), add (sub a b) b = a
  sub_zero : ∀ (a : F), sub a zero = a
  sub_nonneg_iff_le : ∀ (a b : F), le zero (sub a b) ↔ le b a
  sub_le_sub_left : ∀ (a b c : F), le b c → le (sub a c) (sub a b)
  le_refl : ∀ (a : F), le a a
  le_trans : ∀ (a b c : F), le a b → le b c → le a c
  zero_le_one : le zero one
  add_le_add : ∀ (a b c d : F), le a b → le c d → le (add a c) (add b d)
  add_nonneg : ∀ (a b : F), le zero a → le zero b → le zero (add a b)
  mul_nonneg : ∀ (a b : F), le zero a → le zero b → le zero (mul a b)
  mul_le_mul_left : ∀ (m a b : F), le zero m → le a b → le (mul m a) (mul m b)

open OrdField

variable {F : Type} [OrdField F]

instance : Std.Associative (α := F) OrdField.add := ⟨add_assoc⟩
instance : Std.Commutative (α := F) OrdField.add := ⟨add_comm⟩

-- ── Subtraction and distributivity bookkeeping ─────────────────────────────

/-- a + b = c implies a = c - b (subtraction is unique). -/
theorem sub_unique {a b c : F} (h : add a b = c) : a = sub c b := by
  apply add_right_cancel (a := b)
  rw [h, sub_add_cancel]

/-- (a + b) + (c + d) = (a + c) + (b + d). -/
theorem add_swap (a b c d : F) :
    add (add a b) (add c d) = add (add a c) (add b d) := by
  rw [add_assoc a b (add c d), add_assoc a c (add b d)]
  congr 1
  rw [← add_assoc b c d, add_comm b c, add_assoc c b d]

/-- (a + b) * c = a*c + b*c. -/
theorem right_distrib (a b c : F) : mul (add a b) c = add (mul a c) (mul b c) := by
  rw [mul_comm (add a b) c, left_distrib, mul_comm c a, mul_comm c b]

/-- (a - b) * c = a*c - b*c. -/
theorem mul_sub_right (a b c : F) : mul (sub a b) c = sub (mul a c) (mul b c) := by
  apply sub_unique
  rw [← right_distrib, sub_add_cancel]

/-- a * (b - c) = a*b - a*c. -/
theorem mul_sub_left (a b c : F) : mul a (sub b c) = sub (mul a b) (mul a c) := by
  rw [mul_comm a (sub b c), mul_sub_right, mul_comm b a, mul_comm c a]

/-- (a - b) + (c - d) = (a + c) - (b + d). -/
theorem add_sub_sub (a b c d : F) :
    add (sub a b) (sub c d) = sub (add a c) (add b d) := by
  apply sub_unique
  rw [add_swap (sub a b) (sub c d) b d, sub_add_cancel, sub_add_cancel]

/-- (a - b) + c = (a + c) - b. -/
theorem add_sub_add_eq (a b c : F) : add (sub a b) c = sub (add a c) b := by
  apply sub_unique
  rw [add_comm (add (sub a b) c) b, ← add_assoc, add_comm b (sub a b), sub_add_cancel]

/-- a + (b - c) = (a + b) - c. -/
theorem add_sub_right_eq (a b c : F) : add a (sub b c) = sub (add a b) c := by
  rw [add_comm a (sub b c), add_sub_add_eq b c a, add_comm b a]

/-- (a - b) + (c - a) = c - b. -/
theorem sub_add_sub (a b c : F) : add (sub a b) (sub c a) = sub c b := by
  apply sub_unique
  rw [add_assoc, add_comm (sub c a) b, ← add_assoc, sub_add_cancel,
    add_comm a (sub c a), sub_add_cancel]

/-- Forward direction of the sub-equality characterization. -/
theorem sub_eq_add {A B C D : F} (h : sub A B = sub C D) : add A D = add C B := by
  rw [← sub_add_cancel A B, ← sub_add_cancel C D, h, add_assoc, add_assoc,
    add_comm B D]

/-- sub A B = sub C D iff A + D = C + B. -/
theorem sub_eq_iff {A B C D : F} : sub A B = sub C D ↔ add A D = add C B := by
  constructor
  · intro h
    exact sub_eq_add h
  · intro h
    apply add_right_cancel (a := add B D)
    have e1 : add (sub A B) (add B D) = add A D := by
      rw [← add_assoc, sub_add_cancel]
    have e2 : add (sub C D) (add D B) = add C B := by
      rw [← add_assoc, sub_add_cancel]
    rw [e1, h, ← e2, add_comm D B]

-- ── Part 1: Brier bounds ───────────────────────────────────────────────────

/-- 0 - (0 - y) = y. -/
theorem sub_zero_sub_zero (y : F) : sub zero (sub zero y) = y := by
  apply add_right_cancel (a := sub zero y)
  rw [sub_add_cancel, add_comm y (sub zero y), sub_add_cancel]

/-- (p - 1) + (1 - p) = 0 — the two one-sided subtractions are negatives. -/
theorem sub_add_eq_zero (p : F) : add (sub p one) (sub one p) = zero := by
  apply add_right_cancel (a := p)
  rw [add_assoc, sub_add_cancel, sub_add_cancel, add_comm zero p, add_zero]

/-- x + q = 0 implies x*x = q*q (squares of negatives agree). -/
theorem sq_neg_eq (x q : F) (h : add x q = zero) : mul x x = mul q q := by
  have hx : x = sub zero q := sub_unique h
  rw [hx, mul_sub_right, zero_mul, mul_sub_left, mul_zero, sub_zero_sub_zero]

/-- The binary Brier score (Brier 1950): (p - o)^2, o in {0, 1}. -/
def brier (p o : F) : F := mul (sub p o) (sub p o)

theorem brier_bounds_o0 (p : F) (h0 : le zero p) (h1 : le p one) :
    le zero (brier p zero) ∧ le (brier p zero) one := by
  have e : brier p zero = mul p p := by
    show mul (sub p zero) (sub p zero) = mul p p
    rw [sub_zero]
  rw [e]
  refine ⟨mul_nonneg p p h0 h0, ?_⟩
  have hu := mul_le_mul_left p p one h0 h1
  rw [mul_one] at hu
  exact le_trans (mul p p) p one hu h1

theorem brier_bounds_o1 (p : F) (h0 : le zero p) (h1 : le p one) :
    le zero (brier p one) ∧ le (brier p one) one := by
  have hsq : mul (sub p one) (sub p one) = mul (sub one p) (sub one p) :=
    sq_neg_eq (sub p one) (sub one p) (sub_add_eq_zero p)
  have e : brier p one = mul (sub one p) (sub one p) := by
    show mul (sub p one) (sub p one) = mul (sub one p) (sub one p)
    exact hsq
  have q0 : le zero (sub one p) := (sub_nonneg_iff_le one p).mpr h1
  have q1 : le (sub one p) one := by
    have hs : le (sub one p) (sub one zero) := sub_le_sub_left one zero p h0
    rwa [sub_zero] at hs
  rw [e]
  refine ⟨mul_nonneg (sub one p) (sub one p) q0 q0, ?_⟩
  have hu := mul_le_mul_left (sub one p) (sub one p) one q0 q1
  rw [mul_one] at hu
  exact le_trans (mul (sub one p) (sub one p)) (sub one p) one hu q1

/-- Brier bounds: p in [0,1] and o in {0,1} imply (p-o)^2 in [0,1]. -/
theorem brier_bounds (p o : F) (h0 : le zero p) (h1 : le p one)
    (ho : o = zero ∨ o = one) : le zero (brier p o) ∧ le (brier p o) one := by
  rcases ho with rfl | rfl
  · exact brier_bounds_o0 p h0 h1
  · exact brier_bounds_o1 p h0 h1

-- ── Part 2: the Brinson-Fachler per-group identity ─────────────────────────

/-- Brinson-Fachler per-group effects (analysis.rs:610-618). -/
def allocation (wp wb rb RB : F) : F := mul (sub wp wb) (sub rb RB)
def selection (wb rp rb : F) : F := mul wb (sub rp rb)
def interaction (wp wb rp rb : F) : F := mul (sub wp wb) (sub rp rb)

/-- The Brinson-Fachler per-group identity: the three separately reported
effects sum to wp*rp - wb*rb - RB*(wp - wb). Summed over groups with weights
totaling one on both sides, this is the active return Rp - Rb — the
conservation the tool's reconciliation_residual measures. -/
theorem bf_group_identity (wp wb rp rb RB : F) :
    add (add (allocation wp wb rb RB) (selection wb rp rb)) (interaction wp wb rp rb)
      = sub (sub (mul wp rp) (mul wb rb)) (mul RB (sub wp wb)) := by
  show add (add (mul (sub wp wb) (sub rb RB)) (mul wb (sub rp rb)))
      (mul (sub wp wb) (sub rp rb))
    = sub (sub (mul wp rp) (mul wb rb)) (mul RB (sub wp wb))
  -- Regroup (A + S) + I = (A + I) + S
  have hreg : add (add (mul (sub wp wb) (sub rb RB)) (mul wb (sub rp rb)))
      (mul (sub wp wb) (sub rp rb))
    = add (add (mul (sub wp wb) (sub rb RB)) (mul (sub wp wb) (sub rp rb)))
      (mul wb (sub rp rb)) := by
    have h := add_swap (mul (sub wp wb) (sub rb RB)) (mul wb (sub rp rb))
      (mul (sub wp wb) (sub rp rb)) zero
    simp only [add_zero] at h
    exact h
  rw [hreg, ← left_distrib, sub_add_sub rb RB rp]
  -- Goal: (wp−wb)(rp−RB) + wb(rp−rb) = (wp·rp − wb·rb) − RB(wp−wb)
  apply sub_unique
  apply sub_unique
  -- Goal: (((J + S) + RB(wp−wb)) + wb·rb) = wp·rp
  rw [mul_sub_right wp wb (sub rp RB), mul_sub_left wp rp RB, mul_sub_left wb rp RB,
    mul_sub_left wb rp rb, mul_sub_left RB wp wb]
  -- Combine into a single sub X Y
  rw [add_sub_sub (sub (mul wp rp) (mul wp RB)) (sub (mul wb rp) (mul wb RB))
    (mul wb rp) (mul wb rb)]
  rw [add_sub_sub (add (sub (mul wp rp) (mul wp RB)) (mul wb rp))
    (add (sub (mul wb rp) (mul wb RB)) (mul wb rb)) (mul RB wp) (mul RB wb)]
  rw [add_sub_add_eq (add (add (sub (mul wp rp) (mul wp RB)) (mul wb rp)) (mul RB wp))
    (add (add (sub (mul wb rp) (mul wb RB)) (mul wb rb)) (mul RB wb)) (mul wb rb)]
  -- Reduce sub X Y = wp·rp by sub_unique (symmetrized)
  symm
  apply sub_unique
  -- Goal: wp·rp + D = N — expand the nested subs on both sides
  rw [add_sub_add_eq (mul wp rp) (mul wp RB) (mul wb rp)]
  rw [add_sub_add_eq (add (mul wp rp) (mul wb rp)) (mul wp RB) (mul RB wp)]
  rw [add_sub_add_eq (add (add (mul wp rp) (mul wb rp)) (mul RB wp)) (mul wp RB)
    (mul wb rb)]
  rw [add_sub_add_eq (mul wb rp) (mul wb RB) (mul wb rb)]
  rw [add_sub_add_eq (add (mul wb rp) (mul wb rb)) (mul wb RB) (mul RB wb)]
  rw [add_sub_right_eq (mul wp rp) (add (add (mul wb rp) (mul wb rb)) (mul RB wb))
    (mul wb RB)]
  -- Both sides are now single subs; finish by the sub-equality characterization
  apply sub_eq_iff.mpr
  rw [mul_comm RB wb, mul_comm RB wp]
  ac_rfl

end ScoringAuditSpec
