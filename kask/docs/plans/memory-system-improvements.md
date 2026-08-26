# Memory System Improvements — Implementation Plan

Status doc for the memory system improvements, grounded in Dunning's self-knowledge/calibration research and Tetlock's superforecasting framework. RAG synthesis of the John Brooks corpus is in `rag-synthesis-dunning-memory-design.md`.

## Architecture (2026-08-25)

The memory system has been simplified to a single memory process and loop:

- **User/zed agent**: NO memory. The user is human and has their own memory. Context injection continues (system prompt, project rules), but no memory is generated or recalled for user threads. `ingest_turn` is a no-op for non-curator turns. `recall_context` and `recall_thread` return empty vecs.
- **Curator**: has memory (`curator.db`). Only curator turns are ingested. The curator recalls its own memory via `recall_context_curator` / `recall_thread_curator`.
- **Replicas**: static memory from corpus (built through the corpus server).
- **Swarm agents**: shared memory per swarm (as long as that swarm is maintained).

The legacy episodic/semantic distinction has been completely removed:
- `MemorySnippet.source` field removed (no more "episodic"/"semantic" labels).
- `HMemOntology::episodic()` → `HMemOntology::process()`, `HMemOntology::semantic()` → `HMemOntology::state()`, `to_semantic()` removed.
- `HMem::is_episodic()` removed.
- `SEMANTIC_PREDICATE` / `EPISODIC_PREDICATE` SQL constants removed — all h_mems are unified.
- `query_episodic_by_perspective` removed — use `query_by_perspective`.
- `*_semantic_*` query methods renamed to plain names (`count_semantic` → `count`, etc.).
- `promote_episodic_to_semantic` removed — consolidation is now confidence-based cleanup only (no promotion, no re-tagging).
- `store_consolidated` removed (dead code).
- User DB opening removed from `RealMemoryPort::new` — no user store to open.
- `fire_consolidation_pass` → `fire_curator_consolidation_pass` (curator-only, no user consolidation).
- Dead env-var parsers removed (`parse_storage_budget`, `resolve_storage_budget`, `parse_memory_life_days`, `resolve_memory_life_days`).

## Completed (2026-08-25)

- **Episodic/semantic distinction removed** — full removal across all crates. See Architecture above.
- **User memory store removed** — `RealMemoryPort` no longer holds a user `MemoryStore`. Only the curator store remains.
- **Q4 (G12)** — Log pre-login ingest no-ops. `crates/agent/src/thread.rs` now emits `log::warn!` when `memory_port()` returns `None`, naming the thread ID.
- **Q1 (convergence test)** — Mid-session ingest → next-turn recall test. `kask/crates/kask_bridge/src/context_injector.rs` now has `inject_context_recalls_mid_session_ingest` and `inject_context_returns_empty_when_no_match`.
- **Q2 (S6)** — Compact, labeled state block in curator context. `crates/agent/src/curator_agent_server.rs` now fetches a regulation health snapshot at `connect` time.

## Prioritized Implementation Plan

Seven changes, ordered by leverage and dependency. Priorities 1–4 are the recall ranking improvements (confidence + Brier + connectedness). Priorities 5–7 are the writable memory and therapy improvements. Q6 (swarm recall) is independent.

| Priority | Change | Effort | Depends on | Evidence (corpus entity_ref) |
|---|---|---|---|---|
| 1 | Confidence in recall ranking | Low (few lines) | None | Double curse `138299529:5`; Brier scoring `Superforecasting_tetlock:71` |
| 2 | Absence signaling (hypocognition guard) | Low (few lines) | None | Hypocognition `138299529:11`; experts attend to missing info `138299529:13` |
| 3 | Connectedness tracking (co-occurrence links) | Medium (new table) | None | Dilution effect `Superforecasting_tetlock:178`; A-MEM |
| 4 | Brier loop → memory confidence | High (bridge subsystems) | #3 | Feedback gap `Superforecasting_tetlock:273`; fuzzy thinking `Superforecasting_tetlock:274` |
| 5 | Curator memory edit tools | Medium (expose existing methods) | None | Cassandra quandary `138299529:16-17`; MemGPT |
| 6 | Therapy process (skill) | Medium (SKILL.md + templates) | #5 | Three dissonance strategies `Universal_Principles_of_Design:39`; no red teams `Superforecasting_tetlock:94` |
| 7 | Q3 reflection pass | Medium (extend consolidation timer) | #4, #5 | Fuzzy thinking `Superforecasting_tetlock:274`; informed practice `Superforecasting_tetlock:195` |

**Parallelism**: Priorities 1, 2, 3, and 5 have no dependencies and can proceed in parallel. Priority 5 should start early because 6 and 7 depend on it. Priority 4 is the long pole but can proceed in parallel with 5.

---

### Priority 1 — Confidence in recall ranking ✅ DONE

**Status**: Complete (2026-08-25).

**What**: The recall path sorted by `relevance_score` only (`memory.rs:1016-1021`). A memory with confidence 0.51 and one with confidence 0.99 were ranked identically if their embedding similarity was the same. Changed the sort key to `relevance_score × confidence`, using the already-decayed confidence value.

**Evidence**: Dunning's double curse (`138299529:5`) — the model can't self-evaluate, but confidence calibrated by outcomes IS a meaningful signal. Tetlock (`Superforecasting_tetlock:71`) — confidence is a forecast of relevance/truth. Throwing it away at ranking time discards the calibration signal.

**Anchors**:
- Sort: `kask/crates/kask_bridge/src/memory.rs:1026-1032` (changed from `relevance_score` to `relevance_score × confidence`)
- `MemorySnippet.confidence`: `kask/crates/hkask-types/src/ports/memory_port.rs:86`
- Decay applied at: `kask/crates/hkask-memory/src/memory_store.rs:258-280`
- Test: `recall_context_ranks_by_confidence_weighted_relevance` in `kask/crates/kask_bridge/src/memory.rs`

---

### Priority 2 — Absence signaling (hypocognition guard) ✅ DONE

**Status**: Complete (2026-08-25).

**What**: When recall returned zero results, the context injector returned an empty `Vec` (`context_injector.rs:283-285`) — the model got silence. Changed to inject a system message: "No relevant memory found for this query. This may indicate a knowledge gap."

**Evidence**: Dunning (`138299529:13`) — "people who are expert are better at attending to information that is missing... Blatantly pointing out to people that there is information they miss... prompts them to be less overconfident." Dunning (`138299529:11`) — hypocognition is "lacking a linguistic or cognitive representation." Silence is hypocognition.

**Anchors**:
- Zero-count path: `kask/crates/kask_bridge/src/context_injector.rs:283-308` (now injects an absence message instead of returning empty)
- Test: `inject_context_returns_empty_when_no_match` in `kask/crates/kask_bridge/src/context_injector.rs` (updated to expect absence message)

---

### Priority 3 — Connectedness tracking (co-occurrence links)

**Status**: Not started.

**What**: Add a co-occurrence link table: when two memories are recalled in the same `inject_context` call, record a link between them. Over time, frequently co-recalled memories become more connected. The link count becomes the `connectedness` term in the ranking: `relevance × confidence × connectedness`.

**Design decision**: Co-occurrence links (derived from recall behavior) rather than explicit semantic links (which would require LLM judgment — Dunning's double curse applies). Co-occurrence is structural and free.

**Evidence**: Tetlock's dilution effect (`Superforecasting_tetlock:178`) — irrelevant information weakens judgment. Connectedness is the structural guard: a memory with high connectedness has been tested against many contexts; one with low connectedness but high similarity is a dilution candidate.

**Anchors**:
- `HMem` struct (no link field): `kask/crates/hkask-storage/src/hmem.rs:41-59`
- Recall path (where co-occurrence is observed): `kask/crates/kask_bridge/src/context_injector.rs:216-300`
- Sort (where connectedness would be applied): `kask/crates/kask_bridge/src/memory.rs:1015-1021`

---

### Priority 4 — Brier loop → memory confidence

**Status**: Not started. Design analysis in `q3-q5-reflection-writable-memory.md`.

**What**: Bridge the scenarios widget's Brier scoring to memory confidence. When a forecast is resolved, find memories recalled in that context (via co-occurrence table from #3), and update their confidence via `combine_confidences` (`bayesian.rs:86-96`). Low Brier error → confidence increases; high Brier error → decreases.

**Evidence**: Tetlock (`Superforecasting_tetlock:273`) — "effective learning from experience can't happen without clear feedback, and you can't have clear feedback unless your forecasts are unambiguous and scorable." Dunning's feedback gap (Dunning 2011, pp. 264–265) — without feedback, incorrect self-assessments persist.

**Anchors**:
- Brier scoring (not yet wired to memory): `kask/crates/hkask-scenarios-widget/src/block.rs:12-16`, `view.rs:206-247`
- `combine_confidences` (Bayesian): `kask/crates/hkask-memory/src/bayesian.rs:86-96`
- `update_confidence`: `kask/crates/hkask-memory/src/memory_store.rs:599-615`

---

### Priority 5 — Curator memory edit tools ✅ DONE

**Status**: Complete (2026-08-25).

**What**: Added three MCP tools to the curator server: `memory_insert` (curator-only, requires evidence citation, confidence floor 0.5), `memory_update` (curator-only, Bayesian combine via `combine_confidences`), `memory_resolve_contradiction` (curator-only, strategies: expire / update_confidence / delete). Made `find_existing_by_eav`, `update_confidence`, `expire_h_mem`, and `combine_confidences` public so the curator MCP server (a separate crate) can access them.

**Correction**: An earlier version claimed `curator_directive` was "advertised but unimplemented." That was wrong — `curator_directive` exists as an agent tool (`crates/agent/src/tools/curator_tools.rs:626-713`), registered on curator threads (`crates/agent/src/agent.rs:891`). The gap was memory edit tools, not directive tools.

**Evidence**: Dunning's Cassandra quandary (`138299529:16-17`) — poor performers can't evaluate which memories are worth writing. MemGPT (Packer et al., 2023) — OS-style memory management with permission boundaries.

**Anchors**:
- New tools: `kask/mcp-servers/hkask-mcp-curator/src/hkask_mcp_curator.rs` (`memory_insert`, `memory_update`, `memory_resolve_contradiction`)
- New request types: `kask/mcp-servers/hkask-mcp-curator/src/types.rs` (`MemoryInsertRequest`, `MemoryUpdateRequest`, `MemoryResolveContradictionRequest`)
- Visibility changes: `find_existing_by_eav`, `update_confidence`, `expire_h_mem` changed from `pub(crate)` to `pub` in `kask/crates/hkask-memory/src/memory_store.rs`
- `combine_confidences` changed from `pub(crate)` to `pub` and re-exported from `kask/crates/hkask-memory/src/hkask_memory.rs`

---

### Priority 6 — Therapy process (skill) ✅ DONE

**Status**: Complete (2026-08-25). Skill created at `.agents/skills/therapy/SKILL.md` with templates at `kask/registry/templates/therapy/scan.j2`, `classify.j2`, `report.j2`.

**What**: A skill that runs therapy sessions on any memory database (curator, replica/corpus, or swarm). Two concrete goals: (1) memory hygiene — resolve contradictions, fragmentation, miscalibrated confidence; (2) reification — extract lessons from memory into skills/templates/rules, then purge or condense the source memories (cognitive load shedding).

**Grounding**: Cox & Shiffrin (2026, OECS) — memory traces can be altered once retrieved; distorted traces produce retrieval noise. Festinger — three dissonance resolution strategies. Dunning — double curse (agent can't self-evaluate, user judgment required). Goldfish principle (Vardy, 2020) — forgetting is a feature; once a lesson is reified into proactive guidance, the episodic memory can be shed.

**Phases**: Target selection → Scan (contradictions, fragmentation, miscalibrated confidence, reification candidates) → Classify and propose (Festinger strategies + reification proposals) → User review and approval → Execute (memory modifications + skill/template/rule creation + purge/condense) → Report.

**Key design decisions**:
- **Therapy on curator memory must run from a Curator agent panel session.** The curator must remember the act of therapy — the forgetting, the reification, the lessons learned. Therapy run from the zed agent modifies curator memory without the curator's awareness, breaking the cybernetic loop. Forgetting works as long as it is done with awareness and has a purpose.
- User approval required for all modifications — no autonomous memory modification.
- Two distinct processes: memory hygiene (forgetting — NOT learning) and reification (learning — closes the learning loop). Do not conflate.
- The learning loop: experience → memory → therapy (extract meaning + reify) → proactive guidance → new experience. Therapy is the extraction-and-reification step that closes the loop.
- Forgetting (purging/condensing source memories after reification) is a hygiene side-effect, not part of the learning loop.
- Reification proposals include the proposed skill/template/rule content for user review.
- Post-reification forgetting requires separate approval — user may reify but keep source memories.
- Applicable to curator memory (from curator panel), replica/corpus memory, and swarm memory.

---

### Priority 7 — Q3 reflection pass

**Status**: Not started. Design analysis in `q3-q5-reflection-writable-memory.md`.

**What**: Extend `start_consolidation_timer` to fire a reflection pass when contradiction density exceeds a threshold. The reflection prompt forces evidence citation (each insight cites specific h_mem IDs). Insights stored via `memory_insert` at confidence 0.5.

**Evidence**: Tetlock (`Superforecasting_tetlock:274`) — "fuzzy thinking can never be proven wrong." Tetlock (`Superforecasting_tetlock:195`) — "not all practice improves skill. It needs to be informed practice."

**Anchors**:
- Consolidation timer: `kask/crates/kask_bridge/src/memory.rs::start_consolidation_timer`
- `promote_episodic_to_semantic`: `consolidation_service.rs:196-280`
- Depends on: Priorities 4, 5

---

### Q6 (C7) — Swarm→bridge cross-DB recall

**Status**: Not started. Independent of priorities 1–7.

**What**: Add a cross-DB recall path so the curator's `inject_context` can recall from `swarm_memory.db` alongside `curator.db`.

**Evidence**: Dunning's naive realism (Ehrlinger & Dunning, 2003) — without swarm memory, the curator treats its own incomplete model as objective reality.

**Anchors**:
- `BridgeContextInjector::inject_context`: `kask/crates/kask_bridge/src/context_injector.rs:216-285`
- `swarm_memory.db`: owned by `hkask-mcp-swarm` process (D6)

---

## What the evidence says we should NOT do

- **Don't let the model self-assign confidence** — Dunning's double curse (`138299529:5`). Confidence must start at a floor (0.5) and be calibrated by outcomes.
- **Don't let user threads write to memory** — Dunning's Cassandra quandary (`138299529:16-17`). Only the curator (with its feedback loop) should write.
- **Don't do unstructured reflection** — Tetlock (`Superforecasting_tetlock:274`): "fuzzy thinking can never be proven wrong." Reflection must force evidence citation.
- **Don't silently swallow zero-result recall** — Dunning (`138299529:13`): experts attend to missing information. Silence is hypocognition.
- **Don't use "importance" as a ranking signal** — it's either LLM self-assessment (miscalibrates) or a heuristic (loses nuance). Confidence (outcome-calibrated) and connectedness (structural) replace it.
