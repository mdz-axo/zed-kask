# Q3 + Q5 — Reflection, Writable Memory & Therapy Process Design Analysis

Deep-dive investigation for the memory system improvements. These are design analyses, not implementation plans — they identify the infrastructure, constraints, and design decisions before any code changes.

**Revision (2026-08-25)**: Replaced the "importance weighting" concept (Q1) with the correct thesis: **confidence + Brier-scored calibration + graph connectedness** as the recall salience signal. "Importance" is not a measurable property of a memory; confidence (tracked against outcomes via Brier scoring) and connectedness (how many other memories reference this one) are. Both are free — they are part of the memory architecture and must be used in the recall method, not gated behind a separate feature.

## Grounding

### Dunning frameworks (verified citations)

The memory system's calibration thesis rests on Dunning's work on self-knowledge and metacognition, combined with Tetlock's superforecasting. The load-bearing concepts, with primary citations:

- **The double curse / dual-burden** (Kruger & Dunning, 1999, "Unskilled and Unaware of It," *JPSP* 77(6), 1121–1134; reviewed in Dunning, 2011, "The Dunning–Kruger Effect: On Being Ignorant of One's Own Ignorance," *Advances in Experimental Social Psychology* 44, 247–296): the same skills needed to produce correct answers are needed to evaluate whether answers are correct. Incompetence both produces errors AND prevents recognizing them. **Implication for memory**: a model that writes its own confidence scores will miscalibrate — the act of writing confidence requires the metacognitive skill the model lacks. Confidence must be calibrated by external outcomes, not self-assessment.

- **Insensitivity to evidence in low performers** (Jansen, Rafferty & Griffiths, 2021, "A rational model of the Dunning–Kruger effect supports insensitivity to evidence in low performers," *Nature Human Behaviour* 5(6), 756–757): a rational/Bayesian model shows low performers do not update on disconfirming evidence — they are insensitive to it. **Implication**: confidence cannot be revised by asking the model to reconsider; it must be revised by wiring external outcome signals (Brier scoring against resolved forecasts/actions) into the confidence update path.

- **"Make everybody better performers"** (Dunning & Helzer, 2014, "Beyond the Correlation Coefficient in Studies of Self-Assessment Accuracy," *Perspectives on Psychological Science* 9(2), 126–130): the best way to improve self-accuracy is to improve the underlying capability, not the metacognition. **Implication**: confidence calibration is a downstream effect of capability improvement. The memory system should not try to fix confidence directly — it should fix the underlying recall quality (better retrieval, better consolidation) and let confidence track that.

- **The feedback gap** (Dunning, 2011, pp. 264–265; Ehrlinger & Dunning, 2003, "How chronic self-views influence (and potentially mislead) estimates of performance," *JPSP* 84(1), 5–17): without feedback, incorrect self-assessments persist. Domains with poor feedback produce the most overconfidence. **Implication**: a memory system with no outcome feedback loop will accumulate miscalibrated confidence. The Brier-scoring path is the feedback loop.

- **Naive realism / chronic self-views** (Ehrlinger & Dunning, 2003): people assume they see the world objectively and treat their own model as ground truth. **Implication**: the curator (and any agent) will treat its own recalled context as objective reality unless the recall path labels provenance and flags contradictions.

- **Planning vs execution phase** (Dunning, 2011, pp. 264–265): overconfidence is beneficial in execution (motivation, energy) but detrimental in planning (ignoring bad odds, failing to prepare contingencies). **Implication**: high confidence is fine for *acting* on a recalled memory, but the recall *ranking* needs calibrated confidence — overconfident recall suppresses better alternatives.

> **Inference-tier note**: The existing docs attributed "structured vs. unstructured reflection" to Dunning's *Self-Insight* (2005, Psychology Press). The book exists, but its exact chapter structure could not be verified via available search (arXiv returns no psychology texts; the book is not indexed there). The underlying principle — that forced evidence-grounding debiases self-assessment — is consistent with the verified dual-burden account (Kruger & Dunning 1999; Dunning 2011): if you cannot evaluate your own output, forcing external evidence citation is a debiasing mechanism. Treat the specific "structured reflection" chapter attribution as Inference; the principle itself is grounded.

### Tetlock's superforecasting framework

- **Brier scoring** (Brier, 1950; Tetlock & Gardner, 2015, *Superforecasting: The Art and Science of Prediction*): the proper scoring rule for probabilistic forecasts. Mean squared error between predicted probability and binary outcome. **Implication**: confidence in a memory should be treated as a forecast — "this memory is relevant/true with probability p" — and scored against outcomes. The existing Brier infrastructure in `hkask-scenarios-widget` (`block.rs:12-16`, `view.rs:206-247`) is the reference implementation; it must be wired into the memory confidence calibration path.

- **Dragonfly-eye synthesis** (Tetlock & Gardner, 2015): combine outside-view (base rates) and inside-view (specifics) perspectives. **Implication**: recall should combine embedding similarity (inside view — how similar is this memory to the query) with confidence/connectedness (outside view — how well-calibrated and well-connected is this memory).

### LLM-specific evidence (verified)

- **Koch (2026)**, "Beyond the Steeper Curve: AI-Mediated Metacognitive Decoupling" (arXiv:2603.29681) — LLM use improves observable output while degrading metacognitive accuracy. The gap between produced output and underlying understanding widens. Verified.
- **Ghosh & Panday (2026)**, "The Dunning-Kruger Effect in Large Language Models" (arXiv:2603.09985) — empirical: poorly performing LLMs display markedly higher overconfidence (ECE 0.726 at 23.3% accuracy vs. ECE 0.122 at 75.4% accuracy). Verified.

### AI memory system references

- **Generative Agents** (Park et al., 2023, arXiv:2304.03442) — observation → reflection → retrieval pipeline. Reflections are higher-level abstractions generated periodically. Retrieval combines recency + importance + relevance. Ablation shows reflection is critical. **Note**: their "importance" is an LLM-assigned score (1–10) per observation — this is the model self-assessing importance, which Dunning's dual-burden predicts will miscalibrate. zed-kask replaces this with confidence (outcome-calibrated) + connectedness (structural).
- **MemGPT / Letta** (Packer et al., 2023, arXiv:2310.08560) — OS-style hierarchical memory. The LLM self-manages memory via function calls (`insert`/`search`/`replace`). Memory is editable with permission boundaries. **Note**: MemGPT lets the model self-edit memory — zed-kask restricts this to the curator (the one agent with a feedback loop), following Dunning's principle that only the agent with calibrated feedback should write.
- **A-MEM** (agentic memory with self-organizing notes) — memories link to each other and reorganize based on semantic relations. This is the graph-connectedness reference: salience increases with link density.
- **Mem0** (production memory layer) — layered memory with explicit consolidation and contradiction resolution passes.
- **Gallego (2026)**, "Distilling Feedback into Memory-as-a-Tool" (arXiv:2601.05960) — feedback is distilled into memory through a structured process; the model proposes a memory entry, filtered through a rubric before storage. Verified.

---

## The thesis: confidence + Brier + connectedness (replacing "importance weighting")

The original Q1 proposed "importance weighting" — score each memory 1–10 at ingest, combine `relevance × importance × recency` at retrieval. This is wrong for three reasons:

1. **"Importance" is not measurable.** It is either an LLM self-assessment (which Dunning's dual-burden predicts will miscalibrate — Ghosh & Panday 2026 confirm this empirically in LLMs) or a heuristic (which loses semantic nuance). There is no ground truth for "importance."

2. **Confidence IS measurable** — when treated as a forecast and scored against outcomes via Brier scoring. A memory that claims "X is true with p=0.8" and X turns out true has low Brier error; one that claims p=0.8 and X is false has high Brier error. This is the calibration signal Dunning's framework requires: external outcome feedback, not self-assessment.

3. **Connectedness IS measurable** — structurally. A memory referenced by (linked to) many other memories is more salient. This is A-MEM's self-organizing principle and the graph-theoretic analog of Tulving's semantic network density. It requires no LLM judgment.

### What the codebase already has

| Component | Exists? | Where | Used in recall? |
|-----------|---------|-------|-----------------|
| `Confidence` struct | Yes | `hkask-types/src/visibility.rs:141-145` | **As threshold filter only** (`context_injector.rs:240,266`) — NOT as ranking signal |
| `memory_decay` (Wozniak-Gorzelanczyk) | Yes | `visibility.rs:198-202` | Yes — applied at recall (`memory_store.rs:258-280`) |
| `combine_confidences` (Bayesian log-odds) | Yes | `hkask-memory/src/bayesian.rs:86-96` | Only in consolidation (`consolidation_service.rs`) — NOT in recall |
| Brier scoring | Yes | `hkask-scenarios-widget/block.rs:12-16`, `view.rs:206-247` | **NOT wired into memory** — exists only in the scenarios widget |
| Graph connectedness | **No** | — | Not tracked, not used |
| `update_confidence` | Yes | `memory_store.rs:599-615` | Only called by consolidation — NOT by outcome feedback |

### What's missing (the gap)

1. **Confidence as a ranking signal, not just a filter.** The recall path (`context_injector.rs:238-267`) filters by `confidence >= min_confidence` then sorts by `relevance_score` only (`memory.rs:1014-1018`). Confidence should be a ranking multiplier: `score = relevance × decayed_confidence × connectedness`.

2. **Brier-scored confidence calibration.** Confidence is set once (at ingest, default 1.0 via `Confidence::full()`) and only changes via Bayesian combination during consolidation or decay over time. There is no outcome feedback loop — nothing checks whether a recalled memory was *correct* and updates confidence accordingly. The Brier infrastructure exists in the scenarios widget but is not wired to memory.

3. **Graph connectedness tracking.** No entity-link, relation, or edge tracking exists in `hkask-memory` (grep for `connectedness|graph|entity_link|relation|edge|neighbor` returns zero matches). The `HMem` struct (`hkask-storage/src/hmem.rs:41-59`) stores EAV triples but no links between them. Connectedness must be derived or stored.

### Design: the recall ranking function

```
recall_score(memory, query) =
    relevance(memory, query)          // embedding cosine similarity (existing)
  × decayed_confidence(memory)        // Wozniak-Gorzelanczyk decay (existing)
  × connectedness(memory)             // graph link density (NEW)
```

- **No "importance" term.** Confidence (outcome-calibrated) replaces it.
- **Confidence is free** — it's already in the struct, already decayed at recall. The change is using it as a ranking multiplier instead of only a threshold.
- **Connectedness is free** — it's a structural property of the memory graph. The change is tracking it (a link table or derived count) and using it in recall.
- **Brier scoring is the calibration loop** — not in the recall function itself, but in the background process that updates confidence based on outcomes. This is the feedback gap closure.

---

## Q3 — Reflection / Consolidation Pass

### What it is

A background process that periodically abstracts over episodic memories (individual turns) to generate higher-level semantic insights (generalized patterns, lessons learned). The output is stored as new semantic h_mems, automatically retrievable by `recall_context`.

### Existing infrastructure

- **`MemoryConsolidator`** (`kask/crates/hkask-memory/src/consolidation_service.rs:47-49`): holds an `Arc<MemoryStore>`, runs `consolidate()`.
- **`fire_consolidation_pass`** (`kask/crates/kask_bridge/src/memory.rs:619-670`): the shared function called by both the test `maybe_consolidate` and the production `start_consolidation_timer`.
- **`MemoryConsolidator::consolidate`** (`consolidation_service.rs:65-194`): three phases — (1) promote episodic to semantic, (2) delete low-confidence shared h_mems, (3) prune to storage budget.
- **`promote_episodic_to_semantic`** (`consolidation_service.rs:196-280`): re-tags ontology, sets visibility to Shared, checks for existing EAV match (Bayesian combine if match, insert if no match), expires source h_mem.

### What's missing vs. Generative Agents

The existing consolidation is **syntactic** — it promotes episodic h_mems to semantic by re-tagging the ontology blob. It does NOT generate new abstractions. Generative Agents' reflection step is **semantic** — it prompts the LLM with recent observations and asks for higher-level insights, each a NEW memory.

### Design

**Trigger**: when the count of episodic h_mems with divergent confidence (memories that disagree with each other on the same EAV) exceeds a threshold. This replaces the original "importance threshold" — the trigger is now contradiction density, which is the signal that reflection is needed (the therapy process below operates on the same signal).

**Process**:
1. The background timer (existing `start_consolidation_timer`) fires a reflection pass when the contradiction threshold is met.
2. The reflection pass prompts the model with the contradicting episodic memories and asks for a synthesized semantic insight.
3. Each insight MUST cite specific observation h_mem IDs (evidence-grounding — the debiasing mechanism consistent with Dunning's dual-burden account).
4. Each insight is stored as a semantic h_mem with confidence 0.5 (the floor — NOT the model's self-assessed confidence) via `MemoryStore::store_consolidated`.

**Authority**: the reflection pass writes to the semantic store only (not episodic). Following Generative Agents: the model can only add, not delete or overwrite.

**Cost concern**: the reflection pass requires an LLM call per pass. Mitigations: gate on the contradiction threshold; use a lightweight model; cap insights per pass (3, following Generative Agents).

---

## Q5 — Curator-Writable Memory

### What it is

New MCP tools (`memory_insert`, `memory_update`, `memory_resolve_contradiction`) that allow the curator agent to write to and edit its own memory, with evidence-grounding and confidence-floor constraints. **User threads cannot write to memory directly** — only the curator (the one agent with a feedback loop).

### Existing infrastructure

The curator MCP server (`kask/mcp-servers/hkask-mcp-curator/src/hkask_mcp_curator.rs`) has:
- `curator_semantic_search` — read semantic memory by entity (`:399-403`)
- `curator_memory_recall` — read episodic + semantic by entity or ontology axis (`:438-442`)
- `curator_consult` — read both by query (`:570-574`)
- `curator_algedonic_log` — read algedonic events (`:651-655`)
- `curator_report_skill_use_issue` — **constrained write**: stores a skill-use issue as an episodic h_mem (`:766-810`, calls `MemoryStore::store` at `:796`)

The last is the only existing write path. There is no general `memory_insert` / `memory_update` / `memory_delete`.

> **Broken feedback loop found**: the system prompt's "Curator Role" section advertises a `curator_directive` tool (with an `evolve_mcp_tool_schema` variant) for issuing directives and evolving MCP schemas. **This tool does not exist anywhere in the codebase** (grep for `curator_directive|CuratorDirective` across `crates/` and `kask/` returns zero matches). The curator is told it can issue directives but has no tool to do so. This must be resolved — either implement the tool or remove the claim from the system prompt.

### What's missing vs. MemGPT

MemGPT (Packer et al., 2023) allows model-writable memory with OS-style guardrails: `insert`, `search`, `replace`, with permission boundaries. zed-kask has `curator_report_skill_use_issue` (a constrained write) but no general write/edit tools.

### Design

**Authority: curator-only.** Only the curator agent gets `memory_insert` / `memory_update` / `memory_resolve_contradiction` MCP tools. User threads cannot write to memory directly — they can only ingest (automatic, via `inject_context`'s recall path). This follows MemGPT's permission-boundary principle and Dunning's dual-burden: only the agent with calibrated feedback (the curator, which has `curator_status`, regulation loop feedback, and Brier scoring) should write to memory.

**Evidence-grounding requirement**: every `memory_insert` must cite a specific observation (episodic h_mem ID or turn ID). The tool rejects inserts without a citation. This is the debiasing mechanism consistent with Dunning's dual-burden: the model must ground its assertion in evidence, not free-associate. Following Gallego (2026): the model proposes, a rubric filters.

**Confidence floor**: inserted memories start at confidence 0.5 (not the model's self-assessed confidence). Confidence is reinforced by subsequent Brier-scored outcomes — if a memory is recalled and the action it informed succeeds, confidence increases (Bayesian combine); if it fails, confidence decreases. This is the calibration mechanism Dunning's framework requires: confidence calibrated by external outcomes, not self-assessment.

**No deletion by the model**: the model cannot delete or overwrite memories (following Generative Agents). It can only add. Contradictory facts coexist with different confidence scores — the recall function surfaces the higher-confidence one. The therapy process (below) is the exception: it resolves contradictions by operator-approved curator action.

### Confidence calibration mechanism (the Brier loop)

The existing Bayesian combination function (`hkask-memory/src/bayesian.rs:86-96`) combines two confidences in log-odds space. The Brier scoring infrastructure (`hkask-scenarios-widget`) scores forecast accuracy. The calibration loop:

1. `memory_insert` → confidence starts at 0.5
2. Memory is recalled by `recall_context` → `touch_recall` resets the decay clock
3. If the model acts on the recalled memory and the outcome is observed → Brier-score the memory's implicit forecast
4. `memory_update` adjusts confidence: low Brier error (memory was right) → Bayesian combine increases confidence; high Brier error (memory was wrong) → decreases confidence
5. If the memory is never recalled → confidence decays via `memory_decay` (existing)

This is the external calibration loop Dunning's framework requires: confidence is calibrated by outcomes (did the memory help?), not by self-assessment (am I confident?).

### What exists vs. what's needed

| Component | Exists? | Where | What's needed |
|-----------|---------|-------|---------------|
| Constrained write (skill-use issues) | Yes | `hkask_mcp_curator.rs:766-810` | Generalize to `memory_insert` |
| Read (semantic, episodic, consult) | Yes | `hkask_mcp_curator.rs:399-574` | No change |
| Bayesian confidence combination | Yes | `hkask-memory/src/bayesian.rs:86-96` | Use for confidence reinforcement |
| Brier scoring | Yes | `hkask-scenarios-widget/block.rs:12-16` | Wire into memory confidence calibration |
| Confidence decay | Yes | `hkask-types/src/visibility.rs:198-202` | Already used in recall |
| `MemoryStore::store` | Yes | `memory_store.rs:196-212` | Use for `memory_insert` |
| `MemoryStore::update_confidence` | Yes | `memory_store.rs:599-615` | Expose via `memory_update` MCP tool |
| `MemoryStore::expire_h_mem` / `delete_h_mem` | Yes | `memory_store.rs:655-688` | Expose via `memory_resolve_contradiction` (curator-only) |
| Evidence-grounding filter | **No** | — | New: reject inserts without episodic citation |
| Confidence floor (start at 0.5) | **No** | — | New: inserted memories start at 0.5 |
| Permission boundary (curator-only) | **No** | — | New: MCP tools registered on curator threads only |
| `curator_directive` tool | **No** (advertised in system prompt but unimplemented) | — | Either implement or remove the claim |

---

## The Therapy / Dreaming Process (NEW)

### What it is

A structured process the curator runs (operator-initiated or scheduled) to resolve cognitive dissonance and contradictions in the memory database. This is the "dreaming" process referenced in other AI systems (e.g., A-MEM's self-organizing reorganization, Mem0's contradiction resolution passes). It is distinct from Q3's reflection (which generates new abstractions) — therapy resolves contradictions in *existing* memories.

### Why it's needed

Dunning's naive realism (Ehrlinger & Dunning, 2003) predicts: the curator will treat its own recalled context as objective reality. If the memory store contains contradictory facts (e.g., "X is reliable with p=0.8" and "X failed with p=0.9"), the recall path surfaces whichever has higher decayed confidence — but both persist, and the contradiction is invisible to the operator. Over time, the store accumulates unresolved dissonance that degrades recall quality.

The therapy process makes contradictions explicit, resolves them (by confidence adjustment, expiration, or synthesis), and optimizes the graph (re-linking after resolution).

### Design

**Authority: curator-only, operator-approved.** The curator identifies contradictions and proposes resolutions. Resolutions that modify or delete existing memories require operator approval (the curator proposes; the operator approves via the curator panel). This is stricter than Q5's `memory_insert` (which is curator-autonomous within the evidence-grounding constraint) because therapy modifies the *existing* record, not just adds to it.

**Why operator approval for modifications**: Dunning's dual-burden applies to the curator too — it can miscalibrate which memory is the "correct" one in a contradiction. Operator approval is the external check. (Pure additions via `memory_insert` don't need this because they don't destroy information.)

**Process** (a skill or template the curator runs):

1. **Scan** — query the memory store for contradictions: h_mems with the same EAV (entity, attribute) but divergent values or confidence. The existing `find_existing_by_eav` (`memory_store.rs:566-597`) detects EAV matches during consolidation; the therapy process reuses this logic as a scan.

2. **Classify** — for each contradiction, classify:
   - **Temporal**: one is newer and supersedes the older (resolve by expiring the older).
   - **Confidence divergence**: both are current but one has higher Brier-calibrated confidence (resolve by Bayesian combine or expire the lower).
   - **Genuine ambiguity**: both are plausible and the contradiction reflects real-world uncertainty (resolve by keeping both, lowering confidence on each, and linking them as alternatives).

3. **Propose** — the curator generates a resolution proposal per contradiction, citing the h_mem IDs involved and the classification. This is the evidence-grounding constraint.

4. **Operator review** — the operator approves, modifies, or rejects each proposal via the curator panel. Approved resolutions execute via `memory_resolve_contradiction` (curator-only MCP tool that calls `expire_h_mem` / `update_confidence` / `delete_h_mem`).

5. **Re-link** — after resolution, update the graph connectedness: re-link memories that referenced the resolved h_mems, recompute link density. This is the graph optimization step (A-MEM's self-organizing principle).

**Output**: a cleaner memory store with resolved contradictions, recalibrated confidence, and an updated graph. The recall path benefits automatically (higher-quality ranking).

### What exists vs. what's needed

| Component | Exists? | Where | What's needed |
|-----------|---------|-------|---------------|
| EAV match detection | Yes | `memory_store.rs:566-597` (`find_existing_by_eav`) | Reuse as a scan (query all, not just per-insert) |
| `expire_h_mem` | Yes | `memory_store.rs:655-663` | Expose via `memory_resolve_contradiction` |
| `delete_h_mem` | Yes | `memory_store.rs:685-688` | Expose via `memory_resolve_contradiction` |
| `update_confidence` | Yes | `memory_store.rs:599-615` | Expose via `memory_resolve_contradiction` |
| Contradiction scan (all EAVs) | **No** | — | New: batch query for divergent EAVs |
| Graph connectedness tracking | **No** | — | New: link table or derived count |
| Re-linking after resolution | **No** | — | New: graph update pass |
| Operator approval UI | **No** | — | New: curator panel surface for therapy proposals |
| Therapy skill/template | **No** | — | New: SKILL.md + .j2 templates |

### Relationship to Q3 and Q5

- **Q5** provides the write tools (`memory_insert`, `memory_update`, `memory_resolve_contradiction`) that therapy uses.
- **Q3** (reflection) generates new semantic memories from episodic ones; therapy resolves contradictions in *existing* memories. They are complementary: reflection adds, therapy cleans.
- **The Brier loop** (Q5's confidence calibration) feeds therapy: Brier-scored confidence is the signal that classifies contradictions (a memory with low Brier-calibrated confidence is the candidate for expiration).

---

## Q3 ↔ Q5 ↔ Therapy dependency

- **Q5** (write path + Brier loop) is the foundation — it provides the tools and the confidence calibration that the others depend on.
- **Therapy** depends on Q5's `memory_resolve_contradiction` tool and the Brier-calibrated confidence.
- **Q3** (reflection) depends on Q5's `memory_insert` (the storage path for new insights) and benefits from therapy (cleaner store → better reflection input).

**Recommended sequence**: Q5 (write path + Brier loop) → Therapy (contradiction resolution) → Q3 (reflection). The Brier loop in Q5 is the long pole — it requires outcome observation wiring, which is the hardest piece.

---

## Open questions for implementation

1. **Brier outcome observation**: how does the system know whether a recalled memory was "acted on successfully"? Options: (a) the model explicitly calls `memory_update` after acting (requires model cooperation — Dunning predicts this is unreliable), (b) the system infers success from the next turn's content (requires semantic analysis), (c) the operator manually calibrates (requires UI), (d) wire it to the scenarios widget's existing Brier infrastructure (requires a bridge from forecast resolution to memory confidence). Option (d) is the most grounded — the infrastructure exists.

2. **Graph connectedness representation**: a separate link table (entity_ref → entity_ref edges) or a derived count (compute link density on recall from existing EAV overlaps)? A link table is explicit and fast; a derived count is zero-storage but O(n) per recall. A-MEM uses explicit links.

3. **Therapy trigger**: operator-initiated (manual), scheduled (timer), or signal-triggered (when contradiction count exceeds a threshold)? Signal-triggered is most cybernetic (it closes the loop automatically) but risks running without operator oversight; operator-initiated is safest but relies on the operator noticing.

4. **`curator_directive` resolution**: implement the advertised tool (which would give the curator a directive-issuing + MCP-schema-evolution surface) or remove the claim from the system prompt? This is a separate decision but blocks the curator's ability to self-adjust thresholds.
