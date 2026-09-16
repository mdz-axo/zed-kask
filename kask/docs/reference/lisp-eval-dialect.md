---
title: "The lisp_eval Dialect"
audience: [agents, developers]
last_updated: 2026-09-15
version: "0.40.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [composition, trust]
---

# The `lisp_eval` Dialect

This is the canonical reference for the symbolic-neural scaffolding pattern, the `form`/`env` contract, and the sandboxed dialect exposed by the built-in `lisp_eval` tool. The tool delegates to `hkask_lisp::eval_sandboxed_with_budget` (`crates/agent/src/tools/lisp_eval_tool.rs`; `kask/crates/hkask-lisp/src/hkask_lisp.rs:1680-1697`).

## Pattern

After de la Torre (2025, arXiv:2506.10021), Kask interleaves model reasoning with deterministic, stateless checks:

```text
LLM proposes → lisp_eval checks → LLM repairs or proceeds → lisp_eval scores → converge
```

Unlike the paper's persistent SBCL process, each call is stateless. State crosses calls through the JSON `env`; there is no filesystem, network, dynamic `eval`, or module-loading surface (`kask/crates/hkask-lisp/src/hkask_lisp.rs:1-19`).

## `form` / `env` contract

- `form` is the Lisp source string to evaluate.
- `env` is a JSON object whose keys become top-level bindings.
- `max_steps` and `max_depth` bound evaluation. The tool defaults are 100,000 steps and depth 1,024 (`kask/crates/hkask-lisp/src/hkask_lisp.rs:1672-1688`).
- The result is converted back to JSON for the next reasoning step.

Pass prior structured output through `env`, then read object members with `assoc` inside the form. `assoc` tests key presence; an empty string remains a present value and needs its own semantic or `length` check.

## Interpreter surface

The built-in registry is installed by `standard_env` (`kask/crates/hkask-lisp/src/hkask_lisp.rs:782-858`). Important dialect rules:

- Boolean literals are `true`, `false`, and `nil`; `t` is also truthy and is suitable as the final `cond` clause.
- `=` is numeric equality. Use `string=` for string-only equality and `eq` for structural equality (`kask/crates/hkask-lisp/src/hkask_lisp.rs:824-850`, `kask/crates/hkask-lisp/src/hkask_lisp.rs:1306-1321`).
- `append` joins lists; `nil` arguments behave as empty lists.
- `concat` joins strings.
- Prefix and supported infix arithmetic forms are both accepted. Prefer prefix form around `let`, `if`, recursion, and nested logic.
- `define` inside a `let` mutates that child environment. A `define` inside a called lambda mutates the call environment, not the captured parent; recursive helpers should accumulate through return values.
- There is no `eval`, `load`, or `require` builtin.
- Recursive walkers consume multiple depth frames per element. Raise `max_depth` explicitly for large lists rather than relying on the default.

## Worked invariant check

```json
{
  "form": "(let ((items (assoc \"items\" payload))) (if (is_null items) (list \"missing_items\") (if (= (length items) expected) true (list \"wrong_count\"))))",
  "env": {
    "payload": {"items": [1, 2, 3]},
    "expected": 3
  }
}
```

The agent uses the returned boolean or defect list as a gate; it does not treat model self-assessment as the deterministic result.

## Provenance

- de la Torre, J. (2025). *From Tool Calling to Symbolic Thinking: LLMs in a Persistent Lisp Metaprogramming Loop*. arXiv:2506.10021. https://arxiv.org/abs/2506.10021
- This reference was folded from the former `lisp-scaffold-reasoning` demonstration skill on 2026-09-09; the implementation authority is `kask/crates/hkask-lisp/src/hkask_lisp.rs`.
