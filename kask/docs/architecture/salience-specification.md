---
title: "Passage Salience Specification"
audience: [architects, developers, agents]
last_updated: 2026-07-19
version: "0.31.0"
status: "Active"
domain: "Application"
mds_categories: [domain, composition]
---

# Passage Salience Specification — hKask v0.31.0

**MDS Category:** specification/algorithm
**Status:** Active
**Created:** 2026-06-12
**Scope:** `crates/hkask-memory/src/salience.rs` — `compute_salience_batch`

---

## 1. Purpose

Define the salience score used by the style corpus embedding pipeline to rank
passages for budget-gated hMem storage. Salience determines which passages
receive full metadata hMems (entity tags, method signals, position) vs.
embedding-only storage.

---

## 2. Academic Anchoring

### 2.1 MMR — Maximal Marginal Relevance (Carbonell & Goldstein, 1998)

The foundational redundancy-aware selection formula:

```
MMR = λ·Sim₁(Dᵢ, Q) − (1−λ)·maxⱼ(Sim₂(Dᵢ, Dⱼ))
```

Relevance to query **minus** maximum similarity to already-selected items.
Established the pattern: score = relevance_term − redundancy_term.

### 2.2 LexRank (Erkan & Radev, 2004)

Graph-based eigenvector centrality for sentence salience. Builds a sentence
similarity graph (cosine over TF-IDF), then applies PageRank-style centrality.
Established the pattern: salience as graph centrality over textual units.

### 2.3 Local Clustering Coefficient (Watts & Strogatz, 1998)

For node i with kᵢ neighbors and Eᵢ edges between those neighbors:

```
Cᵢ = 2·Eᵢ / (kᵢ·(kᵢ−1))    for kᵢ ≥ 2
Cᵢ = 0                        for kᵢ < 2
```

Measures how tightly a node's neighborhood is interconnected. High Cᵢ = node
sits in a dense clique (redundant). Low Cᵢ = node bridges otherwise-disconnected
communities (unique).

### 2.4 Submodular Selection (Lin & Bilmes, 2010, 2011)

Formalized MMR as budgeted submodular optimization. Key insight: penalizing
redundancy makes the objective non-monotone; rewarding diversity preserves
monotonicity and approximation guarantees. Our multiplicative formulation
`connectedness × (1 − redundancy)` is equivalent to `connectedness × diversity`
— a monotone-friendly form.

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
over a sampled subset for performance. Range [0, 1].

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
well under one second.

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

### 5.1 Budget Gate (embed.rs)

```rust
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
always receive hMems regardless of salience score.

### 5.2 Retrieval Filter (compose.rs)

During prose composition, exemplar passages are retrieved by KNN vector search
and filtered by `salience_min`. Low-salience passages are excluded from the
few-shot context window.

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
