# hkask-memory

Vector embedding + relational lookup memory for hKask.

Implements turn ingestion → h_mem + embedding → cleanup → semantic recall.

## Architecture

The memory system is a **vector + relational** store following the ABW/OpenClaw
model. One entity_ref string (`chat:thread:{thread_id}`) links each embedding
vector to its relational h_mem row, so KNN search results join back to the
full turn text.

- **`MemoryStore`** — wraps `HMemStore` (relational EAV) + `EmbeddingStore`
  (sqlite-vec vectors). Provides `store`, `store_embedding`, `search_similar`,
  `query_deduped`, and decay/touch operations.
- **`MemoryConsolidator`** — confidence-floor cleanup and storage-budget pruning.

The bridge that wires thread turns into this store lives in `kask_bridge`
(`RealMemoryPort`), not in this crate.

## Forgetting Curve

Wozniak & Gorzelanczyk (1995), equation (3): **R(t) = exp(-t/S)**

Where S is memory life in days (configurable, default 180 = 6 months × 30).
After S days without recall, confidence decays to exp(-1) ≈ 36.8%.

At recall (when a memory is pulled into a prompt as context), the decay clock
resets — t goes back to 0, R = 1.0. Only h_mems that survive the
`recall_limit` truncation are touched (prevents a write storm under
concurrent recall).

## Configuration

| Variable                          | Description                            | Default |
| --------------------------------- | -------------------------------------- | ------- |
| `HKASK_MEMORY_LIFE_DAYS`          | Memory life S in days                  | 180     |
| `HKASK_MEMORY_STORAGE_BUDGET`     | Max h_mems before consolidation prunes | 10000   |
| `HKASK_MEMORY_INGEST_CONCURRENCY` | Ingestion semaphore permits            | 1       |
| `HKASK_EMBEDDING_DIM`             | Embedding vector dimension             | 1024    |

## Consolidation

`MemoryConsolidator::consolidate` deletes rows at or below a supplied confidence
floor, then prunes lowest-confidence rows to the requested storage budget when
needed. It does not promote, re-tag, or change visibility. Memory rows are
unified; distillation and age-gated forgetting live in the curator server.

Normalized-value dedup likewise physically deletes lower-confidence duplicates.
There is no expired-memory state or compatibility alias; results report
`deleted_count`. The one-time storage migration converts older soft-deleted
rows to deletion rather than continuing to expose the old lifecycle.

## Documentation

- [Memory System Specification](../../docs/architecture/memory-system-specification.md) — the architecture spec, design rationale, and embedded diagrams (ERD, ingest sequence, recall flow)
