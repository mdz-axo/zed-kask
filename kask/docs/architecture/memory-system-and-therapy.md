# Memory System Architecture & Recursive Self-Improvement Framework

**Status**: Reference document for the zed-kask memory system, therapy skill, and the recursive self-improvement framework. Updated 2026-08-25.

## Academic Grounding

### Computational models of memory

- **Cox & Shiffrin (2026)**, "Computational Models of Memory," *Open Encyclopedia of Cognitive Science* (OECS), MIT Press, DOI: 10.1162/oecs_8c02n2f1. Memory traces can be altered once retrieved; distorted traces produce retrieval noise; traces coevolve across events (Nelson & Shiffrin, 2013). The REM model (Shiffrin & Steyvers, 1997) parameters: `u` (transfer probability), `c` (correct storage probability), `g` (distinctiveness). Low `c` produces error-prone traces — therapy identifies and corrects these.

- **Atkinson & Shiffrin (1968)**, "Human memory: A proposed system and its control processes," *Psychology of Learning and Motivation*. The multi-store model: short-term (active, limited capacity) → long-term (permanent storehouse). Retrieval is a probe-activation process: traces activate in proportion to similarity to the probe.

- **Tulving (1972)**, "Episodic and semantic memory," *Organization of Memory*. The original episodic/semantic distinction — now removed from zed-kask's architecture. All h_mems are unified; the ontology blob carries dual-axis anchoring (PKO process + DC state) but there is no type distinction.

### Cognitive dissonance and resolution

- **Festinger (1957)**, *A Theory of Cognitive Dissonance*. Three resolution strategies: reduce importance (lower confidence), add consonant (insert reconciling memory), remove dissonant (expire/delete). Therapy classifies contradictions by strategy.

- **Lidwell, W., Holden, K., & Butler, J. (2010)**, *Universal Principles of Design*. "People alleviate cognitive dissonance in one of three ways: by reducing the importance of dissonant cognitions, adding consonant cognitions, or removing or changing dissonant cognitions" (p. 39).

### Self-knowledge and calibration

- **Kruger, J. & Dunning, D. (1999)**, "Unskilled and Unaware of It," *Journal of Personality and Social Psychology* 77(6), 1121–1134. The double curse: the same skills needed to produce correct answers are needed to evaluate whether answers are correct. Implication: a model that writes its own confidence scores will miscalibrate.

- **Dunning, D. (2011)**, "The Dunning-Kruger Effect: On Being Ignorant of One's Own Ignorance," *Advances in Experimental Social Psychology* 44, 247–296. The feedback gap: without feedback, incorrect self-assessments persist. The Cassandra quandary: poor performers can't evaluate the expertise of others.

- **Ehrlinger, J. & Dunning, D. (2003)**, "How chronic self-views influence estimates of performance," *JPSP* 84(1), 5–17. Naive realism: people treat their own model as ground truth.

- **Dunning, D. (2018)**, "The Trouble of Not Knowing What You Do Not Know," *Reason, Bias, and Inquiry*, Oxford University Press. Hypocognition: lacking a cognitive representation for something. Overclaiming: claiming knowledge of nonexistent concepts.

- **Jansen, R. A., Rafferty, A. N. & Griffiths, T. L. (2021)**, "A rational model of the Dunning-Kruger effect," *Nature Human Behaviour* 5(6), 756–757. Insensitivity to evidence in low performers.

- **Dunning, D. & Helzer, E. G. (2014)**, "Beyond the Correlation Coefficient," *Perspectives on Psychological Science* 9(2), 126–130. "Make everybody better performers" — fix the underlying capability, not the metacognition.

### Forecasting and calibration

- **Brier, G. W. (1950)**, "Verification of forecasts expressed in terms of probability," *Monthly Weather Review*. The proper scoring rule for probabilistic forecasts.

- **Tetlock, P. & Gardner, D. (2015)**, *Superforecasting: The Art and Science of Prediction*. Brier scoring as the calibration mechanism. "Fuzzy thinking can never be proven wrong" (p. 274). "Not all practice improves skill. It needs to be informed practice" (p. 195). The dilution effect: irrelevant information weakens judgment (p. 178).

### AI memory systems

- **Park, J. S. et al. (2023)**, "Generative Agents: Interactive Simulacra of Human Behavior," arXiv:2304.03442. Observation → reflection → retrieval pipeline. Reflections are higher-level abstractions generated periodically.

- **Packer, C. et al. (2023)**, "MemGPT: Towards LLMs as Operating Systems," arXiv:2310.08560. OS-style hierarchical memory with permission boundaries.

- **Gallego, M. (2026)**, "Distilling Feedback into Memory-as-a-Tool," arXiv:2601.05960. Feedback distilled through a structured process; model proposes, rubric filters.

### The goldfish principle

- **Vardy, M. (2020)**, "Be a Goldfish," *Medium*. "Be a goldfish. Got a 10 second memory." The principle: don't let the past own the present. Applied to memory therapy: forgetting is a feature, not a bug. Once a lesson is reified into proactive guidance, the episodic memory can be forgotten.

---

## Architecture: Who Has Memory

```mermaid
graph TD
    subgraph "zed-kask Memory Architecture"
        User["User (human)<br/>NO kask memory<br/>Has own memory"]
        ZedAgent["Zed Agent<br/>NO memory<br/>Context injection only"]
        Curator["Curator Agent<br/>curator.db<br/>Ingests curator turns<br/>Recalls own memory"]
        Corpus["Replica / Corpus<br/>Static memory<br/>Built from corpus<br/>via corpus server"]
        Swarm["Swarm Agents<br/>swarm_memory.db<br/>Shared per swarm<br/>Per-agent turns"]
    end

    User -->|chats with| ZedAgent
    User -->|chats with| Curator
    Curator -->|therapy on| Curator
    Curator -->|therapy on| Corpus
    Curator -->|therapy on| Swarm
```

**Key principle**: The user is human and has their own memory. The zed agent has NO kask memory — no ingestion, no recall. Only the curator, replicas, and swarm agents have kask memory. Context injection (system prompt, project rules) continues for the zed agent, but no memory is generated or recalled.

### User sovereignty and transparency

The memory system is transparent to the user and respects user sovereignty:

- **The user can see what's in memory.** The curator MCP server's `curator_memory_recall` and `curator_semantic_search` tools are read-only and available to all threads — the user can query the curator's memory at any time to see what's stored.

- **The user approves all memory modifications.** Therapy requires user approval for every modification — no autonomous memory editing. The curator proposes; the user approves.

- **The user can run without memory loops.** The zed agent (the default coding agent) has no memory — no ingestion, no recall, no consolidation timer, no therapy. The user can work in the zed agent with zero memory system activity. Memory only activates when the user explicitly switches to the Curator agent. This is a usage mode that avoids the memory loops entirely — the user is never forced into the cybernetic system.

- **The user controls what the curator remembers.** Only curator turns (when the user is in a curator panel session) are ingested. The user's zed agent conversations are NOT ingested. The user decides what enters the curator's memory by choosing to work in the curator panel.

- **The user can purge memory.** The `memory_resolve_contradiction` tool (curator-only) allows the user to expire or delete any h_mem. The `corpus_purge` tool allows bulk deletion of corpus memory by prefix. The user is never trapped by accumulated memory.

- **Forgetting is deliberate, not automatic.** Consolidation (automatic) only deletes low-confidence h_mems and prunes to budget — it never deletes memories the user might want. Therapy (user-initiated) is the deliberate forgetting process — the user chooses what to forget and why. The goldfish principle applies: forgetting is a feature, but only when done with awareness and purpose.

This design respects the principle that the system serves the user, not the other way around. Memory is a tool the user can use, inspect, modify, or disable — not a surveillance system that records the user without their knowledge or consent.

---

## Entity Relationships: Memory Storage

```mermaid
erDiagram
    HMEM ||--o{ EMBEDDING : "indexed by"
    HMEM ||--o{ MEMORY_LINK : "co-occurs with"
    HMEM {
        string id PK
        string entity
        string attribute
        json value
        datetime observed_at
        float confidence
        string perspective
        string visibility
        string owner_webid
        json ontology
        datetime recalled_at
    }
    EMBEDDING {
        string id PK
        string entity_ref FK
        blob vector
        int dimensions
        string model
    }
    MEMORY_LINK {
        string entity_a PK
        string entity_b PK
        int co_count
        datetime last_linked
    }

    CURATOR_DB ||--|| HMEM : "stores"
    CURATOR_DB ||--|| EMBEDDING : "stores"
    CURATOR_DB ||--|| MEMORY_LINK : "stores"
    CORPUS_DB ||--|| HMEM : "stores"
    CORPUS_DB ||--|| EMBEDDING : "stores"
    SWARM_DB ||--|| HMEM : "stores"
    SWARM_DB ||--|| EMBEDDING : "stores"

    CURATOR_DB {
        string path "agents/curator/curator.db"
        string owner "curator_webid"
    }
    CORPUS_DB {
        string path "corpus/memory/*.db"
        string owner "replica"
    }
    SWARM_DB {
        string path "swarm_memory.db"
        string owner "swarm shared"
    }
```

**Key**: All h_mems are unified — no episodic/semantic distinction. The `ontology` blob carries dual-axis anchoring (PKO process + DC state) but there is no type distinction. `MEMORY_LINK` tracks co-occurrence connectedness (Priority 3 — co-occurrence links recorded during recall).

---

## The Learning Loop

The goal isn't for memory to accrete. The goal is for experiences to be saved in memory and then reified into skills, rules, or templates for **prospective, prescriptive, or expectational** practices and intelligence — rather than descriptive, retrospective, or reflective intelligence which memory is a part of.

```mermaid
graph TD
    Exp["Experience<br/>(agent acts in world)"]
    Mem["Memory<br/>(experience ingested)"]
    Therapy["Therapy<br/>(extract meaning + reify)"]
    Guidance["Proactive Guidance<br/>(skill / template / rule)"]
    NewExp["New Experience<br/>(agent acts more effectively)"]

    Exp -->|ingest| Mem
    Mem -->|therapy session| Therapy
    Therapy -->|reify lesson| Guidance
    Guidance -->|applied in practice| NewExp
    NewExp -->|ingest| Mem

    style Therapy fill:#f9f,stroke:#333,stroke-width:2px
    style Guidance fill:#bfb,stroke:#333,stroke-width:2px
```

**Therapy is the step that closes the learning loop.** Without therapy, memory accumulates but never becomes learning — the past is stored but not applied. Therapy extracts meaning from accumulated experience and reifies it into proactive guidance that shapes future action.

**Memory is retrospective/descriptive/reflective.** Skills, rules, and templates are prospective/prescriptive/expectational. Therapy is the bridge between the two.

---

## Memory Hygiene vs. Reification

Therapy runs two distinct processes. Do not conflate them.

```mermaid
graph TD
    subgraph "Memory Hygiene (forgetting — NOT learning)"
        Scan1["Scan for contradictions"]
        Scan2["Scan for fragmentation"]
        Scan3["Scan for miscalibrated confidence"]
        Resolve["Resolve via Festinger strategies"]
        Purge["Purge stale / low-value"]
        Condense["Condense duplicates"]
        CleanDB["Clean memory database"]

        Scan1 --> Resolve
        Scan2 --> Resolve
        Scan3 --> Resolve
        Resolve --> Purge
        Resolve --> Condense
        Purge --> CleanDB
        Condense --> CleanDB
    end

    subgraph "Reification (learning — closes the learning loop)"
        Scan4["Scan for reification candidates"]
        Extract["Extract lesson from pattern"]
        CreateSkill["Create skill / template / rule"]
        Proactive["Proactive guidance embedded"]

        Scan4 --> Extract
        Extract --> CreateSkill
        CreateSkill --> Proactive
    end

    subgraph "Post-Reification Hygiene (forgetting — side-effect)"
        Forget["Purge or condense<br/>source memories"]
        Goldfish["Goldfish principle:<br/>forget what's been reified"]

        Proactive -->|after approval| Forget
        Forget --> Goldfish
    end
```

**Forgetting is NOT learning.** Purging and condensing source memories after reification is a hygiene side-effect — shedding low-value information so it doesn't obstruct future learning. The goldfish principle: once a lesson is reified into proactive guidance, the episodic memory that produced the lesson can be forgotten.

---

## Therapy Process Flow

```mermaid
graph TD
    P1["Phase 1: Target Selection<br/>(curator / corpus / swarm)"]
    P2["Phase 2: Scan<br/>(contradictions, fragmentation,<br/>miscalibrated confidence,<br/>reification candidates)"]
    P3["Phase 3: Classify & Propose<br/>(Festinger strategies +<br/>reification proposals)"]
    P4["Phase 4: User Review & Approval<br/>(all modifications require approval)"]
    P5["Phase 5: Execute<br/>(hygiene edits +<br/>skill/template/rule creation +<br/>post-reification forgetting)"]
    P6["Phase 6: Report<br/>(hygiene summary +<br/>reification summary)"]

    P1 --> P2
    P2 -->|findings| P3
    P3 -->|proposals| P4
    P4 -->|approved proposals| P5
    P5 -->|execution results| P6
    P6 -->|recommendations| P1

    style P4 fill:#ff9,stroke:#333,stroke-width:2px
    style P5 fill:#f9f,stroke:#333,stroke-width:2px
```

**Therapy must run from a Curator agent panel session** (for curator memory). The curator must remember the act of therapy — the forgetting, the reification, the lessons learned — so the cybernetic loop closes. Therapy run from the zed agent would modify curator memory without the curator's awareness, defeating the cybernetic design.

---

## Recursive Self-Improvement Framework

```mermaid
graph TD
    subgraph "Cycle 1"
        E1["Experience 1"] --> M1["Memory 1"]
        M1 --> T1["Therapy 1"]
        T1 --> R1["Reification 1<br/>(skill / rule / template)"]
        R1 --> G1["Proactive Guidance 1"]
    end

    subgraph "Cycle 2"
        G1 -->|guides action| E2["Experience 2<br/>(more effective)"]
        E2 --> M2["Memory 2"]
        M2 --> T2["Therapy 2"]
        T2 --> R2["Reification 2<br/>(refined skill / new rule)"]
        R2 --> G2["Proactive Guidance 2<br/>(cumulative)"]
    end

    subgraph "Cycle N"
        G2 -->|guides action| EN["Experience N<br/>(increasingly effective)"]
        EN --> MN["Memory N"]
        MN --> TN["Therapy N"]
        TN --> RN["Reification N"]
        RN --> GN["Proactive Guidance N<br/>(accumulated wisdom)"]
    end

    T1 -.->|forget source| F1["Forgotten 1"]
    T2 -.->|forget source| F2["Forgotten 2"]
    TN -.->|forget source| FN["Forgotten N"]

    style T1 fill:#f9f,stroke:#333,stroke-width:2px
    style T2 fill:#f9f,stroke:#333,stroke-width:2px
    style TN fill:#f9f,stroke:#333,stroke-width:2px
    style R1 fill:#bfb,stroke:#333,stroke-width:2px
    style R2 fill:#bfb,stroke:#333,stroke-width:2px
    style RN fill:#bfb,stroke:#333,stroke-width:2px
```

**The framework is recursive**: each cycle produces proactive guidance that makes the next experience more effective. The next experience generates new memory, which therapy processes into refined guidance. The source memories are forgotten (goldfish principle) — what persists is the reified guidance, not the episodic detail.

**Memory does not accrete — it converts.** Experiences are saved in memory, then reified into skills/rules/templates for prospective intelligence. The episodic detail is shed; the proactive guidance accumulates. This is the difference between a system that remembers the past (descriptive/retrospective) and a system that learns from the past (prospective/prescriptive/expectational).

### User sovereignty in the recursive framework

The recursive self-improvement framework is opt-in, not automatic:

- **The user triggers therapy.** Therapy does not run automatically — the user initiates it from a curator panel session. The user decides when to extract lessons and reify them.
- **The user approves each reification.** The user reviews the proposed skill/template/rule content before it is created. The user can modify, reject, or defer.
- **The user approves each forgetting.** The user approves purging or condensing source memories as a separate decision from reification. The user can reify a lesson but keep the source memories.
- **The user can exit the loop.** The zed agent (default mode) has no memory — the user can work without any memory system activity. The recursive framework only runs when the user explicitly chooses to work in the curator panel and invoke therapy.

The framework respects user sovereignty: the system learns only when the user chooses to teach it, forgets only what the user chooses to forget, and applies guidance only through skills/rules/templates the user has approved.

---

## Memory Recall Ranking

```mermaid
graph LR
    Query["User query"] --> Embed["Embed query"]
    Embed --> KNN["KNN search<br/>(embedding similarity)"]
    KNN --> Candidates["Candidate h_mems"]
    Candidates --> Filter["Filter by<br/>confidence threshold"]
    Filter --> Rank["Rank by<br/>relevance × confidence × connectedness"]
    Rank --> Top["Top-K snippets"]
    Top --> Inject["Inject into context"]

    style Rank fill:#ff9,stroke:#333,stroke-width:2px
```

**Recall ranking function** (Priority 1 — done):
```
recall_score = relevance_score × decayed_confidence × connectedness
```

- **Relevance**: embedding cosine similarity (existing)
- **Confidence**: outcome-calibrated, decayed by Wozniak-Gorzelanczyk forgetting curve (existing, now used as ranking signal, not just threshold)
- **Connectedness**: graph co-occurrence link density (Priority 3 — schema added, wiring pending)

**Absence signaling** (Priority 2 — done): when recall returns zero results, the context injector returns a system message: "No relevant memory found for this query. This may indicate a knowledge gap." This is the hypocognition guard (Dunning, 2018).

---

## Consolidation

```mermaid
graph TD
    Timer["Consolidation timer<br/>(background, cadence-based)"]
    Check1["Delete h_mems at/below<br/>confidence floor"]
    Check2["Delete lowest-confidence h_mems<br/>until within storage budget"]
    Done["Consolidation complete"]

    Timer --> Check1
    Check1 --> Check2
    Check2 --> Done
```

**Consolidation is confidence-based cleanup only.** No episodic→semantic promotion, no re-tagging, no reflection. The promotion pipeline was removed when the episodic/semantic distinction was eliminated. Consolidation deletes low-confidence h_mems and prunes to the storage budget — that's it.

Reflection (generating new abstractions from accumulated memory) is handled by the therapy skill, not by the consolidation timer. This separates the automatic hygiene process (consolidation) from the deliberate learning process (therapy).

---

## Tool Wiring

```mermaid
graph TD
    subgraph "Curator Agent Panel Session"
        TherapySkill["Therapy Skill<br/>(SKILL.md)"]
        ScanT["scan.j2<br/>(render_template)"]
        ClassifyT["classify.j2<br/>(render_template)"]
        ReportT["report.j2<br/>(render_template)"]
    end

    subgraph "Curator MCP Server (hkask-mcp-curator)"
        Recall["curator_memory_recall<br/>(read)"]
        Search["curator_semantic_search<br/>(read)"]
        Consult["curator_consult<br/>(read)"]
        Insert["memory_insert<br/>(write — evidence-grounded,<br/>confidence floor 0.5)"]
        Update["memory_update<br/>(write — Bayesian combine)"]
        Resolve["memory_resolve_contradiction<br/>(write — expire/update/delete)"]
    end

    subgraph "Built-in Tools"
        WriteFile["write_file<br/>(create skills/templates/rules)"]
        RenderT["render_template<br/>(render .j2 templates)"]
        LispEval["lisp_eval<br/>(deterministic checks)"]
    end

    subgraph "curator.db"
        HMems["h_mems table"]
        Embeddings["embeddings table"]
        Links["memory_links table"]
    end

    TherapySkill -->|reads| ScanT
    TherapySkill -->|reads| ClassifyT
    TherapySkill -->|reads| ReportT
    ScanT -->|guides agent to call| Recall
    ScanT -->|guides agent to call| Search
    ClassifyT -->|guides agent to call| Insert
    ClassifyT -->|guides agent to call| Update
    ClassifyT -->|guides agent to call| Resolve
    ClassifyT -->|guides agent to call| WriteFile
    TherapySkill -->|uses| RenderT
    TherapySkill -->|uses| LispEval
    Insert -->|writes| HMems
    Update -->|writes| HMems
    Resolve -->|writes| HMems
    Recall -->|reads| HMems
    Search -->|reads| HMems
```

**Templates do NOT make tool calls.** They are prompt structures rendered by `render_template` that guide the agent on what to call and how to structure output. The agent reads the rendered template and makes the actual MCP tool calls.

**The curator remembers the therapy session** because curator turns are ingested to `curator.db` (agent_id == CURATOR_AGENT_ID). This closes the cybernetic loop: the curator learns from the act of therapy.

---

## Implementation Status

| Priority | Change | Status |
|---|---|---|
| Episodic/semantic removal | Complete elimination of the distinction | ✅ Done |
| User memory store removal | RealMemoryPort no longer holds user store | ✅ Done |
| 1 | Confidence in recall ranking | ✅ Done |
| 2 | Absence signaling (hypocognition guard) | ✅ Done |
| 3 | Connectedness tracking (co-occurrence links) | Schema added, wiring pending |
| 4 | Brier loop → memory confidence | Not started |
| 5 | Curator memory edit tools | ✅ Done |
| 6 | Therapy process (skill) | ✅ Done |
| 7 | Q3 reflection pass | Not started |

## Related Documents

- [Memory System Improvements](memory-system-improvements.md) — implementation plan with priorities
- [Q3 + Q5 Design Analysis](q3-q5-reflection-writable-memory.md) — reflection and writable memory design
- [RAG Synthesis: Dunning & Memory Design](rag-synthesis-dunning-memory-design.md) — corpus-grounded evidence
- [Therapy Skill](../../.agents/skills/therapy/SKILL.md) — the therapy skill process document
