---
name: lisp-scaffold-reasoning
description: "Reference skill demonstrating deterministic Lisp scaffolding of LLM probabilistic reasoning. Interleaves LLM hypothesis-generation steps with stateless lisp_eval tool calls that check structural invariants the LLM cannot reliably self-evaluate."
---

# Lisp-Scaffold-Reasoning

Reference skill demonstrating deterministic Lisp scaffolding of LLM probabilistic
reasoning, after de la Torre (2025, arXiv:2506.10021) — "From Tool Calling to
Symbolic Thinking: LLMs in a Persistent Lisp Metaprogramming Loop". The paper
proposes a live SBCL REPL with middleware-intercepted `<lisp>` tags and
persistent state across turns. kask realizes the same symbolic-neural
scaffolding via stateless `lisp_eval` tool calls interleaved between LLM
reasoning steps:

```
LLM propose  →  lisp_eval invariant check  →  LLM refine (gated)  →  lisp_eval score  →  converge
```

The interpreter is stateless, sandboxed (`#![forbid(unsafe_code)]`, no
I/O/FS/network/`eval`, bounded steps+depth). State crosses the stateless
boundary through the `env` parameter — prior step outputs are passed as JSON
bindings the Lisp form can access via `assoc`.

## When to Use

- When you want to see a worked example of `lisp_eval` interleaved with LLM
  reasoning steps.
- When you need a template for adding deterministic structural checks to a
  probabilistic reasoning process (count, completeness, diversity, mutual
  exclusivity).
- When you want to extend the symbolic-neural scaffolding pattern to your own
  skill — call the `lisp_eval` tool and adapt the `form`.

## When NOT to Use

- As a production reasoning skill — use `falsifiability` for real eliminative
  inference. This skill is a reference for the `lisp_eval` pattern.
- For persistent REPL or self-evolving tool scenarios — kask's interpreter is
  deliberately stateless.

## The lisp_eval Pattern

The canonical example implements four structural invariants via recursive
Lisp helpers. The agent calls `lisp_eval` with:

- `form`: a Lisp source string (auditable in the conversation, not a
  runtime-emitted string from the model).
- `env`: a JSON object whose keys become top-level Lisp bindings. Pass prior
  step outputs here — this is how state crosses the stateless boundary.

Example call:

```json
{
  "form": "(let ((hyps (assoc \"hypotheses\" step_1_result))) (if (is_null hyps) (list \"no_hypotheses_field\") (let ((n (length hyps))) (begin (define check-completeness (lambda (hs acc) ...)) (check-completeness hyps nil)))))",
  "env": { "step_1_result": { "hypotheses": [...] } }
}
```

The result is returned as JSON (here, a list of defect strings). The agent
uses the result to decide whether to refine the hypotheses (if defects were
found) or proceed to the report (if the set is clean).

Key points:

- `form` is a static string the agent passes to the tool — it is auditable in
  the conversation log, not a runtime-emitted string.
- `env` binds prior step results into the Lisp environment — this is how state
  crosses the stateless boundary.
- The Lisp form returns a JSON value stored as the tool result and consumed by
  the agent's next reasoning step.
- The agent gates its refinement on the Lisp verdict — if the check found
  structural defects, the agent repairs them; otherwise it proceeds. This is
  the symbolic→neural feedback loop.

### Interpreter constraints honored by the form

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

- The interpreter supports prefix `(+ a b)` and infix `a + b` operator
  notation. Use infix for simple scoring (`score_a * 0.6 + score_b * 0.4`),
  prefix for complex nested logic with `let`, `if`, `assoc`.
- No `eval` builtin (Lisp code cannot evaluate arbitrary strings). No
  `load`/`require`. Bounded recursion depth (default 64) and bounded
  evaluation steps (default 100000). Both are configurable per call via
  `max_steps` and `max_depth` parameters.
- The tool is sandboxed: no I/O, no filesystem, no network, no side effects.