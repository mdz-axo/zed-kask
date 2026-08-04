---
title: "Local Swarm Knowledge Tools — Design Rationale & Coding Guidelines"
audience: [architects, developers]
last_updated: 2026-08-03
version: "0.1.0"
status: "Active"
domain: "Swarm"
mds_categories: [composition, domain]
---

# Local Swarm Knowledge Tools — Design Rationale & Coding Guidelines

Local-mode analogs for the three ABW-only swarm authoring/knowledge tools, in
the kask vernacular: `swarm_search_knowledge_local`, `swarm_generate_prompt_local`,
`swarm_generate_ontology_local`. The analogs replace ABW's fermi-backed
per-agent dreaming-memory KG with the operator's `hkask-memory` (semantic +
episodic), and replace ABW's LLM generation with the local inference port —
executing and resolving entirely on the kask substrate (no ABW round-trips).

This rationale applies `essentialist` (eliminative 3-gate), `grill-me`
(assumption interrogation), `deep-module` (Ousterhout depth), and
`hypothesis-framer` (FINER + PICO) to the design, then states the coding
guidelines for the implementation.

## 1. The ABW shapes (logical-equivalence targets)

| ABW tool | Shape | Substrate |
|----------|-------|-----------|
| `swarm_search_knowledge({agent_name, query})` | GET `/agents/{id}/knowledge/search?q=` → matching knowledge fragments | fermi per-agent dreaming-memory KG (vector) |
| `swarm_generate_prompt({description, agent_name, agent_type?})` | POST `/agents/generate-prompt` → `{prompt, raw}` | fermi LLM authoring aid |
| `swarm_generate_ontology({domain_description})` | POST `/agents/generate-ontology` → seed Mermaid ER diagram | fermi LLM authoring aid |

All three are authoring/read aids (no spend, no consent). The local analogs
must return the same envelope shapes (`knowledge fragments`, `{prompt, raw}`,
Mermaid ER) so the `swarm-intelligence` skill and the Steer prompt can treat
them as drop-in replacements selected by `kask.swarm.mode`.

## 2. The kask substrate (the analogs)

| ABW concept | kask analog | Evidence |
|-------------|-------------|----------|
| per-agent dreaming-memory KG | the operator's `hkask-memory` `SemanticMemory`, scoped by an agent prefix (`agent:<agent_id>:`) | `semantic.rs:456` `search_similar` (vector KNN), `:396` `query_by_attribute`, `:502` `embeddings_by_prefix`, `:489` `entity_refs_by_prefix` |
| fermi LLM generation | the local `InferencePort` (Ollama/cloud via the zed IPC bridge) one-shot `generate` + `embed` | `inference_port.rs:164` `generate`, `:297` `embed`; `AgentExecutor` already holds the resolved inference port |
| ontology as a graph | semantic memory IS a graph (entity-attribute-value triples); render it as Mermaid ER via a one-shot LLM call seeded with the retrieved triples | `semantic.rs:396` `query_by_attribute` returns `HMem` triples |
| generate prompt | `prompt-enhance` skill's `agent-task` typed rewrite, inlined as a one-shot LLM call seeded with the description + retrieved memory | `prompt-enhance/SKILL.md` 7-type taxonomy, `agent-task` row |

The unifying idea — **memory is the knowledge graph** — is the kask
vernacular translation: ABW's "dreaming-memory KG" is the agent's
consolidated memory; in kask, consolidation (`ConsolidationBridge`,
`ConsolidationService`) already promotes episodic memories into semantic
triples. The local agent's "knowledge graph" is its prefix-scoped slice of
the operator's semantic memory.

## 3. Essentialist — eliminative 3-gate

**G1 EXIST (deletion test).** Delete each proposed tool; does complexity
reappear at callers?
- `swarm_search_knowledge_local`: the `swarm-intelligence` SENSE phase probes
  an agent's knowledge to assess variety coverage. Delete it → SENSE must call
  `SemanticMemory::search_similar` + embed + prefix-scope + envelope directly
  (complexity reappears). **PASS.**
- `swarm_generate_prompt_local`: the `author_agent` move needs a system prompt
  seeded with the agent's domain memory. Delete it → the cascade authors the
  prompt with an unseeded LLM call (quality lost, the memory-retrieval +
  prompt-template complexity reappears in the cascade). **PASS.**
- `swarm_generate_ontology_local`: the `author_agent` move optionally needs a
  seed ontology. Delete it → the cascade renders Mermaid ER from retrieved
  triples inline (the graph→ER rendering complexity reappears). **PASS.**

**G2 SURFACE (interface count).** Three tools, one function each. ≤7. Each
maps 1:1 to an ABW tool the skill/Steer prompt names separately — logical
equivalence requires the parallel surface. Could `generate_prompt` +
`generate_ontology` be one `generate_local({kind})`? Yes, but it would diverge
from the ABW surface the skill references; the cost of two thin functions is
lower than the cost of a divergent surface. **PASS** (3 public functions).

**G3 CONTRACT (abstraction trace).** Each tool is a wrapper over
(memory-retrieval + local-inference + sanitize + envelope). Are they
pass-throughs? No — each encapsulates: the agent-prefix scoping, the
embedding-model routing, the prompt template, the `{content, source, trust}`
sanitize envelope, and the error mapping (`SemanticMemoryError`/`InferenceError`
→ `McpToolError`, never leaked). Real behavior beyond a direct call. **PASS.**

Essentialist verdict: 3 tools, none eliminable. Score 0% (already minimal).

## 4. Grill-me — assumption interrogation

- **Recall:** "Is `hkask-memory` available in-process to the swarm server?" —
  Yes, as a crate dep (the server already depends on `hkask-inference`,
  `hkask-ledger`, `hkask-guard` in-process; `hkask-memory` is the same pattern).
- **Mechanism:** "How does the query become a vector?" — `InferencePort::embed`
  routes through the zed IPC bridge to the embedding model; the resulting
  vector feeds `SemanticMemory::search_similar`. No new embedding
  infrastructure — reuse the existing inference port.
- **Rationale:** "Why prefix-scope per agent rather than a per-agent store?" —
  `SemanticMemory` already supports prefix operations
  (`entity_refs_by_prefix`, `embeddings_by_prefix`, `purge_by_prefix`); a
  per-agent store would duplicate storage + consolidation. Prefix-scoping a
  shared store is the deep-module choice (one store, many namespaces).
- **Edge cases:**
  - "Memory unconfigured (no passphrase / no embedding model)?" — degrade
    gracefully: `search_knowledge_local` returns an empty result with a
    `memory_unconfigured` note (not a panic, not a fabricated result); the
    `generate_*` tools fall back to an unseeded LLM call (memory is an
    enhancement, not a dependency). This is the `.rules` "advertised invariants
    need enforcement points" + "unwrap_or(0)" trap avoided: a missing memory is
    signaled, not silently zero.
  - "Empty memory for this agent?" — return an empty fragment list (vacuously
    correct); `generate_*` proceed unseeded.
  - "Agent not in the local registry?" — `search_knowledge_local` requires the
    agent to exist (the prefix is derived from `agent_id`); return `not_found`
    if the card is missing.
- **Synthesis:** the design holds because it reuses three existing deep
  modules (`SemanticMemory`, `InferencePort`, `LocalAgentRegistry`) and adds
  only a thin, prefix-scoped tool surface — no new storage, no new
  consolidation, no fermi code.

## 5. Deep-module — Ousterhout depth

The new surface is three functions on the existing `SwarmServer` that delegate
to a new `LocalSwarmMemory` facet on `LocalSwarmRuntime`.

- **Core operation (one sentence):** "Retrieve an agent's prefix-scoped
  semantic memory and/or generate text seeded with it, via the local
  substrate."
- **Public surface:** 3 `#[tool]` functions (search/prompt/ontology) + the
  `LocalSwarmMemory` facet. ≤7. ✓
- **Information hiding:** the memory store path, the SQLCipher passphrase, the
  embedding model + dim, the prefix scheme (`agent:<agent_id>:`), and the
  prompt templates are all private to the module. Callers see only
  `{agent_name, query}` / `{description, agent_name, agent_type?}` /
  `{domain_description}` and the ABW-compatible envelopes.
- **One error enum:** map `SemanticMemoryError` + `InferenceError` +
  `EmbeddingGenerationError` → `SwarmError` variants (never leak dependency
  error types — the `.rules` "MCP tool error classification" rule).
- **One config struct:** extend `SwarmConfig` with
  `memory_db_path` / `memory_passphrase` / `embedding_model` /
  `embedding_dim`, validated at construction, with safe defaults +
  env-var overrides.
- **Depth score:** the behavior lines (prefix-scoping, embed+search, the two
  prompt templates, sanitize, envelope, error mapping, graceful degradation)
  far exceed the 3-function surface. Deep.

Dependency direction: `SwarmServer` → `LocalSwarmRuntime` →
(`SemanticMemory`, `InferencePort`) — acyclic, pointing toward the stable
hkask-* crates. ✓

## 6. Hypothesis-framer — FINER + PICO

**FINER:**
- **Feasible (9/10):** `hkask-memory` (semantic search + EAV + embeddings) and
  `InferencePort` (generate + embed) exist and are already used by the local
  substrate. The only new config is the memory DB path/passphrase + embedding
  model/dim.
- **Interesting (9/10):** gives local mode parity with ABW's
  knowledge/prompt/ontology features without fermi — closes the "local mode
  lacks cloud-catalogue features" gap from the prior audit.
- **Novel (8/10):** the framing "memory IS the knowledge graph" +
  prefix-scoped per-agent namespaces within the operator's consolidated memory
  is a kask-idiomatic restatement of ABW's per-agent dreaming KG.
- **Ethical (9/10):** the operator's sovereign memory stays on the kask
  substrate; prefix-scoping prevents cross-agent leakage; no data reaches
  ABW. The embedding/model calls go through the governed IPC bridge.
- **Relevant (10/10):** the `swarm-intelligence` SENSE (variety coverage probe)
  and `author_agent` move (prompt + ontology seeding) directly need these.

**PICO:**
- **Population:** local-mode swarms (`kask.swarm.mode = local`).
- **Intervention:** three local memory-backed tools
  (`swarm_search_knowledge_local`, `swarm_generate_prompt_local`,
  `swarm_generate_ontology_local`) over the operator's `hkask-memory` + local
  inference.
- **Comparison:** ABW's fermi-backed tools (the status quo for cloud mode) and
  "no local equivalent" (the current local-mode gap).
- **Outcome:** logical equivalence (same envelope shapes) + local execution
  (zero ABW round-trips) + no fermi dependency.

**H₁:** Local memory-backed analogs achieve logical equivalence with ABW's
fermi-backed knowledge/prompt/ontology tools while executing entirely on the
kask substrate, with the operator's prefix-scoped `SemanticMemory` serving as
the per-agent knowledge graph.

**H₀:** Local analogs cannot achieve equivalence without fermi — they require
ABW's dreaming-memory KG and cannot be backed by the operator's consolidated
memory.

**Testability:** (a) the three tools return the ABW-compatible envelopes
(`knowledge fragments`, `{prompt, raw}`, Mermaid ER); (b) they never call
`self.client` (the ABW client) — verified by grep + the no-ABW-leak property
test; (c) `tool_surface_is_exactly_50_registered_tools` pins the new count;
(d) `search_knowledge_local` on an empty/prefix-scoped memory returns an empty
list (not an error, not a fabricated hit) — the `unwrap_or(0)` trap avoided.

## 7. Coding guidelines (the build contract)

1. **Substrate reuse, not reimplementation.** Depend on `hkask-memory`
   in-process; open one `SemanticMemory` lazily alongside the ledger. Do NOT
   build a per-agent store or copy fermi code. Per-agent scoping is a prefix
   (`agent:<agent_id>:`) on the shared store.
2. **One local inference port, two uses.** Use the already-resolved
   `InferencePort` for one-shot `generate` (prompt + ontology) and `embed`
   (search vector). Do NOT add a second inference path. Expose a small
   `LocalSwarmRuntime::generate` / `::embed` helper rather than reaching into
   `AgentExecutor`.
3. **Graceful degradation, never silent zero.** If memory is unconfigured
   (open fails / no embedding model), `search_knowledge_local` returns
   `{fragments: [], note: "memory_unconfigured"}`; `generate_*` proceed
   unseeded. Never `unwrap_or(empty)` a fallible memory read without a
   `tracing::warn!` naming the failure (the `.rules` trap). The tools MUST NOT
   panic on a missing memory.
4. **Envelope parity.** Match the ABW response shapes so the skill is
   mode-agnostic: `search_knowledge_local` → `{fragments: [{entity, attribute,
   value, confidence}], source, note}`; `generate_prompt_local` → `{prompt,
   raw}` (route the generated text through `sanitize_abw_response`-equivalent —
   reuse `sanitize::sanitize_*`); `generate_ontology_local` → `{ontology,
   raw}` (Mermaid ER).
5. **No ABW leak.** The three tools use `self.local_runtime` / `self.config()`
   only — never `self.client` (the ABW `SwarmClient`). Add a property test
   asserting no local-knowledge tool references `self.client`.
6. **Guard the generated output.** `generate_*` route their LLM output through
   `AgentExecutor::scan_output` (canary/secret scan) — generated prompts and
   ontologies are LLM output and must not exfiltrate the system prompt or
   secrets. Reuse the existing guard; do not bypass.
7. **Error classification per variant.** Map `SemanticMemoryError` /
   `InferenceError` / `EmbeddingGenerationError` to `McpToolError` per variant
   (`not_found` for a missing agent card, `unavailable` for an unconfigured
   memory/inference, `internal` for a genuine store failure) — never a
   blanket `internal(format!("{e}"))` (the `.rules` rule).
8. **Config extension, single source of truth.** Add
   `memory_db_path` / `memory_passphrase` / `embedding_model` /
   `embedding_dim` to `SwarmConfig::default()` + `from_env`
   (`HKASK_SWARM_MEMORY_DB`, `HKASK_SWARM_MEMORY_PASSPHRASE`,
   `HKASK_SWARM_EMBEDDING_MODEL`, `HKASK_SWARM_EMBEDDING_DIM`); keep the
   `Default` impl the single source of truth and mirror in
   `kask_bridge::KaskSwarmSettings::default()` (the `.rules` settings rule).
9. **Skill wiring.** Add the three tools to the swarm-intelligence
   `swarm-patterns.yaml` `move_types` (author_agent local path), the Steer
   system prompt's local-tool list, the SKILL.md 47→50 surface, and the
   `tool_surface_is_exactly_50_registered_tools` test in the same commit.
10. **Tool-surface test is the gate.** The count goes 47 → 50; the test must
    list the three new names. No silent surface change.

## 8. Tool signatures (the contract)

```
swarm_search_knowledge_local({agent_name, query, limit?}) -> {fragments[], source, note}
swarm_generate_prompt_local({description, agent_name, agent_type?}) -> {prompt, raw}
swarm_generate_ontology_local({domain_description, agent_name?}) -> {ontology, raw}
```

`agent_name` scopes the memory prefix for search and seeds the prompt; the
ontology tool takes an optional `agent_name` (a domain ontology may be
agent-scoped or general). All three are read-only authoring aids — no ledger
debit, no consent, no ABW.

## 9. Cross-links

- [Swarm Cybernetics/Semantics Audit](../audits/swarm-cybernetics-semantics-audit.md)
  — the audit that identified the local-mode cloud-catalogue gap.
- [Swarm Systems Reference (47-tool surface)](../diataxis/swarm_system/reference.md)
  — will become 50 with these tools.
- `hkask-memory` API: `kask/crates/hkask-memory/src/semantic.rs`
  (`search_similar:456`, `query_by_attribute:396`, `embeddings_by_prefix:502`).
- ABW targets: `kask/mcp-servers/hkask-mcp-swarm/src/hkask_mcp_swarm.rs`
  (`swarm_generate_prompt:929`, `swarm_generate_ontology:974`,
  `swarm_search_knowledge:2802`).