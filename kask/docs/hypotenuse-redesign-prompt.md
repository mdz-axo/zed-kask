# Hypotenuse Convergence Logic — Redesign Task

## Context

The kask skill manifest system (`kask/registry/manifests/*.yaml`) uses a convergence mechanism called `kata.convergence_check` implemented in `kask/crates/hkask-templates/src/compute.rs` (around line 250). The mechanism has accumulated confusion and needs redesign or removal.

## The current state (verified by reading the code)

The `kata.convergence_check` compute primitive accepts these inputs:
- `hypotenuse` (f64) — supposed to be the "gap" (distance to target condition)
- `hypotenuse_epsilon` (f64, default 0.05) — gap convergence threshold
- `hypotenuse_history` (Vec<f64>) — history of hypotenuse values across iterations
- `cauchy_epsilon` (f64, default 0.03) — Cauchy convergence threshold
- `cauchy_window` (u64, default 3) — window size for Cauchy check
- `brier_history` (Vec<f64>) — Brier score history
- `brier_threshold` (f64, default 0.15) — calibration convergence threshold
- `brier_window` (u64, default 3) — window size for Brier check
- `mode` (string, default "gap_or_cauchy_or_calibration") — which convergence path(s) to use

Three convergence paths:
1. **Gap**: `hypotenuse < hypotenuse_epsilon` (limit of a sequence — the gap closed)
2. **Cauchy**: max pairwise delta in `hypotenuse_history` window < `cauchy_epsilon` (iterates stopped moving)
3. **Calibration**: rolling Brier average < `brier_threshold` for `brier_window` cycles

The `ConvergenceTracker` in `kask/crates/hkask-templates/src/convergence.rs` maintains `hypotenuse_history` by reading `kata_hypotenuse` from the context (pushed by each manifest's loop step).

## The confusion

The term "hypotenuse" is borrowed from the Kata Improvement Kata (`kata.hypotenuse` compute primitive, which computes `sqrt(object_gap^2 + process_gap^2)` — the Euclidean distance between current and target condition). In the Kata context, "hypotenuse" makes sense: it's the geometric distance to the target.

But `kata.convergence_check` reused the term "hypotenuse" for a *generic convergence signal* that manifests push from their loop steps. The result is:

1. **`hypotenuse` is overloaded.** In `kata.hypotenuse` it means "Euclidean gap distance." In `kata.convergence_check` it means "whatever scalar the manifest wants to converge on." These are different concepts sharing a name.

2. **`hypotenuse: 0.0` is dead code.** 40+ manifests hardcode `hypotenuse: 0.0` in their `kata.convergence_check` step while using `mode: "cauchy"`. In "cauchy" mode, the `hypotenuse` field is never read — only `hypotenuse_history` matters. The `0.0` is misleading dead code that suggests a gap signal exists when it doesn't.

3. **`kata_hypotenuse` is a phantom in most manifests.** Manifests push `kata_hypotenuse: "{{ step_N_result.convergence_metric | default(1.0) }}"` where `convergence_metric` is a field the LLM template often doesn't produce. The `default(1.0)` silently substitutes 1.0, making the Cauchy tracker see a flat `[1.0, 1.0, 1.0]` history → premature convergence at `min_iterations`. This is a bug, not a design choice.

4. **The Cauchy check on `hypotenuse_history` is really "did the signal stop moving."** Calling the signal "hypotenuse" implies it's a gap distance that should decrease to zero. But most manifests push counts (violation count, finding count, item count) or LLM-generated scores — these aren't gap distances. The Cauchy check works on any scalar, but the naming creates false intuition that the signal should be monotonically decreasing toward zero.

5. **`kata.hypotenuse` (the real Euclidean gap) is only used by `sequential-inquiry`.** It computes `sqrt(object_gap^2 + process_gap^2)` from `kata.object_gap` and `kata.process_gap` results. Only `sequential-inquiry.yaml` actually wires this path. Every other manifest either hardcodes 0.0 or pushes a non-gap scalar.

## The design question

Should `kata.convergence_check`:

**Option A: Keep the three-path model but rename.** Rename `hypotenuse` → `signal` (or `convergence_signal`), `hypotenuse_history` → `signal_history`, `hypotenuse_epsilon` → `gap_epsilon`. Keep `kata.hypotenuse` as-is for the Kata-specific Euclidean gap. This separates the generic convergence signal from the Kata-specific gap distance. Breaking change for all manifests (rename fields).

**Option B: Simplify to Cauchy-only.** Remove the gap and calibration paths entirely. Most manifests use `mode: "cauchy"` anyway. The convergence check becomes: "did the signal stop moving across the window?" The signal is whatever the manifest pushes via `kata_hypotenuse` (renamed to `convergence_signal`). The gap path (`hypotenuse < epsilon`) and calibration path (Brier) are dead code for 95% of manifests. Keep `kata.hypotenuse` as a separate primitive for `sequential-inquiry`'s use case.

**Option C: Make the signal explicit.** Replace `kata.convergence_check` with a `lisp.eval`-based convergence check that each manifest writes inline. The manifest's Lisp form computes both the signal AND the convergence decision. No more phantom fields — the form is right there in the YAML, auditable. The `kata.convergence_check` primitive becomes a thin wrapper or is removed. This is the most flexible but requires every manifest to write convergence logic.

**Option D: Hybrid — keep `kata.convergence_check` for the Cauchy check only, remove gap/calibration paths, rename `hypotenuse` → `signal`, and let manifests that need gap convergence use `lisp.eval` to compute the gap and compare against epsilon inline.** This is Option B + Option C for the gap path.

## Files to read

- `kask/crates/hkask-templates/src/compute.rs` lines 248-420 (the `kata.convergence_check` implementation)
- `kask/crates/hkask-templates/src/convergence.rs` (the `ConvergenceTracker` that maintains `hypotenuse_history`)
- `kask/registry/manifests/sequential-inquiry.yaml` (the only manifest using real `kata.hypotenuse`)
- `kask/registry/manifests/lisp-scaffold-reasoning.yaml` (the reference skill that uses `lisp.eval` for convergence scoring)
- `kask/registry/manifests/constraint-forces-recast.yaml` (uses `lisp.eval` for a custom Pareto convergence formula)
- `kask/crates/hkask-templates/src/compute.rs` — find `kata.hypotenuse` dispatch (the Euclidean gap primitive)

## Constraints

- This is a Rust change (compute.rs, convergence.rs) + a YAML change (every manifest using `kata.convergence_check`).
- The `ConvergenceTracker` is used by the executor — changes to its API affect the executor.
- Tests in `compute.rs` and `convergence.rs` pin the current behavior.
- The `kata.hypotenuse` primitive (Euclidean gap) should be preserved — it's used by `sequential-inquiry` and makes sense in the Kata context.
- Do not break `sequential-inquiry`'s gap convergence path — it's the one manifest using the gap path correctly.
- The `lisp.eval` compute primitive exists and can compute arbitrary convergence formulas — consider whether `kata.convergence_check` should be replaced by `lisp.eval` in manifests that need custom convergence logic.
