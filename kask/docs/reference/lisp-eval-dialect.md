---
title: "The lisp_eval Dialect"
audience: [agents, developers]
last_updated: 2026-09-09
status: "Active"
domain: "Cross-cutting"
---

# The lisp_eval Dialect

The canonical reference for the `lisp_eval` interpreter: the symbolic-neural
scaffolding pattern, the `form`/`env` contract, and the dialect's builtins
and limits. Every skill that pins a `lisp_eval` form authors against this
document.

Relocated 2026-09-09 from the `lisp-scaffold-reasoning` skill (operator
decision: the skill was a reference demo with zero composers in the corpus;
this dialect documentation is its load-bearing content).

## The pattern

After de la Torre (2025, arXiv:2506.10021) — "From Tool Calling to Symbolic
Thinking: LLMs in a Persistent Lisp Metaprogramming Loop". The paper proposes
a live SBCL REPL with middleware-intercepted `<lisp>` tags and persistent
state across turns. kask realizes the same symbolic-neural scaffolding via
stateless `lisp_eval` tool calls interleaved between LLM reasoning steps:

```
LLM propose  →  lisp_eval invariant check  →  LLM refine (gated)  →  lisp_eval score  →  converge
```

The interpreter is stateless, sandboxed (`#![forbid(unsafe_code)]`, no
I/O/FS/network/`eval`, bounded steps+depth). State crosses the stateless
boundary through the `env` parameter — prior step outputs are passed as JSON
bindings the Lisp form can access via `assoc`.

## The form/env contract

- `form`: a Lisp source string (auditable in the conversation, not a
  runtime-emitted string from the model).
- `env`: a JSON object whose keys become top-level Lisp bindings. Pass prior
  step outputs here — this is how state crosses the stateless boundary.

The form returns a JSON value stored as the tool result and consumed by the
agent's next reasoning step. The agent gates its refinement on the Lisp
verdict — if the check found structural defects, the agent repairs them;
otherwise it proceeds. This is the symbolic→neural feedback loop.

## Interpreter surface

- `define` inside `begin` at the `let` scope mutates the `let`'s child env
  (works — `define` mutates the env it receives, which is the `let` env).
- `define` inside a _called lambda_ mutates the call_env (a child of the
  closure env), NOT the closure env itself. Recursive helpers must accumulate
  via return values, not by mutating an outer variable.
- `=` is numeric-only (`num_eq` calls `as_f64`). Use `string=` for string
  equality: `(string= lk "high")` returns true iff `lk` is the string `"high"`.
- `append` is a builtin: `(append l1 l2 ...)` joins multiple lists. Nil args
  are treated as empty lists. No need for a recursive `append2` helper.
- `concat` is a builtin: `(concat s1 s2 ...)` joins strings. Use this to
  build defect labels from field names: `(concat "missing_" key)`.
- Boolean literals are `true`/`false`/`nil` (not `#t`/`#f`). `t` is also
  bound and truthy — usable as the cond else-clause:
  `(cond (test then) (t else))`.
- `assoc` tests for key _presence_, not non-empty value. An empty-string
  `falsifier` is a present key — it is a semantic defect the LLM should
  catch, not a structural one Lisp flags. To flag empty values, add a
  `length` check on the `assoc` result.
- Prefix `(+ a b)` and infix `a + b` operator notation are both supported.
  Use infix for simple scoring (`score_a * 0.6 + score_b * 0.4`), prefix for
  complex nested logic with `let`, `if`, `assoc`.
- No `eval` builtin (Lisp code cannot evaluate arbitrary strings). No
  `load`/`require`.
- Bounded recursion depth (default 1024) and bounded evaluation steps
  (default 100000). Both are configurable per call via the tool's
  `max_depth` and `max_steps` parameters. Recursive list-walkers consume
  2–4 depth frames per element — for a list of N elements, pass
  `max_depth` ≥ 8×N (a 134-element list needed ~300 frames and failed
  both validation calls at the former 64 default; observed 2026-09-03,
  default confirmed 1024 by live probe 2026-09-09).
- The tool is sandboxed: no I/O, no filesystem, no network, no side effects.

## Worked example

A completeness check over a hypothesis set — four structural invariants via
a recursive helper:

```json
{
  "form": "(let ((hyps (assoc \"hypotheses\" step_1_result))) (if (is_null hyps) (list \"no_hypotheses_field\") (let ((n (length hyps))) (begin (define check-completeness (lambda (hs acc) ...)) (check-completeness hyps nil)))))",
  "env": { "step_1_result": { "hypotheses": [...] } }
}
```

The result is a list of defect strings. The agent refines the hypotheses if
defects were found, or proceeds to the report if the set is clean.

## Provenance

- de la Torre (2025), "From Tool Calling to Symbolic Thinking: LLMs in a
  Persistent Lisp Metaprogramming Loop", arXiv:2506.10021.
- Relocated from the `lisp-scaffold-reasoning` skill, which was removed from
  the corpus 2026-09-09 (zero composers; demo framing). The skill's
  `report.j2` template was removed with it — its final-report prompt shape
  is preserved by the worked example above.
