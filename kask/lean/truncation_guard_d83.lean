/-
zed-kask spec pin (D36 guard invariant, D83 cause-removal context).

The Rust side: `ChatCompletionEventMapper` (crates/language_model_core/src/
chat_completion.rs) drains accumulated tool calls only when every call's
arguments parse as complete JSON; otherwise it emits
`LanguageModelCompletionError::ToolCallTruncated`, dispatches nothing, and
emits no stop event. D83 (crates/open_router/src/open_router.rs) removes the
dominant trigger: the OpenRouter request now carries the model's discovered
output cap as `max_tokens`, so the provider's own default limit no longer
binds and the stream rarely ends mid-tool-call at all.

This file pins the guard's structure at spec level. `complete` is an
UNINTERPRETED oracle for "serde_json parses this string as a complete JSON
document" — the theorems hold for every oracle, so they hold for serde_json's
counterpart without modeling it. The model is the guard's contract, not a
verification of the Rust implementation; the implementation is pinned by the
D36/D83 Rust tests named in DIVERGENCE.md.

Check: `lean_check` on this file (pinned toolchain lean-toolchain,
Lean 4.34.0); the paired `negative_control.lean` must FAIL the same check.
-/

namespace TruncationGuard

structure ToolCall where
  name : String
  args : String

/-- `allComplete complete calls`: every accumulated argument string in the
batch is complete JSON, per the oracle. `complete` is explicit and
uninterpreted — the theorems are parametric in it. -/
def allComplete (complete : String → Bool) : List ToolCall → Bool
  | [] => true
  | c :: cs => complete c.args && allComplete complete cs

theorem allComplete_forall (complete : String → Bool) {l : List ToolCall} :
    allComplete complete l = true ↔ ∀ c ∈ l, complete c.args = true := by
  induction l with
  | nil => simp [allComplete]
  | cons c cs ih => simp [allComplete, ih]

/-- The D36 drain guard: dispatch the whole batch only when every
accumulated argument string is complete JSON; otherwise classify truncation
and dispatch nothing. -/
def drain (complete : String → Bool) (calls : List ToolCall) :
    Option (List ToolCall) :=
  if allComplete complete calls then some calls else none

/-- Dispatch safety: whatever the guard dispatches is the whole batch, and
every dispatched argument is complete JSON. A fragment never reaches a tool. -/
theorem drain_dispatches_only_complete (complete : String → Bool)
    {calls dispatched : List ToolCall}
    (h : drain complete calls = some dispatched) :
    dispatched = calls ∧ ∀ c ∈ dispatched, complete c.args = true := by
  unfold drain at h
  split at h
  · next hcomplete =>
      injection h with heq
      subst heq
      exact ⟨rfl, fun c hc => (allComplete_forall complete (l := _)).mp hcomplete c hc⟩
  · next _ => simp at h

/-- Fragment safety: any incomplete argument in the batch forces the
truncation classification — nothing dispatches, not even the complete
siblings. -/
theorem drain_fragments_dispatch_nothing (complete : String → Bool)
    {calls : List ToolCall}
    (h : ∃ c ∈ calls, complete c.args = false) :
    drain complete calls = none := by
  unfold drain
  have hnot : ¬(allComplete complete calls = true) := by
    obtain ⟨c, hc, hfalse⟩ := h
    intro hall
    have hct := (allComplete_forall complete (l := _)).mp hall c hc
    rw [hfalse] at hct
    exact Bool.noConfusion hct
  simp [hnot]

end TruncationGuard

#print axioms TruncationGuard.drain_dispatches_only_complete
#print axioms TruncationGuard.drain_fragments_dispatch_nothing
