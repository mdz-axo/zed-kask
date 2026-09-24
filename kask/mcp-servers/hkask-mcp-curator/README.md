# hkask-mcp-curator — Curator MCP Server

MCP server exposing escalation management, Regulation history, memory search and per-store liveness (`curator_ping`). Live regulation health (`loop_reading`, alert-log cap and acceptance rate) comes from the **one** built-in `curator_status` AgentTool over the host's metacognition provider, not from a second MCP implementation or an inferred event-history snapshot.

**Version:** v0.40.0 | **Crate:** `hkask-mcp-curator`

## Tools (21)

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
| `curator_federated_search` | Search Curator memory plus configured sealed corpus sources without merging stores; returns source/record provenance and per-source status. |
| `curator_memory_recall` | Recall memory about an entity, optionally scoped to an ontology axis. |
| `curator_consult` | Consult Curator memory with a question. |
| `curator_algedonic_log` | Read the newest algedonic events in a time window. |
| `reg_query` | Query chronological Regulation records across all namespaces, optionally filtering an exact/dot-descendant namespace prefix before limiting. |
| `curator_report_skill_use_issue` | Record a failed or unexpected skill/tool execution with granular `failure_type` and controlled `failure_origin` ownership. |
| `memory_insert` | Insert an evidence-cited semantic memory. |
| `memory_update` | Bayesian-combine new confidence or value evidence into a memory. |
| `memory_resolve_contradiction` | Resolve contradictory memories by forgetting or lowering confidence. |
| `curator_memory_prune` | Prune old memories under the requested retention policy. |
| `curator_memory_dedup` | Deterministically deduplicate normalized string memories. |
| `curator_memory_backfill_embeddings` | Backfill missing semantic embeddings for knowledge-layer memories. |
| `curator_memory_extract` | Extract candidate memories from a thread's turn history. |

`curator_report_skill_use_issue` stores one controlled ownership value:
`skill_contract`, `agent_execution`, `tool_implementation`,
`provider_transport`, `environment_or_baseline`, `operator_interruption`,
`expected_absence`, or explicit `unknown`. The field is required; granular symptoms remain in
`failure_type`; ownership is never inferred from that free-form field.

`reg_query` is the general governance-observability read path. With no
namespace it includes records from every Regulation namespace and cycle phase;
with a namespace such as `reg.skill`, it includes that exact path and
dot-delimited descendants. The time and namespace predicates are applied in
SQL before the requested limit. `curator_algedonic_log` remains the separate
act-phase, algedonic-category view; it returns at most 500 events newest-first
and declares that ordering in its response.

## Advice review semantics

`curator_advice_mark_applied` records a confirmed intervention, not an effectiveness verdict. Its persisted `review_due_at` controls when review can finalize. Missing or stale readings finalize as `insufficient_evidence`, and every review retains `causal_attribution: "unverified"`.

Final review transitions use the escalation row as a durable outbox and publish one idempotent `reg.outcome.advice_review_observed` record. Regulation telemetry keeps `advice_review_progress_score` separate from evidence-bearing `rollout_progress_score`; either is `null` when its evidence channel has no determinate measurement. `curator_advice_reviews` remains the complete read path, including reviews whose originating alerts resolved early.

## Configuration

The server opens its sovereign `curator.db` (SQLCipher) using the `HKASK_CURATOR_DB` path and `HKASK_DB_PASSPHRASE` from the keychain. If the DB cannot be opened at startup, the server self-heals: every tool call re-attempts the open (rate-limited to once per 5s) until it succeeds.

Federated retrieval is configured by the presence of
`$HKASK_DATA_DIR/agents/curator/federated-sources.json`; the explicit search
has no separate enable toggle. Sources are registered manually: create or edit
this JSON file with `schema_version: 1` and a `sources` array. For each source,
provide its unique `id`, `display_name`, `database_path`, `run_identity_path`,
`representations_manifest_path`, and `index_name` from the sealed corpus run.
The settings picker reads these IDs; it does not register a database or seal a
corpus. The current schema is version 1. Each source names an ID,
display name, database path, sealed `run-identity.json`, representation
manifest, and index name. The source run identity must be current schema 3,
and its representation manifest current schema 2. The server re-computes the
producer's sorted-JSON SHA-256 run ID and verifies the representation manifest
and database digests, exact embedding identities, dimensions, passage prefix,
current database schema, and passage count. Legacy manifests, schemas, model
aliases, and database shapes are rejected rather than migrated or adapted.

Configured external stores open through SQLite `mode=ro&immutable=1` only
after confirming the `-wal` sidecar is absent or empty; a nonempty WAL makes the
source incompatible rather than serving a potentially stale main-file image.
The federated path creates no maintenance lock, inventory entry, WAL/SHM
sidecar, schema, migration, recall touch, co-occurrence link, or corpus write.
Corpus results project only `embeddings.passage_text`, never h_mems such as
`method_signals`. `curator_federated_search` rank-interleaves already-ranked
source batches and exposes `ready`, `unconfigured`, `invalid`, `incompatible`,
or `unavailable` status per source. The server rechecks configured source file
identity (path, size, modification time, and Unix inode) on each search;
changes trigger re-admission and full hash verification. A source that changes
during retrieval contributes no hits. This freshness check assumes normal
filesystem metadata changes; it is not a tamper-proof guarantee against a
writer able to restore metadata or race the check. The explicit tool search
remains available independently of the opt-in Curator chat injection setting.
The chat injector reads only selected registered sealed sources, labels their
passages as external evidence, and never promotes them to Curator memory.

**TODO (operator policy decision):** define the global inclusion rules for
which databases, replicas, and corpus chunks may be registered for federation
and why. Specify eligibility, consent/access boundaries, source provenance,
quality and freshness requirements before broadening registration beyond the
current sealed-corpus contract.

## Dependencies

- `hkask-mcp-server` — MCP runtime and dispatch
- `hkask-storage` / `hkask-memory` — sovereign `curator.db` stores
- `governance` module — escalation CRUD + Regulation event emission
- `hkask-tool-port` — DelegationToken consent registry
