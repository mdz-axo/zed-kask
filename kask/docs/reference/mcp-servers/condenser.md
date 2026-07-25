---
title: "Condenser MCP Server Reference"
audience: [developers, architects]
last_updated: 2026-07-24
version: "0.31.0"
status: "Active"
domain: "Composition"
mds_categories: [composition, lifecycle]
---

# Condenser MCP Server Reference

**Crate:** `mcp-servers/hkask-mcp-condenser` (MCP wrapper) + `crates/hkask-condenser` (pure domain)
**Tools:** 7 — `condenser_ping`, `condenser_compress`, `condenser_classify`, `condenser_set_profile`, `condenser_stats`, `condenser_persist`, `condenser_thread_summary`, `condenser_score_saliency`
**Auto-start:** Yes (one of the core servers auto-started at editor startup / agent panel initialization; not in `CORE_EXCLUDED`)

> **Hosting note (v0.31.0):** The deleted `hkask-services-chat` crate has been replaced by zed's
> in-process chat / agent panel (`crates/agent`, `crates/agent_ui`). The 2-phase `condense_history`
> flow is invoked from the in-process agent loop, not from a standalone chat service. The deleted
> REPL boot surface is replaced by editor startup / agent panel initialization.

## Pipeline Architecture (DIAG-RF-006)

The `CondenserServer` (thin MCP wrapper) delegates to `CondenserEngine` (pure domain logic), which dispatches to one of three compression algorithms based on the classified `ContextCategory`. The engine records each compression in a bounded history ring buffer; after 10+ observations per category, it auto-selects the best-performing algorithm (learning). The in-process agent loop's `condense_history` (in zed's `crates/agent`, replacing the deleted `hkask-services-chat`) uses two-phase condensation: CPU pre-compress (Phase 1) then LLM summarize (Phase 2).

```mermaid
flowchart TD
    Client["MCP Client\n(zed agent panel / external)"]
    
    subgraph Wrapper["hkask-mcp-condenser (thin wrapper)"]
        Server["CondenserServer\nMCP tool router"]
        Ping["condenser_ping\n+suggested_profile\n+history_stats"]
        Compress["condenser_compress\n+auto-select algorithm"]
        Classify["condenser_classify"]
        SetProfile["condenser_set_profile"]
        Stats["condenser_stats"]
        Persist["condenser_persist"]
        ThreadSummary["condenser_thread_summary"]
        ScoreSaliency["condenser_score_saliency"]
    end
    
    subgraph Domain["hkask-condenser (pure domain)"]
        Engine["CondenserEngine\nprofile + stats + history"]
        Registry["AlgorithmRegistry\nselect + select_by_name"]
        ClassifyFn["classify_tool\ntool_name to category"]
        AnchorFn["derive_ontology_anchor\ntool_name to OntologyAnchor"]
        SaliencyFn["domain_saliency\nline + anchor to f64"]
        SaliencyModule["saliency module\nscore_against_persona\nextract_query_words\nscore_memory_results\nword_frequencies shared"]
        OntologyGraph["OntologyGraph\nFIBO/CogAT/GOLEM/ML-Schema/OMC/PKO/DC+BIBO"]
        History["CompressionRecord ring buffer\n200 max observations"]
        Learning["recommend_algorithm\nsuggest_profile\ncompression_stats"]
    end
    
    subgraph Algos["Compression Algorithms"]
        Rtk["rtk_style\nhead/tail + density factor"]
        WordRank["word_rank\nTF-IDF + structural + saliency"]
        Flashrank["flashrank\ngreedy marginal utility"]
    end
    
    subgraph ChatSvc["crates/agent (in-process chat / agent panel)"]
        CondenseHistory["condense_history\n2-phase: CPU then LLM"]
        Phase1["Phase 1: CPU pre-compress\nCondenserEngine Heavy profile"]
        Phase2["Phase 2: LLM summarize\nInferencePort call"]
    end
    
    subgraph Infra["Infrastructure"]
        InferencePort["GuardedInferencePort\nover LanguageModelInferencePort\n(D4/D8 — not a centralized router)"]
        Episodic["EpisodicMemory\n(optional, SQLite-backed)"]
        Semantic["SemanticMemory\n(optional, SQLite + embeddings)"]
        EmbeddingStore["EmbeddingStore\n1024-dim KNN search"]
        Daemon["Daemon\nstore_experience\n(DEAD in zed-kask — always None;\nthread-level memory via RealMemoryPort D6 replaces it)"]
    end
    Client -->|"tool call"| Server
    Server --> Ping
    Server --> Compress
    Server --> Classify
    Server --> SetProfile
    Server --> Stats
    Server --> Persist
    Server --> ThreadSummary
    Server --> ScoreSaliency
    
    Ping --> Engine
    Compress --> Engine
    Classify --> Engine
    SetProfile --> Engine
    Stats --> Engine
    
    Engine --> Registry
    Engine --> ClassifyFn
    Engine --> AnchorFn
    Engine --> SaliencyFn
    Engine --> History
    Engine --> Learning
    Learning -->|"reads"| History
    SaliencyFn --> OntologyGraph
    
    Registry -->|"static default_for"| Rtk
    Registry -->|"static default_for"| WordRank
    Registry -->|"static default_for"| Flashrank
    Learning -->|"learned override"| Registry
    
    Rtk -->|"density_factor"| AnchorFn
    WordRank -->|"line_score"| SaliencyFn
    
    Persist --> Episodic
    ThreadSummary --> InferencePort
    ScoreSaliency -->|"against=persona"| SaliencyModule
    ScoreSaliency -->|"against=memory"| Semantic
    ScoreSaliency -->|"against=memory fallback"| Episodic
    ScoreSaliency -->|"score result count"| SaliencyModule
    Semantic --> EmbeddingStore
    
    Compress -.->|"record_experience\n(DEAD — daemon path is a no-op in zed-kask)"| Daemon
    ThreadSummary -.->|"record_experience\n(DEAD — daemon path is a no-op in zed-kask)"| Daemon
    
    CondenseHistory --> Phase1
    Phase1 -->|"CondenserEngine\nProfile::Heavy"| Engine
    Phase1 -->|"compressed text"| Phase2
    Phase2 --> InferencePort
    CondenseHistory -->|"format + estimate"| SaliencyModule
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-RF-006
verified_date: 2026-07-24
verified_against: mcp-servers/hkask-mcp-condenser/src/lib.rs (CondenserServer tool router), crates/hkask-condenser/src/engine.rs (CondenserEngine), crates/hkask-condenser/src/algorithms.rs (AlgorithmRegistry + 3 algorithms); condense_history 2-phase now invoked from zed's crates/agent (deleted hkask-services-chat); InferencePort node relabeled to GuardedInferencePort over LanguageModelInferencePort (D4/D8); Daemon node annotated as dead (always None in zed-kask; thread-level memory via RealMemoryPort D6)
status: VERIFIED (v3 — InferencePort relabeled to GuardedInferencePort/LanguageModelInferencePort; daemon edges annotated as dead no-ops)
-->

## Key paths

- **Compress:** `condenser_compress` → `CondenserEngine` → `AlgorithmRegistry::select` (auto-select after 10+ observations per category) → algorithm (`rtk_style` / `word_rank` / `flashrank`) → `CompressionRecord` appended to ring buffer (200 max)
- **Classify:** `condenser_classify` → `classify_tool` maps tool name → `ContextCategory`
- **Saliency:** `condenser_score_saliency` → `domain_saliency` (line + `OntologyAnchor`) → against persona / memory / memory-fallback
- **Auto-condense (in-process agent loop):** `condense_history` → Phase 1 (CPU pre-compress via `CondenserEngine` Heavy profile) → Phase 2 (LLM summarize via `InferencePort`)
- **Learning loop:** The daemon `record_experience` path is **dead in zed-kask** (the daemon is deleted; `DaemonClient` is always `None`). The call is a no-op. Thread-level memory is captured via `RealMemoryPort` (D6) — the in-process `MemoryPort` trait ingests thread turns via `cx.background_spawn()` at turn completion. `recommend_algorithm` / `suggest_profile` continue to read the in-process ring buffer (200 max observations) to override the static `default_for` selection; the ring buffer is the live learning substrate, not the daemon.

## Cross-links

- [MCP Server Registry](README.md) — all 11 on-disk MCP servers
- [Architecture Patterns](../../explanation/architecture-patterns.md) — MCP bootstrap and tool dispatch sequence
- [Zed Host Architecture Plan](../../architecture/zed-host-architecture-plan.md) — D1–D10 integration seams, essentialist split (hkask-services-chat deleted; chat owned by zed's `crates/agent`)
