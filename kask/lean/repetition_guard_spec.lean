/-
zed-kask spec pin (agent-loop guardrails plan — Candidate 2, repetition
stop-loss).

The Rust side: `ToolRetryTracker` (crates/agent/src/tool_retry_tracker.rs)
tracks a per-`(tool, input_hash)` consecutive-success streak. Before each
dispatch, `check_repetition` clears the key's streak when a different key
was dispatched since its last success (reset-on-distinct — restored by
the 2026-10-02 plan audit; the first implementation kept a per-key map
with no reset, so interleaved identical calls accumulated), so only
back-to-back identical dispatches advance the streak. The verdict ladder
mirrors the failure-side ladder: a teaching hint at WARN_THRESHOLD (3),
refusal at the hard cap (5).

This file pins the streak counter's structure at spec level. The spec's
`Batch` corresponds to the implementation's `(tool_name, input_hash)`
key, and its single `last`-state models the implementation's
`last_dispatched` chain state: an identical consecutive dispatch advances
the streak, any distinct dispatch resets it to 1. The implementation
checks the PRIOR streak before dispatch (the hint lands on the dispatch
after the 3rd identical success) — the full mapping is recorded in the
plan document (kask/docs/plans/agent-loop-guardrails-plan.md §12). The
model is the guard's contract, not a verification of the Rust
implementation; the implementation is pinned by the tracker's Rust tests
named in the plan document.

Check: `lean_check` on this file (pinned toolchain lean-toolchain,
Lean 4.34.0). Negative control, run separately and expected to FAIL:
`example : (1 : Nat) = 2 := by decide` (kask/lean/negative_control.lean
holds the standing control).
-/

namespace RepetitionGuard

/-- One assistant message's tool-call batch, identified by its signature. -/
structure Batch where
  sig : Nat
  deriving DecidableEq

/-- Guard state: the previous batch's signature and the current streak. -/
structure State where
  last : Option Batch
  streak : Nat

/-- Initial state: nothing dispatched yet. -/
def init : State where
  last := none
  streak := 0

/-- The streak after dispatching batch `b` in state `st`: an identical
consecutive signature advances the streak; any distinct signature (or a
first dispatch) resets it to 1. -/
def nextStreak (st : State) (b : Batch) : Nat :=
  if st.last = some b then st.streak + 1 else 1

/-- One dispatch transitions the state. -/
def step (st : State) (b : Batch) : State :=
  { last := some b, streak := nextStreak st b }

/-- The verdict ladder over the streak a dispatch produced. -/
inductive Verdict where
  | allow | hint | refuse
  deriving DecidableEq

def verdict (st : State) (hintAt refuseAt : Nat) : Verdict :=
  if st.streak ≥ refuseAt then Verdict.refuse
  else if st.streak ≥ hintAt then Verdict.hint
  else Verdict.allow

/-- First dispatch: no previous batch, so the streak starts at 1. -/
theorem first_dispatch_streak_one (b : Batch) : nextStreak init b = 1 := by
  simp [nextStreak, init]

/-- Reset-on-change: a signature distinct from the previous batch's always
yields streak 1 — legitimate new work never inherits a stale streak. -/
theorem nextStreak_distinct_resets {st : State} {b b' : Batch}
    (hne : b ≠ b') (hlast : st.last = some b) :
    nextStreak st b' = 1 := by
  have hcond : ¬(st.last = some b') := by
    intro heq
    rw [hlast] at heq
    exact hne (Option.some.inj heq)
  simp [nextStreak, hcond]

/-- Streak shape (the conservation property): every dispatch either resets
to 1 or increments the previous streak by exactly 1 — the streak can never
jump, so a streak of K certifies K consecutive identical dispatches. -/
theorem nextStreak_shape (st : State) (b : Batch) :
    nextStreak st b = 1 ∨ nextStreak st b = st.streak + 1 := by
  by_cases h : st.last = some b <;> simp [nextStreak, h]

/-- No starvation: with any refusal threshold ≥ 2, the first occurrence of a
distinct signature stays strictly below it — legitimate new work always
runs, so the guard can only ever refuse a *repeat*, never new work. -/
theorem nextStreak_distinct_below_cap {st : State} {b b' : Batch}
    (hne : b ≠ b') (hlast : st.last = some b) {refuseAt : Nat}
    (hK : 2 ≤ refuseAt) :
    nextStreak st b' < refuseAt := by
  have h1 : nextStreak st b' = 1 := nextStreak_distinct_resets hne hlast
  omega

/-- Refusal requires the streak: the verdict refuses only at or above the
refusal threshold, so a refusal certifies `refuseAt` consecutive identical
dispatches (by `nextStreak_shape`, the streak cannot arrive there any other
way). -/
theorem verdict_refuse_implies_streak (st : State) (hintAt refuseAt : Nat)
    (hv : verdict st hintAt refuseAt = Verdict.refuse) :
    st.streak ≥ refuseAt := by
  unfold verdict at hv
  by_cases h : st.streak ≥ refuseAt
  · exact h
  · rw [if_neg h] at hv
    by_cases h' : st.streak ≥ hintAt
    · rw [if_pos h'] at hv; simp at hv
    · rw [if_neg h'] at hv; simp at hv

end RepetitionGuard
