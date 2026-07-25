---
title: "MCP Server Registry — Reference"
audience: [developers, architects, agents]
last_updated: 2026-07-24
version: "0.31.0"
status: "Active"
domain: "Composition"
mds_categories: [composition, domain]
---

# MCP Server Registry

**Diataxis type:** Reference
**Status:** Current (v0.31.0)

Built-in MCP servers shipped with hKask and hosted in-process by zed-kask's `context_server`
infrastructure. Each server is a thin surface over domain crates, following the standard bootstrap
path (`hkask_mcp::bootstrap_mcp_server` → `hkask_mcp::run_server`).

> **Hosting note (v0.31.0):** hKask runs in-process inside zed-kask. The standalone `kask mcp start
> <id>` and `kask serve` CLI surfaces have been **deleted**. MCP servers are loaded by zed's
> `context_server` host; the `BUILTIN_SERVERS` constant in
> `kask/crates/hkask-mcp-server/src/lib.rs` enumerates the 11 on-disk servers. Five servers from
> the original 16 have been deleted: `communication` (Matrix/TTS → zed voip), `filesystem` (zed
> provides fs tools), `memory` (consolidated into the `hkask-memory` crate), `skill` (skill
> execution is native via D1), and `regulation` (consolidated into the `hkask-regulation` crate).
> See [`docs/architecture/zed-host-architecture-plan.md`](../../architecture/zed-host-architecture-plan.md)
> §2.4.

## Server Catalog

11 on-disk MCP servers:

| Server | Crate | Domain | Tools | Math Engine |
|--------|-------|--------|-------|-------------|
| CodeGraph | `mcp-servers/hkask-mcp-codegraph` | Code understanding (query, traverse, impact) | 10 | `hkask-mcp-codegraph` |
| [Companies](companies.md) | `mcp-servers/hkask-mcp-companies` | FIBO-anchored financial forecasting | 41 | `hkask-forecast` |
| [Condenser](condenser.md) | `mcp-servers/hkask-mcp-condenser` | Context condensation | 7 | — |
| Corpus / DocProc / Replica | `mcp-servers/hkask-mcp-corpus` | Corpus gathering, document processing, QA generation, style replicas | 24 | — |
| Curator | `mcp-servers/hkask-mcp-curator` | Curator agent metacognition | — | — |
| Kata Kanban | `mcp-servers/hkask-mcp-kata-kanban` | Toyota Kata task boards | 14 | `hkask-services-kata-kanban` |
| Media | `mcp-servers/hkask-mcp-media` | Fal.ai media generation | — | — |
| Replica | `mcp-servers/hkask-mcp-corpus` | Style replicas (subset of corpus server) | — | — |
| Research | `mcp-servers/hkask-mcp-research` | Web search, extraction, browsing, RSS feeds | 17 | `hkask-mcp-research` |
| [Scenarios](scenarios.md) | `mcp-servers/hkask-mcp-scenarios` | Event-tree forecasting (Tetlock/Schwartz/Chermack) | 18 | `hkask-forecast` |
| Training | `mcp-servers/hkask-mcp-training` | LoRA training pipeline | — | — |

> The `curator` MCP server is kept on disk but may be unloaded by default (Curator is a native
> agent, D2). All 11 build clean.

## Common Patterns

All servers follow these patterns:

1. **Bootstrap:** `hkask_mcp::bootstrap_mcp_server(name, target, host_env_var)` → returns `MCPBootstrap { userpod }` (resolves userpod identity only; the daemon was deleted in the 2026-07-25 cleanup, so `daemon_client` is no longer part of the bootstrap result)
2. **Struct:** `hkask_mcp::mcp_server!` macro generates the struct with `webid`, `userpod`, `daemon` fields plus domain fields. The `daemon` field is dead in zed-kask (always `None`; the daemon was deleted — `DaemonClient` is retained only for compile-stability, `record_via_daemon()` is a no-op). Thread-level memory via `RealMemoryPort` (D6) replaces daemon experience recording.
3. **Tool dispatch:** `execute_tool_semantic(self, tool_name, ontology, async { ... })` wraps each tool with Regulation span + daemon outcome recording
4. **Tool router:** `#[tool_handler(router = Self::...router())]` on the `ServerHandler` impl
5. **Error type:** `McpToolError` for tool-level errors, domain `Error` enums (via `thiserror`) for computation errors
6. **Governance:** OCAP is enforced at the dispatcher `GovernedTool` membrane (`DelegationToken` per call), not at the server. The server is the transport pipe; `shell_exec`-style tools are reachable only by agents holding the relevant capability token. See [`lib.rs` (`GovernedTool`)](../../../crates/hkask-regulation/src/governed_tool.rs) (replaces deleted `crates/hkask-pods/src/lib.rs`) and the `kask_bridge` `BridgeToolPort` adapter (`kask/crates/kask_bridge/src/tool_port.rs`, D3/D8).

## Testing standard

Every MCP server MUST include **tool-behavior contract tests** that invoke tools through their public `Parameters<T>` seam (e.g. `server.fs_read(Parameters(FsReadRequest { ... }))`), covering at minimum: the happy path, invalid input, boundary/edge cases, and error-specificity. Helper-seam-only tests (testing `sandbox_path`/services/infrastructure in isolation) are necessary but **not sufficient** — a helper-seam-only suite cannot catch tool-contract bugs (slice-index panics on bad input, canonicalize-on-non-existent, silent no-ops, error-swallowing). The kata-kanban contract test suite is the exemplar pattern. See the fleet test-seam audit for the current coverage gap across all 11 servers.

## Cross-links

- [Companies MCP Server Reference](companies.md) — 41 tools, dual-provider routing, forecast store, portfolio ledger (DIAG-RF-004 inline)
- [Condenser MCP Server Reference](condenser.md) — 7 tools, 3 compression algorithms, 2-phase condensation (DIAG-RF-006 inline)
- [Corpus MCP Server Reference](corpus.md) — corpus gathering, document processing, QA generation, style replicas
- [Scenario Forecasting Pipeline Diagram](scenarios.md) — scenarios tool flow (DIAG-RF-005 inline)
- [Superforecasting: Layered Model](../../explanation/forecasting-and-scenarios.md) — three-layer architecture
- [Architecture Patterns](../../explanation/architecture-patterns.md) — MCP dispatch sequence
- CodeGraph Adversarial Review — adversarial code review of the codegraph server (17 findings, all fixed)
- Companies MCP Code Review — adversarial code review of the companies server
- Companies Semantic Graph Audit — internal module dependency graph health
- Scenarios Adversarial Review — code smell inventory for the scenarios server
- Research MCP Adversarial Review — code smell inventory for the research server
- Research MCP Adversarial Review (Follow-Up 2026-07-20) — 11 new findings: dead CapabilityContext, edit_tags feed-relabeling bug, missing transactions, stored SSRF, stub health checks; 7 follow-up items including panic-safe transactions, permissive SSRF for RSS, and circuit-breaker ADR

## Kata-Kanban Server Architecture (DIAG-IC-017)

The `hkask-mcp-kata-kanban` MCP server (`KanbanServer`) is a thin tri-surface wrapper that delegates every tool call to `KanbanService`. The service owns an `HMemStore` (board/task persistence). Full kata execution is available through the in-process `KataEngine` (constructed directly by the agent loop or the kask panel, D10), which replaces the deleted `kask kata start` CLI command. The kanban service exposes kata prompt generation (`task_coaching_prompt` / `task_improvement_prompt` / `task_practice_prompt`) for MCP and in-process surfaces.

The `--task <id>` binding (previously a CLI flag) is now an in-process parameter that binds a `TaskGasAccountant` to the engine, closing the per-task gas feedback loop: each inference call's actual token usage is deducted from the bound kanban task's `gas_remaining` budget via `task_consume_gas`.

```mermaid
classDiagram
    direction TD

    class KanbanServer {
        +webid: WebID
        +userpod: String
        +daemon: Option~DaemonClient~  // DEAD in zed-kask — always None; daemon deleted in 2026-07-25 cleanup
        +service: KanbanService
        +kanban_board_create() String
        +kanban_board_list() String
        +kanban_task_create() String
        +kanban_task_list() String
        +kanban_task_move() String
        +kanban_task_assign() String
        +kanban_task_verify() String
        +kanban_task_add_gas() String
        +kanban_task_add_rjoules() String
        +kanban_task_comment() String
        +kanban_task_comments_since() String
        +kanban_task_add_deliverable() String
        +kanban_task_reopen() String
        +kanban_task_kata_coaching() String
        +kanban_task_kata_improvement() String
        +kanban_task_kata_practice() String
        +kanban_task_spawn() String
        +contract_propose_expect() String
    }

    class KanbanService {
        +store: HMemStore
        +standard_columns() Vec~ColumnDef~
        +board_create() Result~Board~
        +board_list() Result~Vec~Board~~
        +board_get() Result~Option~Board~~
        +board_view() Result~String~
        +board_delete() Result~usize~
        +task_create() Result~Task~
        +task_list() Result~Vec~Task~~
        +task_get() Result~Option~Task~~
        +task_move() Result~Task~
        +task_claim() Result~Task~
        +task_verify() Result~(Task, Verification)~
        +task_reopen() Result~Task~
        +task_add_gas() Result~Task~
        +task_add_rjoules() Result~Task~
        +task_consume_gas() Result~u64~
        +task_consume_rjoules() Result~u64~
        +task_gas_exhaust() Result~Task~
        +task_comment() Result~Comment~
        +task_comments() Result~Vec~Comment~~
        +task_comments_since() Result~Vec~Comment~~
        +task_add_deliverable() Result~Task~
        +task_unassign() Result~Task~
        +task_delete() Result~()~
        +task_coaching_prompt() Result~String~
        +task_improvement_prompt() Result~String~
        +task_practice_prompt() Result~String~
        +spawn_task() Result~String~
        +unjam_report() Result~Vec~UnjamItem~~
        +unjam_fix() Result~Vec~UnjamFix~~
        +decompose_prompt() Result~String~
        +decompose_populate() Result~(usize, Option~String~)~
        +board_create_from_template() Result~Board~
        +board_add_phase() Result~KanbanPhase~
        +task_set_phase() Result~Task~
        +tasks_by_phase() Result~Vec~Task~~
        +verification_prompt() Result~String~
        +verify_with_llm() Result~(Task, Verification)~
    }

    class KataEngine {
        +inference: Arc~dyn InferencePort~
        +registry: SqliteRegistry
        +consent_check: Option~ConsentCheckFn~
        +ledger_observer: Option~LedgerObserverFn~
        +history: Option~KataHistory~
        +history_store: Option~Arc~KataHistoryStore~~
        +metric_collector: Option~MetricCollectorFn~
        +ledger_runtime: Option~Arc~RwLock~RegulationLedger~~
        +task_gas_accountant: Option~Arc~dyn TaskGasAccountant~~
        +new() KataEngine
        +from_env() KataEngine
        +execute() Result~KataResult~
        +run_bundle() Result~KataResult~
        +load_manifest() Result~KataManifest~
        +record_history_entry() Result~Option~i64~~
        +with_task_gas_accountant() KataEngine
    }

    class TaskGasAccountant {
        <<interface>>
        +consume(cost, reason) Result~u64~
    }

    class KanbanTaskGasAccountant {
        -service: Arc~KanbanService~
        -task_id: TaskId
    }

    class HMemStore {
        +driver: Arc~dyn DatabaseDriver~
        +encryptor: Option~Arc~Encryptor~
        +insert() Result~()~
        +update() Result~()~
        +query_by_entity() Result~Vec~HMem~~
        +query_by_entity_attribute() Result~Vec~HMem~~
        +close_by_id() Result~()~
    }

    class Board {
        +id: BoardId
        +name: String
        +owner: WebID
        +columns: Vec~ColumnDef~
        +phases: Vec~KanbanPhase~
        +created_at: DateTime
    }

    class Task {
        +id: TaskId
        +board_id: BoardId
        +title: String
        +description: Option~String~
        +status: TaskStatus
        +owner: WebID
        +assignee: Option~WebID~
        +criteria: Vec~VerificationCriterion~
        +verification: Option~Verification~
        +story_points: Option~u32~
        +estimated_hours: Option~f64~
        +priority: Option~Priority~
        +labels: Vec~String~
        +comments: Vec~Comment~
        +deliverables: Vec~String~
        +phase_id: Option~PhaseId~
        +gas_remaining: Option~u64~
        +rjoule_remaining: Option~u64~
        +gas_spend: Vec~GasEntry~
    }

    class TaskStatus {
        <<enumeration>>
        Backlog
        Ready
        InProgress
        Review
        Done
    }

    class SocraticRole {
        <<enumeration>>
        Planner
        Diagnoser
        Tutor
        Assessor
    }

    KanbanServer --> KanbanService : delegates
    KanbanService --> HMemStore : persists via
    KanbanService --> Board : manages
    KanbanService --> Task : manages
    KanbanService ..> TaskGasAccountant : gas_accountant_for()
    KanbanTaskGasAccountant ..|> TaskGasAccountant : implements
    KataEngine --> TaskGasAccountant : with_task_gas_accountant()
    KataEngine ..> KanbanService : in-process construction (deleted kask kata start CLI)
    Board --> ColumnDef : contains
    Board --> KanbanPhase : contains
    Task --> TaskStatus : has
    Task --> Comment : contains
    Task --> GasEntry : audit trail
    Task --> Verification : result
    SocraticRole ..> Task : spawns inquiries as
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-IC-017
verified_date: 2026-07-24
verified_against: mcp-servers/hkask-mcp-kata-kanban/src/lib.rs:29-33 (KanbanServer struct — db field deleted; daemon field retained but always None in zed-kask — dead code, R4/T3.0 refactor pending), crates/hkask-services-kata-kanban/src/kanban/service_impl/service.rs:34-37 (KanbanService struct — kata_bridge field deleted, pod_manager removed post-pivot), crates/hkask-services-kata-kanban/src/kata/mod.rs:76-94 (KataEngine struct), crates/hkask-storage/src/hmem.rs:134-138 (HMemStore struct), crates/hkask-services-kata-kanban/src/kanban/types/task.rs:9-55 (Task struct), crates/hkask-services-kata-kanban/src/kanban/types/status.rs:16-27 (TaskStatus enum), crates/hkask-services-kata-kanban/src/kanban/socratic.rs:265-270 (SocraticRole enum); KataEngine construction now in-process (deleted kask kata start CLI surface)
status: VERIFIED (v4 — daemon field annotated as dead in zed-kask; always None; daemon deleted in 2026-07-25 cleanup)
-->
