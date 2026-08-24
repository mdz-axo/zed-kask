---
title: ".rules Proposal — KaskInferenceProvidersSettings Default/From Divergence"
audience: [architects, developers]
last_updated: 2026-08-24
version: "0.2.0"
status: "Draft"
domain: "Settings"
mds_categories: [composition, lifecycle]
---

# `.rules` proposal — `KaskInferenceProvidersSettings` Default/`From` divergence

**Status:** proposal (per `.rules` hygiene: "Don't edit `.rules` inline during
feature work — propose additions in PR descriptions."). This file is the
PR-description payload; merge it into `.rules` under the "Settings and
constants" section in the same PR that lands a renderer that touches the
divergence, or independently once reviewed.

## Proposed `.rules` entry

> `KaskInferenceProvidersSettings` is the one sub-struct where `Default` and
> `From<Content>` diverge: `Default` is all-false (derived, pure, no side
> effects — keeps `KaskSettings::default()` and tests deterministic); `From`
> calls `from_env()` which auto-enables a provider whose API key env var is
> set (`OPENROUTER_API_KEY`).
> Renderers must go through `From` or `from_env()`, never
> `unwrap_or_default()` on the inner field — `Default` would silently hide a
> configured provider from the UI. The divergence is documented at
> `kask/crates/kask_bridge/src/settings.rs:189-225` and
> `crates/settings_ui/src/pages/kask_page/inference_providers.rs:35-44`.

## Why this is a rule (meets the `.rules` bar)

- **Non-obvious:** the `#[derive(Default)]` on the struct makes the all-false
  default look like the natural fallback. A renderer author who writes
  `raw.and_then(|c| c.inference_providers).unwrap_or_default()` gets code that
  compiles, passes type-check, and silently shows every provider toggle as
  off — even when the user has an API key in the environment. The bug is
  invisible without knowing `From` deliberately diverges from `Default`.
- **Repeatedly encountered:** the divergence was already significant enough
  to require a dedicated `from_env()` method, a doc comment on the struct, a
  comment block in the renderer, and three pinning tests
  (`inference_providers_default_is_all_false`,
  `kask_settings_default_inference_providers_all_false`,
  `inference_providers_from_env_does_not_panic`). The renderer comment
  (`inference_providers.rs:39-40`) explicitly says "we must go through `From`
  or `from_env()`" — i.e., the trap has already been stated once, in prose,
  at the call site. A `.rules` entry makes it a project-wide trap, not a
  per-call-site comment.
- **Specific enough to act on:** the rule names the struct, the two
  construction paths, the env vars, and the renderer file. A future renderer
  author can grep for `KaskInferenceProvidersSettings` and find the rule.

## Verification (re-derived from the code, not the prior agent's claims)

- `kask/crates/kask_bridge/src/settings.rs:199` —
  `#[derive(... Default)]` on `KaskInferenceProvidersSettings` → `Default` is
  all-false (the derived `Default` for a struct of `bool` fields sets each to
  `false`).
- `kask/crates/kask_bridge/src/settings.rs:218-224` — `from_env()` reads
  `OPENROUTER_API_KEY` and
  sets the corresponding `*_enabled` to `true` if the env var is present.
- `kask/crates/kask_bridge/src/settings.rs:1411-1421` — `From<Content>` calls
  `from_env()` and uses each env-resolved value as the fallback for a `None`
  field (`.unwrap_or(from_env.<field>)`).
- `kask/crates/kask_bridge/src/settings.rs:1461-1464` — the top-level
  `From<KaskSettingsContent>` for `KaskSettings` uses
  `.unwrap_or_else(KaskInferenceProvidersSettings::from_env)` for the
  `inference_providers` field — NOT `unwrap_or_default()`. This is the
  production path; it deliberately diverges from `Default`.
- `crates/settings_ui/src/pages/kask_page/inference_providers.rs:35-44` — the
  renderer comment documents the divergence and the renderer goes through
  `From`/`from_env()`, not `Default`.
- `kask/crates/kask_bridge/src/settings.rs:2030-2059` — three tests pin the
  contract: `Default` is all-false, `KaskSettings::default()` inherits
  all-false, and `from_env()` does not panic.

## What the rule prevents

A renderer that does:

```rust
let inference = raw
    .and_then(|c| c.inference_providers)
    .unwrap_or_default();  // ← BUG: all-false, ignores env vars
```

…would show every provider toggle as off even when the user has
`OPENROUTER_API_KEY` set in their environment. The user appears unconfigured;
the runtime (which goes through `From`) auto-enables the provider. The UI and
the runtime disagree — a broken feedback loop (the same class of trap as
`unwrap_or(0)` on regulation sense inputs: a missing signal reads as a
measured zero).

The correct form:

```rust
let inference = raw
    .and_then(|c| c.inference_providers)
    .map(Into::into)  // ← From<Content>, calls from_env() for None fields
    .unwrap_or_else(KaskInferenceProvidersSettings::from_env);
```

## Scope

This rule is specific to `KaskInferenceProvidersSettings`. Other kask
sub-structs (`KaskMemorySettings`, `KaskCondenserSettings`, etc.) use
`#[serde(default, deny_unknown_fields)]` and their `Default`/`From` agree —
the divergence is not a general pattern, it's a deliberate exception for the
one sub-struct whose defaults depend on the process environment.
