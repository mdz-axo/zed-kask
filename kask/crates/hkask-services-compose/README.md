# hkask-services-compose — Style Composition

Style-based prose generation with cognition configuration: exemplar retrieval from semantic memory, centroid distance validation, Jinja2 template rendering, and prose assembly. **Not** related to skill bundle composition — that lives in `hkask-services-skill` (`bundle` module).

**Version:** v0.31.0 | **Crate:** `hkask-services-compose`

## Key Types

| Type | Purpose |
|------|---------|
| `CognitionConfig` | Configuration for the cognition pipeline (embedding, retrieval, validation, Jinja2 template) |
| `EmbeddingSection` | Embedding model selection and parameters |
| `RetrievalSection` | Retrieval strategy: k_min/k_max, distance threshold, salience gates |
| `ValidationSection` | Output validation: centroid distance maximum |
| `ComposeRequest` | Full composition request with prompt, DB path, cognition config, inference context |
| `ComposeResult` | Generated prose with exemplar count and optional `CentroidValidation` |
| `CentroidValidation` | Centroid-based validation: distance, threshold, passed flag |
| `ComposeService` | Primary service struct — single `compose(request)` pipeline |
| `cosine_distance` | Utility function for vector distance computation |

## Pipeline

1. Open per-agent semantic database
2. Generate embedding for the prompt
3. Retrieve exemplar passages (k_min..k_max, distance threshold, salience filter)
4. Render system prompt via Jinja2 template (or generic fallback)
5. Generate prose via inference
6. Validate centroid distance (unless `no_validate`)

## Dependencies

- `hkask-types` — Regulation spans, inference types
- `hkask-services-core` — `ServiceConfig`, `ServiceError`, `InferenceContext`
- `hkask-inference` — Inference router for embedding/retrieval
- `hkask-memory` — Semantic memory for context retrieval
- `hkask-storage` — Persistent storage, vector store
- `hkask-ports` — Hexagonal port traits
- `minijinja` — Template rendering
