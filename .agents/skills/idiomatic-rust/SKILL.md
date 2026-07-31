---
name: idiomatic-rust
visibility: public
description: "Idiomatic Rust design through Graydon Hoare's lens, grounded by rust-analyzer and clippy as extrinsic oracles. Convergent inquiry loop: anchor design problems against Hoare's principles using compiler diagnostics as ground truth, propose type-driven solutions informed by LSP code actions, challenge and refine through adversarial review verified against compiler feedback, and converge toward deeper, more idiomatic designs."
---

# Idiomatic Rust

Idiomatic Rust design through Graydon Hoare's lens, grounded by rust-analyzer
and clippy as extrinsic oracles. The LLM reasons about design through Hoare's
principles; the compiler verifies whether the reasoning is correct.

## The compiler as extrinsic oracle

Without compiler feedback, the skill operates on intrinsic reasoning alone —
the LLM assesses and critiques its own proposals. This is the Dunning-Kruger
trap: the LLM may confidently propose a design the borrow checker would reject.
The compiler is the extrinsic oracle that closes the gap:

| Phase | LSP tool | What it grounds |
|-------|----------|-----------------|
| Inquiry | `diagnostics` | Compiler errors/warnings as the starting point for assessment |
| Design | `get_code_actions` | Compiler-verified refactor suggestions as design starting points |
| Challenge | `find_references` + `diagnostics` | Actual blast radius + compile verification of proposed code |
| Challenge | `./script/clippy` | Codified idiomatic Rust lints as ground-truth design feedback |

Each compiler diagnostic is interpreted through the Hoare lens:
- Borrow checker error → Principle 2 (ownership is architecture)
- Type mismatch → Principle 1 (invalid states unrepresentable)
- Lifetime error → Principle 2 (ownership) or Principle 5 (explicit)
- Missing trait impl → Principle 6 (composition over inheritance)
- Panic path → Principle 7 (errors as values)
- Unsafe block → Principle 8 (unsafe as contract)

## When to Use

- Assessing a Rust design problem against Graydon Hoare's principles to identify invariants, invalid states, ownership graphs, and error domains.
- Proposing type-driven Rust solutions with code examples, applying algebraic types, ownership patterns, error propagation, and trait design.
- Conducting adversarial reviews of a Rust design proposal to find gaps, test edge cases, challenge assumptions, and identify deeper ecosystem connections.
- Computing a normalized convergence metric for an idiomatic-rust inquiry cycle to determine if further design refinement is needed.

## PDCA Loop

```
Plan:  Phase 1 — Inquiry   → Assess against Hoare's principles using rust-analyzer diagnostics as ground truth
Do:    Phase 2 — Design    → Propose type-driven solutions informed by LSP code actions
Check: Phase 3 — Challenge → Adversarial review verified by find_references, diagnostics, and clippy
Check: Phase 4 — Converge  → Cauchy criterion on critique score (design has stopped moving)
Act:   Phase 5 — Loop      → Re-enter inquiry (step 1) with refinement directives from challenge
```

The loop targets step 1 (inquiry), not step 2 (design), so challenge findings
re-inform the assessment. The agent re-runs `diagnostics` to check whether the
compiler's view has changed. This closes the feedback loop properly: challenge
findings → re-assess → re-design → re-challenge.

## Improvement Measure

**Field**: `step_4_result.convergence_metric`. **Threshold**: 0.25. **Max iterations**: 3.

The critique score (0.0 = design survives all challenges, 1.0 = design is
broken) is pushed into `kata_hypotenuse` for the Cauchy convergence check.
Oscillating scores (design improves on one dimension, challenge finds new
issues on another) indicate the design is not converging and may need
escalation.

## Instructions

### idiomatic-rust-inquiry

1. **Run `diagnostics`** on the target code (in Zed sessions). The compiler's errors and warnings are the starting point — not your guess.
2. Evaluate the current or proposed design against each of the eight Hoare principles, asking if it satisfies the principle, what specific states or relationships violate it, and the minimum change needed to satisfy it. Interpret each compiler diagnostic through the Hoare lens.
3. List all invariants that must always be true.
4. Identify all invalid states currently possible that should never occur.
5. Map the ownership graph, detailing who creates, observes, mutates, and destroys each value. Use `go_to_definition` to trace the actual ownership graph through the codebase.
6. Define the error domain, specifying what can fail, the handling level, and any silently swallowed errors.
7. Rank principle violations by severity. Mark each as `compiler_confirmed` (extrinsic) or LLM-identified only (intrinsic — lower confidence).
8. Order improvement targets by impact, specifying the exact type, ownership, or error changes needed. Note related LSP code actions where available.

### idiomatic-rust-design

1. **Run `get_code_actions`** on the target code (in Zed sessions). Rust-analyzer's refactor suggestions are compiler-verified starting points.
2. Design types that make wrong usage impossible by replacing `String` with validating newtypes, `bool` with two-variant enums, `Vec<T>` with non-empty types, raw integers with unit-aware newtypes, and invalid `Option<T>` with non-nullable types. Evaluate and extend the compiler's suggestions against the design goals.
3. Map the ownership DAG for each value, explicitly choosing single owners, shared immutable access, shared mutable access, or borrowed access.
4. Ensure every fallible function returns `Result<T, E>`, using `thiserror` for libraries and `anyhow` for applications, avoiding `unwrap()` in library code, and documenting all panics.
5. Design traits that define capabilities rather than taxonomies, preferring many small traits, using `impl Trait` in return positions, deriving common traits, and implementing `From<T>` for conversions.
6. Reference the current Rust ecosystem for each design decision, citing std library patterns, key crates, relevant RFCs, and API guidelines.
7. Record which design decisions were compiler-suggested vs LLM-originated in `compiler_actions_used`.

### idiomatic-rust-challenge

1. **Run `find_references`** on the types being changed (in Zed sessions). The actual blast radius — not your guess.
2. **Run `diagnostics`** on any proposed code. A design that doesn't compile is a critical gap, not a style issue.
3. **Run `./script/clippy`** on the target code (in Zed sessions). Clippy's lints are ground-truth idiomatic Rust feedback. Map each warning to a Hoare principle.
4. Find gaps where the design fails to address the original problem, misses scenarios, or leaves state transitions unhandled, citing specific types or functions. Mark each as `compiler_confirmed` where a diagnostic or clippy lint confirms it.
5. Test edge cases for each type, considering empty inputs, maximum values, concurrent access, errors at each step, and mid-operation shutdowns.
6. Challenge assumptions regarding correctness, performance, or safety by writing counterexamples that attempt to reach invalid states or expose hidden costs.
7. Find deeper connections to broader Rust patterns, comparing the design to std library types, popular crates, applicable RFCs, and API guidelines.
8. Produce refinement directives for each gap or edge case, stating the specific change required, the principle addressed, and the expected improvement. Note related compiler actions where available.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `idiomatic-rust-inquiry.j2` | KnowAct | Assess a Rust design problem against Graydon Hoare's principles using rust-analyzer diagnostics as ground truth. Identify invariants, invalid states, ownership graphs, and error domains. Produce a scored design assessment with specific improvement targets, marking each as compiler-confirmed or LLM-identified. |
| `idiomatic-rust-design.j2` | KnowAct | Propose type-driven Rust solutions with code examples, informed by LSP code actions. Apply algebraic types, ownership patterns, error propagation, and trait design. Reference std library patterns, ecosystem best practices, and relevant RFCs. Record which decisions were compiler-suggested vs LLM-originated. |
| `idiomatic-rust-challenge.j2` | KnowAct | Adversarial review of a Rust design proposal, grounded by find_references (blast radius), diagnostics (compile verification), and clippy (idiomatic lints). Find gaps, test edge cases, challenge assumptions, identify deeper connections. Produce a scored critique with specific refinement directives, marking each as compiler-confirmed or LLM-identified. |

## Fusion Mode

This skill inherits the operator's global `kask.fusion` settings (the manifest
omits the `fusion` block). Recommended configuration: **critique mode** (draft →
panel critiques → revise) to match the design review loop, with skills:
`[coding-guidelines, deep-module]`. Model names are not hardcoded in the
manifest because models evolve quickly; the operator configures the panel via
`kask.fusion.panel_models` or `HKASK_FUSION_PANEL_MODELS`.

**Optional Rust specialist model**: a Rust-specialized model (e.g.,
`strand-rust-coder` via Ollama, a Qwen2.5-Coder-14B fine-tune on 191K Rust
examples) may be included in the fusion panel for the design phase. Its
strengths in code generation, test generation, and refactoring complement the
LSP tools — it can generate candidate code while rust-analyzer verifies types.
Configure via `kask.fusion.panel_models`. Do not hardcode in the manifest.

## Constraints

- `idiomatic-rust-inquiry.j2`: Public.
- `idiomatic-rust-design.j2`: Public.
- `idiomatic-rust-challenge.j2`: Public.
- The loop targets step 1 (inquiry), not step 2 (design) — challenge findings must re-inform the assessment.
- The convergence check (step 5) is mandatory — the loop must not run until gas exhaustion.
- Step 4 uses `lisp.eval` to compute a custom design-quality score (weighted combination of critique score, compiler-confirmed findings, and unresolved issues). This demonstrates inline deterministic compute — no Rust change needed for custom scoring logic. See the manifest for the Lisp form. The interpreter supports both prefix (`(+ a b)`) and infix (`a + b`) operator notation — use infix for simple scoring expressions, prefix for complex nested logic.
- Compiler grounding is preferred but not required — when LSP tools are unavailable (pure FlowDef execution), the skill falls back to intrinsic reasoning with reduced confidence.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
