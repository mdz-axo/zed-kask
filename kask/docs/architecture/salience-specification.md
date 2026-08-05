---
title: "Passage Salience Specification"
audience: [architects, developers, agents]
last_updated: 2026-08-04
version: "0.31.3"
status: "Active"
domain: "Application"
mds_categories: [domain, composition]
---

# Passage Salience Specification — hKask v0.31.0

**MDS Category:** specification/algorithm
**Status:** Active
**Created:** 2026-06-12
**Scope:** `kask/crates/hkask-memory/src/salience.rs` — `compute_salience_batch` (L723)

---

## 1. Purpose

Define the salience score used by the style corpus embedding pipeline to rank
passages for budget-gated hMem storage. Salience determines which passages
receive full metadata hMems (entity tags, method signals, position) vs.
embedding-only storage.[^carbonell-mmr]

---

## 2. Academic Anchoring

### 2.1 MMR — Maximal Marginal Relevance (Carbonell & Goldstein, 1998)

The foundational redundancy-aware selection formula:

```
MMR = λ·Sim₁(Dᵢ, Q) − (1−λ)·maxⱼ(Sim₂(Dᵢ, Dⱼ))
```

Relevance to query **minus** maximum similarity to already-selected items.
Established the pattern: score = relevance_term − redundancy_term.[^carbonell-mmr]

### 2.2 LexRank (Erkan & Radev, 2004)

Graph-based eigenvector centrality for sentence salience. Builds a sentence
similarity graph (cosine over TF-IDF), then applies PageRank-style centrality.
Established the pattern: salience as graph centrality over textual units.[^erkan-lexrank]

### 2.3 Local Clustering Coefficient (Watts & Strogatz, 1998)

For node i with kᵢ neighbors and Eᵢ edges between those neighbors:

```
Cᵢ = 2·Eᵢ / (kᵢ·(kᵢ−1))    for kᵢ ≥ 2
Cᵢ = 0                        for kᵢ < 2
```

Measures how tightly a node's neighborhood is interconnected. High Cᵢ = node
sits in a dense clique (redundant). Low Cᵢ = node bridges otherwise-disconnected
communities (unique).[^watts-strogatz]

### 2.4 Submodular Selection (Lin & Bilmes, 2010, 2011)

Formalized MMR as budgeted submodular optimization. Key insight: penalizing
redundancy makes the objective non-monotone; rewarding diversity preserves
monotonicity and approximation guarantees. Our multiplicative formulation
`connectedness × (1 − redundancy)` is equivalent to `connectedness × diversity`
— a monotone-friendly form.[^lin-bilmes]

### 2.5 How Our Model Relates

| Concept | Literature Standard | Our Instantiation |
|---------|---------------------|-------------------|
| Graph structure | Sentence similarity (TF-IDF, BERT) | Entity co-occurrence (characters, places, events, concepts, methods) |
| Salience signal | Eigenvector centrality (LexRank) | Degree centrality + mean neighbor centrality (one-pass approximation) |
| Redundancy signal | Pairwise similarity (cosine, ROUGE, n-gram) | Local clustering coefficient (structural, not pairwise) |
| Combination | MMR: relevance − max-similarity | Multiplicative: connectedness × (1 − clustering) |
| Selection | Iterative greedy (MMR, submodular) | Single-pass scoring + sort (budget allocation, not summary construction) |

---

## 3. Mathematical Definition

### 3.1 Entity Co-occurrence Graph

Given N passages, each with a set of entity tags (characters, places, events,
concepts, methods):

```
neighbors(i) = { j ≠ i : tags(i) ∩ tags(j) ≠ ∅ }
```

Two passages are neighbors if they share at least one entity tag.

### 3.2 One-Hop (Degree Centrality)

```
one_hop(i) = |neighbors(i)| / N
```

Fraction of all passages directly connected to passage i. Range [0, 1].

### 3.3 Average Neighbor Quality

Sample up to K=50 neighbors via even spacing (step = |neighbors(i)| / K) to
avoid bias toward first neighbors:

```
sample(i) = { neighbors(i)[0], neighbors(i)[step], neighbors(i)[2·step], … }
            truncated to at most K elements

avg_neighbor_quality(i) = (1 / |sample(i)|) × Σ one_hop(j)
                                                    j ∈ sample(i)
```

Mean degree centrality of sampled neighbors. Eigenvector-like signal without
iterative convergence. Range [0, 1].

### 3.4 Connectedness

```
connectedness(i) = (one_hop(i) + avg_neighbor_quality(i)) / 2
```

Unweighted mean of direct centrality and neighbor centrality. A passage scores
high if it is well-connected OR connected to well-connected passages. Range
[0, 1].

### 3.5 Redundancy (Local Clustering Coefficient)

Computed over the same sampled neighbors. For |sample(i)| ≥ 2:

```
Eᵢ = |{ (a, b) : a, b ∈ sample(i), a < b, b ∈ neighbors(a) }|

Cᵢ = Eᵢ / (|sample(i)| × (|sample(i)| − 1) / 2)
```

For |sample(i)| < 2: Cᵢ = 0.

This is the canonical Watts-Strogatz local clustering coefficient, computed
over a sampled subset for performance. Range [0, 1].[^watts-strogatz]

### 3.6 Salience

```
salience(i) = connectedness(i) × (1 − Cᵢ)
```

Multiplicative penalty: redundancy scales the connectedness score down
proportionally. A passage in a fully interconnected clique (Cᵢ = 1) gets zero.
A bridge passage between communities (Cᵢ ≈ 0) keeps full connectedness.
Moderate clustering gets moderate reduction. Range [0, 1].

### 3.7 Interpretation Matrix

| one_hop | avg_nq | clustering | connectedness | redundancy | salience | Interpretation |
|---------|--------|-----------|---------------|-----------|----------|----------------|
| high | high | high | high | high | **low** | Hub in dense clique — representative but redundant |
| high | high | low | high | low | **high** | Bridge between communities — important AND unique |
| high | low | low | medium | low | **medium** | Hub touching peripherals — central but neighbors are weak |
| low | high | low | medium | low | **medium** | Peripheral touching hubs — weak but connected to important |
| low | low | high | low | high | **low** | Peripheral in clique — neither central nor unique |
| 0 | — | — | 0 | — | **0** | Isolated — no entity connections |

---

## 4. Computational Bounds

### 4.1 Complexity

| Phase | Operation | Complexity |
|-------|-----------|------------|
| Inverted index | Build entity→passages map | O(N × T) where T = avg tags per passage |
| Neighbor sets | Union of entity co-occurrences | O(N × T × D) where D = avg passages per entity |
| One-hop | Count neighbors | O(N) |
| Avg neighbor quality | Sum over K samples | O(N × K) |
| Clustering coefficient | K×(K−1)/2 edge checks with O(1) hash lookups | O(N × K²) |
| **Total** | | **O(N × (T×D + K²))** |

With N=2000, T=5, D=500, K=50: ~5M + 2.5M = ~7.5M operations. Completes in
well under one second.[^newman-networks]

### 4.2 Sampling Guarantee

Evenly-spaced sampling (step_by) ensures unbiased estimation. For a passage
with 1000 neighbors, we sample indices 0, 20, 40, …, 980 — 50 evenly
distributed points. The sample mean of one_hop scores is an unbiased estimator
of the true population mean. The sample clustering coefficient approximates the
true coefficient with error bounded by O(1/√K).

### 4.3 Edge Cases

- **Zero tags:** `neighbors(i) = ∅` → `one_hop = 0`, `sample = ∅` → `salience = 0`
- **One neighbor:** `|sample| = 1` → `Cᵢ = 0` (no pairs) → `salience = connectedness`
- **All passages share one entity:** Every `neighbors(i) = N−1` → `one_hop ≈ 1`, `Cᵢ ≈ 1` → `salience ≈ 0` (correct: pure redundancy)

---

## 5. Integration

> **Path correction (2026-08-01 audit):** The prior version cited `embed.rs` and `compose.rs` as the integration surfaces. The actual call site for `compute_salience_batch` is `kask/mcp-servers/hkask-mcp-corpus/src/corpus/embed/service.rs` (the corpus MCP server's `EmbedService::embed_corpus`), not `hkask-memory/src/embed.rs` (which does not exist). The `salience_min` retrieval filter is **implemented** in `kask/mcp-servers/hkask-mcp-corpus/src/compose.rs:79` (struct field), `:278` (filter logic: `if salience < retrieval.salience_min { skip }`), and `:851` (doc). §5.2 is the live design.

### 5.1 Budget Gate (`hkask-mcp-corpus/src/corpus/embed/service.rs`)

```rust
// In EmbedService::embed_corpus — kask/mcp-servers/hkask-mcp-corpus/src/corpus/embed/service.rs
let salience_scores = salience::compute_salience_batch(&all_tags);
for (passage, score) in all_passages.iter_mut().zip(salience_scores.iter()) {
    passage.salience = *score;
}

// Sort by salience descending, allocate hMems top-down
let mut indexed: Vec<(usize, f32, usize)> = all_passages
    .iter().enumerate()
    .map(|(i, p)| (i, p.salience, p.metadata_triple_count()))
    .collect();
indexed.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));
```

Foundational rules (style guides, exemplars) bypass the budget gate — they
always receive hMems regardless of salience score.[^lin-bilmes]

### 5.2 Retrieval Filter (implemented)

During prose composition, exemplar passages are retrieved by KNN vector search
and filtered by `salience_min`. Low-salience passages are excluded from the
few-shot context window. This filter is implemented in
`kask/mcp-servers/hkask-mcp-corpus/src/compose.rs:79` (the `RetrievalConfig.salience_min`
field, default `0.0`), with the filter logic at `compose.rs:278`
(`if salience < retrieval.salience_min { skip }`).

---

## Footnotes

[^carbonell-mmr]: Carbonell, J., & Goldstein, J. (1998). The use of MMR, diversity-based reranking for reordering documents and producing summaries. *Proceedings of the 21st Annual International ACM SIGIR Conference on Research and Development in Information Retrieval*, 335–336. https://doi.org/10.1145/290941.291025
    Cited for the maximal marginal relevance (MMR) formula — the foundational redundancy-aware selection pattern that this salience model instantiates.

[^erkan-lexrank]: Erkan, G., & Radev, D. R. (2004). LexRank: Graph-based lexical centrality as salience in text summarization. *Journal of Artificial Intelligence Research*, 22, 457–479. https://doi.org/10.1613/jair.1503
    Cited for the eigenvector-centrality salience pattern over sentence similarity graphs — the graph-centrality approach this model adapts.

[^watts-strogatz]: Watts, D. J., & Strogatz, S. H. (1998). Collective dynamics of ‘small-world’ networks. *Nature*, 393(6684), 440–442. https://doi.org/10.1038/30918
    Cited for the local clustering coefficient formula used as the redundancy signal in this salience model.

[^lin-bilmes]: Lin, H., & Bilmes, J. (2011). A class of submodular functions for document summarization. *Proceedings of the 49th Annual Meeting of the Association for Computational Linguistics (ACL '11)*, 510–520. https://aclanthology.org/P11-1052
    Cited for the submodular optimization framework that formalizes MMR as budgeted selection — the theoretical basis for the multiplicative connectedness × diversity formulation.

[^newman-networks]: Newman, M. E. J. (2018). *Networks* (2nd ed.). Oxford University Press. https://global.oup.com/academic/product/networks-9780198805090
    Cited for the graph-algorithm complexity analysis framework applied to the computational bounds of the salience computation.

---

## 6. References

1. Carbonell, J. & Goldstein, J. (1998). "The Use of MMR, Diversity-Based
   Reranking for Reordering Documents and Producing Summaries." SIGIR 1998.

2. Erkan, G. & Radev, D. (2004). "LexRank: Graph-based Lexical Centrality as
   Salience in Text Summarization." Journal of Artificial Intelligence Research.

3. Watts, D.J. & Strogatz, S.H. (1998). "Collective dynamics of 'small-world'
   networks." Nature, 393(6684), 440-442.

4. Lin, H. & Bilmes, J. (2010). "Multi-document Summarization via Budgeted
   Maximization of Submodular Functions." NAACL 2010.

5. Lin, H. & Bilmes, J. (2011). "A Class of Submodular Functions for Document
   Summarization." ACL 2011.

6. Bi, K. et al. (2021). "AREDSUM: Adaptive Redundancy-Aware Iterative Sentence
   Ranking for Extractive Document Summarization." EACL 2021.
