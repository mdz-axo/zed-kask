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
  a deliberate restriction on the language surface.
- **No `Hash` type**: JSON objects become association lists rather than a
  separate hash map type. Users implement `map`/`filter` in Lisp itself.

## Supported forms

**Special forms**: `quote`, `if`, `let`, `lambda`, `define`, `begin`, `and`,
`or`, `not`, `cond`

**Built-in functions**: `car`, `cdr`, `cons`, `list`, `length`, `nth`,
`reverse`, `+`, `-`, `*`, `/`, `=`, `!=`, `<`, `<=`, `>`, `>=`, `is_null`,
`numberp`, `listp`, `assoc`, `append`, `member`, `abs`, `sqrt`, `max`,
`min`, `eq`, `string=`, `string-contains`, `concat`

## Usage

```rust
use hkask_lisp::eval_sandboxed;
use serde_json::json;

let form = "(and (> (length findings) 0) (< composite 0.15))";
let env = json!({"findings": ["a", "b"], "composite": 0.12});
let result = eval_sandboxed(form, &env).unwrap();
assert_eq!(result, json!(true));
```

## Numeric admission

Source tokens consisting of digits with an optional `+`/`-` sign must fit
signed `i64`; out-of-range spellings return `LispError::Parse` rather than
falling back to a rounded float. The sandbox also rejects already-parsed
JSON unsigned integers outside `i64`, including nested values. It cannot
recover an original integer spelling already rounded into a JSON float.

The agent tool accepts `env` as JSON text and checks original numeric tokens
before projection into `serde_json::Value`, including nested and duplicate-key
values and integers beyond `u64`. It uses existing serde JSON `RawValue` and a
structure pass retaining serde's default nesting check, not a new parser or
dependency. Explicit finite floats (`1.0`, `1e0`) and numeric-looking strings
remain accepted. H3 rejects non-finite literals, evaluated scalar floats,
numeric operands/results and sandbox output; required integer-to-`f64`
promotion must be exact. Integer-only arithmetic remains checked and exact.
Raw conversions, defaults and infix rewriting are unchanged.

## Skill usage

Native and delegated `host/lisp_eval` share the public
`evaluate_lisp_controlled` helper and `LispEvalToolInput` decoder. With an
invocation flag they use `eval_sandboxed_with_cancellation`; pure
`evaluate_lisp` retains `eval_sandboxed_with_budget`
(`crates/agent/src/tools/lisp_eval_tool.rs:481-490@evaluate_lisp_controlled`).
The SKILL.md instructs the agent to call `lisp_eval` for
convergence checks, invariant validation, and deterministic scoring:

```
Call `lisp_eval`:
  form: "(and (> (length findings) 0) (< composite 0.15))"
  env: JSON text encoding { "findings": <step_2_result.findings>, "composite": <step_3_result.composite> }
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
- Sandbox defaults remain 100000 steps / depth 1024; corpus queries retain 100000 / 100.
- Provisional H4A admission ceilings (approved plan, not an H4C safe-memory/latency envelope):
  `MAX_SOURCE_BYTES` = 65536; `MAX_ENV_JSON_BYTES` = 1048576;
  `MAX_STEPS` = 2000000; `MAX_DEPTH` = 8192.
  Source UTF-8 length is checked before tokenization (including standalone `parse`);
  requested budgets before parsing/`Value` traversal; tool raw-env UTF-8 length before inner JSON validation.
  Oversized requests reject, never clamp, naming field, requested and allowed values.
  Already-parsed `Value` has no recoverable wire length; work/depth guards still apply.
  Zero and budget aliases are unchanged: depth-zero atoms pass, compound forms refuse.
- Raw `from_json` / `to_json` bypass sandbox work/depth and admission checks:
  unsigned integers can become floats; non-finite floats become JSON null.
  Use the sandbox boundary for its checks.
- H4B caps the exact serde-pretty JSON **result document** at `MAX_OUTPUT_BYTES` =
  1048576 (1 MiB), including UTF-8, escaping, indentation and projected object keys
  with duplicate-key last-value semantics. After work/output-depth checks and
  `to_json`, the core counts without retaining JSON bytes before publishing the `Value`,
  including empty-program null. Public `format_json_output` checks raw `Value` depth
  (128) and counts before requesting the exact-size final buffer; agent display reuses it.
  Legal success/error shapes remain; oversized evaluation returns an error, not success-null.
  Final-buffer test observations are not global allocator/heap measurements.
- Strong lexical captures survive evaluation. `EvalBudget`'s private
  `InvocationScopes` RAII guard weakly tracks root, `let` and lambda-call
  scopes; each tracked entry prepays two ticks for registration and cleanup before
  `Rc` allocation/registry push. The fallible private factory's three callers propagate
  refusal; infallible drop upgrades all survivors before clearing bindings and
  parent links, breaking even disconnected closure cycles. Cleanup covers
  success, errors and Rust unwinding, not abort/OOM. Raw `Env` / `eval`
  callers retain ownership without automatic cleanup.
- Outer provider/MCP parsing, typed input decoding and env sorting remain outside
  core work caps. The output cap excludes envelopes, error strings and intermediate
  `Value` heap; it is not a total-heap, time or concurrency quota. Large intermediate
  values can exist before refusal. H4C operating measurements remain pending.
- H5/H6 native titles inspect only the first 80 Unicode form characters, never env.
  Typed decoding/evaluation run via `cx.background_spawn`, with evaluator `Rc`s
  confined to synchronous worker evaluation. One monotonic `Arc<AtomicBool>` per
  invocation requests cooperative cancellation on user cancel or result-task drop;
  `eval_sandboxed_with_cancellation` returns `LispError::Cancelled` at checkpoints
  (`src/hkask_lisp.rs:2505@eval_sandboxed_with_cancellation`). Scope teardown is
  mandatory, not cancellable. Bounded serde/std operations, raw conversions and
  destruction remain nonpreemptible; this is no hard time, heap or concurrency quota.
  Native turn completion/cancel joins worker cleanup receipts; publication checks
  the old turn's cancellation state and weak pending-owner identity prevents stale
  writes/flushes into a successor. Raw/pure APIs are unchanged. Raw-output copying
  and bounded display formatting remain foreground (two counts, one emit).
- H7 authorized IPC host calls (the delegated host decode/eval path through
  `kask/crates/kask_bridge/src/host_skill_tools.rs` over the inference IPC
  `tool_invoke` method) were removed 2026-10-28 with the swarm capability,
  their only consumer. No host/IPC lisp path remains; the in-process native
  decode/evaluation path is unchanged, and no host/IPC lisp promise survives
  (see `kask/docs/reference/lisp-eval-dialect.md`).
- H8 direct/routed corpus queries share one internal body; only Lisp-option work
  is offloaded. The pre-spawn guard and rmcp request-context cancellation cover
  evaluation/join only, leaving downstream inference/retrieval/generation alone
  (`kask/mcp-servers/hkask-mcp-corpus/src/tools/storage.rs:108-220`, `:707-723`).
  Selector/defaults and fixed 100000/100 remain; boolean options accept `true`/`t`
  or `false`, reject `nil` before retrieval, and now teach that contract
  (`storage.rs:605-705@parse_lisp_query`). Explicit rmcp cancel reaches the pending
  worker; plain EOF does not immediately signal it. In the tested lifecycle,
  context cancellation follows service-owner teardown after handler drain.
  Promise handler-owner drop / propagated context, not immediate disconnect or
  service-shutdown-joins-worker
  (`kask/mcp-servers/hkask-mcp-corpus/src/retrieval_tests.rs:2358-2499@h8_rmcp_request_cancel_and_disconnect_capture`,
  `:2520-2642@h8_rmcp_post_lisp_cancellation_preserves_inference`).
  Async IO tests are not live stdio-child or independent mid-core cleanup proof.
- H4C's measured operating envelope remains deferred. H6 native Stop validation
  was not performed; installed live build `1c247be309` predates H7/H8.
  No new runtime, service, dependency, cache or configuration is introduced.
- `#![forbid(unsafe_code)]` — no unsafe blocks anywhere in the crate
