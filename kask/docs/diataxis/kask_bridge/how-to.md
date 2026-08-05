---
title: "kask_bridge — How-to: Wire a New Kask Hook"
audience: [developers]
last_updated: 2026-08-04
version: "0.3.0"
status: "Active"
domain: "Integration"
mds_categories: [composition, lifecycle]
---

# kask_bridge — How-to: Wire a New Kask Hook

This guide shows how to add a new process-global hook using the `set_*`
OnceLock pattern. The pattern is used by `set_manifest_executor`,
`set_memory_port`, `set_thread_condenser`, and the swarm_panel
`set_tool_invoker` hook. A new hook follows the same structure: define the
trait, add the `OnceLock`, add the `set_*` and getter functions, and wire in
the deferred task.

## Source citations

| Symbol | Location |
|--------|----------|
| `set_manifest_executor` | `crates/agent/src/agent.rs:2829` |
| `set_memory_port` | `crates/agent/src/agent.rs:2908` |
| `set_thread_condenser` | `crates/agent/src/agent.rs:3070` |
| `set_tool_invoker` (panel) | `crates/swarm_panel/src/tool_invoker.rs:33` |
| Deferred-task wiring | `crates/zed/src/main.rs:1778` |
| `.rules` hook trap | `zed-kask/.rules` (zed-kask integration traps) |

## Procedure

```mermaid
flowchart TD
    A[Define trait in hkask-types] --> B[Add OnceLock + set/get in agent.rs]
    B --> C[Add log::warn in failure branch]
    C --> D[Construct adapter in kask_bridge]
    D --> E[Wire in deferred task in main.rs]
    E --> F[Add settings key to KaskSettings]
    F --> G[Add tests]
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-DIA-BRIDGE-003
verified_date: 2026-07-29
verified_against: crates/agent/src/agent.rs:2829,2908,3070; crates/swarm_panel/src/tool_invoker.rs:33; crates/zed/src/main.rs:1778
status: VERIFIED
-->

### Step 1: Define the trait

Define the port trait in `hkask-types/src/ports/`. The trait must be
`Send + Sync` and return pinned boxed futures for async methods.

### Step 2: Add the OnceLock and set/get functions

In `crates/agent/src/agent.rs`, add a `static ONCE_LOCK: OnceLock<Option<Arc<dyn NewTrait>>>`
and two functions: `set_new_hook(value: Option<Arc<dyn NewTrait>>)` and
`new_hook() -> Option<Arc<dyn NewTrait>>`. Follow the pattern of
`set_manifest_executor` at `agent.rs:2829`.

Note that `set_memory_port` (`agent.rs:2908`) uses a `Mutex` rather than a
`OnceLock` — it is the one hook that is intentionally re-settable, because
the composition root leaves the hook `None` at startup (no
`LoggingMemoryPort` — deleted in the 2026-07-31 simplification pass) and
upgrades it to a `BridgeMemoryPort` wrapping `RealMemoryPort` once the zed
user resolves (see `main.rs:1153`). Choose `Mutex` only when you need this
upgrade-in-place behavior; otherwise prefer `OnceLock`.

### Step 3: Add log::warn in the failure branch

When the hook is wired conditionally, add a `log::warn!` in the `else`
branch. Name the hook, the failure reason, and the remediation. This is
the `.rules` trap: operators reading logs cannot distinguish "not
configured" from "configured but broken" without the warning. When a
deferred task wires multiple `set_*` hooks inside a single `if` block, the
`else` branch warn must name ALL hooks left unwired, not just one.

### Step 4: Construct the adapter in kask_bridge

Create a bridge adapter struct in `kask/crates/kask_bridge/src/` that
implements the new trait against zed types. Follow the pattern of
`BridgeMemoryPort` at `memory.rs:1615`.

### Step 5: Wire in the deferred task

In `crates/zed/src/main.rs`, inside the deferred task (around line 1778,
where `set_manifest_executor` is called), construct the adapter and call
`agent::set_new_hook(Some(adapter))`. The wiring must happen inside the
deferred task because it depends on
`LanguageModelRegistry::default_model()` being populated. At startup,
before user authentication, `default_model()` returns `None`; wiring
synchronously at startup leaves the hook unwired for the entire session
when no model is configured at startup.

### Step 6: Add settings key

Add a field to `KaskSettings` or a sub-struct in
`kask/crates/kask_bridge/src/settings.rs` so users can configure the hook
in `settings.json`. Per the `.rules` "Kask settings defaults" trap, set
the default in the `Default` impl — the single source of truth. Do not
encode defaults in `#[serde(default = "...")]` attributes (dead code — the
settings system deserializes `SettingsContent`, not `KaskSettings`), in
`From<Content>` literals, or in `mcp_env()` comparison literals.

## See also

- [kask_bridge Reference](./reference.md): class diagram of settings and
  adapters.
- [kask_bridge Explanation](./explanation.md): the composition root sequence.
- [kask_bridge Tutorial](./tutorial.md): your first kask hook.

---

[^once-lock]: Rust Community. (2024). *std::sync::OnceLock.* <https://doc.rust-lang.org/std/sync/struct.OnceLock.html>. The synchronization primitive used for process-global hooks.
