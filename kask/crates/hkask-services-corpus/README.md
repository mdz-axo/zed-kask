# hkask-services-corpus — Corpus Service

Corpus discovery, embedding pipeline, text chunking, OCR, and entity extraction. Powers semantic search across the skill registry, documentation, and style corpora.

**Version:** v0.31.0 | **Crate:** `hkask-services-corpus`

## Modules

| Module | Purpose |
|--------|---------|
| `discover_impl` | Corpus discovery — scan registry, docs, styles for source texts |
| `embed_impl` | Embedding pipeline — chunk text, generate embeddings, index |

## Key Types

- `CorpusDiscovery` — source text discovery across registered corpora
- `EmbeddingPipeline` — chunk → embed → index workflow

## Dependencies

- `hkask-services-core` — `ServiceConfig`, `ServiceError`
- `hkask-storage` — SQLite persistence for chunk/embedding index
- `hkask-inference` — embedding model dispatch
