/-
Negative control for `experimentation_invariants.lean` (PRF-02).

Omits the `r ∈ s.fitnessRuns` disjointness guard in `recordFinalEvalBuggy` and
attempts to prove held-out disjointness is preserved. Must FAIL `lean_check`.
-/

namespace ExperimentationInvariantsNegControl

structure ExpState where
  fitnessRuns : List String
  finalRuns : List String

def Disjoint (s : ExpState) : Prop :=
  ∀ r, r ∈ s.fitnessRuns → r ∉ s.finalRuns

def recordFinalEvalBuggy (s : ExpState) (r : String) : ExpState :=
  { s with finalRuns := r :: s.finalRuns }

theorem buggy_final_preserves_disjoint_must_fail
    (s : ExpState) (r : String) (h : Disjoint s) :
    Disjoint (recordFinalEvalBuggy s r) := by
  intro x hx hxfin
  simp only [recordFinalEvalBuggy, List.mem_cons] at hxfin
  rcases hxfin with rfl | hxfin'
  · exact h x hx (by simp)
  · exact h x hx hxfin'

end ExperimentationInvariantsNegControl
