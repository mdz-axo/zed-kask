# hkask-services-runtime — Runtime Orchestration

Runtime orchestration service: daemon lifecycle, event loop, process management, and cross-subsystem coordination.

**Version:** v0.31.0 | **Crate:** `hkask-services-runtime`

## Modules

| Module | Purpose |
|--------|---------|
| Daemon handler | Background daemon lifecycle (start, stop, health) |
| Event loop | System event dispatch and Regulation feed |
| Lifecycle | Agent lifecycle states and transitions |

## Key Types

- `ServiceDaemonHandler` — daemon process management
- `NuEventSink` — trait for Regulation event emission
- `LoopSystem` — four-loop authority model orchestration

## Dependencies

- `hkask-services-core` — `ServiceConfig`, `ServiceError`
- `hkask-regulation` — Regulation span emission and variety tracking
- `hkask-types` — nu-event, Regulation spans
