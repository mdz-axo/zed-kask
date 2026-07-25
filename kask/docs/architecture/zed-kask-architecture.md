---
title: "zed-kask Integrated Architecture — Composition Root"
audience: [architects, developers, contributors]
last_updated: 2026-07-24
version: "0.31.0"
status: "Active"
domain: "architecture"
mds_categories: [composition, trust, lifecycle]
---

# zed-kask Integrated Architecture

The zed-kask fork integrates hKask into the Zed editor as a native in-process agent platform. This diagram shows the composition root — the dependency injection wiring at startup (`crates/zed/src/main.rs`) that connects zed's editor surfaces to hKask's agent runtime via the `kask_bridge` seam (D8).

The architecture follows a strict dependency invariant: **no hKask crate depends on a zed crate**. The `kask_bridge` crate is the sole bidirectional seam, adapting zed's `LanguageModel`, `CredentialsProvider`, and `ThreadMemoryPort` traits into hKask's `InferencePort`, `SecretsPort`, and `MemoryPort` interfaces. All ten integration seams (D1–D10) are wired at the composition root before the editor event loop begins.

```mermaid
flowchart TD
    subgraph Zed["Zed Editor (upstream — forked, minimal divergence)"]
        Editor["crates/workspace<br/>Editor, Git, Collab"]
        AgentPanel["crates/agent + agent_ui<br/>Agent Panel, Thread Store"]
        LM["crates/language_model*<br/>Inference Routing"]
        CredProv["crates/credentials_provider<br/>Provider Keystore"]
        ContextServer["crates/context_server<br/>MCP stdio Transport"]
        SettingsUI["crates/settings_ui<br/>Kask Settings Page (D9c)"]
        KaskPanel["crates/kask_panel<br/>Dockable Panel (D10)"]
    end

    subgraph Bridge["kask_bridge (D8 — sole bidirectional seam)"]
        BridgeInf["LanguageModelInferencePort<br/>InferencePort over LanguageModel"]
        BridgeSec["CredentialsSecretsPort<br/>SecretsPort over CredentialsProvider"]
        BridgeMem["BridgeMemoryPort<br/>MemoryPort over ThreadMemoryPort"]
        BridgeExec["BridgeManifestExecutor<br/>Skill execution adapter"]
        BridgeTool["BridgeToolPort<br/>ToolPort over McpRuntime"]
    end

    subgraph Guard["Guard Layer (D4)"]
        GuardedInf["GuardedInferencePort<br/>ContentGuard::mandatory()<br/>Injection + secret scanning"]
    end

    subgraph HKask["hKask Crates (29 — compiled in-process)"]
        Templates["hkask-templates<br/>ManifestExecutor + Registry<br/>Skill execution (D1)"]
        Guard2["hkask-guard<br/>Content guard rules"]
        Capability["hkask-capability<br/>OCAP enforcement"]
        Keystore["hkask-keystore<br/>Sovereignty crypto (D5)"]
        Regulation["hkask-regulation<br/>Cybernetic nervous system"]
        Memory["hkask-memory<br/>Semantic + episodic memory"]
        Wallet["hkask-wallet + hkask-ledger<br/>rJoule energy budget"]
        Pods["hkask-pods<br/>Curator + UserPod"]
        Storage["hkask-storage<br/>SQLCipher private sphere"]
    end

    subgraph MCP["MCP Servers (11 — in-process, stdio)"]
        Codegraph["codegraph"]
        Companies["companies"]
        Condenser["condenser"]
        Curator["curator"]
        Docproc["docproc"]
        KataKanban["kata-kanban"]
        Media["media"]
        Replica["replica"]
        Research["research"]
        Scenarios["scenarios"]
        Training["training"]
    end

    subgraph Skills["Skills (51 + 3 templates + 1 bundle)"]
        SkillReg["registry/manifests/<br/>FlowDef .yaml files"]
        SkillTpl["registry/templates/<br/>Jinja2 .j2 template crates"]
    end

    %% Composition root wiring (numbered = startup order)
    CredProv -->|"1. set_secrets_port()"| BridgeSec
    BridgeSec --> Keystore
    Keystore -->|"2. resolve_a2a_secret()"| BridgeSec

    ContextServer -->|"3. McpRuntime"| BridgeTool
    BridgeTool --> MCP

    LM -->|"4-5. LanguageModelRegistry"| BridgeInf
    BridgeInf -->|"6. wrap"| GuardedInf
    Guard2 --> GuardedInf

    GuardedInf -->|"7. BridgeManifestExecutor"| BridgeExec
    BridgeTool --> BridgeExec
    BridgeExec -->|"8. set_manifest_executor()"| AgentPanel

    AgentPanel -->|"9. Thread turn completion"| BridgeMem
    BridgeMem --> Memory

    KaskPanel -->|"10. set_tool_invoker()"| BridgeTool
    KaskPanel -->|"10. set_scoped_inference()"| GuardedInf

    BridgeExec --> Templates
    Templates --> SkillReg
    Templates --> SkillTpl

    GuardedInf --> Capability
    GuardedInf --> Regulation
    BridgeExec --> Wallet
    BridgeExec --> Pods
    BridgeExec --> Storage

    SettingsUI -->|"KaskSettings"| BridgeSec
    SettingsUI -->|"per-server toggles"| ContextServer
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-ARCH-001
verified_date: 2026-07-24
verified_against: kask/docs/architecture/zed-host-architecture-plan.md (§6 composition root, D1-D10 status table); kask/crates/kask_bridge/src/lib.rs; kask/crates/zed/src/main.rs
status: VERIFIED
-->

## Dependency Invariant

The governing architectural constraint: **hKask crates never depend on zed crates**. The dependency direction is one-way — zed depends on hKask via `kask_bridge`, never the reverse. This keeps hKask portable (could run standalone again if needed) and keeps the fork's divergence surface minimal.

```mermaid
flowchart LR
    Zed["zed crates<br/>(editor, agent, language_model)"]
    Bridge["kask_bridge<br/>(D8 — adapters)"]
    HKask["hKask crates<br/>(29 crates)"]

    Zed -->|"depends on"| Bridge
    Bridge -->|"depends on"| HKask
    HKask -.->|"NEVER depends on"| Zed
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-ARCH-002
verified_date: 2026-07-24
verified_against: kask/docs/architecture/zed-host-architecture-plan.md §13.1 (governing invariant); kask/Cargo.toml (workspace deps)
status: VERIFIED
-->

## Integration Seams (D1–D10)

All ten integration seams are wired at the composition root in `crates/zed/src/main.rs` before the editor event loop begins. Each seam is an additive divergence point — zed's code is modified only at the seam, everything else stays byte-identical to upstream.

| Seam | Surface | What It Does |
|------|---------|-------------|
| D1 | Skill execution | `SkillTool` has optional `SkillManifestExecutor`; `BridgeManifestExecutor` connects zed's agent panel to hKask's `ManifestExecutor` + skill registry (51 skills). |
| D2 | Curator agent | `Curator` variant in `agent_ui::Agent` enum; native agent selectable in Agent Panel. |
| D3 | Tools in-process | `BridgeToolPort` wraps `McpRuntime`; MCP servers run as child processes (stdio). OCAP/gas/spans enforced. |
| D4 | Guard layer | `GuardedInferencePort` wraps `InferencePort`; `hkask-guard` scans inputs (injection, role override) and outputs (secret redaction). |
| D5 | Sovereignty keys | `hkask-keystore` crypto-derivation over `SecretsPort`/`CredentialsProvider`; kask namespace (`kask://credentials/<key>`). |
| D6 | Thread → memory | `BridgeMemoryPort` adapts `ThreadMemoryPort` → `MemoryPort`; thread turn completion ingests into hKask memory. |
| D7 | App-identity | `APP_NAME`→`Zed-Kask`, port offset +500, binary `zed-kask`, bundle IDs `dev.zed-kask.*`. |
| D8 | Bridge + adapters | `kask_bridge` crate: `InferencePort`, `SecretsPort`, `BridgeManifestExecutor`, `BridgeToolPort`, `KaskSettings`. |
| D9a/b/c | Settings + credentials + UI | `KaskSettings` in settings.json; `SecretsPort` over `CredentialsProvider`; settings UI page with 5 sub-pages. |
| D10 | Kask panel | Native GPUI dockable panel; `/tool args` direct invocation (OCAP-gated); scoped inference with selected server's tools. |

## Cross-References

- [zed-host-architecture-plan.md](zed-host-architecture-plan.md) — canonical architecture document with full D1–D10 status, essentialist split, and composition root details.
- [PRINCIPLES.md](core/PRINCIPLES.md) — architecture principles P1–P12 governing the design.
- [magna-carta.md](core/magna-carta.md) — the 4 sovereignty principles enforced by the guard layer (D4) and OCAP (D3).
- [skills/README.md](../reference/skills/README.md) — skill registry (51 skills + 3 templates + 1 bundle).
- [mcp-servers/README.md](../reference/mcp-servers/README.md) — 11 in-process MCP servers.
