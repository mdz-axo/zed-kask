---
name: lean-prover
description: "Machine-checked proof construction. Anchor proof obligations against the Prop/Type discipline, construct proofs via tactics and term mode, challenge through counterexample search, and converge toward proofs that compile and erase."
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
3. Call `lisp_eval` only for deterministic *workflow bookkeeping*, e.g. `form: (and (= (length obligations) (length checks)) (not (member "unverified" checks)) (not (member "failed" checks)))`, `env: {"obligations":["base","step"],"checks":["verified","verified"]}`. This checks counts and statuses supplied by the agent, not Lean syntax, semantics, axioms, or proof validity. Always retain the corresponding Lean logs. On failure, revise from the diagnostic and rerun at most twice; otherwise report the unsolved obligation.

### lean-prover-erase

1. Render `lean-prover/lean-prover-erase` only when a Prop/Type, elimination, or runtime-computation claim matters. `Exists` cannot expose its witness as `Nat` by pattern matching; `Sigma`/`Subtype` retain data. Some `Prop` eliminators (e.g. `False`, `Eq`) support elimination into `Type`; verify each particular recursor by checking a Lean example rather than guessing from the universe.
2. Proof irrelevance is **not** permission to replace a proof by `sorry` or `trivial`. Audit the exact theorem's `#print axioms` output, then report the checked statement, assumed axioms, and whether any erasure claim was actually tested.
3. Converge only when all chosen positive declarations check, expected negatives fail for the intended reason, axiom policy holds, and no outcome remains unverified. Re-enter anchor on a changed statement/assumption, construct on a proof error, or stop after two corrections with the remaining gap.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `lean-prover-anchor.j2` | Anchor a proof obligation against the Curry-Howard correspondence and the Prop/Type discipline. Classify the proposition (Prop vs Type), identify the proof obligation's structure (universal, existential, implication, conjunction), determine the appropriate proof method (term mode, tactic mode, decide, native_decide), and assess whether the proposition is decidable. |
| `lean-prover-construct.j2` | Construct a Lean proof for the anchored obligation. Choose between term mode (explicit proof terms) and tactic mode (imperative proof steps). Apply the appropriate tactics (intro, apply, exact, induction, simp, rw, decide). Handle termination via structural recursion or well-founded recursion. Produce a proof that compiles. |
| `lean-prover-refute.j2` | Adversarial review of a proof attempt. Search for counterexamples that invalidate the proposition. Identify failed proof paths (tactics that don't apply, induction that doesn't terminate, simp lemmas that loop). Challenge proof irrelevance assumptions. Test edge cases (empty types, uninhabited Props, proof terms that don't erase). Produce refinement directives for each failure. |
| `lean-prover-erase.j2` | Reason about proof erasure and irrelevance. Determine which proof terms are computationally relevant (in Type) vs irrelevant (in Prop). Assess whether the proof erases to a no-op or carries computational content. Identify small vs large elimination. Verify that the proof doesn't leak computational content across the Prop/Type boundary. |

To render a template, call the `render_template` tool with the template ref (e.g., `lean-prover/lean-prover-anchor`) and a context object with the required variables.

## Constraints

- All templates are prompt templates with `Public` visibility.
- Source basis: *Theorem Proving in Lean 4* (edition targeting Lean 4.33.0), §§3.1–3.3 (proof terms), §3.6 (`sorry`), §4 (quantifiers), §8 (induction), §12 (axioms); Lean repository tag [`v4.34.0/src/Init/Core.lean`](https://github.com/leanprover/lean4/blob/v4.34.0/src/Init/Core.lean) (`Exists`, `Sigma`, `Lean.ofReduceBool`) and [`v4.34.0/src/Init/Tactics.lean`](https://github.com/leanprover/lean4/blob/v4.34.0/src/Init/Tactics.lean) (`decide`, `native_decide`, `sorry`, `induction`). Changes to tactic implementation or trust policy are version-dependent: recheck against the installed toolchain.
- Only Lean checks Lean propositions; `lean_check` reports Lean's result and trust dependencies, not an independent proof. `lisp_eval` is a sandboxed JSON/Lisp invariant checker, not a Lean kernel or proof checker.
- No `sorry`/`admit`, replacement axiom, or unexamined transitive axiom dependency in a completed proof. Record the exact Lean command and result.
- Recursive definitions require termination unless explicitly marked partial; partial computations are not silently treated as total proofs.
- This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.
