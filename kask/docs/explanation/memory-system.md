---
title: "Memory System — Why It Works This Way"
audience: [developers, architects, agents]
last_updated: 2026-08-28
version: "2.0.0"
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

The memory system pairs a **vector embedding** for semantic similarity
search with a **relational lookup table** for the full text, linked by a
shared string key (`entity_ref` / `entity`). This is the standard
retrieval-augmented pattern: an ANN index for "find me similar" plus a
record store for "give me the full document," joined by a stable
key[^vespa-hybrid].

### Why a string key, not a foreign key?

The `embeddings.entity_ref` and `hmems.entity` columns are both `TEXT` with
no foreign key constraint (`kask/crates/hkask-storage/src/core/sql/schema.sql:1`,
`:5`). A normalized design would use an integer FK. But the string-key
design has three advantages:

1. **Debuggability** — you can `SELECT * FROM embeddings WHERE entity_ref
   LIKE 'curator:thread:%'` and immediately see what's stored. An integer FK
   requires a join to interpret.
2. **No schema migration** — the `EmbeddingStore` and `HMemStore` are in
   the same DB but different tables with different APIs. A FK would require
   coordinating the two stores' schemas.
3. **Simplicity** — one store type, one join rule, no type system to keep
   in sync with the ontology blob.

The trade-off: the invariant (`entity_ref == entity`) is enforced by a
comment + test, not by the type system. A future `EntityRef(String)`
newtype would make it compile-time-enforced, but that's deferred until a
third embedding call site appears (YAGNI).

### Why the embedding lives under the shared-copy entity

The embedding is stored under `curator:thread:{thread_id}` — the shared
copy's entity — not `chat:thread:{thread_id}`
(`kask/crates/kask_bridge/src/memory/ingest.rs:160-168`). The shared copy
h_mem is written for **every** turn, while the `chat:thread:` h_mem only
exists for curator turns. An embedding under `chat:thread:` for a
zed-agent turn would join to no h_mem — an orphan the KNN recall path
could never resolve, making every zed-agent turn invisible to semantic
recall. The join key must point at a row that always exists.

### Why two recall legs?

The semantic leg (embedding KNN) catches paraphrased follow-ups and
conceptually related questions that share no words with the stored turn.
The keyword leg catches exact-term matches that the embedding model might
not rank highly (e.g., rare proper nouns). Neither leg alone is
sufficient — the semantic leg misses exact terms, the keyword leg misses
paraphrases. Together they cover the space[^hybrid-retrieval].

The semantic leg ranks above the keyword leg when cosine distance < 0.5
(relevance > 0.5), and below it when distance > 0.5 — the keyword leg's
relevance is the constant `0.5` (`kask/crates/kask_bridge/src/memory.rs:813`).
This is the right default: a strong semantic match is more relevant than a
keyword match, but a weak semantic match is less relevant than a keyword
match.

### Why fire-and-forget ingestion?

The user has already seen the turn's response. The memory ingestion
(h_mem store + embedding HTTP call) adds latency that the user shouldn't
pay. So `ingest_turn` is spawned in the background and the thread moves
on. The ingestion semaphore (default 1 permit) serializes concurrent
ingestions so they don't contend with the recall path for the SQLite pool
(`kask/crates/kask_bridge/src/memory.rs:459-481`).

### Why no query-embedding cache?

Every recall embeds the query fresh via an HTTP call to the embedding
provider (~100–300ms). A query-embedding LRU cache would eliminate repeat
embeddings for identical prompts. But:

1. The `should_recall` gate already skips short prompts (< 20 chars or
   < 3 words, `kask/crates/kask_bridge/src/context_injector.rs:38-42`),
   which are the most common repeat prompts ("yes", "continue", "ok").
2. The latency is acceptable for the simplicity model.
3. A cache adds invalidation complexity (when should a cached query
   embedding be evicted?).

If latency becomes an issue, a small `query → query_vector` LRU is the
right first step. Not needed now.

## Why the ranking multiplies by confidence and connectedness

Candidates are sorted by `relevance × confidence × (1 +
min(connectedness × 0.1, 0.5))` (`memory.rs:839-849`), not by relevance
alone. Two reasons, both grounded in the calibration literature[^tetlock]:

1. **Confidence is the outcome-calibrated signal.** Dunning's double
   curse: the model can't self-evaluate, but confidence that's been
   calibrated by outcomes IS meaningful. Using it as a ranking multiplier
   — not just a threshold filter — means a memory that has been recalled
   many times and never contradicted outranks a fresh, untested memory
   with similar embedding similarity.
2. **Connectedness is a structural prior.** Entities that co-occur
   frequently across recall contexts are more salient — they've been
   tested against more contexts. The bonus is capped at 50% (max 1.5×)
   so a highly-connected entity cannot crowd out fresh memories — the
   dilution-effect guard.

## The consolidation loop: why it's cleanup only

Consolidation runs on a background timer, not in the `ingest_turn` path,
and does exactly two things: delete h_mems at/below the confidence floor,
and delete lowest-confidence h_mems until within the storage budget
(`kask/crates/hkask-memory/src/consolidation_service.rs:82-149`). There is
no promotion, no re-tagging, no reflection. This is because:

1. **Latency.** Consolidation performs potentially many DB writes. Running
   it in the ingestion path would add unpredictable latency to turn
   completion.
2. **Contention.** Consolidation writes to the same SQLite pool as
   ingestion and recall. Running it on a timer spreads the load.
3. **Ashby's Law.** The storage budget (default 10,000 h_mems,
   `memory_store.rs:112`) is the attenuator for unbounded memory
   growth[^ashby]. Decoupling pruning from ingestion means the pruning
   decision is made on a schedule, not under write pressure.
4. **Learning is deliberate.** Reflection (generating new abstractions
   from accumulated memory) is the therapy skill's job — user-initiated,
   user-approved. An automatic promotion pipeline would edit memory
   without consent, which the sovereignty design forbids.

## The decay model: why Wozniak-Gorzelanczyk?

The forgetting curve `R(t) = exp(-t / S)`[^wg95] is used because:

1. **It's a single parameter.** `S` (memory life in days, default 180) is
   the only knob. No multi-exponential decay, no spaced-repetition
   scheduling — just a smooth exponential.
2. **It's well-validated.** SuperMemo (Wozniak's system) has decades of
   empirical data behind this curve.
3. **It's touchable.** `touch_recall` resets `recalled_at` to now, which
   resets `t` to 0, which resets `R` to 1.0. "Memory that gets used stays
   fresh" — the exact semantics we want. Only the h_mems that survive
   truncation are touched, so recall doesn't become a write storm
   (`memory.rs:852-867`).

The decay is applied at recall time (not at write time,
`memory_store.rs:458-467`), so the stored `confidence` is the original
value and the decayed value is computed on the fly. This means a memory's
effective confidence depends on when you ask, not when it was stored —
which is the right model for a memory that degrades with disuse.

## Related

- [Memory System Specification](../architecture/memory-system-specification.md) — the reference doc
- [Memory Ingest Sequence](../diagrams/sequence-memory-ingest.md) — the write-side diagram
- [Memory Recall Flow](../diagrams/flowchart-memory-recall.md) — the read-side diagram
- [Memory Store ERD](../diagrams/erd-memory-store.md) — the storage schema
- [hkask-memory README](../../crates/hkask-memory/README.md) — crate-level docs

[^vespa-hybrid]: Vespa. (2024). *Hybrid search — combining text and vector
    search*. https://docs.vespa.ai/en/hybrid-search.html. Reference
    implementation of the vector + lexical join pattern the memory store
    follows (one ANN index, one record store, joined by a stable key).

[^hybrid-retrieval]: Chen, D., et al. (2023). *Towards Understanding
    Hybrid Retrieval*. https://arxiv.org/abs/2305.15252. Evidence that
    dense and sparse retrieval legs have complementary failure modes —
    the rationale for running both legs and merging.

[^tetlock]: Tetlock, P., & Gardner, D. (2015). *Superforecasting: The Art
    and Science of Prediction*. Broadway Books. The dilution effect
    (irrelevant information weakens judgment) grounds the connectedness
    bonus cap; the ranking rationale is cited in-code at
    `kask/crates/kask_bridge/src/memory.rs:830-845`.

[^ashby]: Ashby, W. R. (1956). *An Introduction to Cybernetics*. Chapman &
    Hall. The Law of Requisite Variety: a regulator must be able to
    attenuate the variety it receives. The storage budget is the
    attenuator for unbounded memory growth.

[^wg95]: Wozniak, P. A., & Gorzelanczyk, E. J. (1995). *Two components of
    long-term memory*. Acta Neurobiologiae Experimentalis. Equation (3):
    R(t) = exp(-t/S). The decay implementation cites this at
    `kask/crates/hkask-memory/src/bayesian.rs:3-7`.
