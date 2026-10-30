---
name: lean-prover
description: "Machine-checked proof construction. Anchor proof obligations against the Prop/Type discipline, construct proofs via tactics and term mode, challenge through counterexample search, and converge toward proofs that compile and erase."
audience: user
---

# Lean Prover

Machine-checked proof construction through the lens of Curry-Howard, de Bruijn,
and Carneiro. Convergent inquiry loop: anchor proof obligations against the
Prop/Type discipline, construct proofs, challenge through counterexample search,
and reason about proof erasure.

## The convergent insight

Three independently discovered principles converge on the same pattern:

| Domain | Exemplar | Principle |
|--------|----------|-----------|
| **Logic** | Curry-Howard correspondence | Propositions are types; proofs are programs |
| **Type theory** | de Bruijn's AUTOMATH | Dependent types as a foundation for mathematics |
| **Implementation** | Carneiro's "The Type Theory of Lean" | Prop/Type distinction; proof irrelevance; erasure |

The convergent pattern: **a proof is a program that inhabits a type, and the
type-checker is the oracle that verifies the proof.** A checked proof establishes its *exact stated proposition relative to its
assumptions and axioms*. A successful `lean` exit alone is insufficient:
`sorry` is accepted with a warning, and `native_decide` adds an axiom.

## D/P labelling

Proof construction (tactic choice, term-mode terms, the search itself) is P —
the prover's judgment, critiqued by the type-checker: a proof compiles or it
doesn't. `lean_check` is D — the pinned toolchain is the oracle, and a
successful exit is NOT proof acceptance: `sorry` passes with a warning and
`native_decide` adds an axiom, so the axiom audit (the tool's theorem flag)
is part of the check, not an optional extra. Counterexample search is P with
a D verdict — a found counterexample eliminates the conjecture hard. A
checked proof establishes its exact stated proposition relative to its
assumptions and axioms; it is never a claim about instances.

## Initial and target condition

- **Initial condition:** the exact Lean declaration, imports, hypotheses, pinned toolchain and trust policy, plus any existing proof attempt and actual diagnostics.
- **Target condition:** every selected positive declaration passes `LP-1` and `LP-2`, the named theorem's axioms satisfy policy, expected negative controls fail for the intended reason, bookkeeping is complete and no outcome is unverified. A failed construction returns through refute to construct with the diagnostic classified. Bound: one initial attempt plus at most two revisions; after three construct/refute cycles, report the remaining goal, diagnostics and trust dependencies as blocked.

## When to Use

- Constructing machine-checked proofs in Lean 4
- Verifying that a proposition is provable (or finding a counterexample)
- Reasoning about proof erasure and computational content
- Checking termination of recursive functions
- Determining whether a proposition is decidable
- Auditing the Prop/Type boundary in a Lean development


## Relationship to falsifiability

`lean-prover` is a sibling to `falsifiability`, not a child. Where
`falsifiability` designs discriminating tests (which can fail), `lean-prover`
constructs machine-checked proofs (which compile or don't). Both are
eliminative inference engines — they rule out what is false — but they operate
at different levels of rigor:

- `falsifiability`: "design a test that could refute the claim"
- `lean-prover`: "construct a proof that the claim holds, or find a counterexample"

Use `falsifiability` when you need empirical falsification. Use `lean-prover`
when you need mathematical verification.

## PDCA Loop

```
Plan:  Phase 1 — Anchor    → Classify the proposition (Prop/Type, decidability, structure)
Do:    Phase 2 — Construct → Build the proof via tactics and term mode
Check: Phase 3 — Refute    → Compile the exact declaration, audit axioms, run negative controls
Act:   Phase 4 — Erase     → Check Prop/Type claims; revise and rerun or report blocked
```

## When NOT to Use

- Informal mathematical argument — testable-but-unproven claims belong to `falsifiability` / `hypothesis-framer`; proof is a stronger standard than falsification.
- Rust type-system design — use `idiomatic-rust`.
- Property-based testing — `tdd`'s property tests sample behavior; they do not prove it.

## Instructions

### lean-prover-anchor

1. Read the *exact* declaration, imports, hypotheses, and toolchain pin (`lean --version`, and `lean-toolchain`/`lakefile` if present). Render `lean-prover/lean-prover-anchor` with the obligation, context, and actual diagnostics; do not label absent diagnostics as clean.
2. Classify the target (`Prop` or data in `Type`), its connectives, whether an executable `Decidable` instance exists, and whether recursion requires an induction hypothesis or termination argument. `∃ x, P x` is `Exists : Prop`, not `Sigma : Type`.
3. Choose a term, tactic, or hybrid proof; specify any permitted assumptions and whether extra trust from `native_decide` is acceptable. Prefer `decide` for small closed decidable goals; `native_decide` is not an axiom-free substitute. For library lemmas, search the imports actually available; Mathlib is optional.

### lean-prover-construct

1. Render `lean-prover/lean-prover-construct` with the anchor and obligation. Write the proof in its real context, retaining the original statement. For implications use `fun`/`intro`; for an existential supply a witness and its proof; for recursive data use `induction` with a named induction hypothesis. Do not introduce new axioms to replace the goal.
2. A proof that `lean_check` must check lives in the project (the tool reads project-relative paths only); a scratch proof that is not a project deliverable goes under `~/Documents/zk-data/skills/lean-prover/{date}-{run}/` with its command log and `#print axioms` output, never under `/tmp`. When `lean_check` is exposed in agent chat, call it with the project-relative path of the **saved, local** `.lean` file and, for a named proof, its fully qualified `theorem` name. The tool requires per-call operator approval to execute the project's pinned Lake toolchain; respect refusal. Read `lean_version`, `exit_code`, `diagnostics`, `goal_text`, `completion_status`, and `axioms`. It is a one-shot checker, not a live tactic stream or an editor-buffer check. If the tool is not exposed in this session, use the existing `terminal` tool to run the project's pinned `lake env lean --json <file>` / `lake build` (or version-identified standalone `lean` when there is no Lake project), and `#print axioms` for the named theorem; preserve the exact commands, version, exit codes, and output. Do not describe an unexposed tool as absent from the registered system. A code action (especially `add sorry`) is only a suggestion.
3. Treat `lean_check` statuses precisely: `axiom_free` applies only to the named theorem whose `#print axioms` was confirmed; `checked_not_axiom_audited` is a successful file check **without** an axiom verdict; `axioms_present`, `warnings`, `audit_unconfirmed`, and `failed` do not establish a completed axiom-free proof. On the terminal path require exit 0, no proof-hole warnings, and an axiom list consistent with the declared policy. `native_decide` may check with an added computation axiom; report it, never call it kernel-only. A missing toolchain or unchecked proof is `unverified`, not `compiled`.

### lean-prover-refute

1. Render `lean-prover/lean-prover-refute` with the actual compiler output. For a decidable universal claim, try a specific counterexample as a *separate* `example : ¬ P witness := by decide`; a failed attempt to prove `P witness` is evidence about that instance, not a universal proof of negation. Distinguish false proposition, invalid tactic, missing instance, and missing import.
2. Run a negative control expected to fail (`example : 1 = 2 := by decide`) and inspect the error, not just its exit status. In a Prop/Type question also test `Exists` elimination into data. See `kask/scripts/test-lean-prover-skill.sh` for runnable controls; give it a Lean binary path.
3. Call `lisp_eval` to enforce the strict compiler & axiom-policy gate (`LP-1`), the Prop-to-Type boundary (`LP-2`), and workflow bookkeeping:
   - `LP-1`: `(and (= exit_code 0) (= sorry_warnings 0) (string= completion_status "axiom_free") (not (member "sorryAx" axioms_used)) (not (member "Lean.ofReduceBool" axioms_used)))`
   - `LP-2`: `(if (and (string= universe "Prop") eliminates_into_type) allows_large_elim true)`
   - Bookkeeping: `(and (= (length obligations) (length checks)) (not (member "unverified" checks)) (not (member "failed" checks)))`
   `Exists` cannot expose its witness as `Nat` by pattern matching (`Exists.casesOn` into `Type` is rejected); `Sigma`/`Subtype` retain data. Proof irrelevance is **not** permission to replace a proof by `sorry` or `trivial`. Always retain the corresponding Lean logs and `#print axioms` output. Converge only when all chosen positive declarations pass `LP-1` and `LP-2`, expected negatives fail for the intended reason, and no outcome remains unverified.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `lean-prover-anchor.j2` | Anchor a proof obligation against the Curry-Howard correspondence and the Prop/Type discipline. |
| `lean-prover-construct.j2` | Construct a Lean proof for the anchored obligation via term or tactic mode and verify termination. |
| `lean-prover-refute.j2` | Adversarial review of a proof attempt and Prop/Type erasure boundary (`erasure_classification`, `prop_type_boundary`). |

To render a template, call the `render_template` tool with the template ref (e.g., `lean-prover/lean-prover-anchor`) and a context object with the required variables.

Template context variables (from each template's [inference] contract):
- `lean-prover-anchor.j2`: `proof_obligation`, `existing_proof`, `context`, `lean_diagnostics`, `iteration_focus`
- `lean-prover-construct.j2`: `proof_obligation`, `anchor`, `existing_proof`, `lean_code_actions`, `iteration_focus`
- `lean-prover-refute.j2`: `proof_obligation`, `proof_attempt`, `anchor`, `lean_diagnostics`

## Regression case

All receipts executed live through `lisp_eval` (2026-10-01, batch-12 audit):

- Workflow bookkeeping, green: two obligations, two verified checks → `true`.
- Bookkeeping, unverified: same counts with one `"unverified"` check →
  `false` — the form checks counts and statuses supplied by the agent, never
  Lean syntax, semantics, axioms, or proof validity (only Lean checks Lean
  propositions; the Lean logs are retained alongside).
- Bookkeeping, count mismatch: two obligations, one check → `false`.
- Bookkeeping floor, axiom status passes: checks
  `["axioms_present","axioms_present"]` → `true` — the form is a floor, not
  a proof gate; the axiom-policy gate (step 3 statuses, `#print axioms`)
  catches what it cannot.

The skill's form is executed at use time, never anchored in code.

## Constraints

- Source basis: *Theorem Proving in Lean 4* (edition targeting Lean 4.33.0), §§3.1–3.3 (proof terms), §3.6 (`sorry`), §4 (quantifiers), §8 (induction), §12 (axioms); Lean repository tag [`v4.34.0/src/Init/Core.lean`](https://github.com/leanprover/lean4/blob/v4.34.0/src/Init/Core.lean) (`Exists`, `Sigma`, `Lean.ofReduceBool`) and [`v4.34.0/src/Init/Tactics.lean`](https://github.com/leanprover/lean4/blob/v4.34.0/src/Init/Tactics.lean) (`decide`, `native_decide`, `sorry`, `induction`). Changes to tactic implementation or trust policy are version-dependent: recheck against the installed toolchain.
- Only Lean checks Lean propositions; `lean_check` reports Lean's result and trust dependencies, not an independent proof. `lisp_eval` is a sandboxed JSON/Lisp invariant checker, not a Lean kernel or proof checker.
- No `sorry`/`admit`, replacement axiom, or unexamined transitive axiom dependency in a completed proof. Record the exact Lean command and result.
- Recursive definitions require termination unless explicitly marked partial; partial computations are not silently treated as total proofs.
