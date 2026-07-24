# hkask-services-self-heal — Self-Healing Engine

Two-stage autonomous error-recovery engine. Maps error patterns to recovery strategies, executes healing actions, and returns `Healed` (retry), `Degraded` (fallback), or `Unhealable` (escalate to Curator). Extracted from `hkask-services-core` (see `tasks/plan-core-scope-contraction.md`, Task 1.1).

**Version:** v0.31.0 | **Crate:** `hkask-services-self-heal`

## Exports

| Type | Purpose |
|------|---------|
| `SelfHealer` | Two-stage recovery engine (`attempt()`, `with_inference()`) |
| `HealRegistry` | Strategy catalog (`with_defaults()`, `add()`, `find_strategy_by_name()`) |
| `HealAction` | Recovery action enum (`RunCommand`, `SetEnv`, `LoadDotEnv`, `CreateDefaultFile`, `RetryWithBackoff`, `ProposeCodeChange`, `LlmAssisted`) |
| `HealContext` | Per-attempt context (operation, error, env, config paths, can_retry) |
| `HealStrategy` | Named pattern → action mapping |
| `HealOutcome` | Result of an attempt (Healed / Degraded / Unhealable + diagnostics) |
| `HealError` | Recovery failure enum |
| `HealInferenceFn` | Stage-2 LLM callback type (optional, via `with_inference()`) |
| `EnvValueSource` | Env-value resolution source (literal / env var / file) |

## Stages

- **Stage 1 (always available):** deterministic env/config healing — no inference required.
- **Stage 2 (optional, `with_inference()`):** LLM template-assisted healing via Jinja2 templates from `registry/templates/heal/`.

## Dependencies

- `hkask-types` — `RegulationSpan::SelfHeal` for Regulation span emission
- `minijinja` — Stage-2 template rendering
- `serde` / `serde_json` — strategy + instruction (de)serialization
- `thiserror` — `HealError`
- `tracing` — Regulation span emission
- `dotenvy` — `LoadDotEnv` action
- `dirs` — default config search paths