# hkask-memory

Vector embedding + relational lookup memory for hKask.

Implements turn ingestion → h_mem + embedding → cleanup → semantic recall.

## Architecture

The memory system is a **vector + relational** store following the OpenClaw
model. One entity_ref string (`chat:thread:{thread_id}`) links each embedding
vector to its relational h_mem row, so KNN search results join back to the
full turn text.

- **`MemoryStore`** — wraps `HMemStore` (relational EAV) + `EmbeddingStore`
  (sqlite-vec vectors). Provides `store`, `store_embedding`, `search_similar`,
  `query_deduped`, and decay/touch operations.
- **`MemoryConsolidator`** — confidence-floor cleanup.

The bridge that wires thread turns into this store lives in `kask_bridge`
(`RealMemoryPort`), not in this crate.

## Centroid selection

`MemoryStore::compute_centroid` retains prefix selection.
`compute_centroid_for_refs` selects existing embeddings by an explicit entity-ref
list, with no source copies or re-embedding. Repeated refs contribute once;
missing eligible refs fail before writing. Both paths exclude `exclude_ref`,
`:rule:` refs, and derived `:centroid` refs, including dimension centroids, so
recomputation never averages prior centroids into the passage mean. Empty
eligible sets and mismatched vector dimensions are errors.

Both paths share mean computation and optional storage. Supplying `store_as`
and `model` atomically replaces previous destination embedding rows and their
vector-index entries in one transaction. Failed inserts preserve the previous
centroid; concurrent recomputations leave one destination row. Ordinary
embedding writes still append. Without both, the result is returned without
storage.

## Forgetting Curve

Wozniak & Gorzelanczyk (1995), equation (3): **R(t) = exp(-t/S)**

Where S is memory life in days (configurable, default 180 = 6 months × 30).
After S days without recall, confidence decays to exp(-1) ≈ 36.8%.

At recall (when a memory is pulled into a prompt as context), the decay clock
resets — t goes back to 0, R = 1.0. Only h_mems that survive the
`recall_limit` truncation are touched (prevents a write storm under
concurrent recall).

`query_by_attribute` is also a recall API: it applies confidence decay and
resets matching rows' recall clocks. `query_by_attribute_untouched` applies
the same decay without updating `recalled_at`; exports such as training dataset
assembly use this path so scanning QA does not prolong its retention.

## Deletion cleanup

`delete_h_mems_by_entity_prefix` deletes with a literal, case-sensitive prefix:
`%`, `_`, and backslash are ordinary characters. Storage returns the deleted-row
count and distinct affected entities from `DELETE ... RETURNING` in one
transaction, without a discovery cap or decoding memory payloads. A failed
deletion rolls back before returning an error.

The memory layer then reuses per-entity cleanup to remove embeddings, vector
index entries, and memory links for emptied entities, including those beyond
100,000 deleted rows. Unrelated entities are preserved. Cleanup remains a
post-commit operation: failures are logged and retried by the periodic orphan
sweep; it is not a cross-store transaction.

## Configuration

| Variable                          | Description                            | Default |
| --------------------------------- | -------------------------------------- | ------- |
| `HKASK_MEMORY_LIFE_DAYS`          | Memory life S in days                  | 180     |

| `HKASK_MEMORY_INGEST_CONCURRENCY` | Ingestion semaphore permits            | 1       |
| `HKASK_EMBEDDING_DIM`             | Embedding vector dimension             | 1024    |

## Consolidation

`MemoryConsolidator::consolidate(&WebID, confidence_floor: f64)` deletes rows
at or below the supplied confidence floor. Its `ConsolidationOutcome` reports
successful row deletions in `deleted_count` and failed row deletions in
`failed_count`; a failed deletion is logged without stopping other candidates.
It does not promote, re-tag, or change visibility. Memory rows are unified;
distillation and age-gated forgetting live in the curator server.

Deletion goes through `MemoryStore::delete_h_mem`: an emptied entity loses its
embeddings and memory links; for a surviving entity, only the deleted row's
passage embedding is removed unless a surviving sibling shares that passage.
Reference-cleanup failures are logged for the periodic orphan sweep rather than
counted as failed row deletions (`src/memory_store.rs::MemoryStore::delete_h_mem`
and `MemoryStore::cleanup_orphaned_references_for_entity`).

Normalized-value dedup likewise physically deletes lower-confidence duplicates.
There is no expired-memory state or compatibility alias; results report
`deleted_count`. The one-time storage migration converts older soft-deleted
rows to deletion rather than continuing to expose the old lifecycle.

## Documentation

- [Memory System Specification](../../docs/architecture/memory-system-specification.md) — the architecture spec, design rationale, and embedded diagrams (ERD, ingest sequence, recall flow)
