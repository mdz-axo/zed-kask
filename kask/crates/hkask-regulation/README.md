# hkask-regulation — Regulation System

Homeostatic self-regulation engine for hKask. Regulation enforces Ashby's Law of Requisite Variety through variety sensing, algedonic alerts, per-agent tool-call caps, OCAP governance, and sovereignty enforcement (Loop 6).

## Public Modules

| Module | Purpose |
|--------|---------|
| `runtime` | `RegulationLedger` — central Regulation state machine |
| `cybernetics_loop` | Loop 6 main sense→compute→act cycle |
| `energy` | Per-agent tool-call caps (`CallCap`, `CallCapManager`) |
| `metacognition` | Curator's sense→compare→compute→act governance loop |
| `set_points` | Loop 6 set-points config & loaders |
| `sensor_provider` | Pluggable metric sensors (Fermi Extractor pattern) — public for cross-loop registration |
| `types::loops` | `CurationInput`, `ExperienceClassification`, `RegulatoryAction` |

## Key Types

| Type | Description |
|------|-------------|
| `RegulationLedger` | Central Regulation state machine with health, variety, alerts |
| `CyberneticsLoop` | Loop 6 regulation cycle |
| `CallCap` | Per-agent tool-call cap (ceiling + remaining + reset cycle) |
| `CallCapManager` | Registry of per-agent `CallCap`s with curation overrides |
| `SetPoints` | Configurable regulatory thresholds |

The `runtime_policy` module (`DefaultPolicy`, `PolicyVerdict`) was removed
2026-08-12. Its FIDES `Source`→`Sink` block read two constants — every tool was
labelled `Pure`, and the untrusted-input flag was always false — so the only
pre-execution check it could apply was an unconfigured rate limit. The live tool
gates are `McpRuntime::invoke` (call metering) and the per-agent `mcp_tools`
allowlist.

The OCAP-gated tool invocation membrane (`McpRuntime::invoke`) lives in `hkask-mcp`; it consumes this crate's `CyberneticsLoop`, `CallCapManager`, and `ToolStats` primitives via the call-charge pattern (`CyberneticsLoop::charge_call`).

## Dependencies

- `hkask-types` — foundation types (WebID, NuEvent, InfrastructureError, `InferencePort`)
- `hkask-tool-port` — OCAP delegation tokens
- `tokio`, `tracing`, `serde`, `chrono`