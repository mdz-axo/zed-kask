/-
zed-kask spec pin (unsloth transfer T5 — the MTP draft routing invariants).

The Rust side: `compile_placement` (kask/crates/hkask-private-inference/
src/local_leg/placement.rs) routes a declared draft module to the GPU
exactly when the VRAM budget covers the used set (offloadable dense +
KV estimate + compute buffer + the spec-decode VERIFY workspace — the
MTP record §7 item 1 term, GPU-side whenever a draft is asked, placement-
independent per the D-B finding) AND the headroom after that covers the
draft's bytes; otherwise the draft rides the CPU (`-ngld 0`). The extended
`VramDenseFit` guard (`evaluate_constraint`,
kask/crates/hkask-private-inference/src/local_leg/manifest.rs) re-derives
the VRAM channel with the verify workspace UNCONDITIONALLY inside the
resident sum (a CPU-routed draft still verifies in GPU memory) and the
GPU-placed draft's bytes, and refuses the plan when it exceeds the
budget.

Modeled over ℕ with the STATED ASSUMPTION that all quantities and their
sums fit u64 — enforced on the Rust side by construction: every guard sum
is checked (an overflow refuses the evaluation with a typed error, pinned
by `byte_sum_overflow_is_a_typed_error_not_a_wrap`), and the routing
predicate's subtraction is guarded by its own `≥` short-circuit (the
zero-byte-draft corner, pinned by
`zero_byte_draft_never_rides_a_saturated_headroom`). The model is the
routing-to-guard contract, not a verification of the Rust implementation;
the implementation is pinned by the placement.rs draft tests (the
routing corner, the typed refusals, the emission shapes).

A proof never establishes that the formalization captures intent — the
T6 measurement and operator review carry that.

Check: `lean_check` on this file (pinned toolchain, lean-toolchain).
-/

namespace DraftRouting

/-- The compiler's routing decision (placement.rs, `compile_placement`):
a declared draft rides the GPU exactly when the VRAM budget covers the
used set (offloadable dense + KV estimate + compute buffer + the
spec-decode verify workspace — GPU-side whenever a draft is asked, the
MTP record §7 item 1 term that the D-B OOM proved placement-independent)
and the headroom after that covers the draft's bytes. -/
def RoutesToGpu (offloadable kv compute workspace draft vram : Nat) : Prop :=
  vram ≥ offloadable + kv + compute + workspace
    ∧ vram - (offloadable + kv + compute + workspace) ≥ draft

/-- The extended VramDenseFit guard (manifest.rs, `evaluate_constraint`,
the draft branch): allow exactly when the offloadable dense set + the KV
estimate + the verify workspace (UNCONDITIONALLY — the CPU-routed draft
still verifies in GPU memory) + the GPU-placed draft's bytes fit the
budget. -/
def VramGuardAllows (offloadable kv workspace draft vram : Nat) : Prop :=
  offloadable + kv + workspace + draft ≤ vram

/-- SOUNDNESS: a GPU-routed draft always passes the extended VRAM guard.
The routing condition is strictly tighter than the guard's sum — it also
covers the compute buffer — so the implication is unconditional in the
model: no GPU-routed draft plan is ever refused by the guard it feeds. -/
theorem gpu_routed_draft_passes_the_vram_guard
    (offloadable kv compute workspace draft vram : Nat)
    (h : RoutesToGpu offloadable kv compute workspace draft vram) :
    VramGuardAllows offloadable kv workspace draft vram := by
  obtain ⟨hfit, hhead⟩ := h
  unfold VramGuardAllows
  omega

/-- The emitted draft offload tier (placement.rs, the emission block):
the GPU tier's layer count when routed to the GPU, else zero. -/
def emittedNgl (on_gpu : Bool) (gpu_layers : Nat) : Nat :=
  if on_gpu then gpu_layers else 0

/-- TOTALITY: a declared draft always emits an offload tier of zero or
the GPU tier's count — there is no third value and no uninitialized
state; a CPU-routed draft is explicitly pinned (`-ngld 0`), never
implicit. -/
theorem emitted_ngl_total (on_gpu : Bool) (gpu_layers : Nat) :
    emittedNgl on_gpu gpu_layers = 0
      ∨ emittedNgl on_gpu gpu_layers = gpu_layers := by
  cases on_gpu <;> simp [emittedNgl]

end DraftRouting

#print axioms DraftRouting.gpu_routed_draft_passes_the_vram_guard
#print axioms DraftRouting.emitted_ngl_total
