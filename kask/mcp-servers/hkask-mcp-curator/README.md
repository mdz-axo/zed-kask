# hkask-mcp-curator — Curator MCP Server

MCP server exposing Curator tools: system health, escalation management, Regulation observability, cross-pod semantic search, memory recall, and algedonic event history.

**Version:** v0.35.0 | **Crate:** `hkask-mcp-curator`

## Tools (8)

| Tool                         | Description                                                                                                                       |
| ---------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `curator_ping`               | Liveness check — reports per-store availability                                                                                   |
| `curator_escalations`        | List all pending escalations requiring review                                                                                     |
| `curator_escalation_resolve` | Resolve an escalation by ID (records the resolution note in the Regulation audit trail)                                           |
| `curator_escalation_dismiss` | Dismiss an escalation as not actionable                                                                                           |
| `curator_semantic_search`    | Query the Curator's semantic memory by entity name                                                                                |
| `curator_memory_recall`      | Recall the Curator's episodic and semantic memory about an entity, or along an ontology axis (`ontology_axis` + `ontology_value`) |
| `curator_algedonic_log`      | Read algedonic event log for a time window                                                                                        |
| `reg_query`                  | Query Regulation records by namespace prefix within a time window                                                                 |

## Configuration

No environment variables required. The server opens its sovereign `curator.db` (SQLCipher) using the `HKASK_CURATOR_DB` path and `HKASK_DB_PASSPHRASE` from the keychain. If the DB cannot be opened at startup, the server self-heals: every tool call re-attempts the open (rate-limited to once per 5s) until it succeeds.

## Dependencies

- `hkask-mcp-server` — MCP runtime and dispatch
- `hkask-storage` / `hkask-memory` — sovereign `curator.db` stores
- `governance` module — escalation CRUD + Regulation event emission
- `hkask-capability` — DelegationToken consent registry
