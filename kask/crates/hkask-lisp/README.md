# hkask-lisp

Sandboxed Lisp interpreter for deterministic manifest compute steps.

## Purpose

Pure-Rust Lisp interpreter with no I/O, no filesystem, no network, and no
environment variable access. Used by the `compute` action in
`hkask-templates` executor (`compute_ref: "lisp.eval"`) to enable
deterministic recursive predicates, structural invariant checks, and
capability-tree walks in manifests without an LLM round-trip.

## Design

Following the [`rust_lisp`](https://github.com/brundonsmith/rust_lisp) reference
by brundonsmith, with these deviations:

- **JSON-native**: input env is `serde_json::Value`, output is `serde_json::Value`.
  JSON objects become association lists (the classic Lisp data structure) at
  the `from_json` boundary — access fields via `(assoc "key" obj)`.
- **Bounded**: `max_steps` (default 100000) and `max_depth` (default 64) prevent
  infinite loops and stack overflow. Depth is checked only for compound forms
  (lists), not atoms.
- **No `eval` builtin**: Lisp code cannot evaluate arbitrary strings. This is
  a deliberate security restriction — the interpreter is safe for
  infrastructure manifests provided the caller respects the `category: skill`
  gate.
- **No `Hash` type**: JSON objects become association lists. This keeps the
  interpreter small (~1000 lines) and avoids the complexity of a hash map
  type. Users implement `map`/`filter` in Lisp itself.

## Supported forms

**Special forms**: `quote`, `if`, `let`, `lambda`, `define`, `begin`, `and`,
`or`, `not`

**Built-in functions**: `car`, `cdr`, `cons`, `list`, `length`, `nth`,
`reverse`, `+`, `-`, `*`, `/`, `=`, `!=`, `<`, `<=`, `>`, `>=`, `is_null`,
`assoc`

## Usage

```rust
use hkask_lisp::eval_sandboxed;
use serde_json::json;

let form = "(and (> (length findings) 0) (< composite 0.15))";
let env = json!({"findings": ["a", "b"], "composite": 0.12});
let result = eval_sandboxed(form, &env).unwrap();
assert_eq!(result, json!(true));
```

## Manifest usage

```yaml
- ordinal: 4
  action: compute
  compute_ref: "lisp.eval"
  input_mapping:
    form: "(and (> (length findings) 0) (< composite 0.15))"
    env:
      findings: "{{ step_2_result.findings }}"
      composite: "{{ step_3_result.composite }}"
```

## Dependencies

- `serde_json` — JSON interop
- `thiserror` — error types

No hKask crate dependencies — this is a standalone computation library.

## Security model

- No `eval` builtin (Lisp code cannot evaluate arbitrary strings)
- No `load` or `require`
- No I/O, no filesystem, no network, no environment variable access
- Bounded recursion depth (64) and bounded evaluation steps (100000)
- Environment is immutable from Lisp's perspective (define mutates a local
  scope discarded after evaluation)
- `#![forbid(unsafe_code)]` — no unsafe blocks anywhere in the crate

The caller must gate `lisp.eval` to `category: skill` manifests only —
infrastructure manifests (`runtime-config`, `daemon-process`) run without human
review and a Turing-complete step language is an attack surface (see `.rules`
trap on manifests).
