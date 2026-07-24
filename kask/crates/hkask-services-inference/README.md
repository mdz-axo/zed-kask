# hkask-services-inference — Inference Service Layer

Service-layer façade over `hkask-inference`'s `InferenceRouter`, plus the process-scoped TTL `ModelCache` for model lists. Extracted from `hkask-services-core` (see `tasks/plan-core-scope-contraction.md`, Task 3.1) — the final slice of the core scope-contraction plan.

**Version:** v0.31.0 | **Crate:** `hkask-services-inference`

## Exports

| Type | Purpose |
|------|---------|
| `InferenceContext` | Shared inference port + default model + config; `from_parts` |
| `InferenceService` | Façade — `resolve_port`, `list_models` (cached), `search_models` (filter over cache) |
| `ModelInfo` | Model descriptor (name, provider, family, params, quant, size); `From<RouterModelEntry>` |
| `ModelCache` | Process-scoped TTL cache (lazy populate, manual `invalidate`, `is_stale`); **poison-recovering** |

## ModelCache — poison recovery

The cache mutex is recovered (not panicked) if a prior holder panicked (`unwrap_or_else(|poison| poison.into_inner())`) — a daemon thread panic no longer crashes model discovery. See ADR-054 / ADR-043 (eliminate-nested-runtime-panics). The regression test poisons the mutex and asserts `list_models` returns `Ok`.

## Dependencies

- `hkask-inference` — `InferenceConfig`, `InferenceRouter`, `ProviderId`, `RouterModelEntry`
- `hkask-ports` — `InferencePort`
- `hkask-services-core` — `ServiceError` (the one remaining foundation dep)
- `tracing` — Regulation span emission
- (dev) `tokio` — `#[tokio::test]` for the cache-lifecycle/poison-recovery test