# Swarm MCP Server Architecture

The swarm server (`hkask-mcp-swarm`) exposes 50 tools (27 ABW + 23 local) across two substrates selected by `kask.swarm.mode`. It is launched by two independent paths — `McpRuntime` (app-global, governed dispatch for the skill cascade + kask panel) and `ContextServerStore` (per-project, for the agent tool picker) — both correct by design. The `swarm-intelligence` skill composes/steers swarms via a 10-step PDCA cascade; the `swarm-steering` skill codifies the local execute-and-feed-back loop for the Kask Curator. See the [Swarm MCP Server Reference](../reference/mcp-servers/swarm.md), the [Swarm Cybernetics/Semantics Audit](../audits/swarm-cybernetics-semantics-audit.md), and the [Cybernetic Swarm Plan](../plans/cybernetic-swarm-plan.md).

```mermaid
flowchart TD
    subgraph launch[Launch Paths]
        MR[McpRuntime<br/>app-global, governed]
        CS[ContextServerStore<br/>per-project]
    end
    SWARM[hkask-mcp-swarm<br/>50 tools: 27 ABW + 23 local]
    MR --> SWARM
    CS --> SWARM

    subgraph abw[ABW Backend cloud]
        ABW_API[ABW REST API<br/>agent-bestiary.world]
        XAMAN[Xaman Ek<br/>curator, steering built-in]
    end

    subgraph local[Local Substrate v2 S15]
        INF[hkask-inference<br/>Ollama / cloud via IPC]
        LEDGER[hkask-ledger<br/>operator-funded SQLite]
        GUARD[hkask-guard<br/>I/O scanning]
    end

    SWARM -->|abw mode| ABW_API
    SWARM -->|abw mode| XAMAN
    SWARM -->|local mode| INF
    SWARM -->|local mode| LEDGER
    SWARM -->|local mode| GUARD

    subgraph ui[UI + Skills]
        PANEL[Swarm Panel<br/>crates/swarm_panel]
        SI[swarm-intelligence skill<br/>10-step PDCA cascade]
        SS[swarm-steering skill<br/>execute-and-feed-back]
    end
    PANEL -->|Steer mode| CURATOR[Kask Curator<br/>Agent::Curator]
    CURATOR -->|runs| SI
    CURATOR -->|runs| SS
    SI -->|emitted_calls plan| SS
    SS -->|steering directive| CURATOR
    CURATOR -->|swarm_delegate_local| SWARM
    SWARM -->|LocalDelegateResult| CURATOR
    CURATOR -->|delegate_results feedback| SI
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-DIA-SWARM-001
verified_date: 2026-08-03
verified_against: kask/mcp-servers/hkask-mcp-swarm/src/hkask_mcp_swarm.rs:3355; crates/swarm_panel/src/swarm_panel.rs:1870; .agents/skills/swarm-intelligence/SKILL.md:62; .agents/skills/swarm-steering/SKILL.md:59
status: VERIFIED
-->