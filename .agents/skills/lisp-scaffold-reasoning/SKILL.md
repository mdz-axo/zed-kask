---
name: lisp-scaffold-reasoning
visibility: public
description: "Reference skill demonstrating deterministic Lisp scaffolding of LLM probabilistic reasoning, after de la Torre (2025, arXiv:2506.10021). Interleaves LLM hypothesis-generation steps with stateless lisp.eval compute steps that check structural invariants the LLM cannot reliably self-evaluate (count, completeness, diversity, mutual exclusivity). Realizes the symbolic-neural scaffolding thesis via kask's manifest cascade rather than a persistent REPL. Read its manifest to see the lisp.eval pattern."
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

The manifest's step 2 is the canonical example:

```yaml
- ordinal: 2
  action: compute
  compute_ref: lisp.eval
  input_mapping:
    form: >
      (let ((hyps (assoc "hypotheses" step_1_result)))
        (if (is_null hyps)
            (list "no_hypotheses_field")
            (begin
              (define n (length hyps))
              (define defects (list))
              (if (< n 3) (define defects (cons "insufficient_count_below_3" defects)) defects)
              (if (> n 7) (define defects (cons "excessive_count_above_7" defects)) defects)
              defects)))
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
- Registry is authoritative — when this SKILL.md disagrees with registry
  templates, the registry wins.
