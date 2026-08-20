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
type-checker is the oracle that verifies the proof.** This makes Lean proofs
the strongest form of falsification — a proof that compiles is a proof that
holds, not a proof that might hold.

## When to Use

- Constructing machine-checked proofs in Lean 4
- Verifying that a proposition is provable (or finding a counterexample)
- Reasoning about proof erasure and computational content
- Checking termination of recursive functions
- Determining whether a proposition is decidable
- Auditing the Prop/Type boundary in a Lean development

Do NOT use for:
- Informal mathematical reasoning (use hypothesis-framer or sequential-inquiry)
- Statistical hypothesis testing (use falsifiability)
- Performance benchmarking (use HELM directly)

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
Check: Phase 3 — Refute    → Search for counterexamples and failed proof paths
Act:   Phase 4 — Erase     → Reason about proof irrelevance and erasure
```

## Instructions

### lean-prover-anchor

1. Classify the proposition: is it in `Prop` or `Type`? What is its logical structure?
2. Determine decidability: is there a `Decidable` instance? Can `decide` or `native_decide` be used?
3. Identify the proof structure: direct, contrapositive, contradiction, induction?
4. Determine the proof method: term mode, tactic mode, or hybrid?
5. Assess termination strategy if recursive functions are involved.
6. Check Lean diagnostics for type errors and termination issues.

### lean-prover-construct

1. Choose the proof mode (term, tactic, hybrid) based on the anchor.
2. Apply the appropriate tactics: intro, apply, exact, induction, simp, rw, decide.
3. Handle termination: structural recursion, well-founded recursion, or tail recursion.
4. Reference Mathlib for existing lemmas and tactics.
5. Verify the proof compiles by running `lean` or `lake build`.

### lean-prover-refute

1. Search for counterexamples: try specific inputs, use `decide`/`native_decide`.
2. Identify failed proof paths: tactics that don't apply, induction with wrong motive.
3. Challenge assumptions about termination, decidability, and erasure.
4. Test edge cases: empty types, uninhabited Props, universe inconsistencies.
5. Produce refinement directives for each failure.

### lean-prover-erase

1. Classify the proof's universe (Prop vs Type).
2. Identify the elimination type (small, large, none).
3. Check the Prop/Type boundary for violations.
4. Assess erasure consequences: does the proof compute or erase?

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `lean-prover-anchor.j2` | KnowAct | Anchor a proof obligation against the Curry-Howard correspondence and the Prop/Type discipline. Classify the proposition (Prop vs Type), identify the proof obligation's structure (universal, existential, implication, conjunction), determine the appropriate proof method (term mode, tactic mode, decide, native_decide), and assess whether the proposition is decidable. |
| `lean-prover-construct.j2` | KnowAct | Construct a Lean proof for the anchored obligation. Choose between term mode (explicit proof terms) and tactic mode (imperative proof steps). Apply the appropriate tactics (intro, apply, exact, induction, simp, rw, decide). Handle termination via structural recursion or well-founded recursion. Produce a proof that compiles. |
| `lean-prover-refute.j2` | KnowAct | Adversarial review of a proof attempt. Search for counterexamples that invalidate the proposition. Identify failed proof paths (tactics that don't apply, induction that doesn't terminate, simp lemmas that loop). Challenge proof irrelevance assumptions. Test edge cases (empty types, uninhabited Props, proof terms that don't erase). Produce refinement directives for each failure. |
| `lean-prover-erase.j2` | KnowAct | Reason about proof erasure and irrelevance. Determine which proof terms are computationally relevant (in Type) vs irrelevant (in Prop). Assess whether the proof erases to a no-op or carries computational content. Identify small vs large elimination. Verify that the proof doesn't leak computational content across the Prop/Type boundary. |

## Constraints

- All templates are `KnowAct` type with `Public` visibility.
- The Lean type-checker is the extrinsic oracle — always run `lean` or `lake build` to verify proofs.
- A proof with `sorry` is not a complete proof — it's a proof obligation with holes.
- The Prop/Type boundary is inviolable: computational content cannot leak from Prop to Type without large elimination.
- Termination is mandatory: every recursive function must have a termination proof.
- This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.
