---
title: "Memory Ingest — Turn → curator h_mems + embedding"
audience: [developers, architects, agents]
last_updated: 2026-08-28
version: "2.0.0"
status: "Active"
domain: "Lifecycle"
mds_categories: [lifecycle, composition]
---

# Memory Ingest — Turn → curator h_mems + embedding

Sequence diagram of `RealMemoryPort::ingest_turn`
(`kask/crates/kask_bridge/src/memory.rs:454-497`) and
`ingest::write_turn` (`kask/crates/kask_bridge/src/memory/ingest.rs:58-235`)
— the path from a completed thread turn to stored h_mems + a stored prompt
embedding in the curator's sovereign `curator.db`. This is the write side
of the memory system. The read side is
[Memory Recall Flow](./flowchart-memory-recall.md).

The ingestion is fire-and-forget from the thread's perspective: the turn
has already completed and the user sees no latency. An ingestion semaphore
(default 1 permit, `HKASK_MEMORY_INGEST_CONCURRENCY`) serializes concurrent
ingestions so they don't contend with the recall path for the SQLite pool
(`memory.rs:459-481`).

```mermaid
sequenceDiagram
    participant Thread as Thread turn loop<br/>(crates/agent)
    participant Bridge as BridgeMemoryPort<br/>(agent::ThreadMemoryPort)
    participant Real as RealMemoryPort
    participant Sem as ingest_semaphore
    participant Write as ingest::write_turn
    participant Curator as CuratorStore<br/>(self-healing, curator.db)
    participant Tokio as tokio runtime
    participant EmbedPort as LanguageModelEmbeddingPort
    participant EmbedProvider as Embedding API<br/>(DeepInfra/Qwen3-Embedding-0.6B)

    Thread->>+Bridge: ingest_turn(TurnRecord)
    Bridge->>+Real: ingest_turn(record)
    Real->>+Sem: acquire permit
    Sem-->>-Real: permit
    Real->>+Write: write_turn(WriteContext, record)

    Write->>Curator: get() — re-attempt open if down<br/>(rebuild consolidation if healed)

    rect rgb(245, 248, 252)
        Note over Write,Curator: Phase 1 — h_mems (curator.db)

        alt is_curator_turn (agent_id == "Curator")
            Write->>Curator: store(curator h_mem)<br/>chat:thread:{id}, "chatted", Private,<br/>curator_webid, PKO process ontology
            Curator-->>-Write: Ok
        end

        Write->>Curator: store(shared copy)<br/>curator:thread:{id}, "turn", Shared,<br/>DC state ontology
        Curator-->>-Write: Ok
    end

    rect rgb(252, 245, 245)
        Note over Write,EmbedProvider: Phase 2 — Prompt embedding (every turn, non-fatal)

        Write->>Write: embedding_entity = curator_entity.clone()<br/>"curator:thread:{thread_id}"
        Write->>+Tokio: spawn(embed(model, [user_input]))
        Tokio->>+EmbedPort: embed(model, [user_input])
        EmbedPort->>+EmbedProvider: POST /embeddings
        EmbedProvider-->>-EmbedPort: Vec<f32> (1024-dim default)
        EmbedPort-->>-Tokio: Ok(vector)
        Tokio-->>-Write: Ok(Ok(vector))

        alt embedding succeeded
            Write->>Curator: store_embedding(curator:thread:{id},<br/>vector, model)
            Curator-->>-Write: Ok
        else embedding failed / no port
            Write-->>Write: tracing::warn (non-fatal)<br/>keyword recall still works
        end
    end

    Write-->>-Real: Ok
    Real-->>-Bridge: Ok
    Bridge-->>-Thread: Ok
    Note over Thread: Turn already completed<br/>user sees no latency
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-PL-MEMORY-INGEST
verified_date: 2026-08-28
verified_against: kask/crates/kask_bridge/src/memory.rs:454-497 (trait impl + semaphore), kask/crates/kask_bridge/src/memory/ingest.rs:58-235 (write_turn: heal 79-95, curator h_mem 100-130, shared copy 132-154, embedding 156-225, entity clone at 168), kask/crates/kask_bridge/src/memory/curator_stores.rs:104-160 (CuratorStore self-heal), kask/crates/hkask-inference/src/model_constants.rs:35 (embedding model)
status: VERIFIED
-->

## Key invariants

1. **The embedding's `entity_ref` equals the shared copy's `entity`**
   (`curator:thread:{thread_id}`) — the shared copy is written for every
   turn, so the join key always resolves. An embedding under
   `chat:thread:{id}` would orphan every zed-agent turn's embedding
   (`ingest.rs:160-168`). See [Memory Store ERD](./erd-memory-store.md).

2. **All writes go to the curator's `curator.db`.** There is no user
   memory store — `RealMemoryPort` holds only the `CuratorStore`
   (`memory.rs:74-119`). Zed-agent turns get the shared copy only; the
   curator-perspective h_mem is curator turns only (`ingest.rs:68`,
   `:100-130`).

3. **Embedding failure is non-fatal.** The h_mems are pure SQL and don't
   need embeddings; recall degrades to keyword-only for that turn with a
   `tracing::warn!` (`ingest.rs:156-159`, `:202-217`).

4. **Curator-store failures are non-fatal and self-healing.** A failed
   initial open leaves the store `None`; every `get()` re-attempts the
   open, and a successful re-open rebuilds the consolidation service
   (`curator_stores.rs:104-160`, `ingest.rs:79-95`). Persistent failure
   warns once per healing attempt — never silently
   (`curator_stores.rs:148-158`).

5. **Consolidation is decoupled.** It runs on the background timer
   (`start_consolidation_timer`, `memory.rs:236-287`), never in the
   ingestion path.

## Related

- [Memory Recall Flow](./flowchart-memory-recall.md) — the read side
- [Memory Store ERD](./erd-memory-store.md) — the storage schema
- [Memory System Specification](../architecture/memory-system-specification.md) — the architecture spec
- [D6: Thread → memory](../../../DIVERGENCE.md) — the divergence seam
