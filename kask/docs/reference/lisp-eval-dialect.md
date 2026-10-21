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

This is the canonical reference for the symbolic-neural scaffolding pattern, the `form`/`env` contract, and the sandboxed dialect exposed by the built-in `lisp_eval` tool. The tool delegates to `hkask_lisp::eval_sandboxed_with_budget` (`crates/agent/src/tools/lisp_eval_tool.rs:382`; `kask/crates/hkask-lisp/src/hkask_lisp.rs:2151`).

## Pattern

After de la Torre (2025, arXiv:2506.10021), Kask interleaves model reasoning with deterministic, stateless checks:

```text
LLM proposes → lisp_eval checks → LLM repairs or proceeds → lisp_eval scores → converge
```

Unlike the paper's persistent SBCL process, each sandbox call is stateless. Strong lexical captures remain intact while it runs. A private `InvocationScopes` RAII guard, held in a private `EvalBudget` field, tracks root, `let` and lambda-call scopes with `Weak` references. On drop it upgrades all surviving scopes before clearing any bindings and parent links, including cycles no longer reachable from the root (`kask/crates/hkask-lisp/src/hkask_lisp.rs:489-539`, `:2151-2179`). Cleanup covers success, returned errors and Rust unwinding, not process abort or OOM. Raw `Env` / `eval` callers retain their owned environments without this cleanup. State crosses sandbox calls through JSON `env`; there is no filesystem, network, dynamic `eval`, or module-loading surface (`kask/crates/hkask-lisp/src/hkask_lisp.rs:1-42`).

## `form` / `env` contract

- `form` is the Lisp source string to evaluate.
- The agent tool's `env` is JSON text encoding an object, not a bare object (`"env": "{\"a\": 1, \"b\": true}"`); omitted `env` defaults to an empty object. Decoded keys become top-level bindings. Values retain their JSON types — bind scalars directly; an object value becomes an association list, so a wrapped scalar reaches arithmetic as a list and fails with a type error. The core Rust API receives an already-parsed `serde_json::Value`.
- `max_steps` and `max_depth` bound evaluation. The tool defaults are 100,000 steps and depth 1,024 (tool-side defaults at `crates/agent/src/tools/lisp_eval_tool.rs:266-277`; the engine entry that applies them at `kask/crates/hkask-lisp/src/hkask_lisp.rs:2151`). Step cost is linear in list length since lisp-repair L3 — a 600-element walker costs ~7,900 steps — but recursive walkers still consume ~2–4 DEPTH frames per element, so raise `max_depth` for lists beyond ~250 elements.
- The result is converted back to JSON for the next reasoning step.

Pass prior structured output through `env`, then read object members with `assoc` (optional/nullable key) or `assoc!` (required key) inside the form. `assoc` returns the value directly (not a `(key . value)` cons pair — do not wrap `assoc` in `cdr`); an empty string remains a present value and needs its own semantic or `length` check.

## Integer admission and limits

Source tokens made of digits with an optional `+`/`-` sign must fit signed `i64`; otherwise parsing returns `LispError::Parse`, not a float fallback (`kask/crates/hkask-lisp/src/hkask_lisp.rs:436-459`). The sandbox rejects already-parsed unsigned JSON integers outside `i64`, including nested values (`:1959-1997`), but cannot recover an integer spelling already rounded into a JSON float.

The agent's `env` decoder checks original numeric spellings before JSON object projection, including nested values, overwritten duplicate-key values and integers beyond `u64`. It uses existing serde JSON `RawValue` plus a structure pass preserving serde's default nesting refusal, not a new parser or dependency (`crates/agent/src/tools/lisp_eval_tool.rs:105-240`). Explicit finite floats such as `1.0` / `1e0`, numeric-looking strings and valid duplicate-key last-value behavior remain accepted.

Lower-level `from_json` / `to_json` are unchecked conversions, not sandbox entry points: `from_json` can convert out-of-i64 unsigned integers to floats, and `to_json` maps non-finite floats to JSON null (`kask/crates/hkask-lisp/src/hkask_lisp.rs:2044-2120`). H3 non-finite arithmetic and mixed-number promotion are not implemented by H1/H2. Step/depth accounting and cycle cleanup are not wall-clock or heap quotas or a claim of complete numeric correctness or sandbox safety. Defaults, output conversion and infix rewriting are unchanged.

## Interpreter surface

The built-in registry is installed by `default_builtins` (`kask/crates/hkask-lisp/src/hkask_lisp.rs:863@default_builtins`). Important dialect rules:

- Boolean literals are `true`, `false`, and `nil`; `t` is also truthy and is suitable as the final `cond` clause.
- `=` is numeric equality and ERRORS on non-numbers (the numeric-only contract; a non-number argument is a type error, never a silent false). All-integer comparisons are exact i64 — never coerced through f64 (`kask/crates/hkask-lisp/src/hkask_lisp.rs:1113@num_eq`). Use `string=` for string-only equality (`nil` evaluates to false so nullable/absent JSON fields are null-safe, while non-string scalars/lists like `(string= 1 "1")` error with `TypeError`, `:1531@string_eq_fn`) and `eq` for structural equality (`:1872@eq_fn`).
- `string-contains` takes the needle FIRST: `(string-contains "needle" "haystack")` — the searched-for string precedes the searched-in string, following the `assoc`/`member` convention (registry at `kask/crates/hkask-lisp/src/hkask_lisp.rs:912@default_builtins`, implementation at `:1611@string_contains_fn`). An empty needle errors rather than matching anything; a needle longer than the haystack errors naming the probable reversal — reversed arguments return false silently otherwise (a 20-quote verification batch failed every check this way, 2026-09-28).
- `append` joins lists; `nil` arguments behave as empty lists.
- `concat` joins strings.
- Prefix and infix arithmetic forms are both accepted, but infix is a token-level rewrite with sharp edges (all five behaviors verified live and pinned): bare `a + b` expands to `(+ a b)` and same-operator chains fold; MIXED operators do not associate (`1 + 2 * 3` parses as three top-level forms and the last one's value wins); a parenthesized `(a + b)` double-wraps and ERRORS — not equivalent to `(+ a b)`; and the rewriter also fires inside prefix forms (`(- 5 - 3)` → `(- (- 5 3))` → -2). Prefer prefix form around `let`, `if`, recursion, and nested logic.
- `define` inside a `let` mutates that child environment. A `define` inside a called lambda mutates the call environment, not the captured parent; recursive helpers should accumulate through return values.
- There is no `eval`, `load`, or `require` builtin.
- Recursive walkers consume ~2–4 depth frames per element: raise `max_depth` explicitly for lists beyond ~250 elements rather than relying on the default 1,024. Step cost is no longer the binding constraint at that scale (linear since lisp-repair L3).
- Integer arithmetic is CHECKED: overflow is a typed error, never a silent wrap (`(+ 9223372036854775807 1)` errors; `kask/crates/hkask-lisp/src/hkask_lisp.rs:978@add`). `/` always returns a Float (`(/ 6 3)` → `2.0`) and division by zero errors; over 3+ arguments `!=` compares adjacent pairs.
- `length` counts CHARACTERS on strings (`"héllo"` → 5; `kask/crates/hkask-lisp/src/hkask_lisp.rs:1242@length`); the `string-contains` reversal guard compares BYTE lengths.
- `stringp` tests for strings (`:1357@stringp`); `numberp` tests numbers; `listp` tests lists-or-nil.
- `starts-with` / `ends-with` (`(starts-with prefix subject)`, `(ends-with suffix subject)` — needle-first like `string-contains`): directional prefix/suffix checks with NO length guard — a prefix/suffix longer than the subject is a legitimate false, so these work where `string-contains`'s reversal guard would error (the adhd-mode lexical gate's case). Empty prefix/suffix errors (the `string-contains` precedent: a check that fires on correct output is worse than no check).
- `boundary-ok` (`(boundary-ok prefix value)`, `:1712@boundary_ok_fn`) is the character-access primitive: true iff `value` begins with `prefix` and the FIRST character AFTER the prefix is printable non-whitespace. It exists because the dialect has no other way to inspect a character — `(nth 0 s)` is a type error on a string. The class is a CLOSED ALLOWLIST (`!is_whitespace() && !is_control()` plus an explicit format-character (Cf) set), not a blocklist: a space/tab/newline blocklist cannot enumerate Unicode whitespace and passes CR, VT, FF, NBSP and ZWSP — all five passed the VC-0 typed-reference predicate while its stated invariant was "no leading whitespace" (live 2026-10-18). The check is BOUNDARY-scoped, so a legitimate multi-word value (`boundary:a b`) passes. An empty `prefix` checks the value's first character; a `value` equal to `prefix` is false (no character to inspect); a non-string is a type error.
- `nth` is 0-based: a negative index errors with a teaching `Runtime` error (`nth: negative index -1 — indices are 0-based non-negative integers; use (- (length list) 1) for the last element`), while out-of-bounds non-negative indices return `nil` (operator ruling 2026-10-14, `:1271@nth`).
- `assoc` vs `assoc!` and the absent-key convention (operator ruling 2026-10-14):
  - `(assoc key alist)` (`:1400@assoc_fn`) is the optional/nullable lookup: it returns `nil` when `key` is absent (or when `alist` is `nil` or a raw string from an LLM step), and rejects non-string primitive scalars (`Int`/`Float`/`Bool`) as `alist`. Guard optional keys with `is_null` before comparing, or rely on `(string= (assoc "k" obj) "x")` returning `false` on `nil`.
  - `(assoc! key alist)` (`:1422@assoc_strict_fn`) is the strict required-key lookup: when `key` is absent, it errors naming the missing key and the sorted list of available keys on `alist` (`assoc!: key "tier" not found — available keys: ["contract", "location", "verdict"]`), closing the absent-key vacuity trap.
- Cost model (lisp-repair L3): nodes are charged once at creation and once per actual traversal — `length` charges its walk (`:1242@length`), `nth`/`reverse`/`append` charge the to_vec spine plus string-head bytes (`charge_list_spine`, `:1918@charge_list_spine`), and `assoc`/`assoc!`/`member`/`eq` charge deep structure. Argument passing costs one tick per argument plus top-level string/symbol bytes — never per node — so list walkers are linear in step cost.

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
