# hkask-services-chat — Chat Session Management

Chat orchestration service: session management, memory recall, turn handling, and compose dispatch.

**Version:** v0.31.0 | **Crate:** `hkask-services-chat`

## Modules

| Module | Purpose |
|--------|---------|
| `chat` | Chat session orchestration, turn management, compose dispatch |
| `memory` | Memory recall integration for conversation context |

## Dependencies

- `hkask-types` — WebID, Regulation spans, nu-event
- `hkask-services-core` — `ServiceConfig`, `ServiceError`, `Goal`
- `hkask-services-context` — `AgentService` context
- `hkask-pods` — Pod and agent types
- `hkask-condenser` — Context condensation
- `hkask-templates` — Template resolution and rendering
- `chat/improv.rs` (in this crate) - Constructive interaction protocol (Plussing, Yes And, Yes But, Freestyling, Riffing); the standalone `hkask-improv` crate was folded in during v0.31.0
- `hkask-capability` — OCAP delegation tokens
- `hkask-ports` — Hexagonal port traits
- `hkask-regulation` — Regulation span emission
