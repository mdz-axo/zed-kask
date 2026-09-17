# hkask-mcp-curator — Curator MCP Server

MCP server exposing Curator tools: system health, escalation management, Regulation observability, semantic memory search, memory recall, and algedonic event history.

**Version:** v0.40.0 | **Crate:** `hkask-mcp-curator`

## Tools (20)

| Tool | Description |
| --- | --- |
| `curator_ping` | Report per-store liveness. |
| `curator_escalations` | List pending escalations requiring review. |
| `curator_advice_mark_applied` | Record an explicitly confirmed intervention and start its seven-day observation window without resolving the alert or claiming effectiveness. |
| `curator_advice_reviews` | Read observational advice reviews, including resolved alerts; outcomes preserve `causal_attribution: "unverified"`. |
| `curator_escalation_resolve` | Resolve an escalation and retain its Regulation audit note. |
| `curator_escalation_dismiss` | Dismiss an escalation as not actionable. |
| `curator_escalation_dismiss_by_pattern` | Dismiss pending escalations with an exact output match. |
| `curator_semantic_search` | Search Curator memory by semantic similarity. |
| `curator_memory_recall` | Recall memory about an entity, optionally scoped to an ontology axis. |
| `curator_consult` | Consult Curator memory with a question. |
| `curator_algedonic_log` | Read the algedonic event log for a time window. |
| `reg_query` | Query chronological Regulation records across all namespaces, optionally filtering an exact/dot-descendant namespace prefix before limiting. |
| `curator_report_skill_use_issue` | Record a failed or unexpected skill/tool execution. |
| `memory_insert` | Insert an evidence-cited semantic memory. |
| `memory_update` | Bayesian-combine new confidence or value evidence into a memory. |
| `memory_resolve_contradiction` | Resolve contradictory memories by forgetting or lowering confidence. |
| `curator_memory_prune` | Prune old memories under the requested retention policy. |
| `curator_memory_dedup` | Deterministically deduplicate normalized string memories. |
| `curator_memory_backfill_embeddings` | Backfill missing semantic embeddings for knowledge-layer memories. |
| `curator_memory_extract` | Extract candidate memories from a thread's turn history. |

`reg_query` is the general governance-observability read path. With no
namespace it includes records from every Regulation namespace and cycle phase;
with a namespace such as `reg.skill`, it includes that exact path and
dot-delimited descendants. The time and namespace predicates are applied in
SQL before the requested limit. `curator_algedonic_log` remains the separate
act-phase, algedonic-category view.

## Configuration

No environment variables required. The server opens its sovereign `curator.db` (SQLCipher) using the `HKASK_CURATOR_DB` path and `HKASK_DB_PASSPHRASE` from the keychain. If the DB cannot be opened at startup, the server self-heals: every tool call re-attempts the open (rate-limited to once per 5s) until it succeeds.

## Dependencies

- `hkask-mcp-server` — MCP runtime and dispatch
- `hkask-storage` / `hkask-memory` — sovereign `curator.db` stores
- `governance` module — escalation CRUD + Regulation event emission
- `hkask-tool-port` — DelegationToken consent registry
