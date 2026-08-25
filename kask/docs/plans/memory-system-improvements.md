# Memory System Improvements — To-Do List

Status doc for the two deferred improvements from the context/memory injection refactor (2026-08-25).

## Completed (2026-08-25)

- **Q4 (G12)** — Log pre-login ingest no-ops. `crates/agent/src/thread.rs` now emits `log::warn!` when `memory_port()` returns `None`, naming the thread ID. Closes the feedback gap (Dunning, *Self-Insight*, 2005).
- **Q1 (convergence test)** — Mid-session ingest → next-turn recall test. `kask/crates/kask_bridge/src/context_injector.rs` now has `inject_context_recalls_mid_session_ingest` and `inject_context_returns_empty_when_no_match`. Pins the per-turn freshness property.
- **Q2 (S6)** — Compact, labeled state block in curator context. `crates/agent/src/curator_agent_server.rs` now fetches a regulation health snapshot at `connect` time and appends it to the curator context with an explicit "snapshot at session start — pull curator_status for live updates" label. Breaks the naive-realist trap.

## Pending

### Q1 — Importance weighting at retrieval

**Status**: Not started.

**What**: Score each memory at ingest time with an importance score (1-10). At retrieval, combine: `score = relevance × importance × recency_decay`.

**Why**: Generative Agents (Park et al., 2023, arXiv:2304.03442) uses a three-dimensional retrieval function (recency × importance × relevance). zed-kask's `recall_context` uses only relevance (embedding similarity). Without importance weighting, a high-importance memory from 50 turns ago is ranked the same as a low-importance memory from 50 turns ago (if they have similar embedding similarity).

**Design questions**:
- How to generate the importance score at ingest time? Options: (a) a lightweight LLM call per turn (cost), (b) a heuristic based on turn content (code changes > chat > tool results), (c) operator-configurable per-skill importance.
- Where to store the score? The `HMem` struct (`hkask-storage/src/hmem.rs:41-59`) has a `confidence: Confidence` field but no `importance` field. Adding one is a schema change.
- How to apply `recency_decay`? The existing `Confidence::memory_decay` (`hkask-types/src/visibility.rs:198-202`) decays confidence by time-since-recall. The same function can be applied to importance.

**Dependencies**: None (can be done independently). This is a prerequisite for Q3 (reflection pass), which needs importance scores to trigger reflection.

**Anchors**:
- `recall_context`: `kask/crates/kask_bridge/src/memory.rs` (the `RealMemoryPort::recall_context` method)
- `HMem` struct: `kask/crates/hkask-storage/src/hmem.rs:41-59`
- `Confidence::memory_decay`: `kask/crates/hkask-types/src/visibility.rs:198-202`
- Reference: Generative Agents (Park et al., 2023, arXiv:2304.03442) — three-dimensional retrieval

---

### Q6 (C7) — Swarm→bridge cross-DB recall

**Status**: Not started.

**What**: Add a cross-DB recall path so the curator's `inject_context` can recall from `swarm_memory.db` alongside `curator.db`.

**Why**: Swarm turns ingest to `swarm_memory.db` (D6), retrievable only via `swarm_recall_local` MCP tool. The curator regulating swarm health has no automatic recall of swarm history — it must pull manually. Dunning's naive realism (Dunning, *Self-Insight*, 2005) predicts: without swarm memory, the curator assumes it knows what's happening in the swarm and treats its own (incomplete) model as objective reality.

**Design questions**:
- Is concurrent read access to `swarm_memory.db` safe with SQLCipher? The swarm MCP server process owns the DB; the bridge would need read access. Need to verify SQLCipher supports concurrent readers across processes.
- Should this be curator-only or also available to user threads? The resolution says curator-only (the curator is the swarm's regulator; user threads don't need swarm memory).
- How to label the recalled fragments? The resolution says: `--- Swarm Memory (data from delegated agents) ---` so the model treats them as external observations, not its own direct experience.

**Dependencies**: None (can be done independently). The cross-DB query is the main implementation question.

**Anchors**:
- `BridgeContextInjector::inject_context`: `kask/crates/kask_bridge/src/context_injector.rs:216-285`
- `swarm_memory.db`: owned by `hkask-mcp-swarm` process (D6)
- `local_knowledge::ingest_turn` / `recall_turns`: `kask/mcp-servers/hkask-mcp-swarm/src/local_knowledge.rs`
- Reference: Generative Agents (Park et al., 2023) — agents observe each other and incorporate those observations into their own memory

---

## Sequencing

Q1 (importance weighting) should be done before Q3 (reflection pass), because the reflection trigger depends on importance scores. Q6 (swarm recall) is independent and can be done in parallel with either.
