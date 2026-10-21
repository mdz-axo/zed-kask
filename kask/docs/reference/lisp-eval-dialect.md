---
title: "The lisp_eval Dialect"
audience: [agents, developers]
last_updated: 2026-10-20
version: "0.42.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [composition, trust]
---

# The `lisp_eval` Dialect

This is the canonical reference for the symbolic-neural scaffolding pattern, the `form`/`env` contract, and the sandboxed dialect exposed by the built-in `lisp_eval` tool. The tool delegates to `hkask_lisp::eval_sandboxed_with_budget` (`crates/agent/src/tools/lisp_eval_tool.rs:440`; `kask/crates/hkask-lisp/src/hkask_lisp.rs:2253@eval_sandboxed_with_budget`).

## Pattern

After de la Torre (2025, arXiv:2506.10021), Kask interleaves model reasoning with deterministic, stateless checks:

```text
LLM proposes → lisp_eval checks → LLM repairs or proceeds → lisp_eval scores → converge
```

Unlike the paper's persistent SBCL process, each sandbox call is stateless. Strong lexical captures remain intact while it runs. A private `InvocationScopes` RAII guard, held in a private `EvalBudget` field, tracks root, `let` and lambda-call scopes with `Weak` references. Each tracked entry prepays two ticks for registration and cleanup before `Rc` allocation/registry push; all three callers propagate the private factory's refusal. Infallible drop upgrades all surviving scopes before clearing any bindings and parent links, including cycles no longer reachable from the root (`kask/crates/hkask-lisp/src/hkask_lisp.rs:512-569`, `:2253-2295`). Cleanup covers success, returned errors and Rust unwinding, not process abort or OOM. Raw `Env` / `eval` callers retain their owned environments without this cleanup. State crosses sandbox calls through JSON `env`; there is no filesystem, network, dynamic `eval`, or module-loading surface (`kask/crates/hkask-lisp/src/hkask_lisp.rs:1-42`).

## `form` / `env` contract

- `form` is the Lisp source string to evaluate.
- The agent tool's `env` is JSON text encoding an object, not a bare object (`"env": "{\"a\": 1, \"b\": true}"`); omitted `env` defaults to an empty object. Decoded keys become top-level bindings. Values retain their JSON types — bind scalars directly; an object value becomes an association list, so a wrapped scalar reaches arithmetic as a list and fails with a type error. The core Rust API receives an already-parsed `serde_json::Value`.
- `max_steps` and `max_depth` bound evaluation; above-ceiling requests reject, never clamp, naming field, requested and allowed values. Zero and `maxSteps`/`maxDepth` aliases are unchanged: depth-zero atoms pass, compound forms refuse. The tool defaults remain 100,000 steps and depth 1,024 (tool-side defaults at `crates/agent/src/tools/lisp_eval_tool.rs:280-291`; the engine entry at `kask/crates/hkask-lisp/src/hkask_lisp.rs:2253@eval_sandboxed_with_budget`). Step cost is linear in list length since lisp-repair L3 — a 600-element walker costs ~7,900 steps — but recursive walkers still consume ~2–4 DEPTH frames per element, so raise `max_depth` for lists beyond ~250 elements.
- The result is converted back to JSON for the next reasoning step.

Pass prior structured output through `env`, then read object members with `assoc` (optional/nullable key) or `assoc!` (required key) inside the form. `assoc` returns the value directly (not a `(key . value)` cons pair — do not wrap `assoc` in `cdr`); an empty string remains a present value and needs its own semantic or `length` check.

## Numeric admission and limits

| Enforced ceiling | Allowed | Admission boundary |
|---|---:|---|
| `MAX_SOURCE_BYTES` | 65536 | Source UTF-8 bytes before tokenization, including standalone `parse` (`:278@parse_with_budget`) |
| `MAX_ENV_JSON_BYTES` | 1048576 | Tool raw-env UTF-8 bytes before inner JSON validation (`crates/agent/src/tools/lisp_eval_tool.rs:110@deserialize_env_field`) |
| `MAX_STEPS` | 2000000 | Requested steps before parsing/`Value` traversal |
| `MAX_DEPTH` | 8192 | Requested depth before parsing/`Value` traversal |
| `MAX_OUTPUT_BYTES` | 1048576 (1 MiB) | Exact serde-pretty result document after projection, before publication/final JSON buffer (`:2209@pretty_json_output_size`, `:2230@format_json_output`) |

H4A ceilings are provisional approved-plan bounds, not an H4C measured operating envelope. Already-parsed `Value` cannot recover wire length; work/depth guards still apply. Outer provider/MCP parsing, typed decoding and env sorting remain outside core work caps. Defaults remain 100000 / 1024 (corpus 100000 / 100).

H4B's private `OutputByteCounter` retains counts, not JSON bytes: serde includes UTF-8, control escaping, indentation and projected object keys/last duplicate value. The core counts after existing work/output-depth guards and `to_json`, including empty-program null. Public `format_json_output` checks raw Rust `Value` depth (128), counts, then requests `Vec::with_capacity(exact_count)`, emits and converts to UTF-8. Agent display reuses it; legal `Success { result: Value }` / `Error` shapes remain, but oversized evaluation returns `Err`, not success-null (`hkask_lisp.rs:2169-2295`; `lisp_eval_tool.rs:293-309`). Final-buffer test observations are not global allocator/heap measurements. Envelopes, error strings, intermediate `Value` heap, time and concurrency are excluded; large intermediate values can exist before refusal.

H5's optional `AgentTool::raw_initial_title` defaults to `None`, consulted before typed clone/decode. Lisp shares a typed/raw helper reading only the first 80 Unicode form characters or generic fallback, never env, including partial input; other tools retain typed titles/malformed fallback (`crates/agent/src/thread.rs:7151-7153,7301-7307`; `lisp_eval_tool.rs:315-359`). `run` background-spawns `input.recv()` typed decoding and pure evaluation; evaluator `Rc`s stay within synchronous worker evaluation (`lisp_eval_tool.rs:362-398`). Generic raw-output copying and bounded display formatting remain foreground: normally two counting passes and one emit. H4C CPU/heap/latency measurements and fresh parent gates remain pending; cancellation/Task-drop joining (H6) and host/corpus offload (H7/H8) are deferred. Host/corpus inherit only the core output cap; no new runtime or result cache.

Source tokens made of digits with an optional `+`/`-` sign must fit signed `i64`; otherwise parsing returns `LispError::Parse`, not a float fallback (`kask/crates/hkask-lisp/src/hkask_lisp.rs:454@parse_atom`). The sandbox rejects already-parsed unsigned JSON integers outside `i64`, including nested values (`:1986@charge_json`), but cannot recover an integer spelling already rounded into a JSON float.

The agent's `env` decoder checks original numeric spellings before JSON object projection, including nested values, overwritten duplicate-key values and integers beyond `u64`. It uses existing serde JSON `RawValue` plus a structure pass preserving serde's default nesting refusal, not a new parser or dependency (`crates/agent/src/tools/lisp_eval_tool.rs:110-254`). Explicit finite floats such as `1.0` / `1e0`, numeric-looking strings and valid duplicate-key last-value behavior remain accepted.

H3 rejects non-finite literals, evaluated scalar floats, numeric operands/results and sandbox output; required integer-to-`f64` promotion must be exact (`kask/crates/hkask-lisp/src/hkask_lisp.rs:454@parse_atom`, `:608@eval_with_budget`, `:988@finite_number`, `:998@as_f64`, `:1960@charge_value`). Integer-only arithmetic remains checked; comparisons remain exact. Raw `from_json` / `to_json` stay unchecked: unsigned integers can become floats, and non-finite floats become JSON null (`:2071@from_json`, `:2111@to_json`). Defaults and infix rewriting are unchanged; these guards and cycle cleanup do not establish complete sandbox safety or wall-clock/heap quotas.

## Interpreter surface

The built-in registry is installed by `default_builtins` (`kask/crates/hkask-lisp/src/hkask_lisp.rs:895@default_builtins`). Important dialect rules:

- Boolean literals are `true`, `false`, and `nil`; `t` is also truthy and is suitable as the final `cond` clause.
- `=` is numeric equality and ERRORS on non-numbers (the numeric-only contract; a non-number argument is a type error, never a silent false). All-integer comparisons are exact i64 — never coerced through f64 (`kask/crates/hkask-lisp/src/hkask_lisp.rs:1130@num_eq`). Use `string=` for string-only equality (`nil` evaluates to false so nullable/absent JSON fields are null-safe, while non-string scalars/lists like `(string= 1 "1")` error with `TypeError`, `:1551@string_eq_fn`) and `eq` for structural equality (`:1896@eq_fn`).
- `string-contains` takes the needle FIRST: `(string-contains "needle" "haystack")` — the searched-for string precedes the searched-in string, following the `assoc`/`member` convention (registry at `kask/crates/hkask-lisp/src/hkask_lisp.rs:895@default_builtins`, implementation at `:1631@string_contains_fn`). An empty needle errors rather than matching anything; a needle longer than the haystack errors naming the probable reversal — reversed arguments return false silently otherwise (a 20-quote verification batch failed every check this way, 2026-09-28).
- `append` joins lists; `nil` arguments behave as empty lists.
- `concat` joins strings.
- Prefix and infix arithmetic forms are both accepted, but infix is a token-level rewrite with sharp edges (all five behaviors verified live and pinned): bare `a + b` expands to `(+ a b)` and same-operator chains fold; MIXED operators do not associate (`1 + 2 * 3` parses as three top-level forms and the last one's value wins); a parenthesized `(a + b)` double-wraps and ERRORS — not equivalent to `(+ a b)`; and the rewriter also fires inside prefix forms (`(- 5 - 3)` → `(- (- 5 3))` → -2). Prefer prefix form around `let`, `if`, recursion, and nested logic.
- `define` inside a `let` mutates that child environment. A `define` inside a called lambda mutates the call environment, not the captured parent; recursive helpers should accumulate through return values.
- There is no `eval`, `load`, or `require` builtin.
- Recursive walkers consume ~2–4 depth frames per element: raise `max_depth` explicitly for lists beyond ~250 elements rather than relying on the default 1,024. Step cost is no longer the binding constraint at that scale (linear since lisp-repair L3).
- Integer arithmetic is CHECKED: overflow is a typed error, never a silent wrap (`(+ 9223372036854775807 1)` errors; `kask/crates/hkask-lisp/src/hkask_lisp.rs:1068@add`). `/` always returns a Float (`(/ 6 3)` → `2.0`) and division by zero errors; over 3+ arguments `!=` compares adjacent pairs.
- `length` counts CHARACTERS on strings (`"héllo"` → 5; `kask/crates/hkask-lisp/src/hkask_lisp.rs:1262@length`); the `string-contains` reversal guard compares BYTE lengths.
- `stringp` tests for strings (`:1377@stringp`); `numberp` tests numbers; `listp` tests lists-or-nil.
- `starts-with` / `ends-with` (`(starts-with prefix subject)`, `(ends-with suffix subject)` — needle-first like `string-contains`): directional prefix/suffix checks with NO length guard — a prefix/suffix longer than the subject is a legitimate false, so these work where `string-contains`'s reversal guard would error (the adhd-mode lexical gate's case). Empty prefix/suffix errors (the `string-contains` precedent: a check that fires on correct output is worse than no check).
- `boundary-ok` (`(boundary-ok prefix value)`, `:1732@boundary_ok_fn`) is the character-access primitive: true iff `value` begins with `prefix` and the FIRST character AFTER the prefix is printable non-whitespace. It exists because the dialect has no other way to inspect a character — `(nth 0 s)` is a type error on a string. The class is a CLOSED ALLOWLIST (`!is_whitespace() && !is_control()` plus an explicit format-character (Cf) set), not a blocklist: a space/tab/newline blocklist cannot enumerate Unicode whitespace and passes CR, VT, FF, NBSP and ZWSP — all five passed the VC-0 typed-reference predicate while its stated invariant was "no leading whitespace" (live 2026-10-18). The check is BOUNDARY-scoped, so a legitimate multi-word value (`boundary:a b`) passes. An empty `prefix` checks the value's first character; a `value` equal to `prefix` is false (no character to inspect); a non-string is a type error.
- `nth` is 0-based: a negative index errors with a teaching `Runtime` error (`nth: negative index -1 — indices are 0-based non-negative integers; use (- (length list) 1) for the last element`), while out-of-bounds non-negative indices return `nil` (operator ruling 2026-10-14, `:1291@nth`).
- `assoc` vs `assoc!` and the absent-key convention (operator ruling 2026-10-14):
  - `(assoc key alist)` (`:1420@assoc_fn`) is the optional/nullable lookup: it returns `nil` when `key` is absent (or when `alist` is `nil` or a raw string from an LLM step), and rejects non-string primitive scalars (`Int`/`Float`/`Bool`) as `alist`. Guard optional keys with `is_null` before comparing, or rely on `(string= (assoc "k" obj) "x")` returning `false` on `nil`.
  - `(assoc! key alist)` (`:1442@assoc_strict_fn`) is the strict required-key lookup: when `key` is absent, it errors naming the missing key and the sorted list of available keys on `alist` (`assoc!: key "tier" not found — available keys: ["contract", "location", "verdict"]`), closing the absent-key vacuity trap.
- Cost model (lisp-repair L3): nodes are charged once at creation and once per actual traversal — `length` charges its walk (`:1262@length`), `nth`/`reverse`/`append` charge the to_vec spine plus string-head bytes (`charge_list_spine`, `:1942@charge_list_spine`), and `assoc`/`assoc!`/`member`/`eq` charge deep structure. Argument passing costs one tick per argument plus top-level string/symbol bytes — never per node — so list walkers are linear in step cost.

## Worked invariant check

```json
{
  "form": "(let ((items (assoc \"items\" payload))) (if (is_null items) (list \"missing_items\") (if (= (length items) expected) true (list \"wrong_count\"))))",
  "env": "{\"payload\": {\"items\": [1, 2, 3]}, \"expected\": 3}"
}
```

The agent uses the returned boolean or defect list as a gate; it does not treat model self-assessment as the deterministic result.

## Provenance

- de la Torre, J. (2025). *From Tool Calling to Symbolic Thinking: LLMs in a Persistent Lisp Metaprogramming Loop*. arXiv:2506.10021. https://arxiv.org/abs/2506.10021
- This reference was folded from the former `lisp-scaffold-reasoning` demonstration skill on 2026-09-09; the implementation authority is `kask/crates/hkask-lisp/src/hkask_lisp.rs`.
