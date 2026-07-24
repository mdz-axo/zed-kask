# hkask-services-kata-kanban — Kata-Kanban Workflow Service

Unified Toyota Kata process engine and Kanban board mechanics. **Kata is the process. Kanban is the tool/board/framework for applying the kata process to work.**

**Version:** v0.31.0 | **Crate:** `hkask-services-kata-kanban`

## Design Principle

PDCA phases map directly to Kanban task statuses:
```
Plan → Backlog | Do → InProgress | Check → Review | Act → Done
```

## Modules

| Module | Purpose |
|--------|---------|
| `kata` | Toyota Kata engine — coaching, improvement, starter, execution, manifest, state, history, metrics |
| `kanban` | Kanban board — types (Board, Task, SpawnSpec), service (CRUD, WIP limits, verification, de-jam), socratic inquiry |

## Key Types

### Kata (`kata`)
- `KataEngine` — primary engine: `new()`, `from_env()`, `execute()`, `load_manifest()`, builder methods
- `KataManifest` — kata definition deserialized from YAML
- `KataState` / `KataResult` — execution state and completion output
- `KataStep` / `KataHistory` / `StepExperience` — step definition and practice tracking
- `ImprovementSignal` / `ImprovementDirection` — PDCA outcome classification

### Kanban (`kanban`)
- `KanbanService` — board CRUD, task CRUD, consent-gated assignment, LLM-mediated verification, de-jam
- `Board` / `ColumnDef` — board with columns and WIP limits
- `Task` / `TaskSpec` / `TaskStatus` / `Priority` — task lifecycle
- `SpawnSpec` / `CapabilityPackage` — sub-userpod spawning with delegated capabilities
- `socratic` — Socratic inquiry cycle (4-stage: Elicit → Structure → Test → Summarize)

## Key Features

- **PDCA → Kanban mapping:** Improvement Kata cycles execute as kanban task state transitions
- **Coaching integration:** 5-question Coaching Kata prompts available as task primitives
- **Regulation observability:** `reg.kata.*` spans for kata execution, `reg.kanban` spans for board/task operations
- **Gas/rJoule tracking:** Per-task resource budgets with exhaustion completion path
- **WIP limits:** Column-level limits per Anderson (2010)
- **Consent gates:** Task assignment requires agent consent (P1)
- **LLM-mediated verification:** Two-step natural-language verification against acceptance criteria
- **De-jamming:** Auto-detection and fix for stuck tasks, stale assignments, unverified completions
- **Socratic inquiry:** 4-role interrogation system for task diagnosis

## Dependencies

- `hkask-services-core` — `ServiceConfig`, `ServiceError`
- `hkask-regulation` — Regulation span emission
- `hkask-storage` — TripleStore, kata history persistence
- `hkask-templates` — Jinja2 template rendering
- `hkask-types` — ID types, Regulation spans, WebID
- `hkask-ports` — Hexagonal port traits
- `hkask-inference` — Inference router
- `hkask-pods` — ActivePods for sub-userpod spawning
- `hkask-capability` — OCAP delegation tokens
