---
title: "hKask Event Store — Class Diagram"
audience: [architects, developers]
last_updated: 2026-08-24
version: "1.0.0"
status: "Active"
domain: "Lifecycle"
mds_categories: [lifecycle, composition, domain]
---

# hKask Event Store — Class Diagram

The `hkask-event-store` crate is the append-only event log for agent rollouts. It captures `model_request` and `verdict` events produced by local swarm delegations, harness runs, and (reserved) curator turns. The store is the data-plane substrate for three downstream consumers: agent evaluation, training-data generation, and regulation.

The crate is wired into the composition root via `kask_bridge/src/rollout_event_bridge.rs` and consumed by `hkask-regulation/src/cybernetics_loop.rs`. The provenance and classification types follow Agent Lightning's `schemas.py` data model, combined with Event Sourcing (DDD, Evans 2003; Fowler, 2005).

```mermaid
classDiagram
    class EventStore {
        -driver: Arc~dyn DatabaseDriver~
        -clock: fn() -> String
        +from_driver(driver) Result~EventStore~
        +from_driver_with_clock(driver, clock) Result~EventStore~
        +driver() ~Arc~dyn DatabaseDriver~~
        -init_schema(driver) Result~()~
        +append(rollout, kind, payload) Result~i64~
        +query(filter) Result~Vec~EventRecord~~
        +compact(cutoff_rfc3339) Result~usize~
        +strip_bodies(cutoff_rfc3339) Result~usize~
        +cursor() Result~Option~i64~~
    }
    class EventRecord {
        +position: i64
        +rollout_id: String
        +kind: String
        +payload: Value
        +created_at: String
    }
    class EventFilter {
        +rollout: Option~String~
        +kind: Option~String~
        +after_position: Option~i64~
        +limit: Option~usize~
    }
    class EventStoreError {
        <<enumeration>>
        Database(DbError)
        PayloadParse(serde_json::Error)
        EmptyRolloutId
        EmptyKind
        NoPosition
    }
    class VerdictSource {
        <<enumeration>>
        DeterministicEvaluator
        Operator
        LlmJudged
        RegulationImpact
        +as_str() &'static str
        +from_str(s) Option~Self~
        +is_trusted_for_task_success() bool
    }
    class RolloutKind {
        <<enumeration>>
        Delegation
        Turn
        HarnessRun
        +as_str() &'static str
        +from_str(s) Option~Self~
    }
    class DatabaseDriver {
        <<trait>>
        +execute_batch(sql) Result~()~
        +execute(sql, params) Result~usize~
        +query(sql, params) Result~Vec~Row~~
        +query_optional(sql, params) Result~Option~Row~~
    }

    EventStore --> DatabaseDriver : backed by
    EventStore ..> EventRecord : produces
    EventStore ..> EventFilter : consumes
    EventStore ..> EventStoreError : propagates
    EventRecord ..> VerdictSource : payload carries
    EventRecord ..> RolloutKind : payload carries
    VerdictSource --|> "trusted for task_success" : DeterministicEvaluator, Operator
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-ES-001
verified_date: 2026-08-24
verified_against: kask/crates/hkask-event-store/src/hkask_event_store.rs (EventStore struct L52-55, from_driver L62-64, from_driver_with_clock L71-77, append L93-130, query L134-173, compact L179-189, strip_bodies L200-208, cursor L212-231, SCHEMA_DDL L235-243), kask/crates/hkask-event-store/src/types.rs (EventRecord L145-161, EventFilter L164-174, EventStoreError L177-189, VerdictSource L43-58 + as_str L64-71 + from_str L76-84 + is_trusted_for_task_success L89-94, RolloutKind L105-118 + as_str L122-128 + from_str L132-139), kask/crates/kask_bridge/src/rollout_event_bridge.rs (wiring L24, L105-107, L167-168, L442-443), kask/crates/hkask-regulation/src/cybernetics_loop.rs:58 (consumer)
status: VERIFIED
-->

## Cross-Links

- [`../architecture/core/MDS.md`](../architecture/core/MDS.md) Composition Root — `hkask-event-store` crate-to-domain mapping (Lifecycle, Composition).
- [`class-swarm-server.md`](class-swarm-server.md) — the `SwarmServer` whose `swarm_delegate_local` / `swarm_eval_agent_local` tools produce the events this store records.
- [`../explanation/abw-swarm-orchestration.md`](../explanation/abw-swarm-orchestration.md) — swarm orchestration context for the rollout lifecycle.

## Provenance model

`VerdictSource` is the single provenance type for all verdicts. Trust classification:

| Variant | Trusted for task success? | Rationale |
|---|---|---|
| `DeterministicEvaluator` | Yes | Deterministic check (contains/regex/exit_code/file_exists). The only automated source trusted for the C0 `s` axis. |
| `Operator` | Yes | Human ground truth (the operator or Curator stamped it). |
| `LlmJudged` | No | An LLM judged the response. ORIENT must downgrade to a hypothesis — the determinism constraint forbids an LLM judging `task_success`. |
| `RegulationImpact` | No (for task success) | The cybernetics loop's `verify_impact` produced this — a before/after measurement, not a task-success check. |

## Schema

One table; `position` (rowid) is the identity. Two indexes optimize the common query paths (by rollout, by kind).

```sql
CREATE TABLE IF NOT EXISTS events (
    position INTEGER PRIMARY KEY AUTOINCREMENT,
    rollout_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    payload TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_events_rollout ON events(rollout_id, position);
CREATE INDEX IF NOT EXISTS idx_events_kind ON events(kind, position);
```

## Feedback-loop discipline

Per the project's `.rules` (no broken feedback loops), the store propagates errors rather than silently coercing:

- `cursor()` distinguishes `Null` (empty log) from a real column-read error — the prior `Ok(row.and_then(...))` silently coerced errors to "empty log".
- `append()` uses `INSERT ... RETURNING position` so concurrent writers receive distinct positions (the prior INSERT-then-SELECT-MAX raced under concurrent connections).
- `query()` surfaces a corrupt payload as `EventStoreError::PayloadParse`, not as a silently-nullable `Null` field.
- `compact()` and `strip_bodies()` return the affected count — callers must surface it, never swallow it.
