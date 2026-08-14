---
title: "Memory Ingest — Turn → h_mem + embedding"
audience: [developers, architects, agents]
last_updated: 2026-08-10
version: "1.0.0"
status: "Active"
domain: "Lifecycle"
mds_categories: [lifecycle, composition]
---

# Memory Ingest — Turn → h_mem + embedding

Sequence diagram of `RealMemoryPort::ingest_turn` — the path from a completed
thread turn to stored episodic h_mems + a stored prompt embedding. This is
the write side of the memory system. The read side is
[Memory Recall Flow](./flowchart-memory-recall.md).

The ingestion is fire-and-forget from the thread's perspective: the turn has
already completed and the user sees no latency. An ingestion semaphore
serializes concurrent ingestions so they don't contend with the recall path
for the SQLite pool.

```mermaid
sequenceDiagram
    participant Thread as Thread<br/>(run_turn)
    participant Bridge as BridgeMemoryPort
    participant Real as RealMemoryPort
    participant Sem as ingest_semaphore
    participant UserStore as user MemoryStore<br/>(memory.db)
    participant CuratorStore as curator MemoryStore<br/>(curator.db)
    participant Tokio as tokio runtime
    participant EmbedPort as LanguageModelEmbeddingPort
    participant EmbedProvider as Embedding API<br/>(DeepInfra/Qwen3)

    Thread->>+Bridge: ingest_turn(TurnRecord)
    Bridge->>+Real: ingest_turn(record)
    Real->>+Sem: acquire permit
    Sem-->>-Real: permit

    rect rgb(245, 248, 252)
        Note over Real,CuratorStore: Phase 1 — Episodic h_mems (every turn)

        Real->>Real: entity = "chat:thread:{thread_id}"
        Real->>+UserStore: store(episodic_h_mem)<br/>Private, user_webid
        UserStore-->>-Real: Ok

        alt is_curator_turn
            Real->>+CuratorStore: store(episodic_h_mem)<br/>Private, curator_webid
            CuratorStore-->>-Real: Ok
        end

        Real->>+CuratorStore: store(semantic_h_mem)<br/>Shared, curator:thread:{id}
        CuratorStore-->>-Real: Ok
    end

    rect rgb(252, 245, 245)
        Note over Real,EmbedProvider: Phase 2 — Prompt embedding (every turn)

        Real->>Real: embedding_entity = entity.clone()<br/>"chat:thread:{thread_id}"
        Real->>+Tokio: spawn(embed(model, [user_input]))
        Tokio->>+EmbedPort: embed(model, [user_input])
        EmbedPort->>+EmbedProvider: POST /embeddings
        EmbedProvider-->>-EmbedPort: Vec<f32> (1024-dim)
        EmbedPort-->>-Tokio: Ok(vector)
        Tokio-->>-Real: Ok(Ok(vector))

        alt embedding succeeded
            Real->>+UserStore: store_embedding(entity, vector, model)
            UserStore-->>-Real: Ok(embedding_id)
            alt is_curator_turn
                Real->>+CuratorStore: store_embedding(entity, vector, model)
                CuratorStore-->>-Real: Ok
            end
        else embedding failed
            Real-->>Real: tracing::warn (non-fatal)<br/>keyword recall still works
        end
    end

    Real-->>-Bridge: Ok
    Bridge-->>-Thread: Ok
    Note over Thread: Turn already completed<br/>user sees no latency
```

## Key invariants

1. **The embedding's `entity_ref` equals the h_mem's `entity`**
   (`chat:thread:{thread_id}`). This is the join key for recall. See
   [Memory Store ERD](./erd-memory-store.md).

2. **Both user and curator stores receive the embedding** for curator turns,
   so the curator can recall its own turns by similarity.

3. **Embedding failure is non-fatal.** The episodic h_mem is still stored;
   recall degrades to keyword-only for that turn.

4. **Consolidation is decoupled.** It runs on a background timer, not in the
   ingestion path. See `start_consolidation_timer` in `RealMemoryPort`.

## Related

- [Memory Recall Flow](./flowchart-memory-recall.md) — the read side
- [Memory Store ERD](./erd-memory-store.md) — the storage schema
- [Memory System Specification](../architecture/memory-system-specification.md) — the architecture spec
- [D6: Thread → memory](../../DIVERGENCE.md) — the divergence seam

<!-- DIAGRAM_ALIGNMENT
id: DIAG-PL-MEMORY-INGEST
verified_date: 2026-08-10
verified_against: kask/crates/kask_bridge/src/memory.rs:1000
status: VERIFIED
-->
