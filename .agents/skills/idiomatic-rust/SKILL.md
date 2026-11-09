---
name: idiomatic-rust
description: "Type-driven Rust design through eight design principles, grounded by rust-analyzer and clippy as extrinsic oracles. Propose type-driven solutions informed by LSP code actions and refine through adversarial review verified against compiler feedback."
audience: user
---

# Idiomatic Rust

Type-driven Rust design, grounded by rust-analyzer and clippy as extrinsic
oracles. The eight principles below are a **design lens** — a synthesis of
widely used Rust design mantras, not a citable spec. The citable anchors are
the Rust API Guidelines (rust-lang.github.io/api-guidelines) and *The Rust
Programming Language* (Klabnik & Nichols). Provenance notes:

- "Make invalid states unrepresentable" is widely attributed to **Yaron
  Minsky** (OCaml), not Graydon Hoare; it is a general type-system mantra.
- The Rust API Guidelines are the canonical, versioned reference for API
  design and are enforced as a checklist gate in the design phase (see
  *API Guidelines checklist gate*).

The LLM reasons about design through the eight principles; the compiler and
clippy verify whether the reasoning is correct.

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
| Challenge | `./script/clippy` (not LSP — the codified-lint oracle) | Codified idiomatic Rust lints as ground-truth design feedback |

Each compiler diagnostic is interpreted through the type-driven lens:
- Borrow checker error → Principle 2 (ownership is architecture)
- Type mismatch → Principle 1 (invalid states unrepresentable)
- Lifetime error → Principle 2 (ownership) or Principle 5 (explicit)
- Missing trait impl → Principle 6 (composition over inheritance)
- Panic path → Principle 7 (errors as values)
- Unsafe block → Principle 8 (unsafe as contract)

## D/P labelling

Design proposals (type-driven solutions, LSP-informed code actions) and the
adversarial review's verdicts are P — judgment, critiqued by the compiler and
clippy as the extrinsic oracles. The verification is D: diagnostics errors on
the proposed code, new `./script/clippy` warnings versus the baseline, and
compiler-confirmed findings, counted from the actual tool output and computed
with `lisp_eval` — a proposal the compiler has not seen is unreviewed, and a
critique the compiler contradicts is overruled.

## Initial and target condition

- **Initial condition:** the Rust design problem and constraints, actual existing/proposed code, baseline compiler diagnostics and clippy output, ownership/invariant/error-domain observations, and the real reference blast radius.
- **Target condition:** `IR-1` and `IR-2` pass and the Improvement Measure returns `converged`: the proposed code compiles, diagnostics contain no errors, clippy adds no warning, and all findings are reconciled with the compiler. `continue` re-enters inquiry with the challenge directives; `oscillating` escalates; `undetermined` remains unverified. Three iterations is the bound.

## When to Use

- Assessing a Rust design problem against the eight design principles to identify invariants, invalid states, ownership graphs, and error domains.
- Proposing type-driven Rust solutions with code examples, applying algebraic types, ownership patterns, error propagation, and trait design.
- Conducting adversarial reviews of a Rust design proposal to find gaps, test edge cases, challenge assumptions, and identify deeper ecosystem connections.
- Deciding whether an idiomatic-rust design has converged, from compiler diagnostics and clippy counts rather than a self-scored critique.

## When NOT to Use

- Non-Rust languages — this skill's oracles are rust-analyzer and clippy.
- Machine-checked proof — use `lean-prover`.
- Reviewing a change against its spec — use `code-review`; this skill designs, it does not adjudicate diffs.

## PDCA Loop

```
Plan:  Phase 1 — Inquiry   → Assess against the eight design principles using rust-analyzer diagnostics as ground truth
Do:    Phase 2 — Design    → Propose type-driven solutions informed by LSP code actions
Check: Phase 3 — Challenge → Adversarial review verified by find_references, diagnostics, and clippy
Check: Phase 4 — Converge  → evaluate the Improvement Measure gate (compiler-confirmed finding stability)
Act:   Phase 5 — Loop      → Re-enter inquiry (step 1) with refinement directives from challenge
```

The loop targets step 1 (inquiry), not step 2 (design), so challenge findings
re-inform the assessment. The agent re-runs `diagnostics` to check whether the
compiler's view has changed. This closes the feedback loop properly: challenge
findings → re-assess → re-design → re-challenge.

## Improvement Measure

**Gate (D — the compiler and clippy are the oracles):** after each challenge, count from the actual tool output: `diagnostics` errors on the proposed code, new `./script/clippy` warnings versus the baseline, and compiler-confirmed findings. Compute with `lisp_eval`:
- form: `(cond ((not clippy_ran) "undetermined") ((> errors 0) "continue") ((> new_warnings 0) "continue") ((and (>= (length confirmed) 3) (< (* (- (nth (- (length confirmed) 2) confirmed) (nth (- (length confirmed) 3) confirmed)) (- (nth (- (length confirmed) 1) confirmed) (nth (- (length confirmed) 2) confirmed))) 0)) "oscillating") (t "converged"))`
- env: `{ "clippy_ran": <true only if clippy ran on the proposed code>, "errors": <diagnostics errors>, "new_warnings": <clippy warnings not in the baseline>, "confirmed": [<compiler-confirmed finding counts, full history oldest first — the form reads the LAST three, so a tail oscillation is never hidden by earlier iterations>] }`

**Max iterations**: 3. `oscillating` (the confirmed-finding count went up then down, or down then up) escalates to the operator rather than looping. `undetermined` — no clippy run, so the challenge was LLM-only — never counts as converged. Gate inquiry completeness via `IR-1` (`(and diagnostics_ran (= unexplained_diagnostics 0) (> invariants_count 0))`) and challenge compile/blast-radius via `IR-2` (`(and proposed_code_compiles (= new_diagnostics 0) (> find_references_count 0))`) — `IR-2` requires at least one reference examined, so a challenge that never ran `find_references` cannot pass.

## Experiment record

The loop is durable: every challenge appends **one** record to
`.agents/skills/idiomatic-rust/experiments.jsonl` (append-only, one JSON object
per line, oldest first):

```json
{"iteration": 1, "next_step": "...", "prediction": "...", "outcome": "..."}
```

- `next_step` — the single change the challenge prescribes.
- `prediction` — what that change is expected to do to the gate.
- `outcome` — the observed gate result (`converged` / `continue` / `oscillating` / `undetermined`).

Write the record **after** the gate runs, so `outcome` is observed rather than
predicted. The inquiry phase reads the most recent record and carries its
`outcome` into `iteration_focus`; a session that starts cold begins at
iteration 1. Never rewrite a record — a changed prediction is a new iteration.
The `iteration` field is what makes the three-iteration bound visible: stop and
escalate when the next record would be iteration 4.

## Instructions

### idiomatic-rust-inquiry

**Read the experiment record first.** If `.agents/skills/idiomatic-rust/experiments.jsonl` exists, read the most recent record and carry its `outcome` into `iteration_focus`; a session that starts cold begins at iteration 1.

1. **Run `diagnostics`** on the target code (in Zed sessions). The compiler's errors and warnings are the starting point — not your guess.
2. Evaluate the current or proposed design against each of the eight design principles, asking if it satisfies the principle, what specific states or relationships violate it, and the minimum change needed to satisfy it. Interpret each compiler diagnostic through the type-driven lens.
3. List all invariants that must always be true.
4. Identify all invalid states currently possible that should never occur.
5. Map the ownership graph, detailing who creates, observes, mutates, and destroys each value. Use `go_to_definition` to trace the actual ownership graph through the codebase.
6. Define the error domain, specifying what can fail, the handling level, and any silently swallowed errors.
7. Rank principle violations by severity. Mark each as `compiler_confirmed` (extrinsic) or LLM-identified only (intrinsic — lower confidence).
8. Order improvement targets by impact, specifying the exact type, ownership, or error changes needed. Note related LSP code actions where available.

### idiomatic-rust-design

1. **Run `get_code_actions`** on the target code (in Zed sessions). Rust-analyzer's refactor suggestions are compiler-verified starting points.
2. Design types that make wrong usage impossible by replacing `String` with validating newtypes, `bool` with two-variant enums, `Vec<T>` with non-empty types, raw integers with unit-aware newtypes, and invalid `Option<T>` with non-nullable types. Where a plain replacement type is not enough, extend the same move with the deeper pattern menu (absorbed from actionbook/rust-skills `m05-type-driven`): typestate — state machines as type parameters so invalid transitions do not compile (`Connection<Disconnected>` consumed to produce `Connection<Connected>`); sealed traits — a closed implementor set without closing use (`mod private { pub trait Sealed {} }`); marker traits — compile-time capability flags (`trait Validated {}`); `PhantomData<T>` — type-level relationships at zero runtime cost; ZST markers — unit structs as compile-time tags (`struct Invariant;`). Evaluate and extend the compiler's suggestions against the design goals.
3. Map the ownership DAG for each value, explicitly choosing single owners, shared immutable access, shared mutable access, or borrowed access.
4. Ensure every fallible function returns `Result<T, E>`, using `thiserror` for libraries and `anyhow` for applications, avoiding `unwrap()` in library code, and documenting all panics.
5. Design traits that define capabilities rather than taxonomies, preferring many small traits, using `impl Trait` in return positions, deriving common traits, and implementing `From<T>` for conversions.
6. Reference the current Rust ecosystem for each design decision, citing std library patterns, key crates, relevant RFCs, and API guidelines.
7. Record which design decisions were compiler-suggested vs LLM-originated in `compiler_actions_used`.
8. **Run the Rust API Guidelines checklist** against the proposed public API (https://rust-lang.github.io/api-guidelines/checklist.html). Record every violated guideline in `guideline_violations` with its guideline ID and the proposed item; a public API that violates a guideline without a recorded justification is not converged.

### idiomatic-rust-challenge

1. **Run `find_references`** on the types being changed (in Zed sessions). The actual blast radius — not your guess.
2. **Run `diagnostics`** on any proposed code. A design that doesn't compile is a critical gap, not a style issue.
3. **Run `./script/clippy`** on the target code (in Zed sessions). Clippy's lints are ground-truth idiomatic Rust feedback. Map each warning to a design principle.
4. Find gaps where the design fails to address the original problem, misses scenarios, or leaves state transitions unhandled, citing specific types or functions. Mark each as `compiler_confirmed` where a diagnostic or clippy lint confirms it.
5. Test edge cases for each type, considering empty inputs, maximum values, concurrent access, errors at each step, and mid-operation shutdowns.
6. Challenge assumptions regarding correctness, performance, or safety by writing counterexamples that attempt to reach invalid states or expose hidden costs.
7. Find deeper connections to broader Rust patterns, comparing the design to std library types, popular crates, applicable RFCs, and API guidelines.
8. Produce refinement directives for each gap or edge case, stating the specific change required, the principle addressed, and the expected improvement. Note related compiler actions where available.
9. **Check `unsafe` soundness with Miri** where the proposed code contains `unsafe` blocks: run `cargo +nightly miri` on the target (Miri requires the nightly toolchain — skip loudly if it is absent). A Miri-reported undefined behaviour is a critical gap. Record the result in `miri_checked` and the block count in `unsafe_blocks`.

## Curated external idiom annex

Six rules from leonardomso/rust-skills (MIT), pinned to upstream commit
`fd2a861ab0406a4ac536a55274d14ea6fd1ca9c9` (master as of 2026-06-14; 265 rules,
"current for Rust 1.96 / 2024 edition" per the upstream README). Selected from
categories clippy does not lint — the gap the codified-lint oracle cannot
close. Every entry is intrinsic guidance (P) until verified against the
compiler/clippy oracles: cite the rule, then check the code.

**Refresh trigger:** refresh this annex when the local toolchain's minor
version advances past the pinned Rust version (the workspace is on 1.98.1), or
when upstream changes a pinned rule. Last refreshed 2026-11-09. The pinned
commit is what the links below resolve against; update the pin and the links
together.

- [serde-enum-representation](https://raw.githubusercontent.com/leonardomso/rust-skills/fd2a861ab0406a4ac536a55274d14ea6fd1ca9c9/rules/serde-enum-representation.md) — choose enum tagging deliberately: externally, internally, adjacently tagged, or untagged.
- [serde-try-from-validate](https://raw.githubusercontent.com/leonardomso/rust-skills/fd2a861ab0406a4ac536a55274d14ea6fd1ca9c9/rules/serde-try-from-validate.md) — validate while deserializing with `#[serde(try_from = "Raw")]`.
- [mem-drop-order](https://raw.githubusercontent.com/leonardomso/rust-skills/fd2a861ab0406a4ac536a55274d14ea6fd1ca9c9/rules/mem-drop-order.md) — struct fields drop top-to-bottom, locals in reverse declaration order; control drop order deliberately.
- [unsafe-extern-block](https://raw.githubusercontent.com/leonardomso/rust-skills/fd2a861ab0406a4ac536a55274d14ea6fd1ca9c9/rules/unsafe-extern-block.md) — Rust 2024: wrap `extern` blocks in `unsafe extern { }` and annotate each item `safe` or `unsafe`.
- [unsafe-no-mangle-unsafe](https://raw.githubusercontent.com/leonardomso/rust-skills/fd2a861ab0406a4ac536a55274d14ea6fd1ca9c9/rules/unsafe-no-mangle-unsafe.md) — Rust 2024: `#[unsafe(no_mangle)]`, `#[unsafe(export_name = "...")]`, `#[unsafe(link_section = "...")]` — not the bare attribute forms.
- [async-cancel-safety](https://raw.githubusercontent.com/leonardomso/rust-skills/fd2a861ab0406a4ac536a55274d14ea6fd1ca9c9/rules/async-cancel-safety.md) — futures used in `tokio::select!` branches must be cancellation-safe.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `idiomatic-rust-inquiry.j2` | Assess a Rust design problem against the eight design principles, using rust-analyzer diagnostics as ground truth. The compiler's errors and warnings are the starting point — not the LLM's guess. Interpret each diagnostic through the type-driven lens (borrow checker → ownership, type mismatch → invalid states, etc.). Identify invariants, invalid states, ownership graphs, and error domains. Produce a scored design assessment with specific improvement targets, marking each as compiler-confirmed (extrinsic) or LLM-identified (intrinsic — lower confidence). |
| `idiomatic-rust-design.j2` | Propose type-driven Rust solutions with code examples, informed by LSP code actions. Rust-analyzer's refactor suggestions (replace with enum, extract struct, add missing trait impl) are compiler-verified starting points. Apply algebraic types, ownership patterns, error propagation, and trait design. Reference std library patterns, ecosystem best practices, and relevant RFCs. Record which decisions were compiler-suggested vs LLM-originated. |
| `idiomatic-rust-challenge.j2` | Adversarial review of a Rust design proposal, grounded by compiler feedback. Uses find_references (actual blast radius, not LLM's guess), diagnostics (compile verification of proposed code), and clippy (codified idiomatic Rust lints as ground-truth design feedback). Find gaps, test edge cases, challenge assumptions, identify deeper connections. Produce a scored critique with specific refinement directives, marking each as compiler-confirmed or LLM-identified. |

To render a template, call the `render_template` tool with the template ref (e.g., `idiomatic-rust/idiomatic-rust-inquiry`) and a context object with the required variables.

Template context variables (from each template's [inference] contract):
- `idiomatic-rust-inquiry.j2`: `design_problem`, `existing_code`, `design_constraints`, `compiler_diagnostics`, `iteration_focus`
- `idiomatic-rust-design.j2`: `design_problem`, `existing_code`, `assessment`, `code_actions`, `iteration_focus`
- `idiomatic-rust-challenge.j2`: `design_problem`, `design_proposal`, `assessment`, `design_constraints`, `clippy_lints`

## Regression case

All receipts executed live through `lisp_eval` (2026-10-01, batch-11 audit for
the original form; 2026-11-09 for the corrected last-three window and `IR-2`).

- Gate, converged: `{clippy_ran: true, errors: 0, new_warnings: 0,
  confirmed: [3, 3, 3]}` → `"converged"`.
- Gate, oscillating: `confirmed: [2, 4, 3]` (up then down) → `"oscillating"`
  (the adjacent-difference product is negative — escalates to the operator).
- Gate, tail oscillation (the 2026-11-09 fix): `confirmed: [3, 3, 3, 2, 4, 3]`
  → `"oscillating"`. Under the previous form this returned `"converged"`
  because it read indices 0–2; the corrected form reads the last three.
- Gate, undetermined: `{clippy_ran: false, ...}` → `"undetermined"` — an
  LLM-only challenge never counts as converged.
- Gate, continue on errors: `{errors: 1, ...}` → `"continue"`.
- Gate, short confirmed list: `confirmed: [3, 3]` → `"converged"` — the
  length guard short-circuits the oscillation check (no out-of-bounds `nth`;
  the clean-compiler branch dominates when errors and new warnings are 0).
- `IR-2`, zero references: `{proposed_code_compiles: true, new_diagnostics: 0,
  find_references_count: 0}` → `false` (the 2026-11-09 fix; the previous
  `(>= ... 0)` conjunct was vacuous and returned `true`).
- `IR-2`, references examined: `{..., find_references_count: 12}` → `true`.

The skill's form is executed at use time, never anchored in code.

## Constraints

- The loop targets step 1 (inquiry), not step 2 (design) — challenge findings must re-inform the assessment.
- The Improvement Measure gate runs after each challenge (not only at
  iteration exhaustion) — the loop must not run unbounded; `oscillating`
  escalates rather than loops.
- The interpreter supports both prefix (`(+ a b)`) and infix (`a + b`) operator notation — use infix for simple scoring expressions, prefix for complex nested logic.
- Compiler grounding is preferred but not required — when LSP tools are unavailable (pure skill execution), the skill falls back to intrinsic reasoning with reduced confidence.
