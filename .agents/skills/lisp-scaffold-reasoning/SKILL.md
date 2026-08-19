---
name: lisp-scaffold-reasoning
visibility: public
description: "Reference skill demonstrating deterministic Lisp scaffolding of LLM probabilistic reasoning. Interleaves LLM hypothesis-generation steps with stateless lisp.eval compute steps that check structural invariants the LLM cannot reliably self-evaluate."
---

# Lisp-Scaffold-Reasoning

Reference skill demonstrating deterministic Lisp scaffolding of LLM probabilistic
reasoning, after de la Torre (2025, arXiv:2506.10021) — "From Tool Calling to
Symbolic Thinking: LLMs in a Persistent Lisp Metaprogramming Loop". The paper
proposes a live SBCL REPL with middleware-intercepted `<lisp>` tags and
persistent state across turns. kask realizes the same symbolic-neural
scaffolding via stateless `lisp.eval` compute steps interleaved between LLM
`select` steps:

```
LLM propose  →  Lisp invariant check  →  LLM refine (gated)  →  Lisp score  →  converge
```

State lives in the manifest's step-result chain (auditable YAML), not in a
persistent REPL (attack surface). The interpreter is stateless, sandboxed
(`#![forbid(unsafe_code)]`, no I/O/FS/network/`eval`, bounded steps+depth),
and gated to `category: skill`.

## When to Use

- When you want to see a worked example of `lisp.eval` interleaved with LLM
  `select` steps — read the manifest at `kask/registry/manifests/lisp-scaffold-reasoning.yaml`.
- When you need a template for adding deterministic structural checks to a
  probabilistic reasoning cascade (count, completeness, diversity, mutual
  exclusivity).
- When you want to extend the symbolic-neural scaffolding pattern to your own
  skill — copy the `compute_ref: lisp.eval` step shape and adapt the `form:`.

## When NOT to Use

- As a production reasoning skill — use `falsifiability` for real eliminative
  inference. This skill is a reference for the `lisp.eval` pattern.
- For persistent REPL or self-evolving tool scenarios — kask's interpreter is
  deliberately stateless. See the skill manifest's header comment for why.

## The lisp.eval Pattern

The manifest's step 2 is the canonical example. It implements all four
structural invariants via recursive Lisp helpers:

```yaml
- ordinal: 2
  action: compute
  compute_ref: lisp.eval
  input_mapping:
    form: >
      (let ((hyps (assoc "hypotheses" step_1_result)))
        (if (is_null hyps)
            (list "no_hypotheses_field")
            (let ((n (length hyps)))
              (begin
                (define append2
                  (lambda (a b)
                    (if (is_null a) b (cons (car a) (append2 (cdr a) b)))))
                (define count-defects ...)
                (define check-completeness (lambda (hs acc) ...))
                (define check-diversity (lambda (hs nh nm nl) ...))
                (define check-duplicates (lambda (hs seen) ...))
                (append2 (append2 count-defects completeness-defects)
                         (append2 diversity-defects duplicate-defects))))))
    env:
      step_1_result: "{{ step_1_result }}"
```

Key points:

- `form:` is a static YAML field (auditable in code review), not a
  runtime-emitted string (the paper's `<lisp>` tag).
- `env:` binds prior step results into the Lisp environment via Jinja
  `{{ }}` expressions — this is how state crosses the stateless boundary.
- The Lisp form returns a JSON value (here, a list of defect strings) stored
  as `step_2_result` and consumed by downstream steps.
- Step 3's `condition: "{{ step_2_result | length > 0 }}"` gates the LLM
  refinement on the Lisp verdict — the symbolic→neural feedback loop.
- Step 1's `prior_defects: "{{ step_2_result | default([]) }}"` carries the
  defect list forward across loop iterations — this closes the feedback loop.
  The binding is to the bare list (not `step_2_result.defects`, which would be
  undefined on a list result).

### Interpreter constraints honored by the form

These constraints are non-obvious and were verified by the
`dispatch_lisp_eval_hypothesis_four_invariants` test in
`kask/crates/hkask-templates/src/compute.rs`:

- `define` inside `begin` at the `let` scope mutates the `let`'s child env
  (works — `define` mutates the env it receives, which is the `let` env).
- `define` inside a _called lambda_ mutates the call_env (a child of the
  closure env), NOT the closure env itself. Recursive helpers must accumulate
  via return values, not by mutating an outer variable.
- `=` is numeric-only (`num_eq` calls `as_f64`). Use `string=` for string
  equality: `(string= lk "high")` returns true iff `lk` is the string `"high"`.
- `append` is a builtin: `(append l1 l2 ...)` joins multiple lists. Nil args
  are treated as empty lists. No need for a recursive `append2` helper.
- `concat` is a builtin: `(concat s1 s2 ...)` joins strings. Use this to build
  defect labels from field names: `(concat "missing_" key)`.
- Boolean literals are `true`/`false`/`nil` (not `#t`/`#f`).
- `assoc` tests for key _presence_, not non-empty value. An empty-string
  `falsifier` is a present key — it is a semantic defect the LLM should catch,
  not a structural one Lisp flags. To flag empty values, add a `length` check
  on the `assoc` result.

## Constraints

- `lisp.eval` is gated to `category: skill` manifests only. Infrastructure
  manifests run without human review and a Turing-complete step language is
  an attack surface.
- The interpreter supports prefix `(+ a b)` and infix `a + b` operator
  notation. Use infix for simple scoring (`score_a * 0.6 + score_b * 0.4`),
  prefix for complex nested logic with `let`, `if`, `assoc`.
- No `eval` builtin (Lisp code cannot evaluate arbitrary strings). No
  `load`/`require`. Bounded recursion depth (64) and bounded evaluation
  steps (100000).
- rJoule cap: 2 per invocation. Maximum 10 iterations.
- Registry is authoritative — when this SKILL.md disagrees with registry
  templates, the registry wins.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `propose-hypotheses.j2` | KnowAct | Probabilistic step: the LLM generates 3-7 candidate hypotheses for the target question. Each hypothesis carries a prediction, a falsifier, and a likelihood estimate. This is the connectionist compute the Lisp steps scaffold — the LLM is good at generation, bad at counting its own outputs and checking structural completeness. |
| `refine-hypotheses.j2` | KnowAct | Probabilistic step: the LLM refines the hypothesis set in response to the deterministic Lisp invariant check. Gated by `condition:` on the Lisp verdict — only runs if the Lisp step found structural defects the LLM must repair (missing falsifier, duplicate hypothesis, insufficient diversity). This is the feedback loop: Lisp finds a structural defect, LLM repairs it, Lisp re-checks. |
| `report.j2` | KnowAct | Final report: the surviving hypothesis set with the Lisp invariant verdict and convergence score. Surfaces which structural defects were found and repaired across iterations, making the symbolic scaffolding auditable. |

