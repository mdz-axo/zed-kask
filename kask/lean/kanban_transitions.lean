/-
zed-kask spec pin (PRF-04 — `hkask-kata-kanban` Done-gate soundness and Done
WIP-limit preservation across `taskMove` and `taskVerify`).

The Rust side: `kask/crates/hkask-kata-kanban/src/kanban/service_impl/service.rs`
(`task_move`, `task_verify`). Slice S3 (`EFF-06`) fixed `task_verify` to use
`mutate_task_scanning` and enforce `Done`-column existence and `wip_limit`.

This file proves:
1. `taskMove_preserves_done_gate`: on a verification-gated board
   (`doneRequiresVerification = true`), `taskMove` preserves the invariant that
   every task in `.done` has `verified = true`.
2. `taskVerify_preserves_done_gate`: `taskVerify` preserves the Done-gate
   invariant (sets `verified := true` when moving to `.done`).
3. `taskVerify_preserves_done_wip`: when `doneWipLimit = some L`, `taskVerify`
   preserves `countDone tasks ≤ L`.

Check: `lean_check` on this file (pinned Lean 4.34.0); the paired
`kanban_transitions_negative_control.lean` must FAIL the same check.
-/

namespace KanbanTransitions

inductive TaskStatus where
  | backlog
  | ready
  | inProgress
  | review
  | done
deriving DecidableEq

structure Task where
  id : Nat
  status : TaskStatus
  verified : Bool
deriving DecidableEq

structure BoardPolicy where
  doneRequiresVerification : Bool
  doneWipLimit : Option Nat

def countDone (ts : List Task) : Nat :=
  (ts.filter (fun t => t.status == .done)).length

def DoneGateHolds (p : BoardPolicy) (ts : List Task) : Prop :=
  p.doneRequiresVerification = true → ∀ t ∈ ts, t.status = .done → t.verified = true

def WipHolds (p : BoardPolicy) (ts : List Task) : Prop :=
  match p.doneWipLimit with
  | none => True
  | some limit => countDone ts ≤ limit

/-- Replace a single task matching `id` with `f t`. -/
def updateTask (ts : List Task) (id : Nat) (f : Task → Task) : List Task :=
  ts.map (fun t => if t.id == id then f t else t)

/-- Pure `task_move` transition: refuses `.done` when `doneRequiresVerification = true`. -/
def taskMove (p : BoardPolicy) (ts : List Task) (id : Nat) (target : TaskStatus) :
    Option (List Task) :=
  if p.doneRequiresVerification && (target == .done) then
    none
  else
    some (updateTask ts id (fun t => { t with status := target }))

/-- Pure `task_verify` transition (Slice S3 `EFF-06`): transitions a `.review`
task to `.done` with `verified := true` only if `doneWipLimit` is not exceeded. -/
def taskVerify (p : BoardPolicy) (ts : List Task) (id : Nat) (passed : Bool) :
    Option (List Task) :=
  if !passed then
    some ts
  else
    match p.doneWipLimit with
    | some limit =>
        if countDone ts < limit then
          some (updateTask ts id (fun t =>
            if t.status == .review then { t with status := .done, verified := true } else t))
        else
          none
    | none =>
        some (updateTask ts id (fun t =>
          if t.status == .review then { t with status := .done, verified := true } else t))

theorem taskMove_preserves_done_gate
    {p : BoardPolicy} {ts ts' : List Task} {id : Nat} {target : TaskStatus}
    (hinv : DoneGateHolds p ts)
    (hstep : taskMove p ts id target = some ts') :
    DoneGateHolds p ts' := by
  intro hreq t ht hdone
  unfold taskMove at hstep
  cases htgt : target
  all_goals (
    simp only [hreq, htgt] at hstep
    try simp at hstep
  )
  all_goals (
    subst hstep
    unfold updateTask at ht
    simp only [List.mem_map] at ht
    rcases ht with ⟨t0, ht0, rfl⟩
    by_cases hid : (t0.id == id) = true
    · simp [hid] at hdone
    · simp only [hid] at hdone ⊢
      exact hinv hreq t0 ht0 hdone
  )

theorem taskVerify_preserves_done_gate
    {p : BoardPolicy} {ts ts' : List Task} {id : Nat} {passed : Bool}
    (hinv : DoneGateHolds p ts)
    (hstep : taskVerify p ts id passed = some ts') :
    DoneGateHolds p ts' := by
  intro hreq t ht hdone
  unfold taskVerify at hstep
  cases passed
  · simp only [Bool.not_false, ↓reduceIte, Option.some.injEq] at hstep
    subst hstep
    exact hinv hreq t ht hdone
  · simp only [Bool.not_true, Bool.false_eq_true, ↓reduceIte] at hstep
    cases hlim : p.doneWipLimit <;> simp only [hlim] at hstep
    · injection hstep with heq
      subst heq
      unfold updateTask at ht
      simp only [List.mem_map] at ht
      rcases ht with ⟨t0, ht0, rfl⟩
      by_cases hid : (t0.id == id) = true
      · simp only [hid, ↓reduceIte] at hdone ⊢
        by_cases hrev : (t0.status == .review) = true
        · simp [hrev]
        · simp only [hrev, Bool.false_eq_true, ↓reduceIte] at hdone ⊢
          exact hinv hreq t0 ht0 hdone
      · simp only [hid] at hdone ⊢
        exact hinv hreq t0 ht0 hdone
    · next limit =>
        by_cases hlt : countDone ts < limit
        · simp only [hlt, ↓reduceIte, Option.some.injEq] at hstep
          subst hstep
          unfold updateTask at ht
          simp only [List.mem_map] at ht
          rcases ht with ⟨t0, ht0, rfl⟩
          by_cases hid : (t0.id == id) = true
          · simp only [hid, ↓reduceIte] at hdone ⊢
            by_cases hrev : (t0.status == .review) = true
            · simp [hrev]
            · simp only [hrev, Bool.false_eq_true, ↓reduceIte] at hdone ⊢
              exact hinv hreq t0 ht0 hdone
          · simp only [hid] at hdone ⊢
            exact hinv hreq t0 ht0 hdone
        · simp [hlt] at hstep

private theorem map_noop_when_absent (l : List Task) (id : Nat) (f : Task → Task)
    (h : ∀ x ∈ l, (x.id == id) = false) :
    l.map (fun x => if x.id == id then f x else x) = l := by
  induction l with
  | nil => rfl
  | cons y ys ih =>
      have hy := h y (by simp)
      simp only [List.map_cons, hy, Bool.false_eq_true, ↓reduceIte]
      exact congrArg (List.cons y) (ih (fun z hz => h z (by simp [hz])))

/-- Updating at most one task (unique IDs) increases `countDone` by at most 1. -/
theorem countDone_update_unique (ts : List Task) (id : Nat)
    (huniq : (ts.filter (fun t => t.id == id)).length ≤ 1)
    (f : Task → Task) :
    countDone (updateTask ts id f) ≤ countDone ts + 1 := by
  induction ts with
  | nil => simp [countDone, updateTask]
  | cons t ts ih =>
      unfold countDone updateTask at *
      simp only [List.map_cons, List.filter_cons] at huniq ⊢
      by_cases hid : (t.id == id) = true
      · simp only [hid, ↓reduceIte, List.length_cons] at huniq ⊢
        have hzero : (ts.filter (fun x => x.id == id)).length = 0 := by omega
        have hnone : ∀ x ∈ ts, (x.id == id) = false := by
          intro x hx
          cases hxe : (x.id == id)
          · rfl
          · have hmem : x ∈ ts.filter (fun y => y.id == id) :=
              List.mem_filter.mpr ⟨hx, hxe⟩
            have hpos : 0 < (ts.filter (fun y => y.id == id)).length :=
              List.length_pos_of_mem hmem
            omega
        have hmap := map_noop_when_absent ts id f hnone
        rw [hmap]
        cases h1 : (f t).status == .done <;> cases h2 : t.status == .done <;>
          simp only [Bool.false_eq_true, ↓reduceIte, List.length_cons] <;> omega
      · simp only [hid, Bool.false_eq_true, ↓reduceIte] at huniq ⊢
        have ih' := ih huniq
        cases h2 : t.status == .done <;>
          simp only [Bool.false_eq_true, ↓reduceIte, List.length_cons] <;> omega

theorem taskVerify_preserves_done_wip
    {p : BoardPolicy} {ts ts' : List Task} {id : Nat} {passed : Bool}
    (huniq : (ts.filter (fun t => t.id == id)).length ≤ 1)
    (hwip : WipHolds p ts)
    (hstep : taskVerify p ts id passed = some ts') :
    WipHolds p ts' := by
  unfold WipHolds taskVerify at *
  cases passed
  · simp only [Bool.not_false, ↓reduceIte, Option.some.injEq] at hstep
    subst hstep
    exact hwip
  · simp only [Bool.not_true, Bool.false_eq_true, ↓reduceIte] at hstep
    cases hlim : p.doneWipLimit with
    | none =>
        simp only [hlim] at hwip hstep ⊢
    | some limit =>
        simp only [hlim] at hwip hstep ⊢
        by_cases hlt : countDone ts < limit
        · simp only [hlt, ↓reduceIte, Option.some.injEq] at hstep
          subst hstep
          have hinc := countDone_update_unique ts id huniq
            (fun t => if t.status == .review then { t with status := .done, verified := true } else t)
          omega
        · simp [hlt] at hstep

end KanbanTransitions

#print axioms KanbanTransitions.taskMove_preserves_done_gate
#print axioms KanbanTransitions.taskVerify_preserves_done_gate
#print axioms KanbanTransitions.taskVerify_preserves_done_wip
