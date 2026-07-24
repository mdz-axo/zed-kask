---
name: idiomatic-rust
visibility: public
description: "Idiomatic Rust design through Graydon Hoare's lens. Convergent inquiry loop: anchor design problems against Hoare's principles, propose type-driven solutions with code examples, challenge and refine through adversarial review, and converge toward deeper, more idiomatic designs.
"
---

# Idiomatic Rust

Idiomatic Rust design through Graydon Hoare's lens. Convergent inquiry loop: anchor design problems against Hoare's principles, propose type-driven solutions with code examples, challenge and refine through adversarial review, and converge toward deeper, more idiomatic designs.


## When to Use

- Assessing a Rust design problem against Graydon Hoare's principles to identify invariants, invalid states, ownership graphs, and error domains.
- Proposing type-driven Rust solutions with code examples, applying algebraic types, ownership patterns, error propagation, and trait design.
- Conducting adversarial reviews of a Rust design proposal to find gaps, test edge cases, challenge assumptions, and identify deeper ecosystem connections.
- Computing a normalized convergence metric for an idiomatic-rust inquiry cycle to determine if further design refinement is needed.

## Instructions

### idiomatic-rust-inquiry

1. Evaluate the current or proposed design against each of the eight Hoare principles, asking if it satisfies the principle, what specific states or relationships violate it, and the minimum change needed to satisfy it.
2. List all invariants that must always be true.
3. Identify all invalid states currently possible that should never occur.
4. Map the ownership graph, detailing who creates, observes, mutates, and destroys each value.
5. Define the error domain, specifying what can fail, the handling level, and any silently swallowed errors.
6. Rank principle violations by severity.
7. Order improvement targets by impact, specifying the exact type, ownership, or error changes needed to address violations.

### idiomatic-rust-design

1. Design types that make wrong usage impossible by replacing `String` with validating newtypes, `bool` with two-variant enums, `Vec<T>` with non-empty types, raw integers with unit-aware newtypes, and invalid `Option<T>` with non-nullable types.
2. Map the ownership DAG for each value, explicitly choosing single owners, shared immutable access, shared mutable access, or borrowed access.
3. Ensure every fallible function returns `Result<T, E>`, using `thiserror` for libraries and `anyhow` for applications, avoiding `unwrap()` in library code, and documenting all panics.
4. Design traits that define capabilities rather than taxonomies, preferring many small traits, using `impl Trait` in return positions, deriving common traits, and implementing `From<T>` for conversions.
5. Reference the current Rust ecosystem for each design decision, citing std library patterns, key crates, relevant RFCs, and API guidelines.

### idiomatic-rust-challenge

1. Find gaps where the design fails to address the original problem, misses scenarios, or leaves state transitions unhandled, citing specific types or functions.
2. Test edge cases for each type, considering empty inputs, maximum values, concurrent access, errors at each step, and mid-operation shutdowns.
3. Challenge assumptions regarding correctness, performance, or safety by writing counterexamples that attempt to reach invalid states or expose hidden costs.
4. Find deeper connections to broader Rust patterns, comparing the design to std library types, popular crates, applicable RFCs, and API guidelines.
5. Produce refinement directives for each gap or edge case, stating the specific change required, the principle addressed, and the expected improvement.

### idiomatic-rust-convergence

1. Evaluate design quality by assessing how idiomatic the current proposal is and whether Phase 1 improvement targets were addressed.
2. Evaluate critique depth by measuring the thoroughness of the adversarial review, flagging suspiciously low critique scores on weak designs.
3. Evaluate connection richness by counting the identified connections to the broader Rust ecosystem.
4. Compute the convergence metric by synthesizing design quality, critique depth, and connection richness into a normalized score.
5. Determine whether to loop back to the design phase by checking if the convergence metric exceeds the threshold and actionable refinements exist.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `idiomatic-rust-inquiry.j2` | KnowAct | Assess a Rust design problem against Graydon Hoare's principles. Identify invariants, invalid states, ownership graphs, and error domains. Produce a scored design assessment with specific improvement targets.  |
| `idiomatic-rust-design.j2` | KnowAct | Propose type-driven Rust solutions with code examples. Apply algebraic types, ownership patterns, error propagation, and trait design. Reference std library patterns, ecosystem best practices, and relevant RFCs.  |
| `idiomatic-rust-challenge.j2` | KnowAct | Adversarial review of a Rust design proposal. Find gaps, test edge cases, challenge assumptions, identify deeper connections. Produce a scored critique with specific refinement directives. Few-shot: if the critique score is below threshold, loop back to design with concrete targets.  |
| `idiomatic-rust-convergence.j2` | KnowAct | Compute a normalized convergence metric for an idiomatic-rust inquiry cycle. Synthesizes design quality, critique depth, and connection richness into a score in [0,1] where 0 means maximally idiomatic.  |

## Fusion Mode

This skill supports **fusion mode** via the `fusion:` block in its flow manifest.
When enabled, all analysis steps route through a multi-model panel — either with
LLM judge synthesis or the **algo / no-judge** path (`judge: algo`) for deterministic
JSON merge without an LLM judge call. This skill uses **critique mode** — Draft →
challenge → refine matches Rust design review.

The convergence check step has `fusion: false` to ensure deterministic rubric
evaluation uses single-model inference.

## Constraints

- `idiomatic-rust-inquiry.j2`: Public.
- `idiomatic-rust-design.j2`: Public.
- `idiomatic-rust-challenge.j2`: Public.
- `idiomatic-rust-convergence.j2`: Public.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
