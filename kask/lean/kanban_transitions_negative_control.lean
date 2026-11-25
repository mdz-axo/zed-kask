/-
Negative control for `kanban_transitions.lean` (PRF-04).

Omits the `doneWipLimit` guard in `taskVerifyBuggy` (modeling the pre-Slice-S3
`EFF-06` bug) and attempts to prove `countDone` stays `≤ limit`. Must FAIL
`lean_check`.
-/

namespace KanbanTransitionsNegControl

inductive TaskStatus where
  | review
  | done
deriving DecidableEq

structure Task where
  id : Nat
  status : TaskStatus

def countDone (ts : List Task) : Nat :=
  (ts.filter (fun t => t.status == .done)).length

def taskVerifyBuggy (ts : List Task) (id : Nat) : List Task :=
  ts.map (fun t => if t.id == id then { t with status := .done } else t)

theorem buggy_verify_preserves_wip_must_fail
    (ts : List Task) (id limit : Nat)
    (hwip : countDone ts ≤ limit) :
    countDone (taskVerifyBuggy ts id) ≤ limit := by
  omega

end KanbanTransitionsNegControl
