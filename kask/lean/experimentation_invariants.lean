/-
zed-kask spec pin (PRF-02 — `hkask-experimentation` preregistration lifecycle,
held-out run disjointness, and checked budget-ceiling preservation).

The Rust side: `kask/crates/hkask-experimentation/src/store.rs`
(`insert_record`, `seal`, `record_selection`). Slice S3 (`EFF-05`) replaced
unchecked `recorded + write.run_count > write.ceiling` with `checked_add`.

This file proves:
1. `resolved_is_absorbing`: once `status = .resolved`, no mutating transition
   succeeds.
2. `recordFitness_preserves_invariants` & `recordFinalEval_preserves_invariants`:
   both held-out run disjointness (`∀ r ∈ fitnessRuns, r ∉ finalRuns`) and
   budget ceiling (`recordedRuns ≤ maxRuns`) are preserved across every step.

Check: `lean_check` on this file (pinned Lean 4.34.0); the paired
`experimentation_invariants_negative_control.lean` must FAIL the same check.
-/

namespace ExperimentationInvariants

def U64_MAX : Nat := 2 ^ 64 - 1

inductive ExpStatus where
  | proposed
  | running
  | resolved
deriving DecidableEq

structure ExpState where
  status : ExpStatus
  sealed : Bool
  fitnessRuns : List String
  finalRuns : List String
  recordedRuns : Nat
  maxRuns : Nat

/-- Core state invariant: held-out final evaluation runs are disjoint from
feedback fitness runs, and total recorded runs never exceed `maxRuns`. -/
def ValidState (s : ExpState) : Prop :=
  (∀ r, r ∈ s.fitnessRuns → r ∉ s.finalRuns) ∧ s.recordedRuns ≤ s.maxRuns

def checkedAdd (a b : Nat) : Option Nat :=
  if a + b ≤ U64_MAX then some (a + b) else none

/-- Seal an experiment (`Proposed -> Running`). -/
def sealExp (s : ExpState) : Option ExpState :=
  match s.status with
  | .proposed => some { s with status := .running, sealed := true }
  | .running | .resolved => none

/-- Record one fitness run ref (`fitness_record`). -/
def recordFitness (s : ExpState) (runRef : String) (runCount : Nat) : Option ExpState :=
  match s.status with
  | .resolved => none
  | .proposed | .running =>
      if runRef ∈ s.finalRuns then none
      else
        match checkedAdd s.recordedRuns runCount with
        | none => none
        | some total =>
            if total ≤ s.maxRuns then
              some { s with fitnessRuns := runRef :: s.fitnessRuns, recordedRuns := total }
            else none

/-- Record one held-out final evaluation run ref (`final_evaluation_record`). -/
def recordFinalEval (s : ExpState) (runRef : String) (runCount : Nat) : Option ExpState :=
  match s.status with
  | .resolved => none
  | .proposed => none
  | .running =>
      if !s.sealed then none
      else if runRef ∈ s.fitnessRuns ∨ runRef ∈ s.finalRuns then none
      else
        match checkedAdd s.recordedRuns runCount with
        | none => none
        | some total =>
            if total ≤ s.maxRuns then
              some { s with finalRuns := runRef :: s.finalRuns, recordedRuns := total }
            else none

/-- Resolve an experiment (`selection_record`). -/
def resolve (s : ExpState) : Option ExpState :=
  match s.status with
  | .resolved => none
  | .proposed | .running => some { s with status := .resolved }

theorem resolved_is_absorbing (s : ExpState) (h : s.status = .resolved) (r : String) (n : Nat) :
    sealExp s = none ∧ recordFitness s r n = none ∧ recordFinalEval s r n = none ∧ resolve s = none := by
  simp [sealExp, recordFitness, recordFinalEval, resolve, h]

theorem recordFitness_preserves_invariants
    {s s' : ExpState} {r : String} {n : Nat}
    (hvalid : ValidState s)
    (hstep : recordFitness s r n = some s') :
    ValidState s' := by
  rcases hvalid with ⟨hdisj, hbud⟩
  unfold recordFitness checkedAdd at hstep
  cases hst : s.status <;> simp only [hst] at hstep
  · by_cases hmem : r ∈ s.finalRuns
    · simp [hmem] at hstep
    · simp only [hmem, ↓reduceIte] at hstep
      by_cases hov : s.recordedRuns + n ≤ U64_MAX
      · simp only [hov, ↓reduceIte] at hstep
        by_cases hcap : s.recordedRuns + n ≤ s.maxRuns
        · simp only [hcap, ↓reduceIte, Option.some.injEq] at hstep
          subst hstep
          refine ⟨?_, hcap⟩
          intro x hx
          simp only [List.mem_cons] at hx
          rcases hx with rfl | hx'
          · exact hmem
          · exact hdisj x hx'
        · simp [hcap] at hstep
      · simp [hov] at hstep
  · by_cases hmem : r ∈ s.finalRuns
    · simp [hmem] at hstep
    · simp only [hmem, ↓reduceIte] at hstep
      by_cases hov : s.recordedRuns + n ≤ U64_MAX
      · simp only [hov, ↓reduceIte] at hstep
        by_cases hcap : s.recordedRuns + n ≤ s.maxRuns
        · simp only [hcap, ↓reduceIte, Option.some.injEq] at hstep
          subst hstep
          refine ⟨?_, hcap⟩
          intro x hx
          simp only [List.mem_cons] at hx
          rcases hx with rfl | hx'
          · exact hmem
          · exact hdisj x hx'
        · simp [hcap] at hstep
      · simp [hov] at hstep
  · simp at hstep

theorem recordFinalEval_preserves_invariants
    {s s' : ExpState} {r : String} {n : Nat}
    (hvalid : ValidState s)
    (hstep : recordFinalEval s r n = some s') :
    ValidState s' := by
  rcases hvalid with ⟨hdisj, hbud⟩
  unfold recordFinalEval checkedAdd at hstep
  cases hst : s.status <;> simp only [hst] at hstep
  · simp at hstep
  · cases hseal : s.sealed
    · simp [hseal] at hstep
    · simp only [hseal, Bool.not_true, Bool.false_eq_true, ↓reduceIte] at hstep
      by_cases hmem : r ∈ s.fitnessRuns ∨ r ∈ s.finalRuns
      · simp [hmem] at hstep
      · simp only [hmem, ↓reduceIte] at hstep
        by_cases hov : s.recordedRuns + n ≤ U64_MAX
        · simp only [hov, ↓reduceIte] at hstep
          by_cases hcap : s.recordedRuns + n ≤ s.maxRuns
          · simp only [hcap, ↓reduceIte, Option.some.injEq] at hstep
            subst hstep
            refine ⟨?_, hcap⟩
            intro x hx hxfin
            simp only [List.mem_cons] at hxfin
            rcases hxfin with rfl | hxfin'
            · exact hmem (Or.inl hx)
            · exact hdisj x hx hxfin'
          · simp [hcap] at hstep
        · simp [hov] at hstep
  · simp at hstep

end ExperimentationInvariants

#print axioms ExperimentationInvariants.resolved_is_absorbing
#print axioms ExperimentationInvariants.recordFitness_preserves_invariants
#print axioms ExperimentationInvariants.recordFinalEval_preserves_invariants
