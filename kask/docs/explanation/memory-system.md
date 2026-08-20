---
title: "Memory System — Why It Works This Way"
audience: [developers, architects, agents]
last_updated: 2026-08-10
version: "1.0.0"
status: "Active"
domain: "Trust"
mds_categories: [trust, curation]
---

# Memory System — Why It Works This Way

> **Companion to:** [Memory System Specification](../architecture/memory-system-specification.md)
> (the reference doc). This is the explanation — the "why."

## The problem

An agent that forgets every conversation is useless for long-horizon work.
But an agent that recalls everything equally is also useless — the context
window fills with irrelevant history, and the model drowns in noise. The
memory system solves two problems:

1. **What to remember** — which turns are worth storing, and how to
   represent them so they can be found again.
2. **What to recall** — given the current prompt, which stored memories
   are relevant enough to inject into the context.

## The design: vector + relational, linked by string key

The memory system follows the ABW/OpenClaw model: a **vector embedding**
for semantic similarity search, plus a **relational lookup table** for the
full text, linked by a shared string key (`entity_ref` / `entity`).

This is deliberately simpler than the previous design, which had separate
`EpisodicMemory` and `SemanticMemory` structs, a `ConsentManager` for
visibility transitions, and a `generate_narrative` loop that fired every 10
experiences. Those abstractions were removed when the standalone daemon was
deleted — they were speculative generality with no consumer in the
in-process host.

### Why a string key, not a foreign key?

The `embeddings.entity_ref` and `hmems.entity` columns are both `TEXT` with
no foreign key constraint. A normalized design would use an integer FK.
But the string-key design has three advantages:

1. **Debuggability** — you can `SELECT * FROM embeddings WHERE entity_ref
LIKE 'chat:thread:%'` and immediately see what's stored. An integer FK
   requires a join to interpret.
2. **No schema migration** — the `EmbeddingStore` and `HMemStore` are in
   the same DB but different tables with different APIs. A FK would require
   coordinating the two stores' schemas.
3. **ABW/OpenClaw compatibility** — the model the user asked for uses a
   string key, and the simplicity is the point.

The trade-off: the invariant (`entity_ref == entity`) is enforced by a
comment + test, not by the type system. A future `EntityRef(String)`
newtype would make it compile-time-enforced, but that's deferred until a
third embedding call site appears (YAGNI).

### Why two recall legs?

The semantic leg (embedding KNN) catches paraphrased follow-ups and
conceptually related questions that share no words with the stored turn.
The keyword leg catches exact-term matches that the embedding model might
not rank highly (e.g., rare proper nouns). Neither leg alone is
sufficient — the semantic leg misses exact terms, the keyword leg misses
paraphrases. Together they cover the space.

The semantic leg ranks above the keyword leg when cosine distance < 0.5
(relevance > 0.5), and below it when distance > 0.5. This is the right
default: a strong semantic match is more relevant than a keyword match,
but a weak semantic match is less relevant than a keyword match.

### Why fire-and-forget ingestion?

The user has already seen the turn's response. The memory ingestion
(h_mem store + embedding HTTP call) adds latency that the user shouldn't
pay. So `ingest_turn` is spawned via `cx.background_spawn()` and the
thread moves on. The ingestion semaphore serializes concurrent
ingestions so they don't contend with the recall path for the SQLite
pool.

### Why no query-embedding cache?

Every recall embeds the query fresh via an HTTP call to the embedding
provider (~100–300ms). A query-embedding LRU cache would eliminate repeat
embeddings for identical prompts. But:

1. The `should_recall` gate already skips short prompts (< 20 chars or
   < 3 words), which are the most common repeat prompts ("yes", "continue",
   "ok").
2. The latency is acceptable for the ABW/OpenClaw simplicity model.
3. A cache adds invalidation complexity (when should a cached query
   embedding be evicted?).

If latency becomes an issue, a small `query → query_vector` LRU is the
right first step. Not needed now.

## The entity_ref bug: why it persisted

The embedding was stored under `embedding:thread:{thread_id}:user_input`
while the h_mem text lived under `chat:thread:{thread_id}`. The recall
path's `query_deduped_untouched(entity_ref)` looked up an h_mem at
`embedding:thread:...` — which didn't exist. The semantic leg was silently
dead code; only the keyword leg returned snippets.

This persisted because:

1. **The error was silent.** `query_deduped_untouched` returns
   `Ok(vec![])` for a missing entity — not an error. The empty vec was
   skipped by `if let Ok(h_mems) = ...`, producing no log, no warning.
2. **No test exercised the end-to-end path.** Every test used
   `LanguageModelEmbeddingPort::for_tests()` — a channel-closed no-op stub
   that returns an error on `embed()`. The embedding leg was silently
   skipped in every test.
3. **The keyword leg masked the failure.** Recall still returned
   snippets (via keyword overlap), so the system appeared to work. The
   semantic leg's absence was invisible unless you asked a paraphrased
   follow-up with no word overlap — which no test did.

The fix (2026-08-10) stores the embedding under the same
`chat:thread:{thread_id}` entity as the h_mem. The regression test
(`recall_context_finds_turn_by_embedding_only`) uses a constant stub
embedding and a query with zero word overlap, so the semantic leg is the
only path to recall. It fails on the old code and passes on the fix.

## The consolidation loop: why it's decoupled

Consolidation (episodic → semantic promotion) runs on a background timer,
not in the `ingest_turn` path. This is because:

1. **Latency.** Consolidation involves Bayesian confidence combination
   and budget-gated pruning — potentially many DB writes. Running it in
   the ingestion path would add unpredictable latency to turn completion.
2. **Contention.** Consolidation writes to the same SQLite pool as
   ingestion and recall. Running it on a timer spreads the load.
3. **Ashby's Law.** The storage budget (default 10,000 h_mems) is the
   Ashby attenuator for unbounded memory growth. Consolidation prunes
   back to the budget when exceeded. Decoupling it from ingestion means
   the pruning decision is made on a schedule, not under write pressure.

## The decay model: why Wozniak-Gorzelanczyk?

The forgetting curve `R(t) = exp(-t / S)` (Wozniak & Gorzelanczyk 1995) is
used because:

1. **It's a single parameter.** `S` (memory life in days, default 180) is
   the only knob. No multi-exponential decay, no spaced-repetition
   scheduling — just a smooth exponential.
2. **It's well-validated.** SuperMemo (Wozniak's system) has decades of
   empirical data behind this curve.
3. **It's touchable.** `touch_recall` resets `recalled_at` to now, which
   resets `t` to 0, which resets `R` to 1.0. "Memory that gets used stays
   fresh" — the exact semantics we want.

The decay is applied at recall time (not at write time), so the stored
`confidence` is the original value and the decayed value is computed on
the fly. This means a memory's effective confidence depends on when you
ask, not when it was stored — which is the right model for a memory that
degrades with disuse.

## Related

- [Memory System Specification](../architecture/memory-system-specification.md) — the reference doc
- [Memory Ingest Sequence](../diagrams/sequence-memory-ingest.md) — the write-side diagram
- [Memory Recall Flow](../diagrams/flowchart-memory-recall.md) — the read-side diagram
- [Memory Store ERD](../diagrams/erd-memory-store.md) — the storage schema
- [hkask-memory README](../../crates/hkask-memory/README.md) — crate-level docs
