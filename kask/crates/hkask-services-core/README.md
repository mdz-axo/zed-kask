# hkask-services-core — Core Service Traits and Types

Foundation crate for the hKask service layer. Defines shared types, configuration, error taxonomy, and port traits used across all service crates.

**Version:** v0.31.0 | **Crate:** `hkask-services-core`

## Modules

| Module | Purpose |
|--------|---------|
| `config` | `ServiceConfig` with `DEFAULT_DB_PATH` |
| `data_category` | Content classification (`DataCategory` parsing) |
| `error` | Canonical `ServiceError` enum |
| `goal` | `Goal`, `GoalArtifact`, `GoalCriterion`, `GoalState` types |
| `identity` | WebID, `UserRole`, identity management |
| `inference_svc` | `InferenceContext`, `InferenceService` trait, `ModelInfo` |
| `settings` | `HkaskSettings` persistence (`load_settings`, `save_settings`) |
| `self_heal` | Self-healing patterns |

## Key Re-exports

- `ServiceConfig` — system-wide configuration
- `ServiceError` — canonical error type for service layer
- `Goal` / `GoalState` — goal tracking types
- `InferenceContext` — context bundle for inference calls
- `InferenceService` — port trait for inference dispatch

## Dependencies

- `hkask-types` — Regulation spans, WebID, nu-event
- `hkask-ports` — hexagonal port traits
- `hkask-keystore` — credential management
