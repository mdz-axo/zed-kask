# hkask-services-core — Core Service Traits and Types

Foundation crate for the hKask service layer. Defines shared types, configuration, error taxonomy, and port traits used across all service crates.

**Version:** v0.39.0 | **Crate:** `hkask-services-core`

## Modules

| Module          | Purpose                                         |
| --------------- | ----------------------------------------------- |
| `config`        | `ServiceConfig`                                 |
| `data_category` | Content classification (`DataCategory` parsing) |
| `error`         | Canonical `ServiceError` enum                   |

| `identity` | WebID, `UserRole`, identity management |
| `inference_svc` | `InferenceContext`, `InferenceService` trait, `ModelInfo` |
| `standalone_settings` | `HkaskSettings` reads shared Zed JSONC settings; no settings writer |
| `self_heal` | Self-healing patterns |

## Shared model settings

The standalone reader accepts comments and trailing commas in
`~/.config/zed-kask/settings.json`, using the host's `serde_json_lenient` parser.
It overlays `kask.models` on code defaults; nonempty model environment overrides
still take precedence. Malformed JSONC or ill-typed model fields log a warning
before falling back to defaults. Reading never rewrites the settings file.

## Key Re-exports

- `ServiceConfig` — system-wide configuration
- `ServiceError` — canonical error type for service layer

- `InferenceContext` — context bundle for inference calls
- `InferenceService` — port trait for inference dispatch

## Dependencies

- `hkask-types` — Regulation spans, WebID, nu-event
- `hkask-ports` — hexagonal port traits
- `hkask-keystore` — credential management
