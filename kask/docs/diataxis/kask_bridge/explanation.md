---
title: "kask_bridge — Explanation"
audience: [developers, architects, agents]
last_updated: 2026-08-04
version: "0.3.0"
status: "Active"
domain: "Integration"
mds_categories: [trust, curation]
---

# kask_bridge — Explanation

The composition root is the single place where zed and hKask are wired
together. It runs in two phases inside `crates/zed/src/main.rs`: an early
block that wires the regulation system and metacognition provider before
any thread can complete a turn, and a deferred task that runs after the zed
user resolves and a default language model becomes available. The
model-dependent hooks (`set_manifest_executor`, `set_thread_condenser`,
`set_tool_invoker`, `set_memory_port`) are wired in the deferred task.
At startup, the `set_memory_port` hook is `None` — turn ingest no-ops until
the deferred task wires `BridgeMemoryPort(RealMemoryPort)` after the zed user
resolves. The design centralizes the integration so that the seams are
visible in one file rather than scattered across the codebase.

## Source citations

| Symbol | Location |
|--------|----------|
| Regulation + metacognition wiring (early) | `crates/zed/src/main.rs:674,749` |
| Deferred-task memory upgrade | `crates/zed/src/main.rs:1153` |
| Deferred-task metacognition re-set | `crates/zed/src/main.rs:1173` |
| Deferred-task context injector | `crates/zed/src/main.rs:1230` |
| Deferred-task tool router | `crates/zed/src/main.rs:1280` |
| Deferred-task panel tool invoker | `crates/zed/src/main.rs:1621` |
| Deferred-task thread condenser | `crates/zed/src/main.rs:1635` |
| Deferred-task manifest executor | `crates/zed/src/main.rs:1778` |
| `set_tool_invoker` (panel) | `crates/swarm_panel/src/tool_invoker.rs:33` |
| `BridgeManifestExecutor` | `kask/crates/kask_bridge/src/skill_executor.rs:30` |
| `BridgeMemoryPort` | `kask/crates/kask_bridge/src/memory.rs:1615` |
| `RealMemoryPort` | `kask/crates/kask_bridge/src/memory.rs:42` |

## Composition root sequence

The sequence below shows the two-phase wiring. The early block runs at
startup (before user auth) and wires the regulation system + metacognition
provider; the deferred task runs after the zed user resolves and wires the
model-dependent hooks (memory, manifest executor, panel). `set_memory_port`
and `set_metacognition_provider` use `Mutex` (re-settable); the manifest
executor and context injectors use `OnceLock` (set once).

```mermaid
sequenceDiagram
    participant Main as main.rs
    participant Bridge as kask_bridge
    participant Agent as agent.rs
    participant Panel as swarm_panel

    Note over Main: Early block (startup)
    Main->>Bridge: construct McpRuntime + CyberneticsLoop
    Main->>Bridge: BridgeMetacognitionProvider
    Main->>Agent: set_metacognition_provider
    Main->>Main: spawn CyberneticsLoop tick + MetacognitionLoop

    Note over Main: Deferred task (post-login)
    Main->>Bridge: provision_agent + RealMemoryPort
    Main->>Bridge: wrap in BridgeMemoryPort
    Main->>Agent: set_memory_port(real)
    Main->>Bridge: re-set metacognition provider with memory probe
    Main->>Bridge: BridgeContextInjector + BridgeCuratorContextInjector
    Main->>Agent: set_context_injector + set_curator_context_injector
    Main->>Agent: set_tool_router (LazyToolRouter)
    Main->>Bridge: construct PanelToolInvoker
    Main->>Panel: set_tool_invoker(invoker)
    Main->>Bridge: BridgeThreadCondenser
    Main->>Agent: set_thread_condenser(condenser)
    Main->>Bridge: LanguageModelInferencePort + GuardedInferencePort
    Main->>Bridge: BridgeManifestExecutor
    Main->>Agent: set_manifest_executor(executor)
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-DIA-BRIDGE-002
verified_date: 2026-08-01
verified_against: crates/zed/src/main.rs:672 (McpRuntime::with_governance), 749 (set_metacognition_provider), 1153 (set_memory_port), 1230 (set_context_injector), 1280 (set_tool_router), 1621 (set_tool_invoker), 1635 (set_thread_condenser), 1778 (set_manifest_executor); crates/swarm_panel/src/tool_invoker.rs:33
status: VERIFIED
-->

## Why deferred wiring

The model-dependent hooks depend on
`LanguageModelRegistry::default_model()` being populated. At startup,
before user authentication, `default_model()` returns `None`. Wiring the
hooks synchronously at startup would leave them unwired for the entire
session when no model is configured at startup. The deferred task runs
after the zed user resolves, ensuring the model is available.

If the deferred task fails to find a model, the hooks remain `None` and the
`skill` tool returns a no-op envelope. This fail-closed behavior is
intentional. A missing model should not silently produce broken skill
output.

## Why OnceLock hooks (and one Mutex)

The `set_*` functions use `static ONCE_LOCK: OnceLock<Option<Arc<dyn Trait>>>`.
The `OnceLock` ensures the hook is set exactly once per process. The
`Option` allows the hook to be absent (fail-closed). The `Arc` allows the
hook to be shared across threads.

`set_memory_port` (`agent.rs:2908`) is the exception: it uses a `Mutex`
rather than a `OnceLock`, because the hook is `None` at startup (the
`LoggingMemoryPort` that previously occupied this slot was deleted in the
2026-07-31 simplification pass) and is upgraded to
`BridgeMemoryPort(RealMemoryPort)` once the zed user resolves. The `Mutex`
allows the `set_memory_port` call in the deferred task to replace the `None`
value in place.

This pattern has a trap: if the condition for wiring fails silently, the
hook is left `None` with no signal. The `.rules` file documents this:
every `set_*` hook that is wired conditionally must `log::warn!` when the
condition fails, so operators can distinguish "not configured" from
"configured but broken." When a deferred task wires multiple `set_*` hooks
inside a single `if` block, the `else` branch warn must name ALL hooks
left unwired, not just one.

## The GPUI/tokio cybernetic boundary

The bridge is where two feedback loops cross. GPUI runs on a single
foreground thread (not `Send`); hKask's `ManifestExecutor` and `ToolPort`
run on tokio (`Send + Sync`). The `LanguageModelInferencePort`
(`inference.rs:46`) solves this with a `tokio::sync::mpsc` channel: the
adapter holds only an `UnboundedSender` (which is `Send + Sync`), and a
GPUI-side spawned task owns the `AsyncApp` and the receiver. The two
halves never cross threads. This is the `.rules` "Cross-thread GPUI
communication uses channels, not `AsyncApp` handles" trap — `AsyncApp` is
not `Send`, so any `Send + Sync` trait implemented over GPUI state must
use a channel, not capture `AsyncApp`.

The `BridgeManifestExecutor` (`skill_executor.rs:30`) holds a
`tokio::runtime::Handle` that is entered around manifest execution so that
`tokio::time::timeout` and other tokio APIs inside `ManifestExecutor` have
a reactor. The `SkillTool` runs on GPUI's foreground executor (not tokio),
so without this guard, any skill with a manifest would panic with "there is
no reactor running."

## See also

- [kask_bridge Reference](./reference.md): class diagram of settings and
  bridge adapters.
- [kask_bridge How-to](./how-to.md): wiring a new kask hook.
- [hkask-types Explanation](../hkask-types/explanation.md): the port trait
  mediation that this crate implements.
- [`kask/docs/architecture/zed-host-architecture-plan.md`](../../architecture/zed-host-architecture-plan.md):
  the D1–D23 integration seams.

---

[^cockburn]: Cockburn, A. (2005). *Hexagonal Architecture.* <https://alistair.cockburn.us/hexagonal-architecture/>. The composition root pattern: a single place where ports and adapters are wired.

[^once-lock]: Rust Community. (2024). *std::sync::OnceLock.* <https://doc.rust-lang.org/std/sync/struct.OnceLock.html>. The synchronization primitive used for process-global hooks.
