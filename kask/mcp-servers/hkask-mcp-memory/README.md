# hkask-mcp-memory

Unified episodic + semantic memory MCP server with cloud backup.

## Tools (20)

| Tool | Description |
|------|-------------|
| `episodic_ping` | Liveness and storage info for episodic memory |
| `episodic_store` | Store an episodic triple (private, perspective-bound) |
| `episodic_recall` | Recall episodic triples by entity (filtered by caller's WebID) |
| `episodic_recall_context` | Recall episodic memories ranked by salience to context. Returns formatted episodes (User:/Agent: pairs for chat history) sorted by keyword relevance |
| `episodic_budget` | Storage usage and budget for episodic memory |
| `episodic_consolidate_status` | Check consolidation candidates and budget status for episodic→semantic promotion |
| `semantic_ping` | Liveness and storage info for semantic memory |
| `semantic_store` | Store a shared semantic triple (no perspective) |
| `semantic_recall` | Recall shared semantic triples by entity |
| `remember` | Store a memory triple — routes to episodic_store or semantic_store based on memory_type |
| `recall` | Recall memory triples by entity — routes based on memory_type |
| `memory_recall` | Paired memory recall — returns both semantic (third-person) and episodic (first-person) memories for an entity in a single call. Episodic results are ranked by salience when context is provided |
| `semantic_embed` | Store an embedding vector for similarity search |
| `semantic_search` | KNN similarity search over embeddings |
| `semantic_centroid` | Compute mean embedding vector (centroid) for embeddings matching a prefix |
| `semantic_purge` | Delete all embeddings whose entity_ref starts with a prefix |
| `semantic_chunk` | Chunk text into passages for embedding, with optional Gutenberg header stripping |
| `semantic_count` | Triple and embedding counts for semantic memory |
| `memory_backup` | Export the memory database to a local backup file |
| `memory_restore` | Restore the memory database from a local backup file |

## Configuration

| Variable | Description | Default |
|----------|-------------|---------|
| `HKASK_DB_PATH` | SQLite database path | In-memory |
| `HKASK_DB_PASSPHRASE` | Database encryption passphrase | Required if DB path set |

## Quick Start

```bash
# In-memory by default (no config needed)
kask chat
# With persistence:
HKASK_DB_PATH=./memory.db HKASK_DB_PASSPHRASE=secret kask chat
```
