# hkask-mcp-replica

Style replica MCP server — embed author corpora, compose prose, and manage style registries.

## Tools (10)

| Tool | Description |
|------|-------------|
| `replica_build` | Embed a style corpus and create an authorial replica. Downloads public domain texts, chunks them, generates embeddings, and computes a style centroid. |
| `replica_compose` | Generate prose in an author's style |
| `replica_rewrite` | Rewrite a passage or code snippet in an author's style, optimized for a specific Gentle Lovelace quality dimension (gentle/schriver/hopper/lovelace/composite) |
| `replica_compare` | Compare all built author replicas, or evaluate a document against a persona's centroids |
| `replica_discover` | Discover an academic author's body of work and generate a corpus.yaml for replica_build. Delegates to the replica-discovery skill manifest which orchestrates multi-source search (Semantic Scholar, arXiv, web, YouTube transcripts), content extraction, and corpus generation. Supports agentic (fully automated) and curated (human-in-the-loop) modes. |
| `replica_explain` | Explain what style centroids are and how the metadata layer works |
| `replica_mashup` | Generate prose blending two authors' styles |
| `replica_registry` | Manage the registry of built author replicas |
| `replica_cache_work` | Cache an extracted work's content to disk for reuse by replica_build. Writes content to {cache_dir}/{slug}.txt so the embedding pipeline can skip re-downloading. |
| `replica_pipeline_run` | Run checkpointed corpus pipeline steps; all tool steps require external MCP execution via `kask mcp invoke`. |

## Configuration

| Variable | Description |
|----------|-------------|
| `HKASK_DB_PATH` | SQLite database path |
| `HKASK_DB_PASSPHRASE` | Database encryption passphrase |
| `HKASK_DEFAULT_MODEL` | Generation model for prose composition (inherits system default) |
| `HKASK_EMBEDDING_MODEL` | Embedding model for vectorization and style centroids |
| `HKASK_DB_PASSPHRASE` | Required by the corpus wrappers to open their persistent corpus database |

## Quick Start

```bash
# The server starts automatically with kask
kask chat
# Or standalone:
hkask-mcp-replica
```

## Usage

```
"Build a style replica from Hemingway's works"   → replica_build
"Compose a paragraph in Hemingway's style"        → replica_compose
"Compare Orwell vs Huxley"                        → replica_compare
```
