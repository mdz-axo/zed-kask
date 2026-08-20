---
name: idiomatic-lisp
description: "Idiomatic Lisp design through the lens of McCarthy, Sussman, and Graham. Convergent inquiry loop: anchor against Lisp's founding principles, propose macro- and data-driven solutions, and refine through adversarial review with lisp_eval evaluation."
---

# Idiomatic Lisp

Idiomatic Lisp design through the lens of McCarthy, Sussman, and Graham. Convergent inquiry loop: anchor design problems against Lisp's founding principles, propose macro- and data-driven solutions, challenge through adversarial review with `lisp_eval` as the extrinsic oracle, and converge toward deeper, more idiomatic designs.


## When to Use

- Assessing a Lisp design problem against the founding principles of Lisp to identify invariants, invalid states, evaluation models, and macro vs. function decisions.
- Proposing idiomatic Lisp solutions with code examples, applying homoiconicity, metacircular evaluation, data-as-program patterns, hygienic macros, and proper use of special forms.
- Conducting adversarial reviews of a Lisp design proposal to find gaps, test edge cases (tail-call depth, multiple-values, restarts, macro hygiene), challenge assumptions, and identify deeper connections.
- Computing a normalized convergence metric for an idiomatic-lisp inquiry cycle to determine if further design refinement is needed.

## Instructions

### idiomatic-lisp-inquiry

1. Evaluate the current or proposed design against each of the eight Lisp principles (homoiconicity, code-is-data, metacircularity, lambda-as-universal-abstraction, recursion-as-natural-control-flow, data-driven-programming, bottom-up-design, interactive-development), asking if it satisfies the principle, what specific forms or evaluation behaviors violate it, and the minimum change needed to satisfy it.
2. List all invariants that must always be true.
3. Identify all invalid states currently possible that should never occur.
4. Determine the evaluation model (substitution, environment, continuation) and check whether the design is consistent across all parts.
5. For each abstraction, evaluate whether the right mechanism was chosen: macro for syntactic transformation, function for value computation, closure for state encapsulation, data-driven dispatch for table/tree-structured problems.
6. Rank principle violations by severity.
7. Order improvement targets by impact, specifying the exact form, macro, or evaluation changes needed to address violations.

### idiomatic-lisp-design

1. Choose the right abstraction mechanism for each part of the design: functions for value computation, macros for code transformation, closures for state encapsulation, data-driven dispatch for structured problems.
2. Design for the correct evaluation model: substitution for pure functions, environment for closures, continuation for non-local control flow.
3. Design macros hygienically: use gensym for symbols that must not collide, verify with macroexpand, document expected input and output forms.
4. Design for tail-call optimization: identify tail-recursive functions, ensure recursive calls are in tail position, use accumulators when needed, verify with trace.
5. Reference the Lisp ecosystem for each design decision: CLHS for Common Lisp, SRFI for Scheme, Clojure Docs for Clojure, and the foundational papers (McCarthy 1960, SICP, On Lisp).

### idiomatic-lisp-challenge

1. Find gaps where the design fails to address the original problem, misses scenarios, or leaves evaluation paths unhandled, citing specific forms or macros.
2. Test edge cases for each function and macro: empty lists, deeply nested structures, circular structures, variable capture in macros, tail-call depth limits, multiple-values in single-value contexts, conditions with no matching restart.
3. Challenge assumptions regarding hygiene, tail-recursion, data-driven design, bottom-up structure, and error handling by writing counterexamples that expose hidden costs or broken invariants. Use `lisp_eval` to verify structural properties of the design (field presence, invariant checks, scoring).
4. Find deeper connections to broader Lisp patterns, comparing the design to CLHS/SRFI functions, classic papers, and cross-dialect equivalents.
5. Produce refinement directives for each gap or edge case, stating the specific change required, the principle addressed, and the expected improvement.

## Constraints

- Use `lisp_eval` to verify deterministic structural properties (invariant checks, scoring, convergence signals). The interpreter is sandboxed: no I/O, no filesystem, no network, bounded steps+depth.
- The interpreter supports prefix `(+ a b)` and infix `a + b` operator notation. Use infix for simple scoring, prefix for complex nested logic.