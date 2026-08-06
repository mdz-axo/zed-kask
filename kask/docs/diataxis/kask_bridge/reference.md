---
title: "kask_bridge — Reference"
audience: [developers, architects, agents]
last_updated: 2026-08-04
version: "0.3.0"
status: "Active"
domain: "Integration"
mds_categories: [domain, composition, trust]
---

# kask_bridge — Reference

`kask_bridge` is the D8 composition root adapter — the sole bidirectional seam
between zed-kask and hKask. It connects zed's internal types to hKask's port
traits. Every integration seam (D1 through D20) passes through this crate. It
defines `KaskSettings`, `BridgeManifestExecutor`, `BridgeMemoryPort`,
`LanguageModelInferencePort`, and the settings structs that configure the kask
subsystem. `McpRuntime` is passed directly as the
`ToolPort` (the former `BridgeToolPort` adapter was collapsed in the
2026-07-31 simplification pass).

## Source citations

| Symbol | Location |
|--------|----------|
| `KaskSettings` | `kask/crates/kask_bridge/src/settings.rs:35` |
| `KaskMcpSettings` | `kask/crates/kask_bridge/src/settings.rs:89` |
| `KaskDataServiceSettings` | `kask/crates/kask_bridge/src/settings.rs:109` |
| `KaskInferenceProvidersSettings` | `kask/crates/kask_bridge/src/settings.rs:143` |
| `KaskCuratorSettings` | `kask/crates/kask_bridge/src/settings.rs:189` |
| `KaskMemorySettings` | `kask/crates/kask_bridge/src/settings.rs:247` |
| `KaskCondenserSettings` | `kask/crates/kask_bridge/src/settings.rs:281` |
| `BridgeManifestExecutor` | `kask/crates/kask_bridge/src/skill_executor.rs:30` |
| `BridgeMemoryPort` | `kask/crates/kask_bridge/src/memory.rs:1615` |
| `LanguageModelInferencePort` | `kask/crates/kask_bridge/src/inference.rs:52` |
| `InferencePort` impl | `kask/crates/kask_bridge/src/inference.rs:281` |
| `resolve_model_names` | `kask/crates/kask_bridge/src/model_resolution.rs` |
| `BridgeThreadCondenser` | `kask/crates/kask_bridge/src/condenser_bridge.rs:22` |
| `provision_agent` | `kask/crates/kask_bridge/src/identity.rs:212` |

## Settings model

The `KaskSettings` struct (`settings.rs:35`) is the root of the kask settings
hierarchy. It is registered with zed's settings system and appears in
`settings.json` under the `"kask"` key. Each sub-struct configures one
subsystem. Per the `.rules` "Kask settings defaults" trap, `Default` impls are
the single source of truth — `From<Content>` and `mcp_env()` read from them,
and `#[serde(default = "...)]` attributes are dead code because the settings
system deserializes `SettingsContent`, not `KaskSettings`.

```mermaid
classDiagram
    class KaskSettings {
        +mcp: KaskMcpSettings
        +data_services: KaskDataServiceSettings
        +curator: KaskCuratorSettings
        +memory: KaskMemorySettings
        +condenser: KaskCondenserSettings
        +models: KaskModelsSettings
        +inference_providers: KaskInferenceProvidersSettings
    }
    class KaskMcpSettings {
        +load_default: bool
        +overrides: HashMap~String,bool~
    }
    class KaskInferenceProvidersSettings {
        +deepinfra_enabled: bool
        +fal_enabled: bool
        +openrouter_enabled: bool
    }
    class KaskMemorySettings {
        +consolidation_cadence_secs: u64
        +confidence_floor: f64
        +recall_limit: u32
        +recall_min_confidence: f64
        +auto_inject: bool
    }
    class KaskCondenserSettings {
        +profile: String
        +auto_compress_tool_results: bool
        +persona_keywords: Vec~String~
        +saliency_window: u32
    }
    class BridgeManifestExecutor {
        +inference: Arc~InferencePort~
        +tools: Arc~ToolPort~
        +registry_manifests_dir: PathBuf
        +registry_templates_dir: PathBuf
        +tokio_handle: Handle
    }
    class BridgeMemoryPort {
        +inner: Arc~MemoryPort~
    }

    KaskSettings --> KaskMcpSettings
    KaskSettings --> KaskInferenceProvidersSettings
    KaskSettings --> KaskMemorySettings
    KaskSettings --> KaskCondenserSettings
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-DIA-BRIDGE-001
verified_date: 2026-08-01
verified_against: kask/crates/kask_bridge/src/settings.rs:35,89,143,247,281; kask/crates/kask_bridge/src/skill_executor.rs:30; kask/crates/kask_bridge/src/memory.rs:42,1615
status: VERIFIED
-->

## Bridge adapters

Two bridge adapters implement hKask port traits against zed types:

- `BridgeManifestExecutor` (`skill_executor.rs:30`) implements zed's
  `SkillManifestExecutor` by delegating to hKask's `ManifestExecutor`. It
  holds an `Arc<dyn InferencePort>` (the `GuardedInferencePort`), an
  `Arc<dyn ToolPort>` (the `McpRuntime` itself — it implements `ToolPort`
  directly, with capability-match gating, gas/rjoule budgeting, and
  `reg.tool.*` span emission), and a `tokio::runtime::Handle` that is
  entered around manifest execution so `tokio::time::timeout` has a
  reactor. (The former `BridgeToolPort` adapter was collapsed in the
  2026-07-31 simplification pass — `McpRuntime` is passed directly as the
  `ToolPort`. The `a2a_secret` field was deleted
  with the OCAP/a2a secret threading.)
- `BridgeMemoryPort` (`memory.rs:1615`) implements zed's `ThreadMemoryPort`
  by delegating to hKask's `MemoryPort`. It wraps an `Arc<dyn MemoryPort>`
  — a `RealMemoryPort` (`memory.rs:42`), wired once the zed user resolves
  (before that the hook is `None` and turn ingest no-ops — the former
  `LoggingMemoryPort` no-op placeholder was deleted in the 2026-07-31
  simplification pass).

The `LanguageModelInferencePort` (`inference.rs:52`, trait impl at `:281`)
implements hKask's `InferencePort` by wrapping zed's `LanguageModel`. It
holds only a `tokio::sync::mpsc::UnboundedSender` — the actual inference
call happens on the GPUI foreground executor via a spawned task that owns
the `AsyncApp`. This channel pattern solves the GPUI/tokio `Send`+`Sync`
boundary: the sender is `Send + Sync`, the receiver task is not, and the
two never cross threads.

## See also

- [kask_bridge Explanation](./explanation.md): sequence diagram of the
  composition root.
- [kask_bridge How-to](./how-to.md): wiring a new kask hook.
- [kask_bridge Tutorial](./tutorial.md): your first kask hook.
- [hkask-types Reference](../hkask-types/reference.md): the port traits these
  adapters implement.

---

[^cockburn]: Cockburn, A. (2005). *Hexagonal Architecture.* <https://alistair.cockburn.us/hexagonal-architecture/>. The adapter pattern that this crate embodies: every port trait gets a bridge adapter.
