---
title: "Ontology-Anchored Embedding Pipeline"
audience: [developers, architects]
last_updated: 2026-08-04
version: "0.33.1"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain, composition]
---

# Ontology-Anchored Embedding Pipeline

The corpus pipeline tools run in-process via the corpus MCP server (D1–D3).

## Design Decision

The corpus pipeline uses **tag → embed** ordering: chunks are classified
against domain ontologies (GOLEM, FIBO, ESO, PKO) BEFORE embedding, and the
ontology tags are prepended to chunk text as "instructions" before computing
the embedding vector.

This follows the INSTRUCTOR paradigm (Su et al., NeurIPS 2023): task-specific
instructions prepended to text produce task-conditioned embeddings that
outperform generic raw-text embeddings.[^instructor-paper]

## Pipeline Order

```
                    ┌─────────────────────────────────────────────┐
                    │                                             │
  files → convert → chunk → tag → embed → extract_triples → dedup │
                                  │         │                    │
                                  │         │                    ▼
                                  │         └─→ DB h_mems     consolidate
                                  │                           │
                                  └─→ tagged_ontology.jsonl    ▼
                                       (ontology tags)      build_prompts
                                                            (KNN + tags + KG)
                                                                │
                                                                ▼
                                                          generate_qa
                                                                │
                                                                ▼
                                                          ingest_qa
                                                                │
                                                                ▼
                                                          assemble → train
```

The tag-before-embed ordering means the embedding step already has ontology
context, so all downstream KNN lookups are domain-aware.[^instructor-pipeline]

## How Ontology-Anchored Embedding Works

### Step 1: Tag (classification from text alone)[^instructor-tag]

`corpus_tag_chunks` reads `chunks.jsonl` and classifies each chunk against
multiple ontologies:

| Ontology | Domain | Example Tags |
|----------|--------|-------------|
| GOLEM | Narrative/literary | metaphor, character development, allegory |
| FIBO | Financial/business | competitive advantage, ROIC, margin of safety |
| ESO | Epistemic/scientific | hypothesis, evidence, falsification |
| PKO | Procedural | procedure, analysis, evaluation, feedback loop |
| OMC | Media creation | scene, sequence, creative work |
| Dublin Core | Bibliographic | bibo:Book, bibo:Article |

Output: `tagged_ontology.jsonl` with `ontology_tags` field per chunk.

### Step 2: Embed (with ontology annotations)

`corpus_embed` reads `chunks.jsonl` AND `tagged_ontology.jsonl`. For each
chunk, it prepends the ontology tags as an instruction prefix:

```
[golem: metaphor, narrative structure | pko: analysis] The Dunning Kruger effect is shown...
```

This produces an embedding vector that encodes both:
- **Content** (what the passage says)
- **Classification** (what kind of passage it is)

### Step 3: Downstream consumers benefit

| Consumer | How it uses ontology-anchored embeddings |
|----------|------------------------------------------|
| Dedup | Near-identical passages with same ontology are more similar → better dedup |
| Consolidate | Clustering is domain-aware → narrative passages cluster with narrative |
| Build prompts | KNN retrieves passages from the same domain → better context for QA gen |

## Research Support

| Paper | Finding | Relevance |
|-------|---------|-----------|
| INSTRUCTOR (Su et al., 2023) | Task instructions prepended to text improve embedding quality across 70 tasks | Direct support for tag-anchored embedding |
| Ontology-enhanced KG embeddings (Wang et al., 2023) | Ontology priors improve KG embedding quality | Ontology tags as structured priors |
| Ontology-driven text classification (2024) | Ontology-based classification outperforms keyword-based | Tagging against ontology is more accurate |
| OntoKG (2024) | Ontology schemas guide LLM extraction → higher quality | Ontology templates guide triple extraction |
| RAG with ontology-guided KGs (2024) | One-time ontology learning reduces LLM cost | Tag once, all downstream steps benefit |

## Bridge Crates

Three ontology bridge crates provide canonical predicate constants:

| Crate | Ontology | Constants |
|-------|----------|-----------|
| `hkask-mcp-corpus::bridge::golem` | Narrative/literary | 16 predicates (hasCharacter, illustrates, metaphorFor, ...) |
| `hkask-mcp-corpus::bridge::fibo` | Financial/business | 12 concepts (competitiveAdvantage, returnOnCapital, ...) |
| `hkask-mcp-corpus::bridge::eso` | Epistemic/scientific | 16 predicates (hasHypothesis, falsifiedBy, implies, ...) |

These follow the pattern of `hkask-bridge-dublincore`:
type alias + const strings, no dependencies, no reasoners.[^dublin-core][^fibo-spec]

## Why Not Embed → Tag?

Running embed before tag produces raw-text embeddings that are
ontology-agnostic. The tagging step would need KNN context from these
"dumb" embeddings to inform classification — but:[^rag-lewis]

1. **Tagging is classification, not generation** — the RAG paradigm
   (retrieve-then-generate) applies to generation, not classification
2. **Bad embeddings → bad KNN → wrong tags** — contamination cascade
3. **Embeddings serve more downstream consumers** — improving embedding
   quality (via tags) benefits 3 steps (dedup, consolidate, build_prompts);
   improving tag quality (via KNN) benefits only tagging
4. **One-time cost, multiple benefits** — tag once, all retrieval steps
   benefit; embed once with tags, all KNN lookups are ontology-aware

See [ADR-050](../architecture/ADRs/ADR-050-ontology-anchored-embedding.md) for the full
design decision record.

## Footnotes

[^instructor-paper]: Su, W., Casper, M., Absar, S., Lo, K., Wang, Y., & Yang, Y. (2023). One Embedder, Any Task: Instruction-Finetuned Text Embeddings. arXiv. https://arxiv.org/abs/2212.09741
    Cited as the direct source for the tag-before-embed paradigm — task instructions prepended to text produce task-conditioned embeddings.

[^instructor-pipeline]: Su, W., Casper, M., Absar, S., Lo, K., Wang, Y., & Yang, Y. (2023). One Embedder, Any Task: Instruction-Finetuned Text Embeddings. arXiv. https://arxiv.org/abs/2212.09741
    Cited for the instruction-conditional embedding quality that the tag-before-embed pipeline ordering exploits.

[^instructor-tag]: Su, W., Casper, M., Absar, S., Lo, K., Wang, Y., & Yang, Y. (2023). One Embedder, Any Task: Instruction-Finetuned Text Embeddings. arXiv. https://arxiv.org/abs/2212.09741
    Cited for the instruction-prefix mechanism the embed step uses — ontology tags are prepended as task instructions.

[^dublin-core]: DCMI. (2020). *Dublin Core Metadata Element Set, Version 1.1*. Dublin Core Metadata Initiative. https://www.dublincore.org/specifications/dublin-core/dces/
    Cited as the pattern the bridge crates follow — lightweight constant definitions with no reasoner dependency.

[^fibo-spec]: EDM Council. (2024). *Financial Industry Business Ontology (FIBO) Specification*. Enterprise Data Management Council. https://spec.edmcouncil.org/fibo/
    Cited for the FIBO ontology the bridge crate exposes as canonical predicate constants.

[^rag-lewis]: Lewis, P., Perez, E., Piktus, A., Petroni, F., Karpukhin, V., Goyal, V., Küttler, H., Lewis, M., Yih, W., Rocktäschel, T., Riedel, S., & Kiela, D. (2020). Retrieval-Augmented Generation for Knowledge-Intensive NLP Tasks. arXiv. https://arxiv.org/abs/2005.11401
    Cited for the RAG (retrieve-then-generate) paradigm the design rationale invokes to distinguish tagging-as-classification from generation.