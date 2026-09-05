# hkask-lisp

Sandboxed Lisp interpreter for deterministic computation in skill processes.

## Purpose

Pure-Rust Lisp interpreter with no I/O, no filesystem, no network, and no
environment variable access. Used by the `lisp_eval` agent tool to enable
deterministic recursive predicates, structural invariant checks, and
convergence signal computation in skill processes without an LLM round-trip.

## Design

Following the [`rust_lisp`](https://github.com/brundonsmith/rust_lisp) reference
by brundonsmith, with these deviations:

- **JSON-native**: input env is `serde_json::Value`, output is `serde_json::Value`.
  JSON objects become association lists (the classic Lisp data structure) at
  the `from_json` boundary — access fields via `(assoc "key" obj)`.
- **Bounded sandbox entry points**: `max_steps` (default 100000) accounts for
  parsing, environment conversion, evaluation, builtin work, and output
  expansion. `max_depth` (default 1024) bounds source/data traversal as well
  as compound-form evaluation. Stack-safe recursive operations and list
  destruction protect the host stack. JSON output has a separate nesting
  boundary of 128, independent of the requested evaluation depth.
- **No `eval` builtin**: Lisp code cannot evaluate arbitrary strings. This is
  a deliberate security restriction — the interpreter is safe for skill
  convergence checks.
- **No `Hash` type**: JSON objects become association lists. This keeps the
  interpreter small (~1000 lines) and avoids the complexity of a hash map
  type. Users implement `map`/`filter` in Lisp itself.

## Supported forms

**Special forms**: `quote`, `if`, `let`, `lambda`, `define`, `begin`, `and`,
`or`, `not`, `cond`

**Built-in functions**: `car`, `cdr`, `cons`, `list`, `length`, `nth`,
`reverse`, `+`, `-`, `*`, `/`, `=`, `!=`, `<`, `<=`, `>`, `>=`, `is_null`,
`numberp`, `listp`, `assoc`, `append`, `member`, `abs`, `sqrt`, `eq`,
`string=`, `string-contains`, `concat`

## Usage

```rust
use hkask_lisp::eval_sandboxed;
use serde_json::json;

let form = "(and (> (length findings) 0) (< composite 0.15))";
let env = json!({"findings": ["a", "b"], "composite": 0.12});
let result = eval_sandboxed(form, &env).unwrap();
assert_eq!(result, json!(true));
```

## Skill usage

The `lisp_eval` agent tool calls `eval_sandboxed` with a Lisp form and a JSON
environment. The SKILL.md instructs the agent to call `lisp_eval` for
convergence checks, invariant validation, and deterministic scoring:

```
Call `lisp_eval`:
  form: "(and (> (length findings) 0) (< composite 0.15))"
  env: { "findings": <step_2_result.findings>, "composite": <step_3_result.composite> }
```

## Dependencies

- `serde_json` — JSON interop
- `thiserror` — error types
- `stacksafe` — recursive operations and destruction without host-stack overflow

No hKask crate dependencies — this is a standalone computation library.

## Security model

- No `eval` builtin (Lisp code cannot evaluate arbitrary strings)
- No `load` or `require`
- No I/O, no filesystem, no network, no environment variable access
- Budgeted sandbox boundary (`eval_sandboxed` / `eval_sandboxed_with_budget`):
  default depth 1024 and work budget 100000; oversized work returns a limit error
- Lower-level conversion helpers do not enforce the sandbox budget themselves
- Returned JSON nesting is independently bounded for caller serialization/drop
- Environment is immutable from Lisp's perspective (define mutates a local
  scope discarded after evaluation)
- `#![forbid(unsafe_code)]` — no unsafe blocks anywhere in the crate
