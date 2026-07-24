# hkask-services-context — AgentService Context

Central `AgentService` struct — the canonical composition root for hKask.
Assembles all shared infrastructure into four sub-contexts.

**Version:** v0.31.0 | **Crate:** `hkask-services-context`

## Exports

| Type | Purpose |
|------|---------|
| `AgentService` | Central runtime context — 4 sub-contexts + 3 properties |
| `PerAgentMemory` | Episodic + semantic memory handle per agent |
| `GovernanceContext` | OCAP, consent, dispatch, A2A, escalations, curation |
| `RegulationContext` | Variety sensing, cybernetics, loops, events, energy |
| `StorageContext` | Registry, goals, specs, agents, users, sovereignty, wallet |
| `InfraContext` | Inference, memory, MCP, pods, wallet, daemon, matrix, seams, gas |

## AgentService Composition

```
AgentService
├── infra: InfraContext          (11 fields — inference, memory, MCP, pods, wallet, daemon, matrix, seams, gas)
├── governance: GovernanceContext (6 fields — checker, consent, dispatcher, a2a, escalations, curation_tx)
├── ledger: RegulationContext    (5 fields — runtime, cybernetics, loops, events, energy)
├── storage: StorageContext      (7 fields — registry, goals, specs, agents, users, sovereignty, wallet)
├── system_webid: WebID
├── curator_ready: Option<oneshot::Receiver<()>>
└── config: ServiceConfig
```

## Accessors

| Method | Returns |
|--------|---------|
| `config()` | `&ServiceConfig` |
| `ledger()` | `&RegulationContext` |
| `storage()` | `&StorageContext` |
| `governance()` | `&GovernanceContext` |
| `infra()` | `&InfraContext` |
| `identity()` | `(&WebID, &Arc<A2ARuntime>)` |
| `memory()` | `(&EpisodicStoragePort, &SemanticStoragePort)` |
| `seam_summary()` | `Option<SeamSummary>` |
| `curator_ready()` | `Result<(), String>` |

## Dependencies

Directly depends on: `hkask-pods`, `hkask-regulation`, `hkask-mcp`, `hkask-memory`,
`hkask-storage`, `hkask-templates`, `hkask-types`, `hkask-wallet`, `hkask-wallet-types`,
`hkask-services-core`, `hkask-services-runtime`, `hkask-services-wallet`,
`hkask-capability`, `hkask-ports`, `hkask-`
