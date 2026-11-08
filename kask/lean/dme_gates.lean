/-
zed-kask research pin (2026-11-08): machine-checked properties of the
direct-model-edit skill's deterministic gates — DME-0 (the config gate),
DME-2 (the surface budget), DME-4 (the convergence check). The Lean-prover
strand of the skill's creation purpose, commissioned by the operator after
the 2026-11-07 anchoring audit (research run 8353bc04dabb5e6b): the audit
found the skill's editing-method core faithfully anchored on the published
case while the gate apparatus was original-but-unlabeled; the remediation
labeled the apparatus, and this pin delivers the machine-checked treatment
of the gates' properties that the skill's Constraints section names as the
open work of its generalization layer.

Core Lean 4 only, no Mathlib (lake-manifest packages = []), like every pin in
this project; checked with the pinned Lake toolchain
(leanprover/lean4:v4.34.0) via lean_check; axiom-audited with #print axioms
— the theorem-flag audits report dme0_matches_iff [propext, Quot.sound],
withinBudget_scale [propext], convergedDec_iff [propext, Classical.choice,
Quot.sound]: only Lean's standard axioms, no sorry, no native_decide
(Lean.ofReduceBool absent). The negative controls
(dme_gates_negative_control.lean) must FAIL to check and do: decide
refutes all three closed false claims.

STATED ASSUMPTIONS (the modeling boundary):
1. These definitions pin the GATES' SEMANTICS as specified in
   .agents/skills/direct-model-edit/SKILL.md — the DME-0/DME-2/DME-4 forms
   and their regression cases — not the lisp_eval binary. The deployed
   gates are checked separately: parse-verified by check_skill_lisp_forms,
   their expected values pinned in the skill's Regression case section. No
   behavioral claim about any model is made or needed here.
2. DME-0's config values are abstract over any type with decidable
   equality: the gate's properties are value-type-independent, and the
   deployed six fields (mode, temperature, top_p, top_k, presence_penalty,
   token_limit — in the form's diff order) are one instance, indexed 0-5
   here. A config is a value function; the gate reads only the six
   indices, which is why every statement about "all fields" is bounded by
   i < 6.
3. DME-2's 0.1% cap is pinned as exact natural arithmetic (edited * 1000
   <= total, the cross-multiplied form of edited <= total * 0.001): exact
   by construction. The deployed lisp form computes the budget by float
   multiplication (* total_weights 0.001), which agrees with this
   predicate on integer surfaces at the deployed magnitudes (the anchor's
   800,000,000 -> 800,000); any float-vs-exact divergence at other
   magnitudes is a property of the deployed implementation, not of the
   cap pinned here.
4. DME-4's decider models the lisp form's arithmetic exactly — the length
   >= 3 guard, then equality of the lookups at xs[len-1], xs[len-2],
   xs[len-3] — with the lookup equality carried by optEq (a structural
   equality on the Option lookups: `some` values compare by their
   payload, matching the form's `=` on the values nth returns; the guard
   makes all three lookups `some`, and the main theorem
   (convergedDec_iff) makes that faithfulness exact).
-/

namespace DmeGates

-- ── DME-0: the six-field config gate ─────────────────────────────────────────

variable {V : Type} [DecidableEq V]

/-- The six card-config fields the DME-0 form diffs, in the form's order
(SKILL.md, the DME-0 form): 0 = mode, 1 = temperature, 2 = top_p, 3 =
top_k, 4 = presence_penalty, 5 = token_limit. -/
def fields : List Nat := [0, 1, 2, 3, 4, 5]

/-- The DME-0 diff: the fields whose run-config and card-config values
differ, in the form's field order. -/
def configDiff (run card : Nat → V) : List Nat :=
  fields.filter (fun i => decide (run i ≠ card i))

/-- The DME-0 verdict: `.matches` when the diff is empty, else `.differs`
carrying the diff (the lisp form's `config-matches-proceed` /
`config-differs-rerun-at-card-config`). -/
inductive ConfigVerdict (V : Type) where
  | matches : ConfigVerdict V
  | differs : List Nat → ConfigVerdict V

def dme0Verdict (run card : Nat → V) : ConfigVerdict V :=
  match configDiff run card with
  | [] => .matches
  | diffs => .differs diffs

/-- Every one of the six fields is a member of the diff's domain. -/
theorem mem_fields {i : Nat} (h : i < 6) : i ∈ fields := by
  have hd : i = 0 ∨ i = 1 ∨ i = 2 ∨ i = 3 ∨ i = 4 ∨ i = 5 := by omega
  rcases hd with rfl | rfl | rfl | rfl | rfl | rfl <;> decide

/-- Membership in the diff's domain is exactly the six fields. -/
theorem fields_lt {i : Nat} (h : i ∈ fields) : i < 6 := by
  simp [fields] at h
  omega

/-- The diff names exactly the moved fields — no more, no less. -/
theorem mem_configDiff_iff (run card : Nat → V) {i : Nat} :
    i ∈ configDiff run card ↔ i < 6 ∧ run i ≠ card i := by
  simp only [configDiff, List.mem_filter]
  constructor
  · rintro ⟨hf, hd⟩
    exact ⟨fields_lt hf, decide_eq_true_iff.mp hd⟩
  · rintro ⟨hlt, hne⟩
    exact ⟨mem_fields hlt, decide_eq_true hne⟩

/-- The diff preserves the form's field order (the regression case's
["mode", "temperature", "top_p"] ordering depends on it). -/
theorem configDiff_sublist (run card : Nat → V) :
    (configDiff run card).Sublist fields :=
  List.filter_sublist

/-- The gate reports `.matches` exactly when all six fields are equal. -/
theorem dme0_matches_iff (run card : Nat → V) :
    dme0Verdict run card = ConfigVerdict.matches ↔ ∀ i, i < 6 → run i = card i := by
  constructor
  · intro h i hlt
    by_cases hne : run i = card i
    · exact hne
    · have hm : i ∈ configDiff run card := (mem_configDiff_iff run card).mpr ⟨hlt, hne⟩
      simp only [dme0Verdict] at h
      cases hdiff : configDiff run card with
      | nil =>
        rw [hdiff] at hm
        simp at hm
      | cons d ds =>
        rw [hdiff] at h
        simp at h
  · intro h
    have hd : configDiff run card = [] := by
      cases hcl : configDiff run card with
      | nil => rfl
      | cons d ds =>
        have hm : d ∈ configDiff run card := by
          rw [hcl]
          exact List.mem_cons_self
        obtain ⟨hlt, hne⟩ := (mem_configDiff_iff run card).mp hm
        exact absurd (h d hlt) hne
    simp only [dme0Verdict, hd]

/-- No false alarm: a `.differs` verdict always carries a nonempty list. -/
theorem dme0_differs_nonempty (run card : Nat → V) {diffs : List Nat}
    (h : dme0Verdict run card = ConfigVerdict.differs diffs) : diffs ≠ [] := by
  cases diffs with
  | nil =>
    simp only [dme0Verdict] at h
    cases hdiff : configDiff run card with
    | nil =>
      rw [hdiff] at h
      simp at h
    | cons d ds =>
      rw [hdiff] at h
      simp at h
  | cons d ds =>
    intro hc
    cases hc

/-- The `.differs` list is the diff: its members are exactly the moved
fields. -/
theorem dme0_differs_mem (run card : Nat → V) {diffs : List Nat} {i : Nat}
    (h : dme0Verdict run card = ConfigVerdict.differs diffs) :
    i ∈ diffs ↔ i < 6 ∧ run i ≠ card i := by
  have hd : diffs = configDiff run card := by
    simp only [dme0Verdict] at h
    cases hdiff : configDiff run card with
    | nil =>
      rw [hdiff] at h
      simp at h
    | cons d ds =>
      rw [hdiff] at h
      simp at h
      exact h.symm
  rw [hd]
  exact mem_configDiff_iff run card

-- ── DME-2: the 0.1% surface budget ────────────────────────────────────────────

/-- The DME-2 surface predicate in exact natural arithmetic: the
cross-multiplied form of `edited ≤ total * 0.001` (the 0.1% cap). -/
def withinBudget (total edited : Nat) : Prop := edited * 1000 ≤ total

/-- The anchor regression case (SKILL.md, Regression case): 131,072 of
800,000,000 passes — kernel-checked arithmetic. -/
theorem anchor_withinBudget : withinBudget 800000000 131072 := by
  show 131072000 ≤ 800000000
  decide

/-- Shrinking the surface preserves passing (SKILL.md Step 4: "shrink the
surface ... or stop"). -/
theorem withinBudget_mono (t e1 e2 : Nat) (h2 : e2 ≤ e1) (h1 : withinBudget t e1) :
    withinBudget t e2 :=
  Nat.le_trans (Nat.mul_le_mul_right 1000 h2) h1

/-- The cap is a ratio property: uniform scaling of model and surface
preserves the verdict. -/
theorem withinBudget_scale (k t e : Nat) (hk : 0 < k) :
    withinBudget (k * t) (k * e) ↔ withinBudget t e := by
  simp only [withinBudget]
  constructor
  · intro h
    have h' : k * (e * 1000) ≤ k * t := by
      rw [← Nat.mul_assoc]
      exact h
    exact Nat.le_of_mul_le_mul_left h' hk
  · intro h
    have h' : k * (e * 1000) ≤ k * t := Nat.mul_le_mul_left k h
    rwa [Nat.mul_assoc]

-- ── DME-4: the convergence check ─────────────────────────────────────────────

/-- The form's equality on nth-lookups: `some` values compare by their
payload (the lisp `=` on the values nth returns); anything else is not
equal. -/
def optEq : Option Nat → Option Nat → Bool
  | some a, some b => a == b
  | none, none => true
  | _, _ => false

/-- DME-4's convergence spec: the recorded open-findings counts end in
three equal values (the lisp form: converged when the last three counts
are equal). -/
def converged (xs : List Nat) : Prop := ∃ pre a, xs = pre ++ [a, a, a]

/-- The DME-4 decider — the lisp form's arithmetic exactly: the length >= 3
guard, then equality of the lookups at xs[len-1], xs[len-2], xs[len-3]
(the guard makes all three lookups `some`, which convergedDec_iff proves
is faithful). -/
def convergedDec (xs : List Nat) : Bool :=
  decide (3 ≤ xs.length)
  && optEq xs[xs.length - 1]? xs[xs.length - 2]?
  && optEq xs[xs.length - 2]? xs[xs.length - 3]?

/-- Convergence requires three counts (SKILL.md, Regression case: "two
counts can never satisfy the form"). -/
theorem converged_length {xs : List Nat} (h : converged xs) : 3 ≤ xs.length := by
  obtain ⟨pre, a, hpa⟩ := h
  rw [hpa, List.length_append]
  have h3 : ([a, a, a] : List Nat).length = 3 := rfl
  rw [h3]
  omega

/-- The regression instance: [12, 12] never converges. -/
theorem not_converged_two : ¬converged [12, 12] := by
  intro h
  have hh := converged_length h
  have h2 : ([12, 12] : List Nat).length = 2 := rfl
  omega

/-- The form's index arithmetic shifts with a cons: with three or more
counts below it, the last three lookups of `x :: R` are the last three
lookups of `R`. -/
theorem get?_cons_shift (x : Nat) (R : List Nat) (j : Nat) (hj : j + 1 ≤ R.length) :
    (x :: R)[R.length - j]? = R[R.length - 1 - j]? := by
  obtain ⟨m, hm⟩ : ∃ m, R.length - j = m + 1 := ⟨R.length - j - 1, by omega⟩
  have hm2 : R.length - 1 - j = m := by omega
  rw [hm, hm2]
  simp

/-- With three or more counts under it, a cons does not change the verdict —
the form's indices track the tail. -/
theorem convergedDec_cons (x : Nat) (R : List Nat) (hR : 3 ≤ R.length) :
    convergedDec (x :: R) = convergedDec R := by
  have h1 : (x :: R)[R.length]? = R[R.length - 1]? :=
    get?_cons_shift x R 0 (by omega)
  have h2 : (x :: R)[R.length - 1]? = R[R.length - 2]? :=
    get?_cons_shift x R 1 (by omega)
  have h3 : (x :: R)[R.length - 2]? = R[R.length - 3]? :=
    get?_cons_shift x R 2 (by omega)
  have ha : R.length + 1 - 1 = R.length := by omega
  have hb : R.length + 1 - 2 = R.length - 1 := by omega
  have hc : R.length + 1 - 3 = R.length - 2 := by omega
  simp only [convergedDec, List.length_cons, ha, hb, hc]
  rw [h1, h2, h3]
  have hd1 : decide (3 ≤ R.length + 1) = true := decide_eq_true (by omega)
  have hd2 : decide (3 ≤ R.length) = true := decide_eq_true (by omega)
  rw [hd1, hd2]

/-- The main correctness theorem: the lisp form's index arithmetic decides
the spec exactly — `convergedDec xs = true` iff the last three counts are
equal. -/
theorem convergedDec_iff (xs : List Nat) : convergedDec xs = true ↔ converged xs := by
  induction xs with
  | nil =>
    constructor
    · intro h
      simp [convergedDec] at h
    · intro h
      exact absurd (converged_length h) (by have hl : ([] : List Nat).length = 0 := rfl; omega)
  | cons x rest ih =>
    cases rest with
    | nil =>
      constructor
      · intro h
        simp [convergedDec] at h
      · intro h
        exact absurd (converged_length h)
          (by have hl : ([x] : List Nat).length = 1 := rfl; omega)
    | cons y rest2 =>
      cases rest2 with
      | nil =>
        constructor
        · intro h
          simp [convergedDec] at h
        · intro h
          exact absurd (converged_length h)
            (by have hl : ([x, y] : List Nat).length = 2 := rfl; omega)
      | cons z rest3 =>
        by_cases h3 : rest3 = []
        · subst h3
          -- the three-count case: the form's comparisons at [x, y, z]
          have hlen : ([x, y, z] : List Nat).length = 3 := rfl
          constructor
          · intro hd
            simp [convergedDec, hlen, optEq, beq_iff_eq] at hd
            obtain ⟨hzy, hyx⟩ := hd
            exact ⟨[], y, by simp [List.nil_append, hzy, ← hyx]⟩
          · rintro ⟨pre, a, hpa⟩
            cases pre with
            | nil =>
              -- [x, y, z] = [a, a, a]
              injection hpa with hx ht
              injection ht with hy ht2
              injection ht2 with hz _
              have hd : convergedDec [x, y, z] = true := by
                simp [convergedDec, hlen, optEq, beq_iff_eq]
                omega
              exact hd
            | cons p ps =>
              -- length contradiction: 3 = 1 + (ps.length + 3)
              have hl : ([x, y, z] : List Nat).length
                  = ((p :: ps) ++ [a, a, a]).length := by rw [hpa]
              simp only [List.length_cons, List.length_nil, List.length_append] at hl
              exact absurd hl (by omega)
        · obtain ⟨w, rest4, rfl⟩ : ∃ w rest4, rest3 = w :: rest4 := by
            cases rest3 with
            | nil => exact absurd rfl h3
            | cons w rest4 => exact ⟨w, rest4, rfl⟩
          -- rest has length >= 3: the cons shifts onto the tail
          rw [convergedDec_cons x (y :: z :: w :: rest4)
            (by simp only [List.length_cons]; omega)]
          constructor
          · intro h
            obtain ⟨pre, a, hpa⟩ := ih.mp h
            exact ⟨x :: pre, a, by simp [List.cons_append, hpa]⟩
          · rintro ⟨pre, a, hpa⟩
            cases pre with
            | nil =>
              -- x :: R = [a, a, a] forces R = [a, a]: length contradiction
              have h5 : (x :: y :: z :: w :: rest4).length
                  = ([a, a, a] : List Nat).length := by rw [hpa, List.nil_append]
              simp only [List.length_cons, List.length_nil] at h5
              exact absurd h5 (by omega)
            | cons p ps =>
              -- x :: R = (p :: ps) ++ [a, a, a]: peel the head
              simp only [List.cons_append, List.cons.injEq] at hpa
              obtain ⟨_, hR⟩ := hpa
              exact ih.mpr ⟨ps, a, hR⟩

/-- Once three equal counts are recorded, appending the same count again
keeps convergence (SKILL.md Step 7 — the converged state is stable under
repetition of the converged value). -/
theorem converged_stable (pre : List Nat) (a : Nat) :
    converged ((pre ++ [a, a, a]) ++ [a]) :=
  ⟨pre ++ [a], a, by simp [List.append_assoc]⟩

/-- A changed count breaks convergence (SKILL.md Step 7: the remaining
instability is reported, not iterated). -/
theorem converged_break (pre : List Nat) (a b : Nat) (h : a ≠ b) :
    ¬converged ((pre ++ [a, a, a]) ++ [b]) := by
  have key : ∀ (l : List Nat), convergedDec (l ++ [a, a, a, b]) = true → False := by
    intro l
    induction l with
    | nil =>
      intro hd
      have hlen : ([a, a, a, b] : List Nat).length = 4 := rfl
      simp [convergedDec, hlen, optEq, beq_iff_eq] at hd
      have hba : b = a := by omega
      exact h hba.symm
    | cons p l' ih =>
      intro hd
      have hd' : convergedDec (l' ++ [a, a, a, b]) = true := by
        rw [← convergedDec_cons p (l' ++ [a, a, a, b])
          (by simp only [List.length_append, List.length_cons, List.length_nil]; omega)]
        exact hd
      exact ih hd'
  intro hc
  have hd : convergedDec ((pre ++ [a, a, a]) ++ [b]) = true :=
    (convergedDec_iff _).mpr hc
  have hxs : (pre ++ [a, a, a]) ++ [b] = pre ++ [a, a, a, b] := by
    simp [List.append_assoc]
  rw [hxs] at hd
  exact key pre hd

end DmeGates
